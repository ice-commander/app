use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::Frame;

use crate::footer::{draw_footer, footer_slots, tool_slots, FooterAction};
use crate::overlay::{draw_overlay, ConfirmAction, InputAction, Overlay};
use crate::pane::{draw_pane, draw_status, Pane};
use crate::term::{render_term, term_height, Focus, Term};
use crate::view::{Step, ViewPane};

pub(crate) struct App {
    pub(crate) panes: [Pane; 2],
    pub(crate) config: client_config::AppConfig,
    pub(crate) terms: [Option<Term>; 2],
    pub(crate) active: usize,
    pub(crate) focus: Focus,
    pub(crate) term_expanded: bool,
    pub(crate) list_rects: [Rect; 2],
    pub(crate) term_rects: [Rect; 2],
    pub(crate) footer_hits: Vec<(Rect, FooterAction)>,
    /// Read once, after the plugins are loaded: the registry is thread-local.
    pub(crate) plugin_actions: Vec<ic_plugin_host::FsAction>,
    pub(crate) picked: std::rc::Rc<std::cell::RefCell<Vec<ic_plugin_host::SelectedItem>>>,
    pub(crate) overlay: Overlay,
    pub(crate) should_quit: bool,
}

impl App {
    pub(crate) fn active_pane(&mut self) -> &mut Pane {
        &mut self.panes[self.active]
    }

    pub(crate) fn set_message(&mut self, title: &str, body: impl Into<String>) {
        self.overlay = Overlay::Message {
            title: title.to_string(),
            body: body.into(),
        };
    }

    pub(crate) fn open_help(&mut self) {
        self.overlay = Overlay::Help;
    }

    /// Installed once; which panel is active is settled when a plugin asks.
    pub(crate) fn watch_selection(&self) {
        let seen = self.picked.clone();
        ic_plugin_host::set_selection_provider(std::rc::Rc::new(move || seen.borrow().clone()));
    }

    pub(crate) fn publish_selection(&mut self) {
        self.panes[self.active].forget_marks_if_moved();
        *self.picked.borrow_mut() = self.panes[self.active].picked();
    }

    pub(crate) async fn toggle_plugin_cell(&mut self, side: usize, key: String) {
        let asked = {
            let pane = &self.panes[side];
            let Some(row) = pane.selected_row() else {
                return;
            };
            let Some(at) = pane.shown_columns().iter().position(|s| s.key == key) else {
                return;
            };
            if row.is_parent {
                return;
            }
            let held = row.extra.get(at).map(String::as_str).unwrap_or_default();
            (
                pane.core.clone(),
                pane.active_relative(),
                row.name,
                !fm_core::rpc::cell_is_ticked(held),
            )
        };
        let (core, dir, name, ticked) = asked;
        if core
            .active_provider()
            .toggle_cell(&dir, &name, &key, ticked)
        {
            let _ = core.refresh().await;
        }
    }

    fn open_views(&mut self) {
        let items: Vec<(String, String)> = ic_plugin_host::view_ids()
            .into_iter()
            .map(|id| {
                let named = ic_plugin_host::view_title(&id).unwrap_or_else(|| id.clone());
                let shown = connection_form::translate_optional(&named).unwrap_or(named);
                (id, shown)
            })
            .collect();
        if items.is_empty() {
            return self.set_message("Plugins", "No plugin offers a window");
        }
        self.overlay = Overlay::Views { items, cursor: 0 };
    }

    fn open_view(&mut self, id: &str, argument: &str) {
        match ViewPane::open(id, argument) {
            Some(pane) => self.overlay = Overlay::View(Box::new(pane)),
            None => self.set_message("Plugins", format!("{id} cannot describe itself")),
        }
    }

    fn redescribe_connection(&mut self, kind: &str) {
        if let Overlay::View(pane) = &mut self.overlay {
            if pane.is_editing_kind(kind) {
                pane.redescribe_now();
            }
        }
    }

