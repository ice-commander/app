use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::overlay::Overlay;
use crate::util::{is_binary, join_rel, to_lines};

pub(crate) struct Editor {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) lines: Vec<String>,
    pub(crate) cx: usize,
    pub(crate) cy: usize,
    pub(crate) dirty: bool,
    pub(crate) status: Option<String>,
}

impl Editor {
    fn cur_len(&self) -> usize {
        self.lines[self.cy].chars().count()
    }
    fn byte_at(line: &str, cx: usize) -> usize {
        line.char_indices().nth(cx).map(|(b, _)| b).unwrap_or(line.len())
    }
    fn clamp_cx(&mut self) {
        let l = self.cur_len();
        if self.cx > l {
            self.cx = l;
        }
    }
    fn insert_char(&mut self, c: char) {
        let b = Self::byte_at(&self.lines[self.cy], self.cx);
        self.lines[self.cy].insert(b, c);
        self.cx += 1;
        self.dirty = true;
    }
    fn newline(&mut self) {
        let b = Self::byte_at(&self.lines[self.cy], self.cx);
        let rest = self.lines[self.cy].split_off(b);
        self.lines.insert(self.cy + 1, rest);
        self.cy += 1;
        self.cx = 0;
        self.dirty = true;
    }
    fn backspace(&mut self) {
        if self.cx > 0 {
            let (s, e) = {
                let line = &self.lines[self.cy];
                (Self::byte_at(line, self.cx - 1), Self::byte_at(line, self.cx))
            };
            self.lines[self.cy].replace_range(s..e, "");
            self.cx -= 1;
        } else if self.cy > 0 {
            let cur = self.lines.remove(self.cy);
            self.cy -= 1;
            self.cx = self.cur_len();
            self.lines[self.cy].push_str(&cur);
        }
        self.dirty = true;
    }
    fn delete(&mut self) {
        let len = self.cur_len();
        if self.cx < len {
            let (s, e) = {
                let line = &self.lines[self.cy];
                (Self::byte_at(line, self.cx), Self::byte_at(line, self.cx + 1))
            };
            self.lines[self.cy].replace_range(s..e, "");
        } else if self.cy + 1 < self.lines.len() {
            let next = self.lines.remove(self.cy + 1);
            self.lines[self.cy].push_str(&next);
        }
        self.dirty = true;
    }
    fn left(&mut self) {
        if self.cx > 0 {
            self.cx -= 1;
        } else if self.cy > 0 {
            self.cy -= 1;
            self.cx = self.cur_len();
        }
    }
    fn right(&mut self) {
        if self.cx < self.cur_len() {
            self.cx += 1;
        } else if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            self.cx = 0;
        }
    }
    fn up(&mut self) {
        if self.cy > 0 {
            self.cy -= 1;
            self.clamp_cx();
        }
    }
    fn down(&mut self) {
        if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            self.clamp_cx();
        }
    }
    fn text(&self) -> String {
        self.lines.join("\n")
    }
}

impl App {
    pub(crate) async fn open_editor(&mut self) {
        let Some(row) = self.panes[self.active].selected_row() else { return };
        if row.is_dir {
            return;
        }
        let rel = join_rel(&self.panes[self.active].active_relative(), &row.name);
        let core = self.panes[self.active].core.clone();
        const MAX: usize = 4 * 1024 * 1024;
        match core.active_provider().read_file(rel.clone(), None).await {
            Ok(b) if b.len() > MAX => self.set_message("Edit", "File too large to edit."),
            Ok(b) if is_binary(&b) => self.set_message("Edit", "Binary file — cannot edit."),
            Ok(b) => {
                let mut lines = to_lines(&b);
                if lines.is_empty() {
                    lines.push(String::new());
                }
                self.overlay = Overlay::Editor(Editor {
                    path: rel,
                    title: row.name,
                    lines,
                    cx: 0,
                    cy: 0,
                    dirty: false,
                    status: None,
                });
            }
            Err(e) => self.set_message("Edit failed", e.to_string()),
        }
    }

    pub(crate) async fn save_active_editor(&mut self) {
        let (path, text) = match &self.overlay {
            Overlay::Editor(ed) => (ed.path.clone(), ed.text()),
            _ => return,
        };
        let core = self.panes[self.active].core.clone();
        match core.active_provider().write_file(path, text.into_bytes(), None, None).await {
            Ok(_) => {
                let _ = core.refresh().await;
                self.overlay = Overlay::None;
            }
            Err(e) => {
                if let Overlay::Editor(ed) = &mut self.overlay {
                    ed.status = Some(format!("Save failed: {e}"));
                }
            }
        }
    }

