use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::Frame;

use crate::footer::{draw_footer, FOOTER};
use crate::overlay::{draw_overlay, ConfirmAction, InputAction, Overlay};
use crate::pane::{draw_pane, draw_status, Pane};
use crate::term::{render_term, term_height, Focus, Term};

pub(crate) struct App {
    pub(crate) panes: [Pane; 2],
    pub(crate) config: client_config::AppConfig,
    pub(crate) terms: [Option<Term>; 2],
    pub(crate) active: usize,
    pub(crate) focus: Focus,
    pub(crate) term_expanded: bool,
    pub(crate) list_rects: [Rect; 2],
    pub(crate) term_rects: [Rect; 2],
    pub(crate) footer_rects: Vec<Rect>,
    pub(crate) overlay: Overlay,
    pub(crate) should_quit: bool,
}

impl App {
    pub(crate) fn active_pane(&mut self) -> &mut Pane {
        &mut self.panes[self.active]
    }

    pub(crate) fn set_message(&mut self, title: &str, body: impl Into<String>) {
        self.overlay = Overlay::Message { title: title.to_string(), body: body.into() };
    }

    pub(crate) fn open_help(&mut self) {
        self.overlay = Overlay::Help;
    }

    async fn activate(&mut self) {
        let Some(row) = self.panes[self.active].selected_row() else { return };
        if row.is_parent {
            self.go_up().await;
        } else if row.is_dir || panel_core::nav::is_archive(&row.name) {
            let core = self.panes[self.active].core.clone();
            let _ = core.enter(&row.name).await;
            self.active_pane().table.select(Some(0));
        }
    }

    async fn go_up(&mut self) {
        let core = self.panes[self.active].core.clone();
        let leaving = core.path.borrow().active().name.clone();
        let _ = core.go_up().await;
        self.active_pane().select_name(&leaving);
    }

    async fn refresh(&mut self) {
        let core = self.panes[self.active].core.clone();
        let _ = core.refresh().await;
    }

    pub(crate) async fn on_key(&mut self, key: KeyEvent) {
        if !matches!(self.overlay, Overlay::None) {
            self.handle_overlay_key(key).await;
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if self.focus == Focus::Term && self.terms[self.active].is_some() {
            match key.code {
                KeyCode::F(9) => self.toggle_term(),
                KeyCode::BackTab => self.defocus_terminal(),
                KeyCode::Char('o') if ctrl => self.term_expanded = !self.term_expanded,
                KeyCode::Enter if alt => self.term_expanded = !self.term_expanded,
                _ => self.feed_terminal(key),
            }
            return;
        }
        self.handle_panel_key(key).await;
    }

    pub(crate) async fn on_click(&mut self, col: u16, row: u16) {
        if !matches!(self.overlay, Overlay::None) {
            return;
        }
        let pos = Position { x: col, y: row };
        for side in 0..2 {
            if self.terms[side].is_some() && self.term_rects[side].contains(pos) {
                self.active = side;
                self.focus = Focus::Term;
                return;
            }
        }
        for side in 0..2 {
            if self.list_rects[side].contains(pos) {
                self.active = side;
                self.focus = Focus::Panel;
                let data_top = self.list_rects[side].y + 2; // top border + header
                if row >= data_top {
                    let visible = (row - data_top) as usize;
                    let idx = self.panes[side].table.offset() + visible;
                    if idx < self.panes[side].rows().len() {
                        self.panes[side].table.select(Some(idx));
                    }
                }
                return;
            }
        }
        let footer_hit = self
            .footer_rects
            .iter()
            .position(|r| r.contains(pos))
            .map(|i| FOOTER[i].2);
        if let Some(action) = footer_hit {
            self.trigger_footer(action).await;
        }
    }

    async fn handle_panel_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let len = self.active_pane().rows().len();
        match key.code {
            KeyCode::Char('q') | KeyCode::F(10) => self.should_quit = true,
            KeyCode::Char('c') if ctrl => self.should_quit = true,
            KeyCode::Char('r') if ctrl => self.refresh().await,
            KeyCode::Char('d') if ctrl => self.open_sources(),
            KeyCode::F(9) => self.toggle_term(),
            KeyCode::BackTab => {
                if self.terms[self.active].is_some() {
                    self.focus = Focus::Term;
                }
            }
            KeyCode::Tab => self.active ^= 1,
            KeyCode::Up | KeyCode::Char('k') => self.active_pane().move_cursor(-1, len),
            KeyCode::Down | KeyCode::Char('j') => self.active_pane().move_cursor(1, len),
            KeyCode::PageUp => self.active_pane().move_cursor(-10, len),
            KeyCode::PageDown => self.active_pane().move_cursor(10, len),
            KeyCode::Home => self.active_pane().move_cursor(isize::MIN, len),
            KeyCode::End => self.active_pane().move_cursor(isize::MAX, len),
            KeyCode::Enter | KeyCode::Right => self.activate().await,
            KeyCode::Backspace | KeyCode::Left => {
                if self.panes[self.active].core.path.borrow().depth() == 1 {
                    self.open_sources();
                } else {
                    self.go_up().await;
                }
            }
            KeyCode::F(1) => self.open_help(),
            KeyCode::F(2) => self.prompt_rename(),
            KeyCode::F(3) => self.open_viewer().await,
            KeyCode::F(4) => self.open_editor().await,
            KeyCode::F(5) => self.prompt_transfer(false),
            KeyCode::F(6) => self.prompt_transfer(true),
            KeyCode::F(7) => self.prompt_mkdir(),
            KeyCode::F(8) => self.prompt_delete(),
            _ => {}
        }
    }