    /// What a plugin asked for, now that it has reached this thread.
    pub(crate) async fn on_plugin_request(&mut self, wanted: ic_plugin_host::Wanted) {
        match wanted {
            ic_plugin_host::Wanted::Open { view, argument } => self.open_view(&view, &argument),
            ic_plugin_host::Wanted::Invalidate { view } => {
                if let Overlay::View(pane) = &mut self.overlay {
                    if pane.session.id == view {
                        pane.redescribe_now();
                    }
                }
            }
            // A terminal has no surface to draw a frame on.
            ic_plugin_host::Wanted::Redraw { .. } => {}
            ic_plugin_host::Wanted::FormChanged { kind } => self.redescribe_connection(&kind),
            ic_plugin_host::Wanted::DrivesChanged => self.refresh_sources(),
            // What the application remembers about those folders is wrong now,
            // whoever is drawing. The panels read them again when they are
            // next asked for.
            ic_plugin_host::Wanted::FsInvalidate { extensions } => {
                fm_core::listing::forget_mounts_of(&extensions);
                self.relist_stale().await;
            }
            // The same, said by a plugin through the filesystem it was let
            // into: one path on one mount rather than a kind of mount.
            ic_plugin_host::Wanted::FsChanged { fs_id, at } => {
                fm_core::listing::forget_id_within(&fs_id, &at);
                self.relist_stale().await;
            }
            ic_plugin_host::Wanted::PinnedConnectionsChanged => {}
            // Already stored by the host, and there is no header bar here.
            ic_plugin_host::Wanted::HeaderLabel { .. }
            | ic_plugin_host::Wanted::HeaderVisible { .. }
            | ic_plugin_host::Wanted::HeaderIcon { .. } => {}
        }
    }

    pub(crate) fn on_timer(&mut self) {
        if let Overlay::View(pane) = &mut self.overlay {
            pane.on_timer();
        }
    }

    async fn settle(&mut self, step: Step) {
        match step {
            Step::Stay => {}
            Step::Save { connect } => self.save_open_editor(connect).await,
            Step::Revert => self.revert_open_editor(),
            Step::Close => {
                if let Overlay::View(pane) = &mut self.overlay {
                    if !pane.may_close() {
                        return;
                    }
                }
                if let Overlay::View(pane) = std::mem::replace(&mut self.overlay, Overlay::None) {
                    pane.closed();
                }
            }
            Step::Pick { node, folder } => {
                let picked = {
                    let pane = &self.panes[self.active];
                    let here = pane.active_relative();
                    match (folder, pane.selected_row()) {
                        (false, Some(row)) if !row.is_dir && !row.is_parent => {
                            crate::util::join_rel(&here, &row.name)
                        }
                        _ => here,
                    }
                };
                if let Overlay::View(pane) = &mut self.overlay {
                    pane.picked(&node, &picked);
                }
            }
        }
    }