    pub(crate) async fn handle_editor_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::F(2) => self.save_active_editor().await,
            KeyCode::Char('s') if ctrl => self.save_active_editor().await,
            _ => {
                if let Overlay::Editor(ed) = &mut self.overlay {
                    match key.code {
                        KeyCode::Up => ed.up(),
                        KeyCode::Down => ed.down(),
                        KeyCode::Left => ed.left(),
                        KeyCode::Right => ed.right(),
                        KeyCode::Home => ed.cx = 0,
                        KeyCode::End => ed.cx = ed.cur_len(),
                        KeyCode::Enter => ed.newline(),
                        KeyCode::Backspace => ed.backspace(),
                        KeyCode::Delete => ed.delete(),
                        KeyCode::Char(c) => ed.insert_char(c),
                        _ => {}
                    }
                }
            }
        }
    }
}

pub(crate) fn draw_editor(f: &mut Frame, ed: &Editor) {
    let area = f.area();
    let dirty = if ed.dirty { "*" } else { "" };
    let title = match &ed.status {
        Some(s) => format!(" Edit: {}{dirty}  —  {s} ", ed.title),
        None => format!(" Edit: {}{dirty}  —  F2/Ctrl-S save · Esc cancel ", ed.title),
    };
    let accent = if ed.status.is_some() { Color::Red } else { Color::Green };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(accent).add_modifier(Modifier::BOLD));
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    let vh = inner.height as usize;
    let scroll = ed.cy.saturating_sub(vh.saturating_sub(1));
    let text: Vec<Line> = ed
        .lines
        .iter()
        .skip(scroll)
        .take(vh)
        .map(|l| Line::from(l.clone()))
        .collect();
    f.render_widget(Paragraph::new(text), inner);
    let cx = (ed.cx as u16).min(inner.width.saturating_sub(1));
    let cy = (ed.cy - scroll) as u16;
    f.set_cursor_position((inner.x + cx, inner.y + cy));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::to_lines;

    fn editor(lines: &[&str]) -> Editor {
        Editor {
            path: "/notes.txt".into(),
            title: "notes.txt".into(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            cx: 0,
            cy: 0,
            dirty: false,
            status: None,
        }
    }

    fn at(lines: &[&str], cy: usize, cx: usize) -> Editor {
        let mut ed = editor(lines);
        ed.cy = cy;
        ed.cx = cx;
        ed
    }

    #[test]
    fn typing_inserts_at_the_cursor_and_advances_it() {
        let mut ed = at(&["ab"], 0, 1);
        ed.insert_char('X');
        assert_eq!(ed.lines, vec!["aXb"]);
        assert_eq!(ed.cx, 2);
        assert!(ed.dirty);
    }

    #[test]
    fn typing_at_the_end_of_a_line_appends() {
        let mut ed = at(&["ab"], 0, 2);
        ed.insert_char('!');
        assert_eq!(ed.lines, vec!["ab!"]);
        assert_eq!(ed.cx, 3);
    }

    #[test]
    fn typing_into_a_multibyte_line_uses_character_positions() {
        let mut ed = at(&["привет"], 0, 3);
        ed.insert_char('-');
        assert_eq!(ed.lines, vec!["при-вет"]);
        assert_eq!(ed.cx, 4);
    }

    #[test]
    fn enter_splits_the_line_at_the_cursor() {
        let mut ed = at(&["hello"], 0, 2);
        ed.newline();
        assert_eq!(ed.lines, vec!["he", "llo"]);
        assert_eq!((ed.cy, ed.cx), (1, 0));
        assert!(ed.dirty);
    }

    #[test]
    fn enter_at_the_end_of_a_line_opens_an_empty_one_below() {
        let mut ed = at(&["a", "b"], 0, 1);
        ed.newline();
        assert_eq!(ed.lines, vec!["a", "", "b"]);
        assert_eq!((ed.cy, ed.cx), (1, 0));
    }

    #[test]
    fn enter_splits_multibyte_lines_on_character_boundaries() {
        let mut ed = at(&["日本語"], 0, 1);
        ed.newline();
        assert_eq!(ed.lines, vec!["日", "本語"]);
    }

    #[test]
    fn backspace_removes_the_character_before_the_cursor() {
        let mut ed = at(&["привет"], 0, 6);
        ed.backspace();
        assert_eq!(ed.lines, vec!["приве"]);
        assert_eq!(ed.cx, 5);
        assert!(ed.dirty);
    }

    #[test]
    fn backspace_at_column_zero_joins_with_the_previous_line() {
        let mut ed = at(&["ab", "cd"], 1, 0);
        ed.backspace();
        assert_eq!(ed.lines, vec!["abcd"]);
        assert_eq!((ed.cy, ed.cx), (0, 2));
    }

    #[test]
    fn backspace_at_the_start_of_the_buffer_leaves_the_text_alone() {
        let mut ed = at(&["ab", "cd"], 0, 0);
        ed.backspace();
        assert_eq!(ed.lines, vec!["ab", "cd"]);
        assert_eq!((ed.cy, ed.cx), (0, 0));
    }

    #[test]
    fn delete_removes_the_character_under_the_cursor() {
        let mut ed = at(&["abc"], 0, 1);
        ed.delete();
        assert_eq!(ed.lines, vec!["ac"]);
        assert_eq!(ed.cx, 1);
    }

    #[test]
    fn delete_at_the_line_end_pulls_the_next_line_up() {
        let mut ed = at(&["ab", "cd"], 0, 2);
        ed.delete();
        assert_eq!(ed.lines, vec!["abcd"]);
        assert_eq!((ed.cy, ed.cx), (0, 2));
    }

    #[test]
    fn delete_at_the_end_of_the_buffer_leaves_the_text_alone() {
        let mut ed = at(&["ab"], 0, 2);
        ed.delete();
        assert_eq!(ed.lines, vec!["ab"]);
    }

    #[test]
    fn moving_left_from_column_zero_wraps_to_the_previous_line_end() {
        let mut ed = at(&["abc", "d"], 1, 0);
        ed.left();
        assert_eq!((ed.cy, ed.cx), (0, 3));
    }

    #[test]
    fn moving_left_at_the_start_of_the_buffer_stays_put() {
        let mut ed = at(&["abc"], 0, 0);
        ed.left();
        assert_eq!((ed.cy, ed.cx), (0, 0));
    }

    #[test]
    fn moving_right_from_the_line_end_wraps_to_the_next_line_start() {
        let mut ed = at(&["ab", "cd"], 0, 2);
        ed.right();
        assert_eq!((ed.cy, ed.cx), (1, 0));
    }

    #[test]
    fn moving_right_at_the_end_of_the_buffer_stays_put() {
        let mut ed = at(&["ab"], 0, 2);
        ed.right();
        assert_eq!((ed.cy, ed.cx), (0, 2));
    }

    #[test]
    fn moving_up_clamps_the_column_to_the_shorter_line() {
        let mut ed = at(&["ab", "longer"], 1, 5);
        ed.up();
        assert_eq!((ed.cy, ed.cx), (0, 2));
    }

    #[test]
    fn moving_down_clamps_the_column_to_the_shorter_line() {
        let mut ed = at(&["longer", "ab"], 0, 5);
        ed.down();
        assert_eq!((ed.cy, ed.cx), (1, 2));
    }

    #[test]
    fn vertical_movement_stops_at_the_first_and_last_line() {
        let mut ed = at(&["a", "b"], 0, 0);
        ed.up();
        assert_eq!(ed.cy, 0);
        ed.down();
        ed.down();
        assert_eq!(ed.cy, 1);
    }

    #[test]
    fn the_column_is_measured_in_characters() {
        let ed = editor(&["привет"]);
        assert_eq!(ed.cur_len(), 6);
    }

    #[test]
    fn moving_around_does_not_mark_the_buffer_dirty() {
        let mut ed = at(&["ab", "cd"], 0, 0);
        ed.right();
        ed.down();
        ed.left();
        ed.up();
        assert!(!ed.dirty);
    }

    #[test]
    fn the_edited_text_round_trips_through_the_line_splitter() {
        let mut ed = editor(&["one", "two"]);
        ed.cy = 1;
        ed.cx = 3;
        ed.newline();
        ed.insert_char('三');
        assert_eq!(ed.text(), "one\ntwo\n三");
        assert_eq!(to_lines(ed.text().as_bytes()), ed.lines);
    }

    #[test]
    fn a_single_empty_line_is_empty_text() {
        assert_eq!(editor(&[""]).text(), "");
    }
}
