use ic_view::{resolve_text, visible, Document, InputVariant, Node, NodeKind, State};
use ratatui::layout::Rect;

pub(crate) type Tr<'a> = dyn Fn(&str) -> Option<String> + 'a;

const CELL_W: u32 = 8;
const CELL_H: u32 = 18;

pub(crate) fn cols(px: u32) -> u16 {
    px.div_ceil(CELL_W).min(u32::from(u16::MAX)) as u16
}

pub(crate) fn rows(px: u32) -> u16 {
    px.div_ceil(CELL_H).min(u32::from(u16::MAX)) as u16
}

/// How wide the title column of a labelled control is.
pub(crate) fn gutter(width: u16) -> u16 {
    (width * 2 / 5).clamp(8, 24).min(width.saturating_sub(4))
}

pub(crate) struct Placed<'a> {
    pub(crate) node: &'a Node,
    pub(crate) rect: Rect,
}

pub(crate) fn wrapped(text: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let limit = usize::from(width);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let extra = if line.is_empty() { 0 } else { 1 };
            if line.chars().count() + extra + word.chars().count() > limit && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

pub(crate) fn text_of(
    conditional: Option<&ic_view::Cond<ic_view::Text>>,
    state: &State,
    tr: &Tr,
) -> Option<String> {
    resolve_text(conditional, state, &|key| tr(key))
}

pub(crate) fn table_rows(
    node: &Node,
    state: &State,
) -> Vec<serde_json::Map<String, serde_json::Value>> {
    let Some(key) = node.rows_key.as_deref() else {
        return Vec::new();
    };
    state
        .data
        .get(key)
        .and_then(|raw| raw.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|row| row.as_object().cloned())
                .collect()
        })
        .unwrap_or_default()
}

fn padding(node: &Node) -> u16 {
    rows(node.padding.unwrap_or(0)).min(2)
}

/// The height the node wants, excluding its own top margin.
pub(crate) fn measure(node: &Node, state: &State, tr: &Tr, width: u16) -> u16 {
    if !visible(node.visible.as_ref(), state) {
        return 0;
    }
    let pad = padding(node);
    let inner = width.saturating_sub(pad * 2);
    let own = match node.kind() {
        NodeKind::View | NodeKind::Column => stacked(node, state, tr, inner),
        NodeKind::Row => split(
            node,
            Rect {
                x: 0,
                y: 0,
                width: inner,
                height: 0,
            },
        )
        .iter()
        .zip(node.children.iter())
        .map(|(slot, child)| measure(child, state, tr, slot.width))
        .max()
        .unwrap_or(0),
        NodeKind::Group => {
            let body = inner.saturating_sub(2);
            stacked(node, state, tr, body) + 2
        }
        NodeKind::Separator => 1,
        NodeKind::Text => {
            let shown = text_of(node.text.as_ref(), state, tr).unwrap_or_default();
            if node.wrap {
                wrapped(&shown, inner).len().max(1) as u16
            } else {
                1
            }
        }
        NodeKind::Icon => 1,
        // No pixels here: one line saying what is there, so the rest still lines up.
        NodeKind::Image | NodeKind::Media | NodeKind::Canvas => 1,
        NodeKind::Input => {
            if node.variant == InputVariant::Multiline {
                node.height.map(rows).unwrap_or(4).max(3)
            } else {
                1
            }
        }
        NodeKind::Switch | NodeKind::Choice | NodeKind::Button | NodeKind::Slider => 1,
        NodeKind::Table => {
            let body = table_rows(node, state).len() as u16;
            let cap = node.height.map(rows).unwrap_or(u16::MAX);
            1 + body.min(cap.saturating_sub(1))
        }
        NodeKind::Chart => {
            let plot = node.height.map(rows).unwrap_or(5).max(3);
            let caption = u16::from(node.caption.is_some());
            plot + caption
        }
        NodeKind::Unknown => 0,
    };
    own + pad * 2
}

fn stacked(parent: &Node, state: &State, tr: &Tr, width: u16) -> u16 {
    let spacing = rows(parent.spacing.unwrap_or(0));
    let mut total = 0u16;
    let mut first = true;
    for child in &parent.children {
        let height = measure(child, state, tr, width);
        if height == 0 {
            continue;
        }
        if !first {
            total = total.saturating_add(spacing);
        }
        total = total
            .saturating_add(rows(child.margin_top.unwrap_or(0)))
            .saturating_add(height);
        first = false;
    }
    total
}