    async fn activate(&mut self) {
        let Some(row) = self.panes[self.active].selected_row() else {
            return;
        };
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

    // Staleness, not the extension: a connection kind can go stale too and has no plugin_scope.
    async fn relist_stale(&mut self) {
        for side in 0..2 {
            let core = self.panes[side].core.clone();
            let fresh = core.path.borrow().active().has_been_read();
            if fresh {
                continue;
            }
            let under = self.panes[side].selected_row().map(|r| r.name);
            let _ = core.list_active().await;
            if let Some(name) = under {
                self.panes[side].keep_cursor_on(&name);
            }
        }
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
        if let Overlay::View(pane) = &mut self.overlay {
            let step = pane.on_click(col, row);
            self.settle(step).await;
            return;
        }
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
                        if let Some(key) = self.panes[side].tick_under(col) {
                            self.toggle_plugin_cell(side, key).await;
                        }
                    }
                }
                return;
            }
        }
        let footer_hit = self
            .footer_hits
            .iter()
            .find(|(rect, _)| rect.contains(pos))
            .map(|(_, what)| *what);
        if let Some(action) = footer_hit {
            self.trigger_footer(action).await;
        }
    }

    /// Nothing when the footer does not show it: a mount that took the write keys away keeps F5 as well.
    async fn handle_named_key(&mut self, named: &str) {
        let hit = tool_slots(self)
            .into_iter()
            .chain(footer_slots(self))
            .find(|slot| slot.key == named);
        if let Some(slot) = hit {
            if let (Some(what), true) = (slot.what, slot.enabled) {
                self.trigger_footer(what).await;
            }
        }
    }

    async fn handle_panel_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(named) = crate::footer::pressed(key) {
            self.handle_named_key(&named).await;
            return;
        }
        let len = self.active_pane().rows().len();
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if ctrl => self.should_quit = true,
            KeyCode::Char('r') if ctrl => self.refresh().await,
            KeyCode::Char('p') if ctrl => self.open_views(),
            KeyCode::BackTab => {
                if self.terms[self.active].is_some() {
                    self.focus = Focus::Term;
                }
            }
            KeyCode::Tab => self.active ^= 1,
            KeyCode::Insert | KeyCode::Char(' ') => {
                self.active_pane().toggle_mark();
                self.active_pane().move_cursor(1, len);
            }
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
            _ => {}
        }
    }

    async fn handle_overlay_key(&mut self, key: KeyEvent) {
        if matches!(self.overlay, Overlay::Editor(_)) {
            self.handle_editor_key(key).await;
            return;
        }
        if let Overlay::View(pane) = &mut self.overlay {
            let step = pane.on_key(key);
            self.settle(step).await;
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
                            ConfirmAction::Delete { rels } => self.do_delete(rels).await,
                            ConfirmAction::Transfer { move_it, items } => {
                                self.do_transfer(move_it, items).await
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
                KeyCode::F(4) => self.edit_selected_connection(),
                KeyCode::F(7) => self.offer_new_connection(),
                KeyCode::F(8) => self.delete_selected_connection(),
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
            Overlay::Kinds { items, cursor } => match key.code {
                KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    if *cursor + 1 < items.len() {
                        *cursor += 1;
                    }
                }
                KeyCode::Home => *cursor = 0,
                KeyCode::End => *cursor = items.len().saturating_sub(1),
                KeyCode::Esc => self.open_sources(),
                KeyCode::Enter => {
                    let chosen = items.get(*cursor).map(|(kind, _)| kind.clone());
                    if let Some(kind) = chosen {
                        self.start_new_connection(&kind);
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
                KeyCode::PageDown => *scroll = (*scroll + 20).min(lines.len().saturating_sub(1)),
                KeyCode::Home => *scroll = 0,
                KeyCode::End => *scroll = lines.len().saturating_sub(1),
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::F(3) | KeyCode::F(10) => {
                    self.overlay = Overlay::None
                }
                _ => {}
            },
            Overlay::Views { items, cursor } => match key.code {
                KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    if *cursor + 1 < items.len() {
                        *cursor += 1;
                    }
                }
                KeyCode::Home => *cursor = 0,
                KeyCode::End => *cursor = items.len().saturating_sub(1),
                KeyCode::Esc => self.overlay = Overlay::None,
                KeyCode::Enter => {
                    let chosen = items.get(*cursor).map(|(id, _)| id.clone());
                    self.overlay = Overlay::None;
                    if let Some(id) = chosen {
                        self.open_view(&id, "null");
                    }
                }
                _ => {}
            },
            Overlay::View(_) => {}
            Overlay::Editor(_) => {}
            Overlay::None => {}
        }
    }
}

pub(crate) fn ui(f: &mut Frame, app: &mut App) {
    app.publish_selection();
    let tools = tool_slots(app);
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(u16::from(!tools.is_empty())),
            Constraint::Length(1),
        ])
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
    let mut hits = draw_footer(f, root[2], &tools);
    let slots = footer_slots(app);
    hits.extend(draw_footer(f, root[3], &slots));
    app.footer_hits = hits;
    draw_overlay(f, &mut app.overlay);
}

#[cfg(test)]
mod hearing_the_plugins {
    use super::*;
    use fm_core::rpc::{FileSystemRpc, RemoteFileEntry};
    use std::rc::Rc;

