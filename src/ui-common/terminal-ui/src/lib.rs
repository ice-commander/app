#![allow(deprecated)]
mod i18n;

use adw::prelude::*;
use gtk::{cairo, gdk};
use relm4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Once;

const FONT_FAMILY: &str = if cfg!(target_os = "macos") {
    "Menlo"
} else if cfg!(target_os = "windows") {
    "Consolas"
} else {
    "Monospace"
};

static RESOURCES_INIT: Once = Once::new();

pub fn init_resources() {
    RESOURCES_INIT.call_once(|| {
        gtk::gio::resources_register_include!("terminal.gresource")
            .expect("Failed to register terminal-ui resources");
    });
}

const ROWS: u16 = 24;
const COLS: u16 = 80;
const FONT_SIZE: f64 = 14.0;

#[derive(Clone, Copy)]
struct TermColors {
    bg: (f64, f64, f64),
    fg: (f64, f64, f64),
}

type CellPos = (u16, u16);
type Selection = (CellPos, CellPos);

pub struct TerminalInit {
    pub show_toolbar: bool,
    pub back_tooltip: Option<String>,
    pub config: client_config::AppConfig,
}

#[derive(Debug)]
pub enum TerminalInput {
    Feed(Vec<u8>),
    Clear,
    ApplyColors,
    GrabFocus,
}

#[derive(Debug)]
pub enum TerminalOutput {
    Input(Vec<u8>),
    Resize { rows: u16, cols: u16 },
    Restart,
    Back,
}

pub struct TerminalModel {
    parser: Rc<RefCell<vt100::Parser>>,
    acs: Rc<RefCell<AcsFilter>>,
    colors: Rc<Cell<TermColors>>,
    selection: Rc<Cell<Option<Selection>>>,
    config: client_config::AppConfig,
    area: gtk::DrawingArea,
}

pub struct TerminalWidgets {
    area: gtk::DrawingArea,
}

impl SimpleComponent for TerminalModel {
    type Init = TerminalInit;
    type Input = TerminalInput;
    type Output = TerminalOutput;
    type Root = gtk::Box;
    type Widgets = TerminalWidgets;

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        crate::init_resources();
        let config = init.config;

        let parser = Rc::new(RefCell::new(vt100::Parser::new(ROWS, COLS, 0)));
        let acs = Rc::new(RefCell::new(AcsFilter::default()));
        let colors = Rc::new(Cell::new(read_colors(&config)));
        let last_size = Rc::new(Cell::new((ROWS, COLS)));
        let selection: Rc<Cell<Option<Selection>>> = Rc::new(Cell::new(None));
        let (cell_w, cell_h, ascent) = measure_cell();

        let area = gtk::DrawingArea::new();
        area.set_focusable(true);
        area.set_hexpand(true);
        area.set_vexpand(true);

        let parser_draw = parser.clone();
        let colors_draw = colors.clone();
        let selection_draw = selection.clone();
        area.set_draw_func(move |_area, cr, _w, _h| {
            let term = colors_draw.get();
            let sel_span = selection_draw.get().map(|(a, b)| {
                let (s, e) = if a <= b { (a, b) } else { (b, a) };
                (s, e)
            });
            let p = parser_draw.borrow();
            let screen = p.screen();

            cr.set_source_rgb(term.bg.0, term.bg.1, term.bg.2);
            let _ = cr.paint();
            cr.set_font_size(FONT_SIZE);

            let (rows, cols) = screen.size();
            for row in 0..rows {
                for col in 0..cols {
                    let Some(cell) = screen.cell(row, col) else {
                        continue;
                    };
                    let x = col as f64 * cell_w;
                    let y = row as f64 * cell_h;

                    let mut fg = color_rgb(cell.fgcolor(), term.fg);
                    let mut bg = color_rgb(cell.bgcolor(), term.bg);
                    let mut bg_visible = !matches!(cell.bgcolor(), vt100::Color::Default);
                    if cell.inverse() {
                        std::mem::swap(&mut fg, &mut bg);
                        bg_visible = true;
                    }
                    let selected =
                        sel_span.is_some_and(|(s, e)| (row, col) >= s && (row, col) <= e);
                    if selected {
                        fg = term.bg;
                        bg = term.fg;
                        bg_visible = true;
                    }
                    if bg_visible {
                        cr.set_source_rgb(bg.0, bg.1, bg.2);
                        cr.rectangle(x, y, cell_w + 0.5, cell_h + 0.5);
                        let _ = cr.fill();
                    }

                    let contents = cell.contents();
                    if !contents.is_empty() {
                        cr.select_font_face(
                            FONT_FAMILY,
                            cairo::FontSlant::Normal,
                            if cell.bold() {
                                cairo::FontWeight::Bold
                            } else {
                                cairo::FontWeight::Normal
                            },
                        );
                        cr.set_source_rgb(fg.0, fg.1, fg.2);
                        cr.move_to(x, y + ascent);
                        let _ = cr.show_text(&contents);
                    }
                }
            }

            if !screen.hide_cursor() {
                let (crow, ccol) = screen.cursor_position();
                cr.set_source_rgba(term.fg.0, term.fg.1, term.fg.2, 0.55);
                cr.rectangle(ccol as f64 * cell_w, crow as f64 * cell_h, cell_w, cell_h);
                let _ = cr.fill();
            }
        });

