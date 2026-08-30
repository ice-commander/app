use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::overlay::Overlay;
use crate::util::{is_binary, join_rel, to_lines};

impl App {
    pub(crate) async fn open_viewer(&mut self) {
        let Some(row) = self.panes[self.active].selected_row() else { return };
        if row.is_dir {
            return;
        }
        let rel = join_rel(&self.panes[self.active].active_relative(), &row.name);
        let core = self.panes[self.active].core.clone();
        const MAX: usize = 8 * 1024 * 1024;
        match core.active_provider().read_file(rel, None).await {
            Ok(b) if b.len() > MAX => {
                self.set_message("View", format!("File too large ({} MB).", b.len() / 1024 / 1024))
            }
            Ok(b) if is_binary(&b) => {
                self.set_message("View", format!("Binary file — {} bytes, not shown.", b.len()))
            }
            Ok(b) => self.overlay = Overlay::Viewer { title: row.name, lines: to_lines(&b), scroll: 0 },
            Err(e) => self.set_message("View failed", e.to_string()),
        }
    }
}

pub(crate) fn draw_viewer(f: &mut Frame, title: &str, lines: &[String], scroll: usize) {
    let area = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" View: {title}  —  ↑↓ PgUp/PgDn Home/End · Esc close "))
        .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    let text: Vec<Line> = lines
        .iter()
        .skip(scroll)
        .take(inner.height as usize)
        .map(|l| Line::from(l.clone()))
        .collect();
    f.render_widget(Paragraph::new(text), inner);
}