    struct Somewhere(&'static str);

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Somewhere {
        async fn list_dir(&self, _: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
            Ok(vec![RemoteFileEntry {
                name: "a-file".to_string(),
                is_dir: false,
                size: 1,
                modified: 0,
                permissions: None,
                extra: Vec::new(),
            }])
        }
        fn fs_id(&self) -> String {
            self.0.to_string()
        }
    }

    fn app_with(config_name: &str) -> App {
        let fs: Rc<dyn FileSystemRpc> = Rc::new(Somewhere("local"));
        let pane = || {
            crate::pane::Pane::new(Rc::new(panel_core::RouterState::new(
                fs.clone(),
                fs.clone(),
                "/".to_string(),
            )))
        };
        App {
            panes: [pane(), pane()],
            config: client_config::AppConfig::new(config_name),
            terms: [None, None],
            active: 0,
            focus: Focus::Panel,
            term_expanded: false,
            list_rects: [ratatui::layout::Rect::default(); 2],
            term_rects: [ratatui::layout::Rect::default(); 2],
            footer_hits: Vec::new(),
            plugin_actions: Vec::new(),
            picked: Rc::new(std::cell::RefCell::new(Vec::new())),
            overlay: crate::overlay::Overlay::None,
            should_quit: false,
        }
    }

    thread_local! {
        static ASKED_WITH: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }

    const A_WINDOW: &str = r#"{"schema":1,"fields":[],"form":{"t":"view","surface":"window",
        "children":[{"t":"text","id":"where","text":"{arg.path}"}]}}"#;

    extern "C" fn describes(
        ctx: *const u8,
        len: u64,
        _user_data: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        let seen = if ctx.is_null() {
            String::new()
        } else {
            String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ctx, len as usize) })
                .into_owned()
        };
        ASKED_WITH.with(|held| *held.borrow_mut() = seen);
        ic_plugin_api::IcBytes {
            data: A_WINDOW.as_ptr(),
            len: A_WINDOW.len() as u64,
        }
    }

    fn a_window_called(id: &str) {
        let table = Box::leak(Box::new(ic_plugin_api::IcViewVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcViewVTable>() as u32,
            describe: describes,
            on_event: None,
            closed: None,
        }));
        let id = std::ffi::CString::new(id).expect("a plain name");
        let title = std::ffi::CString::new("A window").expect("a plain name");
        assert_eq!(
            (ic_plugin_host::host_ref().register_view)(
                id.as_ptr(),
                title.as_ptr(),
                table,
                std::ptr::null_mut(),
            ),
            ic_plugin_api::IC_OK
        );
    }

    /// Every plugin window opened with a bare `"null"`, so `{arg.path}` drew the placeholder.
    #[tokio::test]
    async fn a_window_a_plugin_opens_is_handed_what_it_opened_it_with() {
        a_window_called("asked.with");
        let mut app = app_with("ice-commander-console-argument-test");
        app.on_plugin_request(ic_plugin_host::Wanted::Open {
            view: "asked.with".to_string(),
            argument: r#"{"path":"/films/one.torrent"}"#.to_string(),
        })
        .await;

        let seen = ASKED_WITH.with(|held| held.borrow().clone());
        assert!(
            seen.contains("/films/one.torrent"),
            "the plugin was asked to describe itself without it: {seen}"
        );
        let crate::overlay::Overlay::View(pane) = &app.overlay else {
            panic!("the window did not open");
        };
        assert_eq!(pane.session.argument, r#"{"path":"/films/one.torrent"}"#);

        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20))
            .expect("a test terminal");
        terminal
            .draw(|f| crate::overlay::draw_overlay(f, &mut app.overlay))
            .expect("a frame");
        let buffer = terminal.backend().buffer().clone();
        let drawn = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            drawn.contains("/films/one.torrent"),
            "the window drew the placeholder instead of the path:\n{drawn}"
        );
        assert!(
            !drawn.contains("{arg."),
            "a placeholder was left on screen:\n{drawn}"
        );
    }

    /// Nothing listened for this, so a torrent whose files arrived left stale rows on screen.
    #[tokio::test]
    async fn a_plugin_saying_its_filesystem_moved_on_is_not_dropped_here() {
        fm_core::listing::forget_everything();
        let held: Rc<dyn FileSystemRpc> = Rc::new(Somewhere("local/film.torrent"));
        fm_core::listing::list(&held, "/", fm_core::listing::Freshness::Fresh)
            .await
            .expect("a listing");
        assert!(fm_core::listing::is_remembered(held.as_ref(), "/"));

        let mut app = app_with("ice-commander-console-invalidate-test");
        app.on_plugin_request(ic_plugin_host::Wanted::FsInvalidate {
            extensions: vec![".torrent".to_string()],
        })
        .await;

        assert!(
            !fm_core::listing::is_remembered(held.as_ref(), "/"),
            "the terminal interface swallowed the signal"
        );
        assert!(
            fm_core::listing::remembered_of(held.as_ref(), "/").is_some(),
            "and it should still have something to draw until it reads again"
        );
    }
}

#[cfg(test)]
mod what_a_plugin_is_handed {
    use super::*;
    use fm_core::rpc::{ColumnKind, ColumnSpec, FileSystemRpc, RemoteFileEntry};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Mount {
        id: &'static str,
        columns: Vec<ColumnSpec>,
        clicked: RefCell<Vec<(String, String, String, bool)>>,
    }

