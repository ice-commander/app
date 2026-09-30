use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FooterAction {
    Help,
    Sources,
    Rename,
    View,
    Edit,
    Copy,
    Move,
    Mkdir,
    Delete,
    Term,
    Quit,
    Tick,
    Plugin(usize),
}

pub(crate) struct FooterSlot {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) what: Option<FooterAction>,
    pub(crate) enabled: bool,
    pub(crate) on: bool,
    pub(crate) plugin: bool,
}

impl FooterSlot {
    fn host(key: &str, label: &str, what: FooterAction) -> Self {
        FooterSlot {
            key: key.to_string(),
            label: label.to_string(),
            what: Some(what),
            enabled: true,
            on: false,
            plugin: false,
        }
    }
}

const FOOTER: &[(&str, &str, FooterAction, bool)] = &[
    ("F1", "Help", FooterAction::Help, false),
    ("^D", "Src", FooterAction::Sources, false),
    ("F2", "Ren", FooterAction::Rename, true),
    ("F3", "View", FooterAction::View, false),
    ("F4", "Edit", FooterAction::Edit, true),
    ("F5", "Copy", FooterAction::Copy, true),
    ("F6", "Move", FooterAction::Move, true),
    ("F7", "Mkdir", FooterAction::Mkdir, true),
    ("F8", "Del", FooterAction::Delete, true),
    ("F9", "Term", FooterAction::Term, false),
    ("F10", "Quit", FooterAction::Quit, false),
];

const SEATS: usize = 9;

pub(crate) fn footer_slots(app: &App) -> Vec<FooterSlot> {
    let writes = ic_plugin_host::default_toolbar_shown(&app.plugin_scope());
    let mut slots: Vec<FooterSlot> = FOOTER
        .iter()
        .filter(|(_, _, _, write)| writes || !write)
        .map(|(key, label, what, _)| FooterSlot::host(key, label, *what))
        .collect();
    if app.panes[app.active].tick_column().is_some() {
        slots.insert(
            slots.len() - 2,
            FooterSlot::host("^T", "Tick", FooterAction::Tick),
        );
    }
    slots
}

/// In registration order, so a seat keeps its number whatever the mount answers.
pub(crate) fn tool_slots(app: &App) -> Vec<FooterSlot> {
    let scope = app.plugin_scope();
    if scope.is_empty() {
        return Vec::new();
    }
    let provider = app.panes[app.active].core.active_provider();
    app.plugin_actions
        .iter()
        .enumerate()
        .filter(|(_, action)| action.belongs_to(&scope))
        .take(SEATS)
        .enumerate()
        .map(|(seat, (at, action))| {
            let state = provider.plugin_action_state(&action.action_id);
            let shown = state & ic_plugin_api::IC_ACTION_SHOWN != 0;
            FooterSlot {
                key: format!("M-{}", seat + 1),
                label: if shown {
                    short_label(&action.tooltip)
                } else {
                    String::new()
                },
                what: shown.then_some(FooterAction::Plugin(at)),
                enabled: state & ic_plugin_api::IC_ACTION_ENABLED != 0,
                on: action.is_toggle() && state & ic_plugin_api::IC_ACTION_ON != 0,
                plugin: true,
            }
        })
        .collect()
}

/// A tooltip is written for a toolbar; a plugin may offer `<key>.short` for a footer cell.
fn short_label(tooltip: &str) -> String {
    connection_form::translate_optional(&format!("{tooltip}.short"))
        .or_else(|| connection_form::translate_optional(tooltip))
        .unwrap_or_else(|| tooltip.to_string())
}

pub(crate) fn pressed(key: KeyEvent) -> Option<String> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::F(n) if (1..=10).contains(&n) => Some(format!("F{n}")),
        KeyCode::Char('d') if ctrl => Some("^D".to_string()),
        KeyCode::Char('t') if ctrl => Some("^T".to_string()),
        KeyCode::Char(c) if alt && ('1'..='9').contains(&c) => Some(format!("M-{c}")),
        _ => None,
    }
}

impl App {
    pub(crate) fn plugin_scope(&self) -> String {
        self.panes[self.active]
            .core
            .active_provider()
            .plugin_scope()
            .unwrap_or_default()
    }

    pub(crate) async fn trigger_footer(&mut self, action: FooterAction) {
        match action {
            FooterAction::Help => self.open_help(),
            FooterAction::Sources => self.open_sources(),
            FooterAction::Rename => self.prompt_rename(),
            FooterAction::View => self.open_viewer().await,
            FooterAction::Edit => self.open_editor().await,
            FooterAction::Copy => self.prompt_transfer(false),
            FooterAction::Move => self.prompt_transfer(true),
            FooterAction::Mkdir => self.prompt_mkdir(),
            FooterAction::Delete => self.prompt_delete(),
            FooterAction::Term => self.toggle_term(),
            FooterAction::Quit => self.should_quit = true,
            FooterAction::Tick => {
                if let Some(key) = self.panes[self.active].tick_column() {
                    self.toggle_plugin_cell(self.active, key).await;
                }
            }
            FooterAction::Plugin(at) => self.fire_plugin_action(at),
        }
    }

    fn fire_plugin_action(&mut self, at: usize) {
        let Some(action) = self.plugin_actions.get(at).cloned() else {
            return;
        };
        let provider = self.panes[self.active].core.active_provider();
        let state = provider.plugin_action_state(&action.action_id);
        if state & ic_plugin_api::IC_ACTION_ENABLED == 0 {
            return;
        }
        // A view already being shown is not a choice; pressing it again would run it twice.
        if action.is_toggle() && state & ic_plugin_api::IC_ACTION_ON != 0 {
            return;
        }
        let mount = provider.plugin_mount().unwrap_or(0) as ic_plugin_api::IcFsHandle;
        // The plugin asks for the selection from inside the call below.
        self.publish_selection();
        action.fire(mount, std::ptr::null_mut());
    }
}

pub(crate) fn draw_footer(
    f: &mut Frame,
    area: Rect,
    slots: &[FooterSlot],
) -> Vec<(Rect, FooterAction)> {
    if area.height == 0 || slots.is_empty() {
        return Vec::new();
    }
    let room: Vec<Constraint> = slots
        .iter()
        .map(|s| {
            let want = s.key.chars().count() + s.label.chars().count() + 3;
            Constraint::Fill(want as u16)
        })
        .collect();
    let cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(room)
        .split(area);
    let mut hits = Vec::new();
    for (slot, cell) in slots.iter().zip(cells.iter()) {
        let (key_style, label_style) = if slot.on {
            (
                Style::default().bg(Color::Yellow).fg(Color::Black),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else if slot.what.is_none() || !slot.enabled {
            (
                Style::default().fg(Color::DarkGray),
                Style::default().fg(Color::DarkGray),
            )
        } else if slot.plugin {
            (
                Style::default().bg(Color::Magenta).fg(Color::Black),
                Style::default().fg(Color::Gray),
            )
        } else {
            (
                Style::default().bg(Color::Cyan).fg(Color::Black),
                Style::default().fg(Color::Gray),
            )
        };
        let line = Line::from(vec![
            Span::styled(format!(" {} ", slot.key), key_style),
            Span::styled(format!(" {}", slot.label), label_style),
        ]);
        f.render_widget(Paragraph::new(line), *cell);
        if let (Some(what), true) = (slot.what, slot.enabled) {
            hits.push((*cell, what));
        }
    }
    hits
}