/// Shares a row's width by `weight`, falling back to an even split.
pub(crate) fn split(parent: &Node, area: Rect) -> Vec<Rect> {
    let spacing = cols(parent.spacing.unwrap_or(0)).min(2);
    let gaps = spacing.saturating_mul(parent.children.len().saturating_sub(1) as u16);
    let usable = area.width.saturating_sub(gaps);
    let fixed: Vec<Option<u16>> = parent
        .children
        .iter()
        .map(|child| child.width.map(cols))
        .collect();
    let spoken: u16 = fixed.iter().filter_map(|w| *w).sum();
    let free = usable.saturating_sub(spoken);
    let weights: Vec<u32> = parent
        .children
        .iter()
        .zip(fixed.iter())
        .map(|(child, width)| {
            if width.is_some() {
                0
            } else {
                child.weight.unwrap_or(1).max(1)
            }
        })
        .collect();
    let sum: u32 = weights.iter().sum();
    let mut slots = Vec::with_capacity(parent.children.len());
    let mut x = area.x;
    let mut spent = 0u16;
    let flexible = weights.iter().filter(|w| **w > 0).count();
    let mut placed = 0usize;
    for (index, width) in fixed.iter().enumerate() {
        let share = match width {
            Some(fixed) => *fixed,
            None => {
                placed += 1;
                if placed == flexible {
                    free.saturating_sub(spent)
                } else {
                    let part = if sum == 0 {
                        0
                    } else {
                        (u32::from(free) * weights[index] / sum) as u16
                    };
                    spent = spent.saturating_add(part);
                    part
                }
            }
        };
        slots.push(Rect {
            x,
            width: share,
            ..area
        });
        x = x.saturating_add(share).saturating_add(spacing);
    }
    slots
}

pub(crate) fn place<'a>(
    node: &'a Node,
    rect: Rect,
    state: &State,
    tr: &Tr,
    out: &mut Vec<Placed<'a>>,
) {
    if rect.width == 0 || rect.height == 0 || !visible(node.visible.as_ref(), state) {
        return;
    }
    let pad = padding(node);
    let inner = Rect {
        x: rect.x.saturating_add(pad),
        y: rect.y.saturating_add(pad),
        width: rect.width.saturating_sub(pad * 2),
        height: rect.height.saturating_sub(pad * 2),
    };
    match node.kind() {
        NodeKind::View | NodeKind::Column => {
            out.push(Placed { node, rect });
            stack_place(node, inner, state, tr, out);
        }
        NodeKind::Row => {
            out.push(Placed { node, rect });
            for (child, slot) in node.children.iter().zip(split(node, inner)) {
                let height = measure(child, state, tr, slot.width).min(slot.height);
                place(child, Rect { height, ..slot }, state, tr, out);
            }
        }
        NodeKind::Group => {
            out.push(Placed { node, rect });
            let body = Rect {
                x: inner.x.saturating_add(1),
                y: inner.y.saturating_add(1),
                width: inner.width.saturating_sub(2),
                height: inner.height.saturating_sub(2),
            };
            stack_place(node, body, state, tr, out);
        }
        NodeKind::Unknown => {}
        _ => out.push(Placed { node, rect: inner }),
    }
}

fn stack_place<'a>(
    parent: &'a Node,
    area: Rect,
    state: &State,
    tr: &Tr,
    out: &mut Vec<Placed<'a>>,
) {
    let spacing = rows(parent.spacing.unwrap_or(0));
    let mut cursor = area;
    let mut first = true;
    for child in &parent.children {
        let wanted = measure(child, state, tr, cursor.width);
        if wanted == 0 {
            continue;
        }
        let lead = rows(child.margin_top.unwrap_or(0)) + if first { 0 } else { spacing };
        cursor.y = cursor.y.saturating_add(lead);
        cursor.height = cursor.height.saturating_sub(lead);
        if cursor.height == 0 {
            break;
        }
        let height = wanted.min(cursor.height);
        place(child, Rect { height, ..cursor }, state, tr, out);
        cursor.y = cursor.y.saturating_add(height);
        cursor.height = cursor.height.saturating_sub(height);
        first = false;
        if cursor.height == 0 {
            break;
        }
    }
}