    impl Mount {
        fn new(id: &'static str, columns: Vec<ColumnSpec>) -> Rc<Self> {
            Rc::new(Mount {
                id,
                columns,
                clicked: RefCell::new(Vec::new()),
            })
        }
    }

    fn tick_column() -> Vec<ColumnSpec> {
        vec![ColumnSpec {
            key: "fetch".to_string(),
            title: "torrent.col_fetch".to_string(),
            width: Some(60),
            kind: ColumnKind::Check,
        }]
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Mount {
        async fn list_dir(&self, _: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
            let row = |name: &str, cell: &str| RemoteFileEntry {
                name: name.to_string(),
                is_dir: false,
                size: 1,
                modified: 0,
                permissions: None,
                extra: vec![cell.to_string()],
            };
            Ok(vec![row("one.flac", "1"), row("two.flac", "")])
        }
        fn fs_id(&self) -> String {
            self.id.to_string()
        }
        fn extra_columns(&self) -> Vec<ColumnSpec> {
            self.columns.clone()
        }
        fn toggle_cell(&self, dir: &str, name: &str, column: &str, ticked: bool) -> bool {
            self.clicked.borrow_mut().push((
                dir.to_string(),
                name.to_string(),
                column.to_string(),
                ticked,
            ));
            true
        }
    }

    async fn app_inside(mount: Rc<Mount>, config_name: &str) -> App {
        fm_core::listing::forget_everything();
        let fs: Rc<dyn FileSystemRpc> = mount;
        fm_core::listing::list(&fs, "/film.torrent", fm_core::listing::Freshness::Fresh)
            .await
            .expect("a listing");
        let pane = || {
            let core = Rc::new(panel_core::RouterState::new(
                fs.clone(),
                fs.clone(),
                "/".to_string(),
            ));
            let mut path =
                panel_core::nav::NavPath::new(panel_core::nav::PathLevel::new("", "/", fs.clone()));
            path.push(panel_core::nav::PathLevel::new(
                "film.torrent",
                "/film.torrent",
                fs.clone(),
            ));
            *core.path.borrow_mut() = path;
            crate::pane::Pane::new(core)
        };
        App {
            panes: [pane(), pane()],
            config: client_config::AppConfig::new(config_name),
            terms: [None, None],
            active: 0,
            focus: Focus::Panel,
            term_expanded: false,
            list_rects: [ratatui::layout::Rect::default(); 2],
            term_rects: [ratatui::layout::Rect::default(); 2],
            footer_hits: Vec::new(),
            plugin_actions: Vec::new(),
            picked: Rc::new(RefCell::new(Vec::new())),
            overlay: crate::overlay::Overlay::None,
            should_quit: false,
        }
    }

    fn what_a_plugin_sees() -> Vec<String> {
        (ic_plugin_host::host_ref().selection)()
            .as_slice()
            .iter()
            .filter_map(|i| i.path_string())
            .collect()
    }

    /// No provider was installed here, so every plugin button acting on files was handed nothing.
    #[tokio::test]
    async fn the_active_panel_is_what_a_plugin_asking_for_the_selection_gets() {
        let mount = Mount::new("selection/mount", Vec::new());
        let mut app = app_inside(mount, "ice-commander-console-selection-test").await;
        app.watch_selection();
        app.panes[0].table.select(Some(1));
        app.panes[1].table.select(Some(2));

        app.publish_selection();
        assert_eq!(what_a_plugin_sees(), ["/film.torrent/one.flac"]);

        app.active = 1;
        app.publish_selection();
        assert_eq!(what_a_plugin_sees(), ["/film.torrent/two.flac"]);
    }

    #[tokio::test]
    async fn marking_rows_is_what_reaches_the_plugin_instead_of_the_cursor() {
        let mount = Mount::new("selection/marks", Vec::new());
        let mut app = app_inside(mount, "ice-commander-console-marks-test").await;
        app.watch_selection();
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await;
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .await;
        app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .await;

        app.publish_selection();
        assert_eq!(
            what_a_plugin_sees(),
            ["/film.torrent/one.flac", "/film.torrent/two.flac"]
        );
    }

    #[tokio::test]
    async fn ctrl_t_ticks_the_box_the_folder_offered() {
        let mount = Mount::new("tick/mount", tick_column());
        let held = mount.clone();
        let mut app = app_inside(mount, "ice-commander-console-tick-test").await;
        app.panes[0].table.select(Some(2));
        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
            .await;
        app.panes[0].table.select(Some(1));
        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
            .await;

        let asked: Vec<(String, String, String, bool)> = held.clicked.borrow().clone();
        assert_eq!(
            asked,
            [
                (
                    "/film.torrent".to_string(),
                    "two.flac".to_string(),
                    "fetch".to_string(),
                    true
                ),
                (
                    "/film.torrent".to_string(),
                    "one.flac".to_string(),
                    "fetch".to_string(),
                    false
                ),
            ]
        );
    }

    #[tokio::test]
    async fn the_footer_names_the_tick_key_only_where_there_is_a_box() {
        let bare = Mount::new("tick/bare", Vec::new());
        let app = app_inside(bare, "ice-commander-console-footer-bare").await;
        assert!(!crate::footer::footer_slots(&app)
            .iter()
            .any(|slot| slot.key == "^T"));

        let ticked = Mount::new("tick/offered", tick_column());
        let app = app_inside(ticked, "ice-commander-console-footer-tick").await;
        let slots = crate::footer::footer_slots(&app);
        let keys: Vec<String> = slots.iter().map(|slot| slot.key.clone()).collect();
        assert_eq!(keys.iter().filter(|k| **k == "^T").count(), 1);
        assert_eq!(&keys[keys.len() - 3..], ["^T", "F9", "F10"]);
    }
}

#[cfg(test)]
mod the_folders_own_buttons {
    use super::*;
    use fm_core::rpc::{FileSystemRpc, RemoteFileEntry};
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::rc::Rc;

