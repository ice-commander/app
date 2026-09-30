use std::collections::BTreeSet;
use std::rc::Rc;

use fm_core::rpc::{ColumnKind, ColumnSpec, RemoteFileEntry};
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
    pub(crate) extra: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Shows {
    Name,
    Size,
    Modified,
    Cell {
        at: usize,
        key: String,
        kind: ColumnKind,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DrawnColumn {
    pub(crate) x: u16,
    pub(crate) width: u16,
    pub(crate) shows: Shows,
}

pub(crate) struct Pane {
    pub(crate) core: Rc<RouterState>,
    pub(crate) table: TableState,
    pub(crate) drawn: Vec<DrawnColumn>,
    marked: BTreeSet<String>,
    marked_at: String,
}

impl Pane {
    pub(crate) fn new(core: Rc<RouterState>) -> Self {
        let mut table = TableState::default();
        table.select(Some(0));
        let marked_at = core.path.borrow().absolute_path();
        Self {
            core,
            table,
            drawn: Vec::new(),
            marked: BTreeSet::new(),
            marked_at,
        }
    }

    pub(crate) fn rows(&self) -> Vec<Row> {
        let path = self.core.path.borrow();
        let mut rows = Vec::new();
        if path.depth() > 1 {
            rows.push(Row {
                name: "..".into(),
                is_dir: true,
                is_parent: true,
                size: 0,
                modified: 0,
                extra: Vec::new(),
            });
        }
        let held = path.active().entries();
        let mut entries: Vec<&RemoteFileEntry> = held.iter().collect();
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
            extra: e.extra.clone(),
        }));
        rows
    }

    pub(crate) fn shown_columns(&self) -> Vec<ColumnSpec> {
        self.core.path.borrow().active().shown_as().columns
    }

    pub(crate) fn columns_replace_defaults(&self) -> bool {
        self.core
            .path
            .borrow()
            .active()
            .shown_as()
            .columns_replace_defaults
    }

    pub(crate) fn tick_column(&self) -> Option<String> {
        self.shown_columns()
            .into_iter()
            .find(|spec| spec.kind == ColumnKind::Check)
            .map(|spec| spec.key)
    }

    pub(crate) fn tick_under(&self, x: u16) -> Option<String> {
        self.drawn.iter().find_map(|c| match &c.shows {
            Shows::Cell {
                key,
                kind: ColumnKind::Check,
                ..
            } if x >= c.x && x < c.x + c.width => Some(key.clone()),
            _ => None,
        })
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

    pub(crate) fn forget_marks_if_moved(&mut self) {
        let here = self.core.path.borrow().absolute_path();
        if here != self.marked_at {
            self.marked.clear();
            self.marked_at = here;
        }
    }

    pub(crate) fn toggle_mark(&mut self) {
        self.forget_marks_if_moved();
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.is_parent {
            return;
        }
        if !self.marked.remove(&row.name) {
            self.marked.insert(row.name);
        }
    }

    pub(crate) fn marked_now(&self) -> Vec<String> {
        let rows = self.rows();
        rows.into_iter()
            .filter(|r| !r.is_parent && self.marked.contains(&r.name))
            .map(|r| r.name)
            .collect()
    }

    /// Marked rows, or the row under the cursor when none are marked.
    pub(crate) fn chosen(&self) -> Vec<Row> {
        let rows = self.rows();
        let marked: Vec<Row> = rows
            .iter()
            .filter(|r| !r.is_parent && self.marked.contains(&r.name))
            .cloned()
            .collect();
        if !marked.is_empty() {
            return marked;
        }
        rows.into_iter()
            .nth(self.cursor())
            .filter(|r| !r.is_parent)
            .into_iter()
            .collect()
    }

    /// The same, spelled as the user sees it: a mount strips its own prefix off this.
    pub(crate) fn picked(&self) -> Vec<ic_plugin_host::SelectedItem> {
        let here = self.core.path.borrow().absolute_path();
        self.chosen()
            .into_iter()
            .map(|r| {
                let mut parts = fm_core::path::split_joined(&here);
                parts.push(r.name.as_str());
                let path = fm_core::path::join_segment_names(&parts);
                ic_plugin_host::SelectedItem {
                    key: path.clone(),
                    path,
                    is_dir: r.is_dir,
                }
            })
            .collect()
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

    /// Unlike `select_name`, a row that is gone leaves the cursor where it is.
    pub(crate) fn keep_cursor_on(&mut self, name: &str) {
        if let Some(at) = self.rows().iter().position(|r| r.name == name) {
            self.table.select(Some(at));
        }
    }
}

const SP: u16 = 3;
const SIZE_W: u16 = 9;
const DATE_W: u16 = 16;
const NAME_MIN: u16 = 12;
const TICK_W: u16 = 3;

fn plugin_width(spec: &ColumnSpec) -> u16 {
    let titled = ic_i18n::tr(&spec.title).chars().count() as u16;
    if spec.kind == ColumnKind::Check {
        return titled.clamp(TICK_W, 12);
    }
    match spec.width {
        // A plugin measures its columns in pixels, which say nothing here.
        Some(px) => (px / 8).clamp(6, 20) as u16,
        None => titled.clamp(6, 20),
    }
}

pub(crate) fn layout_columns(
    inner_width: u16,
    specs: &[ColumnSpec],
    replace_defaults: bool,
) -> Vec<DrawnColumn> {
    let mut tail: Vec<(u16, Shows)> = Vec::new();
    if !replace_defaults {
        tail.push((SIZE_W, Shows::Size));
        tail.push((DATE_W, Shows::Modified));
    }
    for (at, spec) in specs.iter().enumerate() {
        tail.push((
            plugin_width(spec),
            Shows::Cell {
                at,
                key: spec.key.clone(),
                kind: spec.kind,
            },
        ));
    }

    let room = |tail: &[(u16, Shows)]| -> Option<u16> {
        let taken: u16 = tail.iter().map(|(w, _)| *w).sum::<u16>() + SP * tail.len() as u16;
        inner_width
            .checked_sub(taken)
            .filter(|left| *left >= NAME_MIN)
    };
    let name_w = loop {
        if let Some(left) = room(&tail) {
            break left;
        }
        if let Some(i) = tail.iter().position(|(_, s)| *s == Shows::Modified) {
            tail.remove(i);
        } else if let Some(i) = tail.iter().position(|(_, s)| *s == Shows::Size) {
            tail.remove(i);
        } else if tail.pop().is_none() {
            break inner_width.max(1);
        }
    };

    let mut drawn = vec![DrawnColumn {
        x: 0,
        width: name_w,
        shows: Shows::Name,
    }];
    let mut x = name_w;
    for (width, shows) in tail {
        x += SP;
        drawn.push(DrawnColumn { x, width, shows });
        x += width;
    }
    drawn
}

fn header_of(column: &DrawnColumn, specs: &[ColumnSpec]) -> String {
    match &column.shows {
        Shows::Name => "Name".to_string(),
        Shows::Size => "Size".to_string(),
        Shows::Modified => "Modified".to_string(),
        Shows::Cell { at, .. } => specs
            .get(*at)
            .map(|spec| ic_i18n::tr(&spec.title))
            .unwrap_or_default(),
    }
}

fn cell_of(row: &Row, column: &DrawnColumn) -> (String, Style) {
    let dim = Style::default().fg(Color::DarkGray);
    match &column.shows {
        Shows::Name => {
            let text = if row.is_dir && !row.is_parent {
                format!("{}/", row.name)
            } else {
                row.name.clone()
            };
            (text, Style::default())
        }
        Shows::Size => {
            let size = if row.is_dir {
                "[DIR]".to_string()
            } else {
                human_size(row.size)
            };
            (format!("{size:>9}"), dim)
        }
        Shows::Modified => (fmt_time(row.modified), dim),
        Shows::Cell { .. } if row.is_parent => (String::new(), dim),
        Shows::Cell {
            at,
            kind: ColumnKind::Check,
            ..
        } => {
            let held = row.extra.get(*at).map(String::as_str).unwrap_or_default();
            let box_ = if fm_core::rpc::cell_is_ticked(held) {
                "[x]"
            } else {
                "[ ]"
            };
            (box_.to_string(), Style::default().fg(Color::Green))
        }
        Shows::Cell { at, .. } => (
            row.extra.get(*at).cloned().unwrap_or_default(),
            Style::default().fg(Color::Gray),
        ),
    }
}

pub(crate) fn draw_pane(f: &mut Frame, area: Rect, pane: &mut Pane, active: bool) {
    pane.forget_marks_if_moved();
    let rows = pane.rows();
    let len = rows.len();
    if pane.cursor() >= len {
        pane.table.select(Some(len.saturating_sub(1)));
    }
    let specs = pane.shown_columns();
    let replace_defaults = pane.columns_replace_defaults();

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
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style);
    let inner = block.inner(area);

    let columns = layout_columns(inner.width, &specs, replace_defaults);
    let table_rows: Vec<TableRow> = rows
        .iter()
        .map(|r| {
            let marked = !r.is_parent && pane.marked.contains(&r.name);
            let name_style = if marked {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if r.is_dir {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            TableRow::new(
                columns
                    .iter()
                    .map(|c| {
                        let (text, style) = cell_of(r, c);
                        let style = match c.shows {
                            Shows::Name => name_style,
                            _ => style,
                        };
                        Cell::from(text).style(style)
                    })
                    .collect::<Vec<Cell>>(),
            )
        })
        .collect();

    let highlight = if active {
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    };
    let header = TableRow::new(
        columns
            .iter()
            .map(|c| header_of(c, &specs))
            .collect::<Vec<String>>(),
    )
    .style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let widths: Vec<Constraint> = columns
        .iter()
        .map(|c| Constraint::Length(c.width))
        .collect();
    let table = Table::new(table_rows, widths)
        .header(header)
        .block(block)
        .column_spacing(SP)
        .row_highlight_style(highlight);
    f.render_stateful_widget(table, area, &mut pane.table);

    let buf = f.buffer_mut();
    for c in columns.iter().take(columns.len().saturating_sub(1)) {
        let sx = inner.x + c.x + c.width + SP / 2;
        if sx >= inner.x + inner.width {
            continue;
        }
        for y in inner.y..inner.y.saturating_add(inner.height) {
            if let Some(cell) = buf.cell_mut((sx, y)) {
                cell.set_symbol("│");
                cell.set_fg(Color::DarkGray);
            }
        }
    }

    pane.drawn = columns
        .into_iter()
        .map(|c| DrawnColumn {
            x: inner.x + c.x,
            ..c
        })
        .collect();
}

pub(crate) fn draw_status(f: &mut Frame, area: Rect, pane: &Pane) {
    let text = match pane.selected_row() {
        Some(r) if r.is_parent => "..".to_string(),
        Some(r) if r.is_dir => format!("{}    <DIR>", r.name),
        Some(r) => format!(
            "{}    {}    {}",
            r.name,
            human_size(r.size),
            fmt_time(r.modified)
        ),
        None => String::new(),
    };
    let marked = pane.marked_now().len();
    let tail = if marked > 0 {
        format!("    [{marked} marked]")
    } else {
        String::new()
    };
    let bar = Style::default().bg(Color::Rgb(30, 34, 40)).fg(Color::White);
    f.render_widget(Paragraph::new(format!(" {text}{tail}")).style(bar), area);
}

#[cfg(test)]
mod what_the_panel_hands_over {
    use super::*;
    use fm_core::rpc::FileSystemRpc;

    struct Mount {
        id: &'static str,
        columns: Vec<ColumnSpec>,
        rows: Vec<RemoteFileEntry>,
    }

    fn file(name: &str, extra: &[&str]) -> RemoteFileEntry {
        RemoteFileEntry {
            name: name.to_string(),
            is_dir: false,
            size: 7,
            modified: 0,
            permissions: None,
            extra: extra.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Mount {
        async fn list_dir(&self, _: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
            Ok(self.rows.clone())
        }
        fn fs_id(&self) -> String {
            self.id.to_string()
        }
        fn extra_columns(&self) -> Vec<ColumnSpec> {
            self.columns.clone()
        }
    }

    fn tick(key: &str) -> ColumnSpec {
        ColumnSpec {
            key: key.to_string(),
            title: "Fetch".to_string(),
            width: Some(60),
            kind: ColumnKind::Check,
        }
    }

    fn text(key: &str, px: i32) -> ColumnSpec {
        ColumnSpec {
            key: key.to_string(),
            title: "torrent.col_status".to_string(),
            width: Some(px),
            kind: ColumnKind::Text,
        }
    }

    /// A panel standing inside `/film.torrent`, listed and ready to draw.
    async fn inside_a_mount(id: &'static str, columns: Vec<ColumnSpec>) -> Pane {
        fm_core::listing::forget_everything();
        let mount: Rc<dyn FileSystemRpc> = Rc::new(Mount {
            id,
            columns,
            rows: vec![
                file("one.flac", &["1", "42%"]),
                file("two.flac", &["", "idle"]),
            ],
        });
        let mut path =
            panel_core::nav::NavPath::new(panel_core::nav::PathLevel::new("", "/", mount.clone()));
        path.push(panel_core::nav::PathLevel::new(
            "film.torrent",
            "/film.torrent",
            mount.clone(),
        ));
        fm_core::listing::list(&mount, "/film.torrent", fm_core::listing::Freshness::Fresh)
            .await
            .expect("a listing");
        let core = Rc::new(RouterState::new(
            mount.clone(),
            mount.clone(),
            "/".to_string(),
        ));
        *core.path.borrow_mut() = path;
        Pane::new(core)
    }

    fn paths(pane: &Pane) -> Vec<String> {
        pane.picked().into_iter().map(|i| i.path).collect()
    }

    #[tokio::test]
    async fn the_row_under_the_cursor_is_what_a_plugin_sees_when_nothing_is_marked() {
        let mut pane = inside_a_mount("marks/cursor", Vec::new()).await;
        pane.table.select(Some(1));
        assert_eq!(paths(&pane), ["/film.torrent/one.flac"]);
        pane.table.select(Some(2));
        assert_eq!(paths(&pane), ["/film.torrent/two.flac"]);
    }

    #[tokio::test]
    async fn marked_rows_are_handed_over_instead_of_the_one_under_the_cursor() {
        let mut pane = inside_a_mount("marks/marked", Vec::new()).await;
        pane.table.select(Some(2));
        pane.toggle_mark();
        pane.table.select(Some(1));
        assert_eq!(paths(&pane), ["/film.torrent/two.flac"]);
        pane.table.select(Some(2));
        pane.toggle_mark();
        pane.table.select(Some(1));
        assert_eq!(
            paths(&pane),
            ["/film.torrent/one.flac"],
            "unmarking hands the cursor back"
        );
    }

    #[tokio::test]
    async fn the_way_up_is_never_handed_to_a_plugin() {
        let mut pane = inside_a_mount("marks/parent", Vec::new()).await;
        pane.table.select(Some(0));
        assert_eq!(pane.rows()[0].name, "..");
        pane.toggle_mark();
        assert!(paths(&pane).is_empty());
    }

    #[tokio::test]
    async fn marks_do_not_follow_the_panel_into_another_folder() {
        let mut pane = inside_a_mount("marks/moved", Vec::new()).await;
        pane.table.select(Some(1));
        pane.toggle_mark();
        assert_eq!(paths(&pane), ["/film.torrent/one.flac"]);

        let fs = pane.core.path.borrow().active().fs.clone();
        fm_core::listing::list(
            &fs,
            "/film.torrent/disc",
            fm_core::listing::Freshness::Fresh,
        )
        .await
        .expect("a listing");
        pane.core
            .path
            .borrow_mut()
            .push(panel_core::nav::PathLevel::new(
                "disc",
                "/film.torrent/disc",
                fs,
            ));
        pane.forget_marks_if_moved();

        assert!(pane.marked_now().is_empty(), "a mark stayed behind");
        assert_eq!(paths(&pane), ["/film.torrent/disc/one.flac"]);
    }

    #[tokio::test]
    async fn a_column_of_the_plugins_own_travels_with_the_row() {
        let pane = inside_a_mount(
            "cols/extra",
            vec![tick("fetch"), text("torrent_status", 110)],
        )
        .await;
        let rows = pane.rows();
        assert_eq!(rows[1].extra, ["1", "42%"]);
        assert_eq!(pane.shown_columns().len(), 2);
        assert_eq!(pane.tick_column().as_deref(), Some("fetch"));
    }

    #[test]
    fn a_plugins_columns_are_laid_out_after_the_panels_own() {
        let specs = vec![tick("fetch"), text("torrent_status", 110)];
        let drawn = layout_columns(70, &specs, false);
        assert_eq!(
            drawn.iter().map(|c| c.width).collect::<Vec<_>>(),
            [15, SIZE_W, DATE_W, 5, 13]
        );
        assert_eq!(drawn[3].x, 15 + SP + SIZE_W + SP + DATE_W + SP);
        assert!(matches!(
            drawn[3].shows,
            Shows::Cell {
                at: 0,
                kind: ColumnKind::Check,
                ..
            }
        ));
    }

    #[test]
    fn a_narrow_panel_gives_up_the_date_before_the_size() {
        let specs = vec![tick("fetch")];
        let shows = |w: u16| -> Vec<Shows> {
            layout_columns(w, &specs, false)
                .into_iter()
                .map(|c| c.shows)
                .collect()
        };
        assert!(shows(60).contains(&Shows::Modified));
        assert!(!shows(40).contains(&Shows::Modified));
        assert!(shows(40).contains(&Shows::Size));
        assert!(!shows(20).contains(&Shows::Size));
        assert!(shows(20).iter().any(|s| matches!(s, Shows::Cell { .. })));
    }

    #[test]
    fn a_folder_that_replaces_the_panels_columns_keeps_only_its_own() {
        let specs = vec![text("torrent_status", 110)];
        let drawn = layout_columns(60, &specs, true);
        assert_eq!(
            drawn.into_iter().map(|c| c.shows).collect::<Vec<_>>(),
            [
                Shows::Name,
                Shows::Cell {
                    at: 0,
                    key: "torrent_status".to_string(),
                    kind: ColumnKind::Text
                }
            ]
        );
    }

    #[tokio::test]
    async fn a_ticked_cell_is_drawn_where_the_panel_says_it_is() {
        let mut pane = inside_a_mount("cols/drawn", vec![tick("fetch")]).await;
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 8))
            .expect("a test terminal");
        terminal
            .draw(|f| draw_pane(f, f.area(), &mut pane, true))
            .expect("a frame");
        let buffer = terminal.backend().buffer().clone();
        let lines: Vec<Vec<String>> = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();
        let shown = lines
            .iter()
            .map(|line| line.concat())
            .collect::<Vec<String>>()
            .join("\n");
        let box_at = |line: &[String], want: &str| -> Option<u16> {
            (0..line.len())
                .find(|x| line[*x..].iter().take(3).cloned().collect::<String>() == want)
                .map(|x| x as u16)
        };
        let ticked = lines
            .iter()
            .find(|line| line.concat().contains("one.flac"))
            .unwrap_or_else(|| panic!("no row for the ticked file:\n{shown}"));
        assert!(
            lines.iter().any(|line| box_at(line, "[ ]").is_some()),
            "the file nobody asked for:\n{shown}"
        );

        let at = box_at(ticked, "[x]").unwrap_or_else(|| panic!("no box:\n{shown}"));
        let column = pane
            .drawn
            .iter()
            .find(|c| {
                matches!(
                    c.shows,
                    Shows::Cell {
                        kind: ColumnKind::Check,
                        ..
                    }
                )
            })
            .expect("a tick column on screen");
        assert_eq!(at, column.x, "a click would miss the box:\n{shown}");
        assert_eq!(pane.tick_under(at).as_deref(), Some("fetch"));
    }
}