        let parser_resize = parser.clone();
        let last_size_resize = last_size.clone();
        let sender_resize = sender.clone();
        area.connect_resize(move |_a, w, h| {
            let cols = ((w as f64 / cell_w).floor() as u16).max(2);
            let rows = ((h as f64 / cell_h).floor() as u16).max(2);
            if last_size_resize.get() == (rows, cols) {
                return;
            }
            last_size_resize.set((rows, cols));
            parser_resize.borrow_mut().set_size(rows, cols);
            let _ = sender_resize.output(TerminalOutput::Resize { rows, cols });
        });

        if init.show_toolbar {
            let empty_title = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let top_bar = adw::HeaderBar::builder()
                .show_start_title_buttons(false)
                .show_end_title_buttons(false)
                .title_widget(&empty_title)
                .build();

            let back_tooltip = init
                .back_tooltip
                .unwrap_or_else(|| crate::i18n::tr("terminal.back_tooltip").to_string());
            let btn_back = gtk::Button::builder()
                .child(&gtk::Image::from_resource(
                    "/com/terminal-ui/gtk/arrow-left.svg",
                ))
                .tooltip_text(&back_tooltip)
                .build();
            btn_back.set_cursor_from_name(Some("pointer"));
            let sender_back = sender.clone();
            btn_back.connect_clicked(move |_| {
                let _ = sender_back.output(TerminalOutput::Back);
            });
            top_bar.pack_start(&btn_back);

            let back_separator = gtk::Separator::builder()
                .orientation(gtk::Orientation::Vertical)
                .margin_top(8)
                .margin_bottom(8)
                .build();
            top_bar.pack_start(&back_separator);

            let btn_restart = gtk::Button::from_icon_name("view-refresh-symbolic");
            btn_restart.set_cursor_from_name(Some("pointer"));
            btn_restart.set_tooltip_text(Some(&crate::i18n::tr("terminal.restart_tooltip")));
            let sender_restart = sender.clone();
            btn_restart.connect_clicked(move |_| {
                sender_restart.input(TerminalInput::Clear);
                let _ = sender_restart.output(TerminalOutput::Restart);
            });
            top_bar.pack_start(&btn_restart);

            let btn_clear = gtk::Button::from_icon_name("edit-clear-all-symbolic");
            btn_clear.set_cursor_from_name(Some("pointer"));
            btn_clear.set_tooltip_text(Some(&crate::i18n::tr("terminal.clear_tooltip")));
            let sender_clear = sender.clone();
            btn_clear.connect_clicked(move |_| {
                sender_clear.input(TerminalInput::Clear);
            });
            top_bar.pack_start(&btn_clear);

            let term = colors.get();
            let bg_color_btn = gtk::ColorButton::builder()
                .rgba(&gdk::RGBA::new(
                    term.bg.0 as f32,
                    term.bg.1 as f32,
                    term.bg.2 as f32,
                    1.0,
                ))
                .use_alpha(false)
                .valign(gtk::Align::Center)
                .tooltip_text(&*crate::i18n::tr("terminal.bg_color_tooltip"))
                .build();
            top_bar.pack_end(&bg_color_btn);

            let fg_color_btn = gtk::ColorButton::builder()
                .rgba(&gdk::RGBA::new(
                    term.fg.0 as f32,
                    term.fg.1 as f32,
                    term.fg.2 as f32,
                    1.0,
                ))
                .use_alpha(false)
                .valign(gtk::Align::Center)
                .tooltip_text(&*crate::i18n::tr("terminal.fg_color_tooltip"))
                .build();
            top_bar.pack_end(&fg_color_btn);

            root.append(&top_bar);

            let config_bg = config.clone();
            let colors_bg = colors.clone();
            let area_bg = area.clone();
            bg_color_btn.connect_color_set(move |btn| {
                let key = if adw::StyleManager::default().is_dark() {
                    "ui.term_dark_bg"
                } else {
                    "ui.term_light_bg"
                };
                config_bg.set(key, rgba_to_hex(&btn.rgba()));
                config_bg.save();
                colors_bg.set(read_colors(&config_bg));
                area_bg.queue_draw();
            });

            let config_fg = config.clone();
            let colors_fg = colors.clone();
            let area_fg = area.clone();
            fg_color_btn.connect_color_set(move |btn| {
                let key = if adw::StyleManager::default().is_dark() {
                    "ui.term_dark_fg"
                } else {
                    "ui.term_light_fg"
                };
                config_fg.set(key, rgba_to_hex(&btn.rgba()));
                config_fg.save();
                colors_fg.set(read_colors(&config_fg));
                area_fg.queue_draw();
            });

            let bg_btn_sync = bg_color_btn.clone();
            let fg_btn_sync = fg_color_btn.clone();
            let config_sync = config.clone();
            let colors_sync = colors.clone();
            let area_sync = area.clone();
            adw::StyleManager::default().connect_dark_notify(move |_sm| {
                let c = read_colors(&config_sync);
                colors_sync.set(c);
                bg_btn_sync.set_rgba(&gdk::RGBA::new(
                    c.bg.0 as f32,
                    c.bg.1 as f32,
                    c.bg.2 as f32,
                    1.0,
                ));
                fg_btn_sync.set_rgba(&gdk::RGBA::new(
                    c.fg.0 as f32,
                    c.fg.1 as f32,
                    c.fg.2 as f32,
                    1.0,
                ));
                area_sync.queue_draw();
            });
        } else {
            let config_sync = config.clone();
            let colors_sync = colors.clone();
            let area_sync = area.clone();
            adw::StyleManager::default().connect_dark_notify(move |_sm| {
                colors_sync.set(read_colors(&config_sync));
                area_sync.queue_draw();
            });
        }

