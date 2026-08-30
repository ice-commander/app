
use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const PER_ROW: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Hex,
    Ascii,
}

struct Cursor {
    byte: usize,
    low_nibble: bool,
    column: Column,
}

pub(super) struct HexView {
    pub widget: gtk::Widget,
    list: gtk::ListView,
    model: gtk::StringList,
    bytes: Rc<RefCell<Vec<u8>>>,
    original: Rc<Vec<u8>>,
    cursor: Rc<RefCell<Cursor>>,
    editable: Rc<Cell<bool>>,
    on_changed: Rc<dyn Fn()>,
    status: gtk::Label,
}

fn rows_for(len: usize) -> usize {
    len.div_ceil(PER_ROW).max(1)
}

fn escape(c: char) -> String {
    match c {
        '&' => "&amp;".to_string(),
        '<' => "&lt;".to_string(),
        '>' => "&gt;".to_string(),
        _ => c.to_string(),
    }
}

fn row_markup(
    row: usize,
    bytes: &[u8],
    original: &[u8],
    cursor: &Cursor,
    focused: bool,
) -> String {
    let start = row * PER_ROW;
    let mut out = format!("<span alpha='60%'>{:08x}</span>  ", start);

    for i in 0..PER_ROW {
        let idx = start + i;
        if i == 8 {
            out.push(' ');
        }
        match bytes.get(idx) {
            Some(byte) => {
                let changed = original.get(idx) != Some(byte);
                let on_cursor = focused && idx == cursor.byte;
                let text = format!("{byte:02x}");
                if on_cursor && cursor.column == Column::Hex {
                    let (head, tail) = text.split_at(1);
                    let (a, b) = if cursor.low_nibble {
                        (head.to_string(), format!("<span background='#3584e4' foreground='#ffffff'>{tail}</span>"))
                    } else {
                        (format!("<span background='#3584e4' foreground='#ffffff'>{head}</span>"), tail.to_string())
                    };
                    out.push_str(&format!("{a}{b} "));
                } else if on_cursor {
                    out.push_str(&format!("<span background='#3584e4' foreground='#ffffff'>{text}</span> "));
                } else if changed {
                    out.push_str(&format!("<span foreground='#e66100'><b>{text}</b></span> "));
                } else {
                    out.push_str(&format!("{text} "));
                }
            }
            None => out.push_str("   "),
        }
    }

    out.push_str(" |");
    for i in 0..PER_ROW {
        let idx = start + i;
        let Some(byte) = bytes.get(idx) else {
            break;
        };
        let ch = if (32..=126).contains(byte) { *byte as char } else { '.' };
        let text = escape(ch);
        let changed = original.get(idx) != Some(byte);
        if focused && idx == cursor.byte && cursor.column == Column::Ascii {
            out.push_str(&format!("<span background='#3584e4' foreground='#ffffff'>{text}</span>"));
        } else if changed {
            out.push_str(&format!("<span foreground='#e66100'><b>{text}</b></span>"));
        } else {
            out.push_str(&text);
        }
    }
    out.push('|');
    out
}

