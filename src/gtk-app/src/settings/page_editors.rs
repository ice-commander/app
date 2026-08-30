use adw::prelude::*;
use gtk::{Align, Box, Button, Label, Orientation};

pub(super) fn build(page_box: &Box, parent: &gtk::Window, config: client_config::AppConfig) {
    let page_title = Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_editors")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .margin_bottom(16)
        .build();
    page_box.append(&page_title);

    let group = adw::PreferencesGroup::new();

    let is_external = config.get::<String>("ui.editor_type").as_deref() == Some("external");

    let type_row = adw::ComboRow::builder()
        .title(&*crate::i18n::tr("settings.editor_type"))
        .subtitle(&*crate::i18n::tr("settings.desc_editor_type"))
        .model(&gtk::StringList::new(&[
            &crate::i18n::tr("settings.editor_internal"),
            &crate::i18n::tr("settings.editor_external"),
        ]))
        .selected(u32::from(is_external))
        .build();
    group.add(&type_row);

    let path_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .valign(Align::Center)
        .build();
    let entry_path = gtk::Entry::builder()
        .text(
            &config
                .get::<String>("ui.external_editor_path")
                .unwrap_or_default(),
        )
        .placeholder_text(&*crate::i18n::tr("settings.editor_cmd_placeholder"))
        .valign(Align::Center)
        .width_request(240)
        .hexpand(true)
        .build();
    let browse_btn = Button::builder()
        .label(&*crate::i18n::tr("common.browse"))
        .valign(Align::Center)
        .build();
    browse_btn.set_cursor_from_name(Some("pointer"));
    path_box.append(&entry_path);
    path_box.append(&browse_btn);

    let row_path = adw::ActionRow::builder()
        .title(&*crate::i18n::tr("settings.editor_command"))
        .subtitle(&*crate::i18n::tr("settings.desc_editor_command"))
        .build();
    row_path.add_suffix(&path_box);
    row_path.set_sensitive(is_external);
    group.add(&row_path);

    let fast_save_row = adw::SwitchRow::builder()
        .title(&*crate::i18n::tr("settings.fast_save"))
        .subtitle(&*crate::i18n::tr("settings.desc_fast_save"))
        .active(config.get::<bool>("ui.fast_save").unwrap_or(false))
        .sensitive(!is_external)
        .build();
    group.add(&fast_save_row);

    {
        let config = config.clone();
        let row_path = row_path.clone();
        let fast_save_row = fast_save_row.clone();
        type_row.connect_selected_notify(move |row| {
            let external = row.selected() == 1;
            row_path.set_sensitive(external);
            fast_save_row.set_sensitive(!external);
            config.set(
                "ui.editor_type",
                if external { "external" } else { "internal" }.to_string(),
            );
            config.save();
        });
    }

    {
        let config = config.clone();
        fast_save_row.connect_active_notify(move |row| {
            config.set("ui.fast_save", row.is_active());
            config.save();
        });
    }

    let apply_path: std::rc::Rc<dyn Fn(&gtk::Entry)> = {
        let config = config.clone();
        std::rc::Rc::new(move |e: &gtk::Entry| {
            let text = e.text().trim().to_string();
            if config.get::<String>("ui.external_editor_path").unwrap_or_default() == text {
                return;
            }
            config.set("ui.external_editor_path", text);
            config.save();
        })
    };
    {
        let apply = apply_path.clone();
        entry_path.connect_activate(move |e| apply(e));
    }
    {
        let apply = apply_path.clone();
        let entry = entry_path.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(move |_| apply(&entry));
        entry_path.add_controller(focus);
    }

    {
        let parent = parent.clone();
        let entry_path = entry_path.clone();
        let apply = apply_path.clone();
        browse_btn.connect_clicked(move |_| {
            let entry = entry_path.clone();
            let apply = apply.clone();
            let dialog = gtk::FileDialog::builder()
                .title(&*crate::i18n::tr("settings.editor_pick"))
                .build();
            dialog.open(Some(&parent), gtk::gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        let text = path.to_string_lossy().to_string();
                        entry.set_text(&if text.contains(char::is_whitespace) {
                            format!("\"{text}\"")
                        } else {
                            text
                        });
                        apply(&entry);
                    }
                }
            });
        });
    }

    page_box.append(&group);
}