    thread_local! {
        static FIRED: RefCell<Vec<(String, usize)>> = const { RefCell::new(Vec::new()) };
    }

    extern "C" fn remember(
        mount: ic_plugin_api::IcFsHandle,
        user_data: *mut c_void,
        _parent: *mut c_void,
    ) {
        let which = unsafe { CStr::from_ptr(user_data as *const c_char) }
            .to_string_lossy()
            .to_string();
        FIRED.with(|held| held.borrow_mut().push((which, mount as usize)));
    }

    fn fired() -> Vec<(String, usize)> {
        FIRED.with(|held| held.borrow().clone())
    }

    /// Registered the way a plugin does it, through the table it was handed.
    fn offer(action_id: &str, kind: u32) {
        let extensions = CString::new(".torrent").expect("a claim");
        let id = CString::new(action_id).expect("an id");
        let svg = CString::new("<svg/>").expect("a picture");
        let tooltip = CString::new(action_id).expect("a phrase");
        let taken = (ic_plugin_host::host_ref().register_fs_action)(
            extensions.as_ptr(),
            id.as_ptr(),
            svg.as_ptr(),
            tooltip.as_ptr(),
            ic_plugin_api::IC_ENABLE_ALWAYS,
            kind,
            remember,
            id.as_ptr() as *mut c_void,
        );
        assert_eq!(taken, ic_plugin_api::IC_OK);
        std::mem::forget(id);
    }

    fn keep_the_panels_own_buttons(shown: bool) {
        let extensions = CString::new(".torrent").expect("a claim");
        (ic_plugin_host::host_ref().set_default_toolbar_visible)(
            extensions.as_ptr(),
            i32::from(shown),
        );
    }

    const MOUNT: usize = 0xFACE;

    struct Plain;

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Plain {
        async fn list_dir(&self, _: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
            Ok(Vec::new())
        }
        fn fs_id(&self) -> String {
            "local".to_string()
        }
    }

    struct Torrent {
        id: &'static str,
        state: HashMap<String, u32>,
        listed: Cell<usize>,
        grows: bool,
    }

    impl Torrent {
        fn new(id: &'static str, state: &[(&str, u32)]) -> Rc<Self> {
            Rc::new(Torrent {
                id,
                state: state.iter().map(|(k, v)| ((*k).to_string(), *v)).collect(),
                listed: Cell::new(0),
                grows: false,
            })
        }

