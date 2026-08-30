use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;

#[derive(Clone, Copy)]
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
}

pub(crate) const FOOTER: &[(&str, &str, FooterAction)] = &[
    ("F1", "Help", FooterAction::Help),
    ("^D", "Src", FooterAction::Sources),
    ("F2", "Ren", FooterAction::Rename),
    ("F3", "View", FooterAction::View),
    ("F4", "Edit", FooterAction::Edit),
    ("F5", "Copy", FooterAction::Copy),
    ("F6", "Move", FooterAction::Move),
    ("F7", "Mkdir", FooterAction::Mkdir),
    ("F8", "Del", FooterAction::Delete),
    ("F9", "Term", FooterAction::Term),
    ("F10", "Quit", FooterAction::Quit),
];

impl App {
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
        }
    }
}

pub(crate) fn draw_footer(f: &mut Frame, area: Rect) -> Vec<Rect> {
    let key = Style::default().bg(Color::Cyan).fg(Color::Black);
    let hint = Style::default().fg(Color::Gray);
    let n = FOOTER.len();
    let cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, n as u32); n])
        .split(area);
    for (i, (k, label, _)) in FOOTER.iter().enumerate() {
        let line = Line::from(vec![
            Span::styled(format!(" {k} "), key),
            Span::styled(format!(" {label}"), hint),
        ]);
        f.render_widget(Paragraph::new(line), cells[i]);
    }
    cells.to_vec()
}
