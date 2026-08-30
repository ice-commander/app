use adw::prelude::*;
use gtk::{Align, Box, Button, Label, Orientation};

const TARGET_KEYS: [ic_logging::Target; 4] = [
    ic_logging::Target::Off,
    ic_logging::Target::Console,
    ic_logging::Target::File,
    ic_logging::Target::Both,
];

const LEVEL_KEYS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

pub(super) fn build(page_box: &Box, parent: &gtk::Window, config: client_config::AppConfig) {
    let page_title = Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_logging")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .margin_bottom(16)
        .build();
    page_box.append(&page_title);

    let group = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.log_group"))
        .description(&*crate::i18n::tr("settings.log_group_desc"))
        .build();

    let target_row = adw::ComboRow::builder()
        .title(&*crate::i18n::tr("settings.log_target"))
        .subtitle(&*crate::i18n::tr("settings.desc_log_target"))
        .model(&gtk::StringList::new(&[
            &crate::i18n::tr("settings.log_target_off"),
            &crate::i18n::tr("settings.log_target_console"),
            &crate::i18n::tr("settings.log_target_file"),
            &crate::i18n::tr("settings.log_target_both"),
        ]))
        .build();
    let current_target = crate::logging::target(&config);
    target_row.set_selected(
        TARGET_KEYS.iter().position(|t| *t == current_target).unwrap_or(0) as u32,
    );
    group.add(&target_row);

    let level_row = adw::ComboRow::builder()
        .title(&*crate::i18n::tr("settings.log_level"))
        .subtitle(&*crate::i18n::tr("settings.desc_log_level"))
        .model(&gtk::StringList::new(&[
            &crate::i18n::tr("settings.log_level_error"),
            &crate::i18n::tr("settings.log_level_warn"),
            &crate::i18n::tr("settings.log_level_info"),
            &crate::i18n::tr("settings.log_level_debug"),
            &crate::i18n::tr("settings.log_level_trace"),
        ]))
        .build();
    let current_level = ic_logging::level_as_str(crate::logging::level(&config));
    level_row.set_selected(
        LEVEL_KEYS.iter().position(|l| *l == current_level).unwrap_or(2) as u32,
    );
    group.add(&level_row);

    let path_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .valign(Align::Center)
        .build();
    let entry_path = gtk::Entry::builder()
        .text(crate::logging::path(&config).to_string_lossy().as_ref())
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
        .title(&*crate::i18n::tr("settings.log_file"))
        .subtitle(&*crate::i18n::tr("settings.desc_log_file"))
        .build();
    row_path.add_suffix(&path_box);
    group.add(&row_path);

    let size_row = adw::SpinRow::builder()
        .title(&*crate::i18n::tr("settings.log_max_size"))
        .subtitle(&*crate::i18n::tr("settings.desc_log_max_size"))
        .adjustment(&gtk::Adjustment::new(
            crate::logging::max_mb(&config) as f64,
            1.0,
            1024.0,
            1.0,
            5.0,
            0.0,
        ))
        .build();
    group.add(&size_row);

    let open_btn = Button::builder()
        .label(&*crate::i18n::tr("settings.log_open"))
        .valign(Align::Center)
        .build();
    open_btn.set_cursor_from_name(Some("pointer"));
    let row_open = adw::ActionRow::builder()
        .title(&*crate::i18n::tr("settings.log_open"))
        .subtitle(&*crate::i18n::tr("settings.desc_log_open"))
        .build();
    row_open.add_suffix(&open_btn);
    group.add(&row_open);

    let refresh_sensitivity = {
        let row_path = row_path.clone();
        let size_row = size_row.clone();
        let row_open = row_open.clone();
        let target_row = target_row.clone();
        let level_row = level_row.clone();
        move || {
            let target = TARGET_KEYS
                .get(target_row.selected() as usize)
                .copied()
                .unwrap_or(ic_logging::Target::Off);
            let to_file = matches!(
                target,
                ic_logging::Target::File | ic_logging::Target::Both
            );
            row_path.set_sensitive(to_file);
            size_row.set_sensitive(to_file);
            row_open.set_sensitive(to_file);
            level_row.set_sensitive(target != ic_logging::Target::Off);
        }
    };
    refresh_sensitivity();

    {
        let config = config.clone();
        let refresh = refresh_sensitivity.clone();
        target_row.connect_selected_notify(move |row| {
            let target = TARGET_KEYS
                .get(row.selected() as usize)
                .copied()
                .unwrap_or(ic_logging::Target::Off);
            config.set("ui.log_target", target.as_str().to_string());
            config.set("ui.enable_logging", target != ic_logging::Target::Off);
            config.save();
            crate::logging::apply(&config);
            refresh();
        });
    }

    {
        let config = config.clone();
        level_row.connect_selected_notify(move |row| {
            let level = LEVEL_KEYS.get(row.selected() as usize).copied().unwrap_or("info");
            config.set("ui.log_level", level.to_string());
            config.save();
            crate::logging::apply(&config);
        });
    }

    {
        let config = config.clone();
        size_row.connect_value_notify(move |row| {
            config.set("ui.log_max_mb", row.value() as u64);
            config.save();
            crate::logging::apply(&config);
        });
    }

    let apply_path: std::rc::Rc<dyn Fn(&gtk::Entry)> = {
        let config = config.clone();
        std::rc::Rc::new(move |entry: &gtk::Entry| {
            let text = entry.text().trim().to_string();
            if text == crate::logging::path(&config).to_string_lossy() {
                return;
            }
            config.set("ui.log_file_path", text);
            config.save();
            crate::logging::apply(&config);
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
                .title(&*crate::i18n::tr("settings.log_pick_file"))
                .build();
            dialog.save(Some(&parent), gtk::gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        entry.set_text(&path.to_string_lossy());
                        apply(&entry);
                    }
                }
            });
        });
    }

    {
        let config = config.clone();
        open_btn.connect_clicked(move |_| {
            let path = crate::logging::path(&config);
            let target = if path.exists() {
                path.clone()
            } else {
                path.parent().map(|p| p.to_path_buf()).unwrap_or(path.clone())
            };
            crate::utils::open_with_system(&target);
        });
    }

    page_box.append(&group);
}
