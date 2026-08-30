use tokio::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ic_platform::terminal::{spawn_pty_command, spawn_pty_session, PtySession};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::app::App;

pub(crate) struct Term {
    pub(crate) parser: vt100::Parser,
    pub(crate) input_tx: mpsc::Sender<Vec<u8>>,
    pub(crate) resize_tx: mpsc::Sender<(u16, u16)>,
    pub(crate) output_rx: mpsc::Receiver<Vec<u8>>,
    pub(crate) rows: u16,
    pub(crate) cols: u16,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Focus {
    Panel,
    Term,
}

impl App {
    pub(crate) fn toggle_term(&mut self) {
        let side = self.active;
        if self.terms[side].is_some() {
            self.terms[side] = None;
            self.focus = Focus::Panel;
            self.term_expanded = false;
            return;
        }
        let core = self.panes[side].core.clone();
        let cwd = core.path.borrow().active().relative_path.clone();
        let spawned = match core.active_provider().get_ssh_connection_command(&cwd) {
            Some(args) if !args.is_empty() => spawn_pty_command(args, None),
            _ => spawn_pty_session(Some(cwd)),
        };
        match spawned {
            Ok(PtySession { input_tx, output_rx, resize_tx }) => {
                self.terms[side] = Some(Term {
                    parser: vt100::Parser::new(24, 80, 0),
                    input_tx,
                    resize_tx,
                    output_rx,
                    rows: 0,
                    cols: 0,
                });
                self.focus = Focus::Term;
            }
            Err(e) => self.set_message("Terminal failed", e.to_string()),
        }
    }

    pub(crate) fn feed_terminal(&mut self, key: KeyEvent) {
        if let Some(bytes) = encode_key(key) {
            if let Some(t) = &self.terms[self.active] {
                let _ = t.input_tx.try_send(bytes);
            }
        }
    }

    pub(crate) fn defocus_terminal(&mut self) {
        self.focus = Focus::Panel;
        self.term_expanded = false;
    }
}

pub(crate) fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let bytes = match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                let up = c.to_ascii_uppercase() as u8;
                match up {
                    b'A'..=b'Z' => vec![up - b'A' + 1],
                    b' ' => vec![0],
                    b'\\' => vec![28],
                    b']' => vec![29],
                    b'^' => vec![30],
                    b'_' => vec![31],
                    _ => c.to_string().into_bytes(),
                }
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => return None,
        },
        _ => return None,
    };
    Some(bytes)
}

pub(crate) fn term_height(pane_h: u16) -> u16 {
    if pane_h < 10 {
        return pane_h.saturating_sub(3).max(1);
    }
    let frac = if pane_h >= 30 { pane_h / 2 } else { pane_h / 3 };
    frac.clamp(5, pane_h.saturating_sub(3))
}

