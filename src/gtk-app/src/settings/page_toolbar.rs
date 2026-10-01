use adw::prelude::*;
use gtk::{Align, Box, Label};
use std::rc::Rc;

pub const BUTTONS: &[(&str, &str, &str, bool)] = &[
    (
        "ui.toolbar.terminal",
        "settings.toolbar_terminal",
        "settings.desc_toolbar_terminal",
        true,
    ),
    (
        "ui.toolbar.search",
        "settings.toolbar_search",
        "settings.desc_toolbar_search",
        true,
    ),
];

/// A row for every button a plugin put on the toolbar, and for every panel it
/// offers. The application does not know their names: the plugin supplies the
/// key and the wording.
fn plugin_buttons() -> Vec<(String, String, String)> {
    let named = |text: &str, fallback: &str| -> String {
        let shown =
            crate::connection_manager::translate_optional(text).unwrap_or_else(|| text.to_string());
        if shown.trim().is_empty() {
            fallback.to_string()
        } else {
            shown
        }
    };
    let mut rows = Vec::new();
    for side in [ic_plugin_api::IC_SIDE_LEFT, ic_plugin_api::IC_SIDE_RIGHT] {
        for entry in crate::plugin_host::toolbar_entries(side) {
            rows.push((
                format!("ui.toolbar.{}", entry.id),
                named(&entry.tooltip, &entry.id),
                crate::i18n::tr("settings.desc_toolbar_plugin").to_string(),
            ));
        }
    }
    for source in crate::plugin_host::panel_sources() {
        rows.push((
            format!("ui.toolbar.{}", source.id),
            named(&source.title, &source.id),
            crate::i18n::tr("settings.desc_toolbar_plugin").to_string(),
        ));
    }
    rows.sort();
    rows.dedup_by(|a, b| a.0 == b.0);
    rows
}

pub fn shown(config: &client_config::AppConfig, key: &str) -> bool {
    let default = BUTTONS
        .iter()
        .find(|(k, ..)| *k == key)
        .map(|(.., d)| *d)
        .unwrap_or(true);
    config.get::<bool>(key).unwrap_or(default)
}

pub(super) fn build(
    page_box: &Box,
    config: client_config::AppConfig,
    on_changed: Rc<dyn Fn() + 'static>,
) {
    let title = Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_toolbar")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .margin_bottom(16)
        .build();
    page_box.append(&title);

    let restart_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(12)
        .halign(Align::Start)
        .visible(false)
        .build();
    let warning = Label::builder()
        .use_markup(true)
        .halign(Align::Start)
        .build();
    warning.set_markup(&format!(
        "<span foreground='orange'><b>{}</b></span>",
        crate::i18n::tr("restart_required")
    ));
    let restart = gtk::Button::builder()
        .label(&*crate::i18n::tr("restart_now"))
        .build();
    restart.connect_clicked(|_| ic_utils::app::restart_app());
    restart_box.append(&warning);
    restart_box.append(&restart);
    let group = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.toolbar_group"))
        .description(&*crate::i18n::tr("settings.desc_toolbar_group"))
        .build();

    for (key, title_key, subtitle_key, default) in BUTTONS {
        let row = adw::SwitchRow::builder()
            .title(&*crate::i18n::tr(title_key))
            .subtitle(&*crate::i18n::tr(subtitle_key))
            .active(config.get::<bool>(key).unwrap_or(*default))
            .build();
        let config = config.clone();
        let on_changed = on_changed.clone();
        let key = *key;
        let reveal = restart_box.clone();
        row.connect_active_notify(move |row| {
            config.set(key, row.is_active());
            config.save();
            reveal.set_visible(true);
            on_changed();
        });
        group.add(&row);
    }

    for (key, title, subtitle) in plugin_buttons() {
        let row = adw::SwitchRow::builder()
            .title(&title)
            .subtitle(&subtitle)
            .active(shown(&config, &key))
            .build();
        let config = config.clone();
        let on_changed = on_changed.clone();
        let reveal = restart_box.clone();
        row.connect_active_notify(move |row| {
            config.set(&key, row.is_active());
            config.save();
            reveal.set_visible(true);
            on_changed();
        });
        group.add(&row);
    }

    page_box.append(&group);
    page_box.append(&restart_box);
}
