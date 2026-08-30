use adw::prelude::*;
use gtk::{Align, Box, Button, Label, ListBox, Orientation, SelectionMode};
use std::cell::RefCell;
use std::rc::Rc;

const KEY: &str = "ui.custom_associations";

pub(super) fn build(page_box: &Box, parent: &gtk::Window, config: client_config::AppConfig) {
    let page_title = Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_applications")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .margin_bottom(16)
        .build();
    page_box.append(&page_title);

    let open_group = adw::PreferencesGroup::new();
    let action_row = adw::ComboRow::builder()
        .title(&*crate::i18n::tr("settings.open_action"))
        .subtitle(&*crate::i18n::tr("settings.desc_open_action"))
        .model(&gtk::StringList::new(&[
            &crate::i18n::tr("settings.open_action_viewer"),
            &crate::i18n::tr("settings.open_action_system"),
        ]))
        .build();
    action_row.set_selected(match crate::external::fallback(&config) {
        crate::external::Fallback::System => 1,
        crate::external::Fallback::Viewer => 0,
    });
    {
        let config = config.clone();
        action_row.connect_selected_notify(move |row| {
            let value = if row.selected() == 1 { "system" } else { "viewer" };
            config.set("ui.double_click_action", value.to_string());
            config.save();
        });
    }
    open_group.add(&action_row);
    page_box.append(&open_group);

    let assoc_group = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.assoc_group"))
        .description(&*crate::i18n::tr("settings.assoc_group_desc"))
        .margin_top(24)
        .build();

    let assoc_list = ListBox::builder().selection_mode(SelectionMode::None).build();
    assoc_list.add_css_class("boxed-list");

    let rebuild_cell: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let rebuild = build_rebuild(&config, &assoc_list, &rebuild_cell, parent);
    *rebuild_cell.borrow_mut() = Some(rebuild.clone());
    rebuild();

    let add_row = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .margin_top(8)
        .build();

    let ext_entry = gtk::Entry::builder()
        .placeholder_text(&*crate::i18n::tr("settings.assoc_ext_placeholder"))
        .width_request(110)
        .valign(Align::Center)
        .build();
    let app_entry = gtk::Entry::builder()
        .placeholder_text(&*crate::i18n::tr("settings.assoc_cmd_placeholder"))
        .hexpand(true)
        .valign(Align::Center)
        .build();
    let pick_btn = Button::builder()
        .label(&*crate::i18n::tr("common.browse"))
        .valign(Align::Center)
        .build();
    pick_btn.set_cursor_from_name(Some("pointer"));
    let add_btn = Button::builder()
        .label(&*crate::i18n::tr("settings.assoc_add"))
        .css_classes(vec!["suggested-action"])
        .valign(Align::Center)
        .build();
    add_btn.set_cursor_from_name(Some("pointer"));

    add_row.append(&ext_entry);
    add_row.append(&app_entry);
    add_row.append(&pick_btn);
    add_row.append(&add_btn);

    {
        let parent = parent.clone();
        let app_entry = app_entry.clone();
        pick_btn.connect_clicked(move |_| {
            let entry = app_entry.clone();
            let dialog = gtk::FileDialog::builder()
                .title(&*crate::i18n::tr("settings.assoc_pick_app"))
                .build();
            dialog.open(Some(&parent), gtk::gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        entry.set_text(&quote_if_needed(&path.to_string_lossy()));
                    }
                }
            });
        });
    }

    let commit_add: Rc<dyn Fn()> = {
        let config = config.clone();
        let ext_entry = ext_entry.clone();
        let app_entry = app_entry.clone();
        let rebuild = rebuild.clone();
        Rc::new(move || {
            let ext = ext_entry
                .text()
                .trim()
                .trim_start_matches('.')
                .to_lowercase();
            let app = app_entry.text().trim().to_string();
            if ext.is_empty() || app.is_empty() {
                return;
            }
            let mut current = crate::external::associations(&config);
            current.insert(ext, app);
            config.set(KEY, current);
            config.save();
            ext_entry.set_text("");
            app_entry.set_text("");
            rebuild();
        })
    };
    {
        let commit = commit_add.clone();
        add_btn.connect_clicked(move |_| commit());
    }
    {
        let commit = commit_add.clone();
        app_entry.connect_activate(move |_| commit());
    }
    {
        let commit = commit_add.clone();
        ext_entry.connect_activate(move |_| commit());
    }

    assoc_group.add(&assoc_list);
    assoc_group.add(&add_row);
    page_box.append(&assoc_group);
}