        root.append(&area);

        let parser_key = parser.clone();
        let area_key = area.clone();
        let selection_key = selection.clone();
        let sender_key = sender.clone();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, keyval, _, state| {
            let ctrl_shift = state.contains(gdk::ModifierType::CONTROL_MASK)
                && state.contains(gdk::ModifierType::SHIFT_MASK);

            if ctrl_shift && (keyval == gdk::Key::v || keyval == gdk::Key::V) {
                paste_into_pty(&area_key, &sender_key, &parser_key);
                return gtk::glib::Propagation::Stop;
            }
            if ctrl_shift && (keyval == gdk::Key::c || keyval == gdk::Key::C) {
                if let Some(text) = selection_text(&parser_key, selection_key.get()) {
                    area_key.clipboard().set_text(&text);
                }
                return gtk::glib::Propagation::Stop;
            }

            let app_cursor = parser_key.borrow().screen().application_cursor();
            if let Some(bytes) = key_to_bytes(keyval, state, app_cursor) {
                let _ = sender_key.output(TerminalOutput::Input(bytes));
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        area.add_controller(key_controller);

        let click_gesture = gtk::GestureClick::new();
        let area_focus = area.clone();
        click_gesture.connect_pressed(move |_, _, _, _| {
            area_focus.grab_focus();
        });
        area.add_controller(click_gesture);

        let menu_gesture = gtk::GestureClick::new();
        menu_gesture.set_button(gdk::BUTTON_SECONDARY);
        let area_menu = area.clone();
        let parser_menu = parser.clone();
        let selection_menu = selection.clone();
        let sender_menu = sender.clone();
        menu_gesture.connect_pressed(move |_, _, x, y| {
            area_menu.grab_focus();

            let popover = gtk::Popover::new();
            popover.set_parent(&area_menu);
            popover.set_has_arrow(false);
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.connect_closed(|p| p.unparent());

            let paste_btn = gtk::Button::builder()
                .label("Paste")
                .css_classes(vec!["flat"])
                .build();
            let area_p = area_menu.clone();
            let sender_p = sender_menu.clone();
            let parser_p = parser_menu.clone();
            let popover_p = popover.clone();
            paste_btn.connect_clicked(move |_| {
                paste_into_pty(&area_p, &sender_p, &parser_p);
                popover_p.popdown();
            });

            let menu_box = gtk::Box::new(gtk::Orientation::Vertical, 0);

            if let Some(text) = selection_text(&parser_menu, selection_menu.get()) {
                let copy_btn = gtk::Button::builder()
                    .label("Copy")
                    .css_classes(vec!["flat"])
                    .build();
                let area_c = area_menu.clone();
                let popover_c = popover.clone();
                copy_btn.connect_clicked(move |_| {
                    area_c.clipboard().set_text(&text);
                    popover_c.popdown();
                });
                menu_box.append(&copy_btn);
            }

            if selection_menu.get().is_some() {
                let clear_btn = gtk::Button::builder()
                    .label("Clear Selection")
                    .css_classes(vec!["flat"])
                    .build();
                let sel_clear = selection_menu.clone();
                let area_clear = area_menu.clone();
                let popover_clear = popover.clone();
                clear_btn.connect_clicked(move |_| {
                    sel_clear.set(None);
                    area_clear.queue_draw();
                    popover_clear.popdown();
                });
                menu_box.append(&clear_btn);
            }

            menu_box.append(&paste_btn);
            popover.set_child(Some(&menu_box));
            popover.popup();
        });
        area.add_controller(menu_gesture);

        let drag = gtk::GestureDrag::new();
        drag.set_button(gdk::BUTTON_PRIMARY);
        let drag_anchor: Rc<Cell<Option<CellPos>>> = Rc::new(Cell::new(None));
        let mouse_fwd: Rc<Cell<Option<CellPos>>> = Rc::new(Cell::new(None));

        let area_db = area.clone();
        let parser_db = parser.clone();
        let anchor_db = drag_anchor.clone();
        let sel_db = selection.clone();
        let fwd_db = mouse_fwd.clone();
        let sender_db = sender.clone();
        drag.connect_drag_begin(move |g, x, y| {
            area_db.grab_focus();
            let shift = g
                .current_event_state()
                .contains(gdk::ModifierType::SHIFT_MASK);
            let mode = parser_db.borrow().screen().mouse_protocol_mode();
            if !shift && mode != vt100::MouseProtocolMode::None {
                let cell = px_to_cell(&parser_db, cell_w, cell_h, x, y);
                fwd_db.set(Some(cell));
                mouse_report(&parser_db, &sender_db, 0, cell, true, false);
                return;
            }
            fwd_db.set(None);
            anchor_db.set(Some(px_to_cell(&parser_db, cell_w, cell_h, x, y)));
            sel_db.set(None);
            area_db.queue_draw();
        });

        let area_du = area.clone();
        let parser_du = parser.clone();
        let anchor_du = drag_anchor.clone();
        let sel_du = selection.clone();
        let fwd_du = mouse_fwd.clone();
        let sender_du = sender.clone();
        drag.connect_drag_update(move |g, ox, oy| {
            let Some((sx, sy)) = g.start_point() else {
                return;
            };
            if let Some(last) = fwd_du.get() {
                let mode = parser_du.borrow().screen().mouse_protocol_mode();
                if matches!(
                    mode,
                    vt100::MouseProtocolMode::ButtonMotion | vt100::MouseProtocolMode::AnyMotion
                ) {
                    let cell = px_to_cell(&parser_du, cell_w, cell_h, sx + ox, sy + oy);
                    if cell != last {
                        fwd_du.set(Some(cell));
                        mouse_report(&parser_du, &sender_du, 0, cell, true, true);
                    }
                }
                return;
            }
            let Some(anchor) = anchor_du.get() else {
                return;
            };
            let cursor = px_to_cell(&parser_du, cell_w, cell_h, sx + ox, sy + oy);
            sel_du.set(Some((anchor, cursor)));
            area_du.queue_draw();
        });

        let parser_de = parser.clone();
        let fwd_de = mouse_fwd.clone();
        let sender_de = sender.clone();
        drag.connect_drag_end(move |g, ox, oy| {
            if fwd_de.get().is_none() {
                return;
            }
            fwd_de.set(None);
            let Some((sx, sy)) = g.start_point() else {
                return;
            };
            let mode = parser_de.borrow().screen().mouse_protocol_mode();
            if mode != vt100::MouseProtocolMode::None && mode != vt100::MouseProtocolMode::Press {
                let cell = px_to_cell(&parser_de, cell_w, cell_h, sx + ox, sy + oy);
                mouse_report(&parser_de, &sender_de, 0, cell, false, false);
            }
        });
        area.add_controller(drag);

        let pointer_pos: Rc<Cell<(f64, f64)>> = Rc::new(Cell::new((0.0, 0.0)));
        let motion_ctl = gtk::EventControllerMotion::new();
        let pos_mc = pointer_pos.clone();
        motion_ctl.connect_motion(move |_, x, y| pos_mc.set((x, y)));
        area.add_controller(motion_ctl);

        let scroll_ctl = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let parser_sc = parser.clone();
        let sender_sc = sender.clone();
        let pos_sc = pointer_pos.clone();
        let wheel_acc = Rc::new(Cell::new(0.0f64));
        scroll_ctl.connect_scroll(move |_, _dx, dy| {
            if parser_sc.borrow().screen().mouse_protocol_mode() == vt100::MouseProtocolMode::None {
                return gtk::glib::Propagation::Proceed;
            }
            let total = wheel_acc.get() + dy;
            let steps = total.trunc();
            wheel_acc.set(total - steps);
            let (x, y) = pos_sc.get();
            let cell = px_to_cell(&parser_sc, cell_w, cell_h, x, y);
            for _ in 0..steps.abs() as i32 {
                let button = if steps < 0.0 { 64 } else { 65 }; // up / down
                mouse_report(&parser_sc, &sender_sc, button, cell, true, false);
            }
            gtk::glib::Propagation::Stop
        });
        area.add_controller(scroll_ctl);

        let model = TerminalModel {
            parser,
            acs,
            colors,
            selection,
            config,
            area: area.clone(),
        };
        let widgets = TerminalWidgets { area };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            TerminalInput::Feed(data) => {
                let data = self.acs.borrow_mut().feed(&data);
                self.parser.borrow_mut().process(&data);
                self.selection.set(None);

                if data.windows(4).any(|w| w == b"\x1b[6n") {
                    let (row, col) = self.parser.borrow().screen().cursor_position();
                    let _ = sender.output(TerminalOutput::Input(
                        format!("\x1b[{};{}R", row + 1, col + 1).into_bytes(),
                    ));
                }
            }
            TerminalInput::Clear => {
                reset_parser(&self.parser);
                *self.acs.borrow_mut() = AcsFilter::default();
                self.selection.set(None);
            }
            TerminalInput::ApplyColors => {
                self.colors.set(read_colors(&self.config));
            }
            TerminalInput::GrabFocus => {
                self.area.grab_focus();
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.area.queue_draw();
    }
}

fn mouse_seq(
    encoding: vt100::MouseProtocolEncoding,
    button: u8,
    (row, col): CellPos,
    pressed: bool,
    motion: bool,
) -> Vec<u8> {
    let cb = button + if motion { 32 } else { 0 };
    match encoding {
        vt100::MouseProtocolEncoding::Sgr => format!(
            "\x1b[<{};{};{}{}",
            cb,
            u32::from(col) + 1,
            u32::from(row) + 1,
            if pressed { 'M' } else { 'm' }
        )
        .into_bytes(),
        _ => vec![
            0x1b,
            b'[',
            b'M',
            32 + if pressed { cb } else { 3 },
            32 + ((col + 1).min(223) as u8),
            32 + ((row + 1).min(223) as u8),
        ],
    }
}

fn mouse_report(
    parser: &Rc<RefCell<vt100::Parser>>,
    sender: &ComponentSender<TerminalModel>,
    button: u8,
    cell: CellPos,
    pressed: bool,
    motion: bool,
) {
    let encoding = parser.borrow().screen().mouse_protocol_encoding();
    let _ = sender.output(TerminalOutput::Input(mouse_seq(
        encoding, button, cell, pressed, motion,
    )));
}

fn px_to_cell(
    parser: &Rc<RefCell<vt100::Parser>>,
    cell_w: f64,
    cell_h: f64,
    x: f64,
    y: f64,
) -> CellPos {
    let (rows, cols) = parser.borrow().screen().size();
    let col = ((x / cell_w).max(0.0) as u16).min(cols.saturating_sub(1));
    let row = ((y / cell_h).max(0.0) as u16).min(rows.saturating_sub(1));
    (row, col)
}

fn selection_text(
    parser: &Rc<RefCell<vt100::Parser>>,
    selection: Option<Selection>,
) -> Option<String> {
    let (a, b) = selection?;
    let (start, end) = if a <= b { (a, b) } else { (b, a) };

    let p = parser.borrow();
    let screen = p.screen();
    let (rows, cols) = screen.size();
    if cols == 0 || rows == 0 {
        return None;
    }

    let mut out = String::new();
    let last_row = end.0.min(rows - 1);
    for row in start.0..=last_row {
        let col_start = if row == start.0 { start.1 } else { 0 };
        let col_end = if row == end.0 { end.1 } else { cols - 1 }.min(cols - 1);
        let mut line = String::new();
        for col in col_start..=col_end {
            match screen.cell(row, col) {
                Some(cell) if cell.is_wide_continuation() => {}
                Some(cell) if cell.has_contents() => line.push_str(&cell.contents()),
                _ => line.push(' '),
            }
        }
        out.push_str(line.trim_end());
        if row != last_row {
            out.push('\n');
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn reset_parser(parser: &Rc<RefCell<vt100::Parser>>) {
    let (rows, cols) = parser.borrow().screen().size();
    *parser.borrow_mut() = vt100::Parser::new(rows, cols, 0);
}

fn paste_into_pty(
    area: &gtk::DrawingArea,
    sender: &ComponentSender<TerminalModel>,
    parser: &Rc<RefCell<vt100::Parser>>,
) {
    let clipboard = area.clipboard();
    let sender = sender.clone();
    let parser = parser.clone();
    clipboard.read_text_async(gtk::gio::Cancellable::NONE, move |res| {
        let Ok(Some(text)) = res else { return };
        if text.is_empty() {
            return;
        }
        let bracketed = parser.borrow().screen().bracketed_paste();
        let mut bytes = Vec::new();
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend_from_slice(text.as_bytes());
        if bracketed {
            bytes.extend_from_slice(b"\x1b[201~");
        }
        let _ = sender.output(TerminalOutput::Input(bytes));
    });
}


fn acs_char(b: u8) -> Option<char> {
    Some(match b {
        b'q' => '─',
        b'x' => '│',
        b'l' => '┌',
        b'k' => '┐',
        b'm' => '└',
        b'j' => '┘',
        b'n' => '┼',
        b't' => '├',
        b'u' => '┤',
        b'w' => '┬',
        b'v' => '┴',
        b'a' => '▒',
        b'h' => '░',
        b'0' => '█',
        b'`' => '◆',
        b'~' => '·',
        b'f' => '°',
        b'g' => '±',
        b'y' => '≤',
        b'z' => '≥',
        b'{' => 'π',
        b'|' => '≠',
        b'}' => '£',
        b'+' => '→',
        b',' => '←',
        b'-' => '↑',
        b'.' => '↓',
        b'o' => '⎺',
        b'p' => '⎻',
        b'r' => '⎼',
        b's' => '⎽',
        _ => return None,
    })
}

#[derive(Default, Clone, Copy, PartialEq)]
enum AcsState {
    #[default]
    Text,
    Esc,
    DesignateG0, // after ESC (
    DesignateG1, // after ESC )
    Csi,         // until a final byte 0x40..=0x7E
    Osc,         // until BEL or ESC \
    OscEsc,
}

#[derive(Default)]
struct AcsFilter {
    g0_graphics: bool,
    g1_graphics: bool,
    shifted: bool, // SO (0x0E) selects G1, SI (0x0F) back to G0
    state: AcsState,
}

impl AcsFilter {
    fn feed(&mut self, input: &[u8]) -> Vec<u8> {
        use AcsState::*;
        let mut out = Vec::with_capacity(input.len());
        for &b in input {
            match self.state {
                Text => match b {
                    0x1b => self.state = Esc,
                    0x0e => self.shifted = true,
                    0x0f => self.shifted = false,
                    _ => {
                        let graphics = if self.shifted {
                            self.g1_graphics
                        } else {
                            self.g0_graphics
                        };
                        if graphics {
                            if let Some(ch) = acs_char(b) {
                                let mut buf = [0u8; 4];
                                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                                continue;
                            }
                        }
                    }
                },
                Esc => {
                    self.state = match b {
                        b'(' => DesignateG0,
                        b')' => DesignateG1,
                        b'[' => Csi,
                        b']' => Osc,
                        _ => Text,
                    }
                }
                DesignateG0 => {
                    self.g0_graphics = b == b'0';
                    self.state = Text;
                }
                DesignateG1 => {
                    self.g1_graphics = b == b'0';
                    self.state = Text;
                }
                Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        self.state = Text;
                    }
                }
                Osc => match b {
                    0x07 => self.state = Text,
                    0x1b => self.state = OscEsc,
                    _ => {}
                },
                OscEsc => self.state = if b == b'\\' { Text } else { Osc },
            }
            out.push(b);
        }
        out
    }
}

fn measure_cell() -> (f64, f64, f64) {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 16, 16).unwrap();
    let cr = cairo::Context::new(&surface).unwrap();
    cr.select_font_face(
        FONT_FAMILY,
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cr.set_font_size(FONT_SIZE);
    let fe = cr.font_extents().unwrap();
    let te = cr.text_extents("M").unwrap();
    let cell_w = if te.x_advance() > 0.0 {
        te.x_advance()
    } else {
        fe.max_x_advance()
    };
    (cell_w, fe.height(), fe.ascent())
}

fn read_colors(config: &client_config::AppConfig) -> TermColors {
    let is_dark = adw::StyleManager::default().is_dark();
    let (bg_key, fg_key, bg_def, fg_def) = if is_dark {
        ("ui.term_dark_bg", "ui.term_dark_fg", "#0b1221", "#38bdf8")
    } else {
        ("ui.term_light_bg", "ui.term_light_fg", "#ffffff", "#1d4ed8")
    };
    let bg = config
        .get::<String>(bg_key)
        .unwrap_or_else(|| bg_def.to_string());
    let fg = config
        .get::<String>(fg_key)
        .unwrap_or_else(|| fg_def.to_string());
    TermColors {
        bg: hex_to_rgb(&bg, (0.04, 0.07, 0.13)),
        fg: hex_to_rgb(&fg, (0.90, 0.90, 0.90)),
    }
}

fn hex_to_rgb(hex: &str, fallback: (f64, f64, f64)) -> (f64, f64, f64) {
    match hex.parse::<gdk::RGBA>() {
        Ok(c) => (c.red() as f64, c.green() as f64, c.blue() as f64),
        Err(_) => fallback,
    }
}

fn rgba_to_hex(rgba: &gdk::RGBA) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgba.red() * 255.0) as u8,
        (rgba.green() * 255.0) as u8,
        (rgba.blue() * 255.0) as u8
    )
}

