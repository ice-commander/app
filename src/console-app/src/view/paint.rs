use ic_view::{
    as_text, choice_index, is_truthy, series_bounds, series_of, visible, Document, InputVariant,
    NodeKind, State,
};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use serde_json::Value;

use super::layout::{self, gutter, table_rows, text_of, wrapped, Placed, Tr};
use super::{Spot, Widgets};

const BARS: [char; 8] = [
    '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
];

pub(crate) struct Surface<'a> {
    pub(crate) state: &'a State,
    pub(crate) widgets: &'a Widgets,
    pub(crate) focus: Option<&'a Spot>,
    pub(crate) tr: &'a Tr<'a>,
}

fn text_style(role: Option<&str>) -> Style {
    match role {
        Some("title1") | Some("title2") | Some("heading") => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        Some("caption") | Some("dim") => Style::default().fg(Color::DarkGray),
        Some("error") => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::Gray),
    }
}

fn button_style(role: Option<&str>, focused: bool) -> Style {
    let base = match role {
        Some("primary") => Style::default().fg(Color::Black).bg(Color::Cyan),
        Some("destructive") => Style::default().fg(Color::White).bg(Color::Red),
        _ => Style::default().fg(Color::White).bg(Color::DarkGray),
    };
    if focused {
        base.add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        base
    }
}

impl Surface<'_> {
    fn is_focused(&self, id: Option<&String>) -> bool {
        match (self.focus, id) {
            (Some(Spot::Node(current)), Some(wanted)) => current == wanted,
            _ => false,
        }
    }

    fn shown(&self, bind: Option<&String>) -> Value {
        bind.and_then(|bind| self.widgets.value_of(bind))
            .unwrap_or(Value::Null)
    }

    fn title_and_box(&self, placed: &Placed, insensitive: bool) -> (Rect, Rect) {
        let label = gutter(placed.rect.width);
        let title = Rect {
            width: label,
            height: 1,
            ..placed.rect
        };
        let field = Rect {
            x: placed.rect.x.saturating_add(label),
            width: placed.rect.width.saturating_sub(label),
            ..placed.rect
        };
        let _ = insensitive;
        (title, field)
    }
}

/// How far along a slider is, as a part of one.
fn slider_part(node: &ic_view::Node, state: &ic_view::State) -> f64 {
    let low = node.min.unwrap_or(0.0);
    let high = node.max.unwrap_or(low + 1.0);
    let at = node
        .value_key
        .as_deref()
        .and_then(|key| state.data.get(key))
        .or_else(|| {
            node.bind
                .as_deref()
                .and_then(|bind| state.state.get(bind))
        })
        .and_then(|value| value.as_f64())
        .unwrap_or(low);
    if high > low {
        ((at - low) / (high - low)).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Nothing to take hold of in a terminal: it shows where it is, the document's keys move it.
fn draw_slider(f: &mut Frame, placed: &Placed, surface: &Surface) {
    let node = placed.node;
    let room = usize::from(placed.rect.width);
    if room == 0 {
        return;
    }
    let filled = ((slider_part(node, surface.state) * room as f64).round() as usize).min(room);
    let mut drawn = "\u{2501}".repeat(filled);
    drawn.push_str(&"\u{2500}".repeat(room - filled));
    let live = visible(node.sensitive.as_ref(), surface.state);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            drawn,
            Style::default().fg(if live { Color::White } else { Color::DarkGray }),
        ))),
        placed.rect,
    );
}