fn quote_if_needed(path: &str) -> String {
    if path.contains(char::is_whitespace) {
        format!("\"{path}\"")
    } else {
        path.to_string()
    }
}

fn build_rebuild(
    config: &client_config::AppConfig,
    assoc_list: &ListBox,
    rebuild_cell: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    parent: &gtk::Window,
) -> Rc<dyn Fn()> {
    let config = config.clone();
    let assoc_list = assoc_list.clone();
    let rebuild_weak = Rc::downgrade(rebuild_cell);
    let parent = parent.clone();

    Rc::new(move || {
        while let Some(child) = assoc_list.first_child() {
            assoc_list.remove(&child);
        }

        let map = crate::external::associations(&config);
        if map.is_empty() {
            assoc_list.append(
                &adw::ActionRow::builder()
                    .title(&*crate::i18n::tr("settings.assoc_empty"))
                    .subtitle(&*crate::i18n::tr("settings.assoc_empty_hint"))
                    .build(),
            );
            return;
        }

        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();

        for key in keys {
            let row = adw::ActionRow::builder().title(format!(".{key}")).build();

            let entry = gtk::Entry::builder()
                .text(map.get(key).map(String::as_str).unwrap_or_default())
                .valign(Align::Center)
                .width_request(220)
                .build();

            let apply: Rc<dyn Fn(&gtk::Entry)> = {
                let config = config.clone();
                let key = key.clone();
                Rc::new(move |e: &gtk::Entry| {
                    let value = e.text().trim().to_string();
                    let mut current = crate::external::associations(&config);
                    if current.get(&key).map(String::as_str) == Some(value.as_str()) {
                        return;
                    }
                    current.insert(key.clone(), value);
                    config.set(KEY, current);
                    config.save();
                })
            };
            {
                let apply = apply.clone();
                entry.connect_activate(move |e| apply(e));
            }
            {
                let apply = apply.clone();
                let entry_c = entry.clone();
                let focus = gtk::EventControllerFocus::new();
                focus.connect_leave(move |_| apply(&entry_c));
                entry.add_controller(focus);
            }

            let pick_img = gtk::Image::from_icon_name("document-open-symbolic");
            let pick_btn = Button::builder()
                .child(&pick_img)
                .css_classes(["flat"])
                .valign(Align::Center)
                .tooltip_text(&*crate::i18n::tr("settings.assoc_pick_app"))
                .build();
            pick_btn.set_cursor_from_name(Some("pointer"));
            {
                let parent = parent.clone();
                let entry = entry.clone();
                let apply = apply.clone();
                pick_btn.connect_clicked(move |_| {
                    let entry = entry.clone();
                    let apply = apply.clone();
                    let dialog = gtk::FileDialog::builder()
                        .title(&*crate::i18n::tr("settings.assoc_pick_app"))
                        .build();
                    dialog.open(Some(&parent), gtk::gio::Cancellable::NONE, move |res| {
                        if let Ok(file) = res {
                            if let Some(path) = file.path() {
                                entry.set_text(&quote_if_needed(&path.to_string_lossy()));
                                apply(&entry);
                            }
                        }
                    });
                });
            }

            let del_img = gtk::Image::from_resource("/com/icecommander/gtk/delete-file.svg");
            del_img.set_pixel_size(16);
            let del_btn = Button::builder()
                .child(&del_img)
                .css_classes(["flat"])
                .valign(Align::Center)
                .tooltip_text(&*crate::i18n::tr("settings.assoc_remove"))
                .build();
            del_btn.set_cursor_from_name(Some("pointer"));
            {
                let config = config.clone();
                let key = key.clone();
                let rebuild_weak = rebuild_weak.clone();
                del_btn.connect_clicked(move |_| {
                    let mut current = crate::external::associations(&config);
                    current.remove(&key);
                    config.set(KEY, current);
                    config.save();
                    if let Some(cell) = rebuild_weak.upgrade() {
                        let cb = cell.borrow().clone();
                        if let Some(cb) = cb {
                            cb();
                        }
                    }
                });
            }

            row.add_suffix(&entry);
            row.add_suffix(&pick_btn);
            row.add_suffix(&del_btn);
            assoc_list.append(&row);
        }
    })
}
