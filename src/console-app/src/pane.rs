use std::rc::Rc;

use fm_core::rpc::RemoteFileEntry;
use panel_core::RouterState;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row as TableRow, Table, TableState};
use ratatui::Frame;

use crate::util::{fmt_time, human_size};

#[derive(Clone)]
pub(crate) struct Row {
    pub(crate) name: String,
    pub(crate) is_dir: bool,
    pub(crate) is_parent: bool,
    pub(crate) size: u64,
    pub(crate) modified: u64,
}

pub(crate) struct Pane {
    pub(crate) core: Rc<RouterState>,
    pub(crate) table: TableState,
}

impl Pane {
    pub(crate) fn new(core: Rc<RouterState>) -> Self {
        let mut table = TableState::default();
        table.select(Some(0));
        Self { core, table }
    }

    pub(crate) fn rows(&self) -> Vec<Row> {
        let path = self.core.path.borrow();
        let mut rows = Vec::new();
        if path.depth() > 1 {
            rows.push(Row { name: "..".into(), is_dir: true, is_parent: true, size: 0, modified: 0 });
        }
        let mut entries: Vec<&RemoteFileEntry> = path.active().entries.iter().collect();
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        rows.extend(entries.into_iter().map(|e| Row {
            name: e.name.clone(),
            is_dir: e.is_dir,
            is_parent: false,
            size: e.size,
            modified: e.modified,
        }));
        rows
    }

    pub(crate) fn cursor(&self) -> usize {
        self.table.selected().unwrap_or(0)
    }

    pub(crate) fn selected_row(&self) -> Option<Row> {
        self.rows().into_iter().nth(self.cursor())
    }

    pub(crate) fn active_relative(&self) -> String {
        self.core.path.borrow().active().relative_path.clone()
    }

    pub(crate) fn move_cursor(&mut self, delta: isize, len: usize) {
        if len == 0 {
            self.table.select(Some(0));
            return;
        }
        let cur = self.cursor() as isize;
        let next = (cur + delta).clamp(0, len as isize - 1) as usize;
        self.table.select(Some(next));
    }

    pub(crate) fn select_name(&mut self, name: &str) {
        let idx = self.rows().iter().position(|r| r.name == name).unwrap_or(0);
        self.table.select(Some(idx));
    }
}

pub(crate) fn draw_pane(f: &mut Frame, area: Rect, pane: &mut Pane, active: bool) {
    let rows = pane.rows();
    let len = rows.len();
    if pane.cursor() >= len {
        pane.table.select(Some(len.saturating_sub(1)));
    }
    let table_rows: Vec<TableRow> = rows
        .iter()
        .map(|r| {
            let name_style = if r.is_dir {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let name = if r.is_dir && !r.is_parent {
                format!("{}/", r.name)
            } else {
                r.name.clone()
            };
            let size = if r.is_dir {
                "[DIR]".to_string()
            } else {
                human_size(r.size)
            };
            let date = fmt_time(r.modified);
            TableRow::new(vec![
                Cell::from(name).style(name_style),
                Cell::from(format!("{size:>9}")).style(Style::default().fg(Color::DarkGray)),
                Cell::from(date).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    let count = rows.iter().filter(|r| !r.is_parent).count();
    let title = {
        let path = pane.core.path.borrow();
        let abs = path.absolute_path();
        let name = path.levels().first().and_then(|l| l.fs.display_name());
        match name {
            Some(n) if abs == "/" => format!(" {n}:/  ({count}) "),
            Some(n) => format!(" {n}:{abs}  ({count}) "),
            None => format!(" {abs}  ({count}) "),
        }
    };
    let border_style = if active {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default().borders(Borders::ALL).title(title).border_style(border_style);
    let inner = block.inner(area);
    let highlight = if active {
        Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    };
    let header = TableRow::new(vec!["Name", "Size", "Modified"])
        .style(Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD));
    const SP: u16 = 3;
    const SIZE_W: u16 = 9;
    const DATE_W: u16 = 16;
    let widths = [Constraint::Fill(1), Constraint::Length(SIZE_W), Constraint::Length(DATE_W)];
    let table = Table::new(table_rows, widths)
        .header(header)
        .block(block)
        .column_spacing(SP)
        .row_highlight_style(highlight);
    f.render_stateful_widget(table, area, &mut pane.table);

    if inner.width > SIZE_W + DATE_W + 2 * SP + 2 {
        let name_w = inner.width - SIZE_W - DATE_W - 2 * SP;
        let seps = [inner.x + name_w + SP / 2, inner.x + name_w + SP + SIZE_W + SP / 2];
        let buf = f.buffer_mut();
        for sx in seps {
            for y in inner.y..inner.y.saturating_add(inner.height) {
                if let Some(c) = buf.cell_mut((sx, y)) {
                    c.set_symbol("│");
                    c.set_fg(Color::DarkGray);
                }
            }
        }
    }
}

pub(crate) fn draw_status(f: &mut Frame, area: Rect, pane: &Pane) {
    let text = match pane.selected_row() {
        Some(r) if r.is_parent => "..".to_string(),
        Some(r) if r.is_dir => format!("{}    <DIR>", r.name),
        Some(r) => format!("{}    {}    {}", r.name, human_size(r.size), fmt_time(r.modified)),
        None => String::new(),
    };
    let bar = Style::default().bg(Color::Rgb(30, 34, 40)).fg(Color::White);
    f.render_widget(Paragraph::new(format!(" {text}")).style(bar), area);
}