fn conv_color(c: vt100::Color) -> Option<Color> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(Color::Indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

pub(crate) fn render_term(f: &mut Frame, area: Rect, term: &mut Term, focused: bool, expanded: bool) {
    let title = if !focused {
        " terminal "
    } else if expanded {
        " terminal — Ctrl-O/Alt+Enter collapse · Shift-Tab back · F9 close "
    } else {
        " terminal — Ctrl-O/Alt+Enter expand · Shift-Tab back · F9 close "
    };
    let border = if focused {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default().borders(Borders::ALL).title(title).border_style(border);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let (rows, cols) = (inner.height, inner.width);
    if rows == 0 || cols == 0 {
        return;
    }
    if term.rows != rows || term.cols != cols {
        term.parser.set_size(rows, cols);
        let _ = term.resize_tx.try_send((rows, cols));
        term.rows = rows;
        term.cols = cols;
    }
    let screen = term.parser.screen();
    let buf = f.buffer_mut();
    for y in 0..rows {
        for x in 0..cols {
            let Some(cell) = screen.cell(y, x) else { continue };
            let contents = cell.contents();
            let mut style = Style::default();
            if let Some(c) = conv_color(cell.fgcolor()) {
                style = style.fg(c);
            }
            if let Some(c) = conv_color(cell.bgcolor()) {
                style = style.bg(c);
            }
            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.italic() {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if cell.inverse() {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if let Some(target) = buf.cell_mut((inner.x + x, inner.y + y)) {
                target.set_symbol(if contents.is_empty() { " " } else { &contents });
                target.set_style(style);
            }
        }
    }
    if focused && !screen.hide_cursor() {
        let (cy, cx) = screen.cursor_position();
        if cy < rows && cx < cols {
            f.set_cursor_position((inner.x + cx, inner.y + cy));
        }
    }
}

pub(crate) async fn term_recv(term: &mut Option<Term>) -> Option<Vec<u8>> {
    match term {
        Some(t) => t.output_rx.recv().await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(code: KeyCode) -> Option<Vec<u8>> {
        encode_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(c: char) -> Option<Vec<u8>> {
        encode_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    #[test]
    fn plain_characters_are_sent_as_their_utf8_bytes() {
        assert_eq!(plain(KeyCode::Char('a')), Some(b"a".to_vec()));
        assert_eq!(plain(KeyCode::Char('A')), Some(b"A".to_vec()));
        assert_eq!(plain(KeyCode::Char('я')), Some("я".as_bytes().to_vec()));
        assert_eq!(plain(KeyCode::Char('€')), Some("€".as_bytes().to_vec()));
    }

    #[test]
    fn ctrl_letters_become_control_codes_regardless_of_case() {
        assert_eq!(ctrl('a'), Some(vec![1]));
        assert_eq!(ctrl('A'), Some(vec![1]));
        assert_eq!(ctrl('c'), Some(vec![3]));
        assert_eq!(ctrl('z'), Some(vec![26]));
    }

    #[test]
    fn ctrl_space_is_a_nul_byte() {
        assert_eq!(ctrl(' '), Some(vec![0]));
    }

    #[test]
    fn ctrl_punctuation_maps_to_the_separator_codes() {
        assert_eq!(ctrl('\\'), Some(vec![28]));
        assert_eq!(ctrl(']'), Some(vec![29]));
        assert_eq!(ctrl('^'), Some(vec![30]));
        assert_eq!(ctrl('_'), Some(vec![31]));
    }

    #[test]
    fn ctrl_with_an_unmapped_character_sends_the_character_itself() {
        assert_eq!(ctrl('1'), Some(b"1".to_vec()));
    }

    #[test]
    fn editing_keys_use_their_control_bytes() {
        assert_eq!(plain(KeyCode::Enter), Some(vec![b'\r']));
        assert_eq!(plain(KeyCode::Tab), Some(vec![b'\t']));
        assert_eq!(plain(KeyCode::Backspace), Some(vec![0x7f]));
        assert_eq!(plain(KeyCode::Esc), Some(vec![0x1b]));
    }

    #[test]
    fn arrow_and_navigation_keys_are_escape_sequences() {
        assert_eq!(plain(KeyCode::Up), Some(b"\x1b[A".to_vec()));
        assert_eq!(plain(KeyCode::Down), Some(b"\x1b[B".to_vec()));
        assert_eq!(plain(KeyCode::Right), Some(b"\x1b[C".to_vec()));
        assert_eq!(plain(KeyCode::Left), Some(b"\x1b[D".to_vec()));
        assert_eq!(plain(KeyCode::Delete), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn the_first_four_function_keys_use_ss3_and_the_rest_use_csi() {
        assert_eq!(plain(KeyCode::F(1)), Some(b"\x1bOP".to_vec()));
        assert_eq!(plain(KeyCode::F(4)), Some(b"\x1bOS".to_vec()));
        assert_eq!(plain(KeyCode::F(5)), Some(b"\x1b[15~".to_vec()));
        assert_eq!(plain(KeyCode::F(12)), Some(b"\x1b[24~".to_vec()));
    }

    #[test]
    fn function_keys_outside_one_to_twelve_are_not_encoded() {
        assert_eq!(plain(KeyCode::F(0)), None);
        assert_eq!(plain(KeyCode::F(13)), None);
    }

    #[test]
    fn keys_without_a_terminal_encoding_are_dropped() {
        assert_eq!(plain(KeyCode::Null), None);
        assert_eq!(plain(KeyCode::CapsLock), None);
    }

    #[test]
    fn every_function_key_encoding_is_distinct() {
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for n in 1..=12u8 {
            let bytes = plain(KeyCode::F(n)).expect("F1..F12 are encoded");
            assert!(!seen.contains(&bytes), "F{n} repeats an earlier encoding");
            seen.push(bytes);
        }
    }

    #[test]
    fn a_short_pane_still_gets_a_single_terminal_row() {
        assert_eq!(term_height(1), 1);
        assert_eq!(term_height(3), 1);
        assert_eq!(term_height(5), 2);
        assert_eq!(term_height(9), 6);
    }

    #[test]
    fn a_medium_pane_gives_the_terminal_a_third_but_never_less_than_five() {
        assert_eq!(term_height(10), 5);
        assert_eq!(term_height(20), 6);
        assert_eq!(term_height(29), 9);
    }

    #[test]
    fn a_tall_pane_splits_evenly_with_the_list() {
        assert_eq!(term_height(30), 15);
        assert_eq!(term_height(60), 30);
    }

    #[test]
    fn the_terminal_never_takes_the_whole_pane() {
        for h in 1..=400u16 {
            let t = term_height(h);
            assert!(t >= 1 && t <= h, "pane {h} got terminal height {t}");
            if h >= 10 {
                assert!(h - t >= 3, "pane {h} left only {} rows for the list", h - t);
            }
        }
    }
}
