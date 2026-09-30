use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::overlay::Overlay;
use crate::util::{is_binary, join_rel, to_lines};
use crate::view::ViewPane;

impl App {
    pub(crate) async fn open_viewer(&mut self) {
        let Some(row) = self.panes[self.active].selected_row() else {
            return;
        };
        if row.is_dir {
            return;
        }
        let rel = join_rel(&self.panes[self.active].active_relative(), &row.name);
        let core = self.panes[self.active].core.clone();
        // By extension only: sniffing bytes would be a request per file on a server.
        if let Some(viewer) = ic_plugin_host::viewer_for(&row.name) {
            self.show_through(&viewer, &core, &rel, &row.name).await;
            return;
        }
        const MAX: usize = 8 * 1024 * 1024;
        match core.active_provider().read_file(rel, None).await {
            Ok(b) if b.len() > MAX => self.set_message(
                "View",
                format!("File too large ({} MB).", b.len() / 1024 / 1024),
            ),
            Ok(b) if is_binary(&b) => self.set_message(
                "View",
                format!("Binary file — {} bytes, not shown.", b.len()),
            ),
            Ok(b) => {
                self.overlay = Overlay::Viewer {
                    title: row.name,
                    lines: to_lines(&b),
                    scroll: 0,
                }
            }
            Err(e) => self.set_message("View failed", e.to_string()),
        }
    }

    /// A file on a disk is read where it lies; one on a server is fetched once, here.
    async fn show_through(
        &mut self,
        viewer: &str,
        core: &std::rc::Rc<panel_core::RouterState>,
        rel: &str,
        name: &str,
    ) {
        let provider = core.active_provider();
        let staged = match fm_core::host_fs::stage(&provider, rel).await {
            Ok(staged) => staged,
            Err(e) => return self.set_message("View failed", e.to_string()),
        };
        let source =
            fm_core::host_fs::HostSource::rooted_at(&staged.root, &provider.fs_id(), "").open();
        match ViewPane::viewing(viewer, name, source, staged) {
            Some(pane) => self.overlay = Overlay::View(Box::new(pane)),
            None => self.set_message("View failed", format!("{viewer} could not show {name}")),
        }
    }
}

pub(crate) fn draw_viewer(f: &mut Frame, title: &str, lines: &[String], scroll: usize) {
    let area = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " View: {title}  —  ↑↓ PgUp/PgDn Home/End · Esc close "
        ))
        .border_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
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