fn color_rgb(c: vt100::Color, default: (f64, f64, f64)) -> (f64, f64, f64) {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0),
        vt100::Color::Idx(i) => idx_rgb(i),
    }
}

fn idx_rgb(i: u8) -> (f64, f64, f64) {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];
    let (r, g, b) = if i < 16 {
        BASE[i as usize]
    } else if i < 232 {
        let i = i - 16;
        let conv = |v: u8| if v == 0 { 0u8 } else { 55 + v * 40 };
        (conv(i / 36), conv((i % 36) / 6), conv(i % 6))
    } else {
        let v = 8 + (i - 232) * 10;
        (v, v, v)
    };
    (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
}

fn key_to_bytes(keyval: gdk::Key, state: gdk::ModifierType, app_cursor: bool) -> Option<Vec<u8>> {
    use gdk::Key;
    let cursor = |c: u8| -> Option<Vec<u8>> {
        Some(if app_cursor {
            vec![0x1b, b'O', c]
        } else {
            vec![0x1b, b'[', c]
        })
    };
    match keyval {
        Key::Up | Key::KP_Up => return cursor(b'A'),
        Key::Down | Key::KP_Down => return cursor(b'B'),
        Key::Right | Key::KP_Right => return cursor(b'C'),
        Key::Left | Key::KP_Left => return cursor(b'D'),
        Key::Home | Key::KP_Home => return cursor(b'H'),
        Key::End | Key::KP_End => return cursor(b'F'),
        _ => {}
    }
    let named: Option<&[u8]> = match keyval {
        Key::Return | Key::KP_Enter => Some(b"\r"),
        Key::BackSpace => Some(b"\x7f"),
        Key::Tab => Some(b"\t"),
        Key::ISO_Left_Tab => Some(b"\x1b[Z"),
        Key::Escape => Some(b"\x1b"),
        Key::Page_Up => Some(b"\x1b[5~"),
        Key::Page_Down => Some(b"\x1b[6~"),
        Key::Delete => Some(b"\x1b[3~"),
        Key::Insert => Some(b"\x1b[2~"),
        Key::F1 => Some(b"\x1bOP"),
        Key::F2 => Some(b"\x1bOQ"),
        Key::F3 => Some(b"\x1bOR"),
        Key::F4 => Some(b"\x1bOS"),
        Key::F5 => Some(b"\x1b[15~"),
        Key::F6 => Some(b"\x1b[17~"),
        Key::F7 => Some(b"\x1b[18~"),
        Key::F8 => Some(b"\x1b[19~"),
        Key::F9 => Some(b"\x1b[20~"),
        Key::F10 => Some(b"\x1b[21~"),
        Key::F11 => Some(b"\x1b[23~"),
        Key::F12 => Some(b"\x1b[24~"),
        _ => None,
    };
    if let Some(s) = named {
        return Some(s.to_vec());
    }

    let ch = keyval.to_unicode()?;
    if state.contains(gdk::ModifierType::CONTROL_MASK) {
        let up = ch.to_ascii_uppercase() as u32;
        if (0x40..0x80).contains(&up) {
            return Some(vec![(up & 0x1f) as u8]);
        }
    }
    if ch.is_control() {
        return None;
    }
    let mut buf = [0u8; 4];
    Some(ch.encode_utf8(&mut buf).as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::AcsFilter;

    fn run(filter: &mut AcsFilter, bytes: &[u8]) -> String {
        String::from_utf8(filter.feed(bytes)).unwrap()
    }

    #[test]
    fn translates_inside_esc0_and_stops_after_escb() {
        let mut f = AcsFilter::default();
        let out = run(&mut f, b"\x1b(0lqqk\x1b(Bq");
        assert_eq!(out, "\x1b(0┌──┐\x1b(Bq");
    }

    #[test]
    fn csi_final_bytes_survive_graphics_mode() {
        let mut f = AcsFilter::default();
        let out = run(&mut f, b"\x1b(0\x1b[0mq");
        assert_eq!(out, "\x1b(0\x1b[0m─");
    }

    #[test]
    fn so_si_shift_g1() {
        let mut f = AcsFilter::default();
        let out = run(&mut f, b"\x1b)0q\x0eq\x0fq");
        assert_eq!(out, "\x1b)0q\x0e─\x0fq");
    }

    #[test]
    fn state_survives_chunk_boundary() {
        let mut f = AcsFilter::default();
        let a = run(&mut f, b"\x1b(");
        let b = run(&mut f, b"0x");
        assert_eq!(format!("{a}{b}"), "\x1b(0│");
    }
}

#[cfg(test)]
mod mouse_tests {
    use super::mouse_seq;
    use vt100::MouseProtocolEncoding as Enc;

    #[test]
    fn sgr_press_and_release() {
        assert_eq!(
            mouse_seq(Enc::Sgr, 0, (4, 11), true, false),
            b"\x1b[<0;12;5M"
        );
        assert_eq!(
            mouse_seq(Enc::Sgr, 0, (4, 11), false, false),
            b"\x1b[<0;12;5m"
        );
    }

    #[test]
    fn sgr_motion_adds_32_and_wheel_is_64() {
        assert_eq!(mouse_seq(Enc::Sgr, 0, (0, 0), true, true), b"\x1b[<32;1;1M");
        assert_eq!(
            mouse_seq(Enc::Sgr, 64, (2, 3), true, false),
            b"\x1b[<64;4;3M"
        );
    }

    #[test]
    fn legacy_x10_bytes_and_release_button_3() {
        assert_eq!(
            mouse_seq(Enc::Default, 0, (4, 11), true, false),
            vec![0x1b, b'[', b'M', 32, 32 + 12, 32 + 5]
        );
        assert_eq!(mouse_seq(Enc::Default, 0, (4, 11), false, false)[3], 32 + 3);
    }
}