pub(crate) fn wanted_size(
    document: &Document,
    state: &State,
    tr: &Tr,
    area: Rect,
    has_actions: bool,
) -> (u16, u16) {
    let width = document
        .form
        .width
        .map(cols)
        .unwrap_or(72)
        .clamp(30, area.width.saturating_sub(4).max(30));
    let body = measure(&document.form, state, tr, width.saturating_sub(2));
    let height = (body + u16::from(has_actions) + 2).clamp(5, area.height.saturating_sub(2).max(5));
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plain(key: &str) -> Option<String> {
        let _ = key;
        None
    }

    fn node(raw: serde_json::Value) -> Node {
        serde_json::from_value(raw).expect("a node")
    }

    #[test]
    fn a_pixel_size_becomes_whole_cells() {
        assert_eq!(cols(0), 0);
        assert_eq!(cols(1), 1);
        assert_eq!(cols(8), 1);
        assert_eq!(cols(9), 2);
        assert_eq!(rows(18), 1);
        assert_eq!(rows(150), 9);
    }

    #[test]
    fn a_field_whose_condition_is_false_takes_no_room_at_all() {
        let form = node(json!({
            "t": "column",
            "children": [
                { "t": "input", "id": "user", "bind": "user", "title": "User" },
                { "t": "input", "id": "key", "bind": "key_path", "title": "Key",
                  "visible": { "eq": ["state.auth_type", "key"] } },
            ],
        }));
        let tr: &Tr = &plain;
        let mut state = State::default();
        assert_eq!(
            measure(&form, &state, tr, 60),
            1,
            "only the visible row counts"
        );

        state.set_state("auth_type", json!("key"));
        assert_eq!(measure(&form, &state, tr, 60), 2, "the condition now holds");

        let mut placed = Vec::new();
        place(
            &form,
            Rect {
                x: 0,
                y: 0,
                width: 60,
                height: 10,
            },
            &state,
            tr,
            &mut placed,
        );
        let ids: Vec<&str> = placed
            .iter()
            .filter_map(|spot| spot.node.id.as_deref())
            .collect();
        assert_eq!(ids, vec!["user", "key"]);
        let key = placed
            .iter()
            .find(|spot| spot.node.id.as_deref() == Some("key"))
            .expect("placed");
        assert_eq!(key.rect.y, 1, "it sits under the row above it");
    }

    #[test]
    fn a_row_shares_its_width_by_weight_and_honours_a_fixed_one() {
        let row = node(json!({
            "t": "row",
            "children": [
                { "t": "input", "id": "host", "bind": "host", "weight": 3 },
                { "t": "input", "id": "port", "bind": "port", "width": 80 },
            ],
        }));
        let slots = split(
            &row,
            Rect {
                x: 0,
                y: 0,
                width: 40,
                height: 1,
            },
        );
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[1].width, 10, "80px is ten cells wide");
        assert_eq!(slots[0].width, 30, "the flexible child takes the rest");
        assert_eq!(slots[1].x, 30);
    }

    #[test]
    fn a_wrapped_label_grows_to_the_lines_it_needs() {
        assert_eq!(wrapped("one two three", 20), vec!["one two three"]);
        assert_eq!(wrapped("one two three", 8), vec!["one two", "three"]);
        assert_eq!(wrapped("", 8), vec![""]);
        let label = node(json!({
            "t": "text", "id": "hint", "wrap": true,
            "text": "a hint long enough to need two lines",
        }));
        let tr: &Tr = &plain;
        assert_eq!(measure(&label, &State::default(), tr, 20), 2);
        assert_eq!(measure(&label, &State::default(), tr, 80), 1);
    }

    #[test]
    fn a_group_spends_two_rows_on_its_frame() {
        let group = node(json!({
            "t": "group", "title": "Tunnel",
            "children": [{ "t": "input", "id": "h", "bind": "tunnel_host" }],
        }));
        let tr: &Tr = &plain;
        assert_eq!(measure(&group, &State::default(), tr, 40), 3);
    }

    #[test]
    fn a_table_is_a_header_plus_the_rows_the_data_carries() {
        let table = node(json!({
            "t": "table", "id": "procs", "rows_key": "rows",
            "columns": [{ "key": "pid" }, { "key": "name" }],
        }));
        let tr: &Tr = &plain;
        let mut state = State::default();
        assert_eq!(
            measure(&table, &state, tr, 40),
            1,
            "a header and nothing else"
        );
        state.data.insert(
            "rows".to_string(),
            json!([{ "pid": 1, "name": "init" }, { "pid": 2, "name": "sh" }]),
        );
        assert_eq!(measure(&table, &state, tr, 40), 3);
        assert_eq!(table_rows(&table, &state).len(), 2);
    }
}