        fn growing(id: &'static str) -> Rc<Self> {
            Rc::new(Torrent {
                id,
                state: HashMap::new(),
                listed: Cell::new(0),
                grows: true,
            })
        }
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Torrent {
        async fn list_dir(&self, _: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
            self.listed.set(self.listed.get() + 1);
            let file = |name: &str| RemoteFileEntry {
                name: name.to_string(),
                is_dir: false,
                size: 1,
                modified: 0,
                permissions: None,
                extra: Vec::new(),
            };
            let mut rows = vec![file("one.flac")];
            if self.grows && self.listed.get() > 1 {
                rows.insert(0, file("aaa-arrived.flac"));
            }
            Ok(rows)
        }
        fn fs_id(&self) -> String {
            self.id.to_string()
        }
        fn plugin_scope(&self) -> Option<String> {
            Some(".torrent".to_string())
        }
        fn plugin_mount(&self) -> Option<usize> {
            Some(MOUNT)
        }
        fn plugin_action_state(&self, action_id: &str) -> u32 {
            self.state
                .get(action_id)
                .copied()
                .unwrap_or(ic_plugin_api::IC_ACTION_DEFAULT)
        }
    }

    fn app_on(fs: Rc<dyn FileSystemRpc>, at: &str, config_name: &str) -> App {
        let pane = || {
            let core = Rc::new(panel_core::RouterState::new(
                fs.clone(),
                fs.clone(),
                "/".to_string(),
            ));
            if !at.is_empty() {
                let mut path = panel_core::nav::NavPath::new(panel_core::nav::PathLevel::new(
                    "",
                    "/",
                    fs.clone(),
                ));
                path.push(panel_core::nav::PathLevel::new(at, "/", fs.clone()));
                *core.path.borrow_mut() = path;
            }
            crate::pane::Pane::new(core)
        };
        App {
            panes: [pane(), pane()],
            config: client_config::AppConfig::new(config_name),
            terms: [None, None],
            active: 0,
            focus: Focus::Panel,
            term_expanded: false,
            list_rects: [ratatui::layout::Rect::default(); 2],
            term_rects: [ratatui::layout::Rect::default(); 2],
            footer_hits: Vec::new(),
            plugin_actions: ic_plugin_host::fs_actions(),
            picked: Rc::new(std::cell::RefCell::new(Vec::new())),
            overlay: crate::overlay::Overlay::None,
            should_quit: false,
        }
    }

    fn alt(digit: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(digit), KeyModifiers::ALT)
    }

    /// The terminal read `fs_actions()` nowhere, so these buttons existed only in the desktop.
    #[tokio::test]
    async fn a_folders_own_buttons_take_a_seat_each_while_the_panel_stands_in_it() {
        offer("torrent.view_all", ic_plugin_api::IC_ACTION_TOGGLE);
        offer("torrent.start", ic_plugin_api::IC_ACTION_BUTTON);
        let inside: Rc<dyn FileSystemRpc> = Torrent::new("seats/mount", &[]);
        let app = app_on(inside, "film.torrent", "ice-commander-console-seats");
        let seats = crate::footer::tool_slots(&app);
        let named: Vec<(String, String)> = seats
            .iter()
            .map(|s| (s.key.clone(), s.label.clone()))
            .collect();
        assert_eq!(
            named,
            [
                ("M-1".to_string(), "torrent.view_all".to_string()),
                ("M-2".to_string(), "torrent.start".to_string()),
            ]
        );

        let elsewhere: Rc<dyn FileSystemRpc> = Rc::new(Plain);
        let outside = app_on(elsewhere, "", "ice-commander-console-seats-outside");
        assert!(crate::footer::tool_slots(&outside).is_empty());
    }

    #[tokio::test]
    async fn a_seat_pressed_reaches_the_plugin_with_the_mount_the_panel_stands_in() {
        offer("torrent.start", ic_plugin_api::IC_ACTION_BUTTON);
        let inside: Rc<dyn FileSystemRpc> = Torrent::new("press/mount", &[]);
        let mut app = app_on(inside, "film.torrent", "ice-commander-console-press");
        app.on_key(alt('1')).await;
        assert_eq!(fired(), [("torrent.start".to_string(), MOUNT)]);
    }