pub(crate) fn draw_placed(f: &mut Frame, placed: &Placed, surface: &Surface) {
    let node = placed.node;
    let state = surface.state;
    let tr = surface.tr;
    let live = visible(node.sensitive.as_ref(), state);
    let dim = if live {
        Style::default()
    } else {
        Style::default().fg(Color::DarkGray)
    };
    match node.kind() {
        NodeKind::Group => {
            let mut block =
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(if live {
                        Color::DarkGray
                    } else {
                        Color::Black
                    }));
            if let Some(title) = text_of(node.title.as_ref(), state, tr) {
                block = block.title(format!(" {title} "));
            }
            f.render_widget(block, placed.rect);
        }
        NodeKind::Separator => {
            let rule = "\u{2500}".repeat(usize::from(placed.rect.width));
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    rule,
                    Style::default().fg(Color::DarkGray),
                ))),
                placed.rect,
            );
        }
        NodeKind::Text => {
            let shown = text_of(node.text.as_ref(), state, tr).unwrap_or_default();
            let role = text_of(node.role.as_ref(), state, tr);
            let style = text_style(role.as_deref()).patch(dim);
            let lines: Vec<Line> = if node.wrap {
                wrapped(&shown, placed.rect.width)
                    .into_iter()
                    .map(|line| Line::from(Span::styled(line, style)))
                    .collect()
            } else {
                vec![Line::from(Span::styled(shown, style))]
            };
            f.render_widget(Paragraph::new(lines), placed.rect);
        }
        NodeKind::Image | NodeKind::Media => {
            let named = text_of(node.src.as_ref(), state, tr).unwrap_or_default();
            let named = named
                .strip_prefix("file:")
                .or_else(|| named.strip_prefix("part:"))
                .unwrap_or(&named);
            let mark = if node.kind() == NodeKind::Media {
                "\u{25b6}"
            } else {
                "\u{25a3}"
            };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("{mark} {named}"),
                    Style::default().fg(Color::DarkGray),
                ))),
                placed.rect,
            );
        }
        NodeKind::Canvas => {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "\u{25a6}",
                    Style::default().fg(Color::DarkGray),
                ))),
                placed.rect,
            );
        }
        NodeKind::Input => draw_input(f, placed, surface, live),
        NodeKind::Switch => {
            let (title, field) = surface.title_and_box(placed, !live);
            if let Some(label) = text_of(node.title.as_ref(), state, tr) {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        label,
                        Style::default().fg(Color::Gray).patch(dim),
                    ))),
                    title,
                );
            }
            let on = is_truthy(&surface.shown(node.bind.as_ref()));
            let mark = if on { "[\u{2714}]" } else { "[ ]" };
            let mut style = Style::default().fg(if on { Color::Green } else { Color::Gray });
            if surface.is_focused(node.id.as_ref()) {
                style = style.add_modifier(Modifier::REVERSED);
            }
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(mark, style.patch(dim)))),
                field,
            );
        }
        NodeKind::Choice => {
            let (title, field) = surface.title_and_box(placed, !live);
            if let Some(label) = text_of(node.title.as_ref(), state, tr) {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        label,
                        Style::default().fg(Color::Gray).patch(dim),
                    ))),
                    title,
                );
            }
            let current = surface.shown(node.bind.as_ref());
            let label = choice_index(node, &current)
                .and_then(|index| node.options.get(index))
                .and_then(|option| option.label.as_ref())
                .map(|text| text.resolve(&|key| tr(key)))
                .unwrap_or_else(|| as_text(&current));
            let mut style = Style::default().fg(Color::White);
            if surface.is_focused(node.id.as_ref()) {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let room = usize::from(field.width).saturating_sub(4);
            let clipped: String = label.chars().take(room).collect();
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("\u{2039} {clipped:room$} \u{203a}"),
                    style.patch(dim),
                ))),
                field,
            );
        }
        NodeKind::Button => {
            let label = text_of(node.label.as_ref(), state, tr)
                .or_else(|| text_of(node.text.as_ref(), state, tr))
                .unwrap_or_default();
            let role = text_of(node.role.as_ref(), state, tr);
            let style = button_style(role.as_deref(), surface.is_focused(node.id.as_ref()));
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {label} "),
                    if live {
                        style
                    } else {
                        style.fg(Color::DarkGray)
                    },
                ))),
                placed.rect,
            );
        }
        NodeKind::Slider => draw_slider(f, placed, surface),
        NodeKind::Table => draw_table(f, placed, surface),
        NodeKind::Chart => draw_chart(f, placed, surface),
        NodeKind::View | NodeKind::Column | NodeKind::Row | NodeKind::Icon | NodeKind::Unknown => {}
    }
}