    async fn handle_overlay_key(&mut self, key: KeyEvent) {
        if matches!(self.overlay, Overlay::Editor(_)) {
            self.handle_editor_key(key).await;
            return;
        }
        match &mut self.overlay {
            Overlay::Input { value, .. } => match key.code {
                KeyCode::Enter => {
                    if let Overlay::Input { action, value, .. } =
                        std::mem::replace(&mut self.overlay, Overlay::None)
                    {
                        match action {
                            InputAction::MkDir => self.do_mkdir(value).await,
                            InputAction::Rename { old_rel, dir } => {
                                self.do_rename(old_rel, dir, value).await
                            }
                        }
                    }
                }
                KeyCode::Esc => self.overlay = Overlay::None,
                KeyCode::Backspace => {
                    value.pop();
                }
                KeyCode::Char(c) => value.push(c),
                _ => {}
            },
            Overlay::Confirm { .. } => match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Overlay::Confirm { action, .. } =
                        std::mem::replace(&mut self.overlay, Overlay::None)
                    {
                        match action {
                            ConfirmAction::Delete { rel, .. } => self.do_delete(rel).await,
                            ConfirmAction::Transfer { move_it, src_rel, dst_rel, .. } => {
                                self.do_transfer(move_it, src_rel, dst_rel).await
                            }
                        }
                    }
                }
                _ => self.overlay = Overlay::None,
            },
            Overlay::Message { .. } => self.overlay = Overlay::None,
            Overlay::Sources { items, cursor } => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    *cursor = cursor.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *cursor + 1 < items.len() {
                        *cursor += 1;
                    }
                }
                KeyCode::Home => *cursor = 0,
                KeyCode::End => *cursor = items.len().saturating_sub(1),
                KeyCode::Esc => self.overlay = Overlay::None,
                KeyCode::Enter => {
                    if let Overlay::Sources { items, cursor } =
                        std::mem::replace(&mut self.overlay, Overlay::None)
                    {
                        if let Some(src) = items.into_iter().nth(cursor) {
                            self.activate_source(src).await;
                        }
                    }
                }
                _ => {}
            },
            Overlay::Help => self.overlay = Overlay::None,
            Overlay::Viewer { lines, scroll, .. } => match key.code {
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    if *scroll + 1 < lines.len() {
                        *scroll += 1;
                    }
                }
                KeyCode::PageUp => *scroll = scroll.saturating_sub(20),
                KeyCode::PageDown => {
                    *scroll = (*scroll + 20).min(lines.len().saturating_sub(1))
                }
                KeyCode::Home => *scroll = 0,
                KeyCode::End => *scroll = lines.len().saturating_sub(1),
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::F(3) | KeyCode::F(10) => {
                    self.overlay = Overlay::None
                }
                _ => {}
            },
            Overlay::Editor(_) => {}
            Overlay::None => {}
        }
    }
}

pub(crate) fn ui(f: &mut Frame, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)])
        .split(f.area());
    let root0 = root[0];
    let active = app.active;
    let focus = app.focus;
    app.list_rects = [Rect::default(); 2];
    app.term_rects = [Rect::default(); 2];
    if app.term_expanded && app.terms[active].is_some() {
        app.term_rects[active] = root0;
        if let Some(t) = app.terms[active].as_mut() {
            render_term(f, root0, t, true, true);
        }
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(root0);
        for side in 0..2 {
            let area = cols[side];
            let list_active = active == side && focus == Focus::Panel;
            if app.terms[side].is_some() {
                let h = term_height(area.height);
                let split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(3), Constraint::Length(h)])
                    .split(area);
                app.list_rects[side] = split[0];
                app.term_rects[side] = split[1];
                draw_pane(f, split[0], &mut app.panes[side], list_active);
                let term_focused = active == side && focus == Focus::Term;
                if let Some(t) = app.terms[side].as_mut() {
                    render_term(f, split[1], t, term_focused, false);
                }
            } else {
                app.list_rects[side] = area;
                draw_pane(f, area, &mut app.panes[side], list_active);
            }
        }
    }
    draw_status(f, root[1], &app.panes[active]);
    app.footer_rects = draw_footer(f, root[2]);
    draw_overlay(f, &app.overlay);
}