impl HexView {
    pub fn new(bytes: Vec<u8>, on_changed: Rc<dyn Fn()>) -> Rc<Self> {
        let original = Rc::new(bytes.clone());
        let bytes = Rc::new(RefCell::new(bytes));
        let model = gtk::StringList::new(&[]);
        for row in 0..rows_for(bytes.borrow().len()) {
            model.append(&row.to_string());
        }

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let label = gtk::Label::builder()
                .use_markup(true)
                .halign(gtk::Align::Start)
                .xalign(0.0)
                .margin_start(8)
                .margin_end(8)
                .build();
            label.add_css_class("hex-row");
            item.set_child(Some(&label));
        });

        let list = gtk::ListView::new(
            Some(gtk::NoSelection::new(Some(model.clone()))),
            Some(factory.clone()),
        );
        list.set_single_click_activate(false);
        list.set_can_focus(true);
        list.add_css_class("hex-view");

        let status = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .margin_start(12)
            .margin_end(12)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        status.add_css_class("dim-label");

        let scrolled = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&list)
            .build();

        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.append(&scrolled);
        column.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        column.append(&status);

        let view = Rc::new(Self {
            widget: column.upcast(),
            list: list.clone(),
            model,
            bytes,
            original,
            cursor: Rc::new(RefCell::new(Cursor { byte: 0, low_nibble: false, column: Column::Hex })),
            editable: Rc::new(Cell::new(false)),
            on_changed,
            status,
        });

        {
            let view = view.clone();
            factory.connect_bind(move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let Some(label) = item.child().and_downcast::<gtk::Label>() else {
                    return;
                };
                view.render_row(item.position() as usize, &label);
            });
        }

        {
            let view = view.clone();
            let controller = gtk::EventControllerKey::new();
            controller.connect_key_pressed(move |_, keyval, _, state| view.on_key(keyval, state));
            list.add_controller(controller);
        }

        {
            let view = view.clone();
            let click = gtk::GestureClick::new();
            click.connect_pressed(move |gesture, _, _, _| {
                if let Some(widget) = gesture.widget() {
                    widget.grab_focus();
                }
                view.refresh_visible();
            });
            list.add_controller(click);
        }

        view.update_status();
        view
    }

    fn render_row(&self, row: usize, label: &gtk::Label) {
        let markup = row_markup(
            row,
            &self.bytes.borrow(),
            &self.original,
            &self.cursor.borrow(),
            self.list.has_focus() || self.editable.get(),
        );
        label.set_markup(&markup);
    }

    fn refresh_row(&self, row: usize) {
        if row < self.model.n_items() as usize {
            self.model.splice(row as u32, 1, &[&row.to_string()]);
        }
    }

    fn refresh_visible(&self) {
        let row = self.cursor.borrow().byte / PER_ROW;
        self.refresh_row(row);
        self.update_status();
    }

    pub fn set_editable(&self, editable: bool) {
        self.editable.set(editable);
        self.refresh_visible();
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.bytes.borrow().clone()
    }

    pub fn set_bytes(&self, new_bytes: &[u8]) {
        *self.bytes.borrow_mut() = new_bytes.to_vec();
        let rows = rows_for(new_bytes.len());
        let all: Vec<String> = (0..rows).map(|r| r.to_string()).collect();
        let refs: Vec<&str> = all.iter().map(|s| s.as_str()).collect();
        self.model.splice(0, self.model.n_items(), &refs);
        self.cursor.borrow_mut().byte = 0;
        self.update_status();
    }

    fn update_status(&self) {
        let cursor = self.cursor.borrow();
        let bytes = self.bytes.borrow();
        let text = match bytes.get(cursor.byte) {
            Some(byte) => {
                let printable = if (32..=126).contains(byte) {
                    format!(" '{}'", *byte as char)
                } else {
                    String::new()
                };
                crate::i18n::trf(
                    "hex.status",
                    &[
                        ("offset", &format!("0x{:08X}", cursor.byte)),
                        ("hex", &format!("0x{byte:02X}{printable}")),
                        ("dec", &byte.to_string()),
                        ("size", &bytes.len().to_string()),
                    ],
                )
            }
            None => crate::i18n::trf("hex.status_empty", &[("size", &bytes.len().to_string())]),
        };
        self.status.set_text(&text);
    }

    fn move_cursor(&self, delta: isize) {
        let len = self.bytes.borrow().len();
        if len == 0 {
            return;
        }
        let old_row = self.cursor.borrow().byte / PER_ROW;
        {
            let mut cursor = self.cursor.borrow_mut();
            let target = cursor.byte as isize + delta;
            cursor.byte = target.clamp(0, len as isize - 1) as usize;
            cursor.low_nibble = false;
        }
        let new_row = self.cursor.borrow().byte / PER_ROW;
        self.refresh_row(old_row);
        if new_row != old_row {
            self.refresh_row(new_row);
            self.list.scroll_to(new_row as u32, gtk::ListScrollFlags::empty(), None);
        }
        self.update_status();
    }

    pub fn go_to_offset(&self, offset: usize) {
        let len = self.bytes.borrow().len();
        if len == 0 {
            return;
        }
        let old_row = self.cursor.borrow().byte / PER_ROW;
        {
            let mut cursor = self.cursor.borrow_mut();
            cursor.byte = offset.min(len - 1);
            cursor.low_nibble = false;
        }
        let row = self.cursor.borrow().byte / PER_ROW;
        self.refresh_row(old_row);
        self.refresh_row(row);
        self.list.scroll_to(row as u32, gtk::ListScrollFlags::empty(), None);
        self.update_status();
        self.list.grab_focus();
    }

    fn write_byte(&self, value: u8) {
        let row = {
            let cursor = self.cursor.borrow();
            let mut bytes = self.bytes.borrow_mut();
            let Some(slot) = bytes.get_mut(cursor.byte) else {
                return;
            };
            *slot = value;
            cursor.byte / PER_ROW
        };
        self.refresh_row(row);
        (self.on_changed)();
    }

    fn type_hex_digit(&self, digit: u8) {
        let (byte, low, index) = {
            let cursor = self.cursor.borrow();
            let bytes = self.bytes.borrow();
            let Some(byte) = bytes.get(cursor.byte).copied() else {
                return;
            };
            (byte, cursor.low_nibble, cursor.byte)
        };

        let updated = if low {
            (byte & 0xF0) | digit
        } else {
            (byte & 0x0F) | (digit << 4)
        };
        self.write_byte(updated);

        let len = self.bytes.borrow().len();
        let mut cursor = self.cursor.borrow_mut();
        if low {
            cursor.low_nibble = false;
            if index + 1 < len {
                cursor.byte = index + 1;
            }
        } else {
            cursor.low_nibble = true;
        }
        drop(cursor);
        self.refresh_visible();
    }

    fn type_ascii(&self, ch: char) {
        if !ch.is_ascii() {
            return;
        }
        self.write_byte(ch as u8);
        let len = self.bytes.borrow().len();
        {
            let mut cursor = self.cursor.borrow_mut();
            if cursor.byte + 1 < len {
                cursor.byte += 1;
            }
        }
        self.refresh_visible();
    }

    fn on_key(&self, keyval: gtk::gdk::Key, state: gtk::gdk::ModifierType) -> gtk::glib::Propagation {
        use gtk::gdk::Key;

        let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        if ctrl {
            return gtk::glib::Propagation::Proceed;
        }

        match keyval {
            Key::Left => self.move_cursor(-1),
            Key::Right => self.move_cursor(1),
            Key::Up => self.move_cursor(-(PER_ROW as isize)),
            Key::Down => self.move_cursor(PER_ROW as isize),
            Key::Page_Up => self.move_cursor(-(PER_ROW as isize) * 16),
            Key::Page_Down => self.move_cursor(PER_ROW as isize * 16),
            Key::Home => self.go_to_offset(0),
            Key::End => {
                let len = self.bytes.borrow().len();
                self.go_to_offset(len.saturating_sub(1));
            }
            Key::space if self.cursor.borrow().column == Column::Hex => self.move_cursor(1),
            Key::BackSpace => self.move_cursor(-1),
            Key::Tab => {
                {
                    let mut cursor = self.cursor.borrow_mut();
                    cursor.column = match cursor.column {
                        Column::Hex => Column::Ascii,
                        Column::Ascii => Column::Hex,
                    };
                    cursor.low_nibble = false;
                }
                self.refresh_visible();
            }
            _ => {
                if !self.editable.get() {
                    return gtk::glib::Propagation::Proceed;
                }
                let Some(ch) = keyval.to_unicode() else {
                    return gtk::glib::Propagation::Proceed;
                };
                let column = self.cursor.borrow().column;
                match column {
                    Column::Hex => match ch.to_digit(16) {
                        Some(digit) => self.type_hex_digit(digit as u8),
                        None => return gtk::glib::Propagation::Proceed,
                    },
                    Column::Ascii => {
                        if !(' '..='~').contains(&ch) {
                            return gtk::glib::Propagation::Proceed;
                        }
                        self.type_ascii(ch);
                    }
                }
            }
        }
        gtk::glib::Propagation::Stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_shows_offset_bytes_and_ascii() {
        let cursor = Cursor { byte: 99, low_nibble: false, column: Column::Hex };
        let markup = row_markup(0, b"AB", b"AB", &cursor, false);
        assert!(markup.contains("00000000"), "{markup}");
        assert!(markup.contains("41 42"), "{markup}");
        assert!(markup.ends_with("|AB|"), "{markup}");
    }

    #[test]
    fn an_edited_byte_is_highlighted() {
        let cursor = Cursor { byte: 99, low_nibble: false, column: Column::Hex };
        let markup = row_markup(0, b"AC", b"AB", &cursor, false);
        assert!(markup.contains("<span foreground='#e66100'><b>43</b></span>"), "{markup}");
    }

    #[test]
    fn markup_escapes_ascii_specials() {
        let cursor = Cursor { byte: 99, low_nibble: false, column: Column::Hex };
        let markup = row_markup(0, b"<&>", b"<&>", &cursor, false);
        assert!(markup.ends_with("|&lt;&amp;&gt;|"), "{markup}");
    }

    #[test]
    fn the_cursor_marks_the_nibble_it_will_replace() {
        let low = Cursor { byte: 0, low_nibble: true, column: Column::Hex };
        let markup = row_markup(0, b"A", b"A", &low, true);
        assert!(markup.contains("4<span background='#3584e4' foreground='#ffffff'>1</span>"), "{markup}");
    }

    #[test]
    fn rows_cover_every_byte() {
        assert_eq!(rows_for(0), 1);
        assert_eq!(rows_for(1), 1);
        assert_eq!(rows_for(16), 1);
        assert_eq!(rows_for(17), 2);
    }
}

pub(super) fn ask_for_offset(parent: &gtk::Window, view: Rc<HexView>) {
    let entry = gtk::Entry::builder()
        .placeholder_text(&*crate::i18n::tr("hex.offset_hint"))
        .activates_default(true)
        .build();

    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr("hex.go_to_offset"))
        .extra_child(&entry)
        .build();
    dialog.add_response("cancel", &crate::i18n::tr("editor.cancel"));
    dialog.add_response("go", &crate::i18n::tr("hex.go_to_offset"));
    dialog.set_default_response(Some("go"));

    let entry_response = entry.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "go" {
            return;
        }
        let text = entry_response.text().trim().to_string();
        let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            Some(hex) => usize::from_str_radix(hex, 16).ok(),
            None => text.parse::<usize>().ok(),
        };
        if let Some(offset) = parsed {
            view.go_to_offset(offset);
        }
    });
    dialog.present(Some(parent));
}