    /// A radio group only by agreement: pressing the view already shown would run it twice.
    #[tokio::test]
    async fn a_button_that_is_off_or_already_pressed_does_nothing() {
        offer("torrent.view_all", ic_plugin_api::IC_ACTION_TOGGLE);
        offer("torrent.cleanup", ic_plugin_api::IC_ACTION_BUTTON);
        let inside: Rc<dyn FileSystemRpc> = Torrent::new(
            "state/mount",
            &[
                (
                    "torrent.view_all",
                    ic_plugin_api::IC_ACTION_DEFAULT | ic_plugin_api::IC_ACTION_ON,
                ),
                ("torrent.cleanup", ic_plugin_api::IC_ACTION_SHOWN),
            ],
        );
        let mut app = app_on(inside, "film.torrent", "ice-commander-console-state");
        let seats = crate::footer::tool_slots(&app);
        assert!(seats[0].on);
        assert!(!seats[1].enabled);

        app.on_key(alt('1')).await;
        app.on_key(alt('2')).await;
        // A cell drawn while the mount still wanted it, pressed after it stopped.
        app.trigger_footer(FooterAction::Plugin(1)).await;
        assert!(fired().is_empty());
    }

    /// In a terminal the keys have to go with the buttons, or the footer says one thing and F5 another.
    #[tokio::test]
    async fn a_mount_that_asked_for_no_write_buttons_loses_the_keys_as_well() {
        keep_the_panels_own_buttons(false);
        fm_core::listing::forget_everything();
        let inside: Rc<dyn FileSystemRpc> = Torrent::new("bare/mount", &[]);
        fm_core::listing::list(&inside, "/", fm_core::listing::Freshness::Fresh)
            .await
            .expect("a listing");
        let mut app = app_on(inside, "film.torrent", "ice-commander-console-bare");
        app.panes[0].table.select(Some(1));
        assert!(!app.panes[0].chosen().is_empty());
        let keys: Vec<String> = crate::footer::footer_slots(&app)
            .iter()
            .map(|s| s.key.clone())
            .collect();
        assert_eq!(keys, ["F1", "^D", "F3", "F9", "F10"]);

        app.on_key(KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE))
            .await;
        assert!(
            matches!(app.overlay, Overlay::None),
            "a key the footer does not offer still acted"
        );

        keep_the_panels_own_buttons(true);
        let kept: Rc<dyn FileSystemRpc> = Torrent::new("kept/mount", &[]);
        let back = app_on(kept, "film.torrent", "ice-commander-console-kept");
        assert!(crate::footer::footer_slots(&back)
            .iter()
            .any(|s| s.key == "F5"));
    }

    /// The signal cleared the memory and stopped there, so the panel drew on until Ctrl-R.
    #[tokio::test]
    async fn a_filesystem_that_moved_on_is_read_again_with_nobody_pressing_anything() {
        fm_core::listing::forget_everything();
        let mount = Torrent::new("local/film.torrent", &[]);
        let held = mount.clone();
        let fs: Rc<dyn FileSystemRpc> = mount;
        fm_core::listing::list(&fs, "/", fm_core::listing::Freshness::Fresh)
            .await
            .expect("a listing");
        assert_eq!(held.listed.get(), 1);

        let mut app = app_on(fs.clone(), "", "ice-commander-console-relist");
        app.on_plugin_request(ic_plugin_host::Wanted::FsInvalidate {
            extensions: vec![".torrent".to_string()],
        })
        .await;

        assert_eq!(held.listed.get(), 2, "nobody went and looked again");
        assert!(fm_core::listing::is_remembered(fs.as_ref(), "/"));
    }

    /// A torrent grows rows while it runs, and a cursor sliding with every tick is worse than a still one.
    #[tokio::test]
    async fn a_relisting_leaves_the_cursor_on_the_file_it_was_on() {
        fm_core::listing::forget_everything();
        let fs: Rc<dyn FileSystemRpc> = Torrent::growing("local/film.torrent");
        fm_core::listing::list(&fs, "/", fm_core::listing::Freshness::Fresh)
            .await
            .expect("a listing");

        let mut app = app_on(fs, "", "ice-commander-console-cursor");
        assert_eq!(
            app.panes[0].selected_row().map(|r| r.name).as_deref(),
            Some("one.flac")
        );

        app.on_plugin_request(ic_plugin_host::Wanted::FsInvalidate {
            extensions: vec![".torrent".to_string()],
        })
        .await;

        assert_eq!(app.panes[0].rows().len(), 2);
        assert_eq!(
            app.panes[0].selected_row().map(|r| r.name).as_deref(),
            Some("one.flac")
        );
    }
}
