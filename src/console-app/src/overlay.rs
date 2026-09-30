use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::editor::{draw_editor, Editor};
use crate::sources::{draw_sources, Source};
use crate::util::centered_rect;
use crate::view::ViewPane;
use crate::viewer::draw_viewer;

pub(crate) enum Overlay {
    None,
    Input {
        title: String,
        value: String,
        action: InputAction,
    },
    Confirm {
        title: String,
        message: String,
        action: ConfirmAction,
    },
    Message {
        title: String,
        body: String,
    },
    Sources {
        items: Vec<Source>,
        cursor: usize,
    },
    Help,
    Viewer {
        title: String,
        lines: Vec<String>,
        scroll: usize,
    },
    Editor(Editor),
    Views {
        items: Vec<(String, String)>,
        cursor: usize,
    },
    Kinds {
        items: Vec<(String, String)>,
        cursor: usize,
    },
    View(Box<ViewPane>),
}

impl Overlay {
    pub(crate) fn deadline(&self) -> Option<std::time::Instant> {
        match self {
            Overlay::View(pane) => pane.deadline(),
            _ => None,
        }
    }
}

pub(crate) enum InputAction {
    MkDir,
    Rename { old_rel: String, dir: String },
}

pub(crate) enum ConfirmAction {
    Delete {
        rels: Vec<String>,
    },
    Transfer {
        move_it: bool,
        items: Vec<(String, String)>,
    },
}

const HELP_ROWS: &[(&str, &str)] = &[
    ("Navigation", ""),
    ("  ↑ ↓   j k", "move cursor"),
    ("  PgUp PgDn Home End", "jump"),
    ("  Enter  →", "open directory / archive"),
    ("  Backspace  ←", "up  (sources at the root)"),
    ("  Tab", "switch panel"),
    (
        "  Insert / Space",
        "mark a row (file operations act on the marks)",
    ),
    ("  click", "select panel / row"),
    ("Sources", ""),
    ("  Ctrl-D", "drives / network connections"),
    ("  Ctrl-P", "plugin windows"),
    ("  F4 / F7 / F8", "edit / new / delete a connection"),
    ("File operations", ""),
    ("  F2", "rename"),
    ("  F3 / F4", "view / edit file"),
    ("  F5 / F6", "copy / move to other panel"),
    ("  F7 / F8", "make directory / delete"),
    ("Plugin folder", ""),
    (
        "  Alt-1 … Alt-9 / click",
        "the buttons this folder puts there",
    ),
    ("Terminal", ""),
    ("  F9", "open / close under the panel"),
    ("  Ctrl-O / Alt+Enter", "expand / collapse"),
    ("  Shift-Tab / click", "focus list ↔ terminal"),
    ("Misc", ""),
    ("  Ctrl-R", "refresh"),
    ("  Ctrl-T / click", "tick a box a plugin's folder offers"),
    ("  F1", "this help"),
    ("  F10 / q / Ctrl-C", "quit"),
];

fn draw_help(f: &mut Frame) {
    let area = f.area();
    let h = (HELP_ROWS.len() as u16 + 3).clamp(6, area.height.saturating_sub(2));
    let w = 52u16.min(area.width.saturating_sub(4)).max(30);
    let rect = centered_rect(w, h, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Ice Commander — keys · any key to close ")
        .border_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    let lines: Vec<Line> = HELP_ROWS
        .iter()
        .map(|(k, d)| {
            if d.is_empty() {
                Line::from(Span::styled(
                    *k,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(vec![
                    Span::styled(format!("{k:<22}"), Style::default().fg(Color::Cyan)),
                    Span::styled(*d, Style::default().fg(Color::Gray)),
                ])
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_picklist(f: &mut Frame, title: &str, items: &[(String, String)], cursor: usize) {
    let area = f.area();
    let h = (items.len() as u16 + 2).clamp(4, area.height.saturating_sub(2));
    let w = 46u16.min(area.width.saturating_sub(4)).max(24);
    let rect = centered_rect(w, h, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string())
        .border_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    let rows: Vec<ratatui::widgets::ListItem> = items
        .iter()
        .map(|(_, title)| ratatui::widgets::ListItem::new(title.clone()))
        .collect();
    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(cursor.min(items.len().saturating_sub(1))));
    let list = ratatui::widgets::List::new(rows)
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, inner, &mut state);
}

pub(crate) fn draw_overlay(f: &mut Frame, overlay: &mut Overlay) {
    match overlay {
        Overlay::Sources { items, cursor } => return draw_sources(f, items, *cursor),
        Overlay::Help => return draw_help(f),
        Overlay::Viewer {
            title,
            lines,
            scroll,
        } => return draw_viewer(f, title, lines, *scroll),
        Overlay::Editor(ed) => return draw_editor(f, ed),
        Overlay::Views { items, cursor } => {
            return draw_picklist(
                f,
                " Plugins \u{2014} Enter open \u{b7} Esc back ",
                items,
                *cursor,
            )
        }
        Overlay::Kinds { items, cursor } => {
            return draw_picklist(
                f,
                " New connection \u{2014} Enter choose \u{b7} Esc back ",
                items,
                *cursor,
            )
        }
        Overlay::View(pane) => return pane.draw(f),
        _ => {}
    }
    let area = f.area();
    let (title, body, footer, accent) = match overlay {
        Overlay::None
        | Overlay::Sources { .. }
        | Overlay::Help
        | Overlay::Viewer { .. }
        | Overlay::Editor(_)
        | Overlay::Views { .. }
        | Overlay::Kinds { .. }
        | Overlay::View(_) => return,
        Overlay::Input { title, value, .. } => (
            title.clone(),
            format!("{value}\u{2588}"),
            "[Enter] OK   [Esc] Cancel",
            Color::Yellow,
        ),
        Overlay::Confirm { title, message, .. } => (
            title.clone(),
            message.clone(),
            "[Enter/Y] Yes   [Esc] No",
            Color::Yellow,
        ),
        Overlay::Message { title, body } => {
            (title.clone(), body.clone(), "[Esc] Close", Color::Red)
        }
    };
    let width = body.len().max(footer.len()).max(title.len()).min(60) as u16 + 4;
    let rect = centered_rect(width.max(24), 6, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(Style::default().fg(accent).add_modifier(Modifier::BOLD));
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    let lines = vec![
        Line::from(body),
        Line::from(""),
        Line::from(Span::styled(footer, Style::default().fg(Color::DarkGray))),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}