fn draw_input(f: &mut Frame, placed: &Placed, surface: &Surface, live: bool) {
    let node = placed.node;
    let state = surface.state;
    let tr = surface.tr;
    let focused = surface.is_focused(node.id.as_ref());
    let raw = as_text(&surface.shown(node.bind.as_ref()));
    let caret = node
        .id
        .as_ref()
        .map(|id| surface.widgets.caret_of(id))
        .unwrap_or_default();
    let masked = matches!(
        node.variant,
        InputVariant::Masked | InputVariant::MaskedReveal
    );
    let multiline = node.variant == InputVariant::Multiline;

    let (title, field) = if multiline {
        (
            Rect {
                height: 1,
                ..placed.rect
            },
            Rect {
                y: placed.rect.y.saturating_add(1),
                height: placed.rect.height.saturating_sub(1),
                ..placed.rect
            },
        )
    } else {
        surface.title_and_box(placed, !live)
    };
    if let Some(label) = text_of(node.title.as_ref(), state, tr) {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                label,
                Style::default().fg(if live { Color::Gray } else { Color::DarkGray }),
            ))),
            title,
        );
    }
    let mut style = Style::default().fg(Color::White).bg(Color::Black);
    if focused {
        style = style.fg(Color::Black).bg(Color::Cyan);
    } else if !live {
        style = style.fg(Color::DarkGray);
    }
    let empty = raw.is_empty();
    let hint = text_of(node.placeholder.as_ref(), state, tr).unwrap_or_default();

    if multiline {
        let lines: Vec<&str> = raw.split('\n').collect();
        let room = usize::from(field.height);
        let top = caret.cy.saturating_sub(room.saturating_sub(1));
        let shown: Vec<Line> = lines
            .iter()
            .skip(top)
            .take(room)
            .enumerate()
            .map(|(offset, line)| {
                let text: String = line.chars().take(usize::from(field.width)).collect();
                let row = top + offset;
                if focused && row == caret.cy {
                    Line::from(with_caret(&text, caret.cx, style))
                } else {
                    Line::from(Span::styled(text, style))
                }
            })
            .collect();
        f.render_widget(Clear, field);
        f.render_widget(Paragraph::new(shown).style(style), field);
        return;
    }

    let room = usize::from(field.width);
    if empty && !focused {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                hint.chars().take(room).collect::<String>(),
                Style::default().fg(Color::DarkGray),
            ))),
            field,
        );
        return;
    }
    let body: String = if masked {
        "\u{2022}".repeat(raw.chars().count())
    } else {
        raw
    };
    let start = caret.cx.saturating_sub(room.saturating_sub(1));
    let window: String = body
        .chars()
        .skip(start)
        .take(room)
        .chain(std::iter::repeat(' '))
        .take(room)
        .collect();
    let spans = if focused {
        with_caret(&window, caret.cx - start, style)
    } else {
        vec![Span::styled(window, style)]
    };
    f.render_widget(Paragraph::new(Line::from(spans)), field);
}

fn with_caret(text: &str, at: usize, style: Style) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let before: String = chars.iter().take(at).collect();
    let under = chars.get(at).copied().unwrap_or(' ');
    let after: String = chars.iter().skip(at + 1).collect();
    vec![
        Span::styled(before, style),
        Span::styled(
            under.to_string(),
            style.add_modifier(Modifier::REVERSED | Modifier::BOLD),
        ),
        Span::styled(after, style),
    ]
}

