use adw::prelude::*;
use gtk::{Align, Box, Button, ListBox, Orientation, SelectionMode};
use std::cell::RefCell;
use std::rc::Rc;

fn show_key_capture_dialog(
    parent: &gtk::Window,
    config: client_config::AppConfig,
    hotkey_id: &str,
    hotkey_desc: &str,
    on_rebound: impl Fn() + 'static,
) {
    let capture_dialog = gtk::Window::builder()
        .title(&*crate::i18n::tr("settings.hotkey_assign"))
        .transient_for(parent)
        .modal(true)
        .default_width(360)
        .default_height(200)
        .resizable(false)
        .build();

    let main_box = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_start(24)
        .margin_end(24)
        .margin_top(24)
        .margin_bottom(24)
        .valign(Align::Center)
        .build();

    let desc_label = gtk::Label::builder()
        .label(&format!(
            "<span size='large' weight='bold'>{}</span>",
            hotkey_desc
        ))
        .use_markup(true)
        .halign(Align::Center)
        .build();
    main_box.append(&desc_label);

    let instruction_label = gtk::Label::builder()
        .label(&format!(
            "{}\n{}",
            crate::i18n::tr("settings.hotkey_press"),
            crate::i18n::tr("settings.hotkey_cancel")
        ))
        .justify(gtk::Justification::Center)
        .halign(Align::Center)
        .build();
    main_box.append(&instruction_label);

    capture_dialog.set_child(Some(&main_box));

    let key_controller = gtk::EventControllerKey::new();
    let dialog_clone = capture_dialog.clone();
    let hotkey_id = hotkey_id.to_string();
    let on_rebound = Rc::new(on_rebound);
    let config_capture = config.clone();

    key_controller.connect_key_pressed(move |_, keyval, _, state| {
        if keyval == gtk::gdk::Key::Escape {
            dialog_clone.close();
            return gtk::glib::Propagation::Stop;
        }

        if let Some(name) = keyval.name() {
            let name_str = name.as_str();
            match name_str {
                "Control_L" | "Control_R" | "Alt_L" | "Alt_R" | "Shift_L" | "Shift_R"
                | "Super_L" | "Super_R" | "Meta_L" | "Meta_R" => {
                    return gtk::glib::Propagation::Proceed;
                }
                _ => {}
            }
        }

        let bound_str = crate::hotkey::keyval_to_string(keyval, state);
        if !bound_str.is_empty() {
            let mut current = crate::hotkey::get_hotkeys(&config_capture);
            if let Some(hk) = current.iter_mut().find(|h| h.id == hotkey_id) {
                hk.keys = bound_str;
            }
            crate::hotkey::save_hotkeys(&config_capture, &current);
            on_rebound();
            dialog_clone.close();
            return gtk::glib::Propagation::Stop;
        }

        gtk::glib::Propagation::Proceed
    });

    capture_dialog.add_controller(key_controller);
    capture_dialog.present();
}

pub(super) fn build(
    page_box: &Box,
    parent: &gtk::Window,
    config: client_config::AppConfig,
    on_connections_changed: Rc<dyn Fn() + 'static>,
) {
    let title_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .margin_bottom(16)
        .build();

    let page_title = gtk::Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_hotkeys")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .hexpand(true)
        .build();
    title_box.append(&page_title);

    let reset_btn = Button::builder()
        .label(&*crate::i18n::tr("settings.hotkey_reset"))
        .halign(Align::End)
        .valign(Align::Center)
        .build();
    reset_btn.set_cursor_from_name(Some("pointer"));
    title_box.append(&reset_btn);

    page_box.append(&title_box);

    let list_box = ListBox::builder()
        .selection_mode(SelectionMode::None)
        .build();
    list_box.add_css_class("boxed-list");

    page_box.append(&list_box);

    let rebuild_cell = Rc::new(RefCell::new(None::<Rc<dyn Fn()>>));
    let rebuild_cell_clone = rebuild_cell.clone();

    let rebuild_cell_reset = rebuild_cell.clone();
    let on_connections_changed_reset = on_connections_changed.clone();
    let config_reset = config.clone();
    reset_btn.connect_clicked(move |_| {
        let defaults = crate::hotkey::get_default_hotkeys();
        crate::hotkey::save_hotkeys(&config_reset, &defaults);
        crate::favorites::reset_to_defaults(&config_reset);
        on_connections_changed_reset();
        if let Some(rebuild_cb) = rebuild_cell_reset.borrow().as_ref() {
            rebuild_cb();
        }
    });

    let list_box_clone = list_box.clone();
    let parent_clone = parent.clone();
    let config_rebuild = config.clone();

    let rebuild_list = Rc::new(move || {
        while let Some(child) = list_box_clone.first_child() {
            list_box_clone.remove(&child);
        }

        let hotkeys = crate::hotkey::get_hotkeys(&config_rebuild);
        for hk in hotkeys {
            let description = crate::hotkey::description(&hk.id);
            let row = adw::ActionRow::builder().title(&description).build();

            let btn = Button::builder()
                .label(hk.keys.clone())
                .valign(Align::Center)
                .build();
            btn.set_cursor_from_name(Some("pointer"));

            let parent_inner = parent_clone.clone();
            let hk_id = hk.id.clone();
            let hk_desc = description.clone();
            let rebuild_cell_inner = rebuild_cell_clone.clone();
            let config_capture = config_rebuild.clone();

            btn.connect_clicked(move |_| {
                let rebuild_cb = rebuild_cell_inner.borrow().as_ref().unwrap().clone();
                show_key_capture_dialog(
                    &parent_inner,
                    config_capture.clone(),
                    &hk_id,
                    &hk_desc,
                    move || {
                        rebuild_cb();
                    },
                );
            });

            row.add_suffix(&btn);
            list_box_clone.append(&row);
        }
    });

    *rebuild_cell.borrow_mut() = Some(rebuild_list.clone());
    rebuild_list();
}