fn draw_table(f: &mut Frame, placed: &Placed, surface: &Surface) {
    let node = placed.node;
    let tr = surface.tr;
    let rows = table_rows(node, surface.state);
    if node.columns.is_empty() {
        return;
    }
    let spoken: u16 = node
        .columns
        .iter()
        .filter_map(|column| column.width.map(layout::cols))
        .sum();
    let loose = node
        .columns
        .iter()
        .filter(|c| c.width.is_none())
        .count()
        .max(1) as u16;
    let each = placed.rect.width.saturating_sub(spoken) / loose;
    let widths: Vec<u16> = node
        .columns
        .iter()
        .map(|column| column.width.map(layout::cols).unwrap_or(each).max(3))
        .collect();

    let header: Vec<Span> = node
        .columns
        .iter()
        .zip(widths.iter())
        .map(|(column, width)| {
            let title = column
                .title
                .as_ref()
                .map(|text| text.resolve(&|key| tr(key)))
                .unwrap_or_else(|| column.key.clone());
            Span::styled(
                cell(&title, *width),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        })
        .collect();
    let mut lines = vec![Line::from(header)];
    let room = usize::from(placed.rect.height).saturating_sub(1);
    for row in rows.iter().take(room) {
        let cells: Vec<Span> = node
            .columns
            .iter()
            .zip(widths.iter())
            .map(|(column, width)| {
                let raw = row.get(&column.key).cloned().unwrap_or(Value::Null);
                Span::styled(
                    cell(&ic_view::display(&raw, &|key| tr(key)), *width),
                    Style::default().fg(Color::Gray),
                )
            })
            .collect();
        lines.push(Line::from(cells));
    }
    f.render_widget(Paragraph::new(lines), placed.rect);
}

fn cell(text: &str, width: u16) -> String {
    let room = usize::from(width).saturating_sub(1);
    let mut shown: String = text.chars().take(room).collect();
    while shown.chars().count() < usize::from(width) {
        shown.push(' ');
    }
    shown
}

fn draw_chart(f: &mut Frame, placed: &Placed, surface: &Surface) {
    let node = placed.node;
    let tr = surface.tr;
    let Some(key) = node.series_key.as_deref() else {
        return;
    };
    let series = series_of(surface.state, key);
    let (low, high) = series_bounds(&series, node);
    let caption = text_of(node.caption.as_ref(), surface.state, tr);
    let plot = placed
        .rect
        .height
        .saturating_sub(u16::from(caption.is_some()));
    let mut lines: Vec<Line> = Vec::new();
    for (index, line) in series.iter().take(usize::from(plot)).enumerate() {
        let room = usize::from(placed.rect.width);
        let tail = line.values.len().saturating_sub(room);
        let drawn: String = line.values[tail..]
            .iter()
            .map(|value| {
                let share = ((value - low) / (high - low)).clamp(0.0, 1.0);
                let step = ((share * (BARS.len() - 1) as f64).round() as usize).min(BARS.len() - 1);
                BARS[step]
            })
            .collect();
        let hue = [Color::Cyan, Color::Green, Color::Yellow, Color::Magenta][index % 4];
        lines.push(Line::from(Span::styled(drawn, Style::default().fg(hue))));
    }
    if let Some(caption) = caption {
        lines.push(Line::from(Span::styled(
            caption,
            Style::default().fg(Color::DarkGray),
        )));
    }
    f.render_widget(Paragraph::new(lines), placed.rect);
}

pub(crate) fn draw_actions(
    f: &mut Frame,
    document: &Document,
    own: &[(&str, &str, &str)],
    area: Rect,
    surface: &Surface,
) -> Vec<(Rect, String)> {
    let mut offered: Vec<(String, String, Option<String>)> = Vec::new();
    for action in &document.actions {
        if !visible(action.visible.as_ref(), surface.state) {
            continue;
        }
        offered.push((
            action.id.clone(),
            text_of(action.label.as_ref(), surface.state, surface.tr)
                .unwrap_or_else(|| action.id.clone()),
            text_of(action.role.as_ref(), surface.state, surface.tr),
        ));
    }
    for (id, key, fallback) in own {
        let label = (surface.tr)(key).unwrap_or_else(|| (*fallback).to_string());
        let role = (*id == "@save").then(|| "primary".to_string());
        offered.push(((*id).to_string(), label, role));
    }
    let mut hits = Vec::new();
    let mut x = area.x;
    for (id, label, role) in offered {
        let focused = matches!(surface.focus, Some(Spot::Action(current)) if *current == id);
        let width = label.chars().count() as u16 + 4;
        if x.saturating_add(width) > area.x.saturating_add(area.width) {
            break;
        }
        let slot = Rect {
            x,
            width,
            height: 1,
            ..area
        };
        let style = button_style(role.as_deref(), focused);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(format!("[ {label} ]"), style))),
            slot,
        );
        hits.push((slot, id));
        x = x.saturating_add(width).saturating_add(1);
    }
    hits
}
