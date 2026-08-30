use adw::prelude::*;
use gtk::{Align, Box, Label, Orientation};
use std::rc::Rc;

const LANGUAGES: &[(&str, &str)] = &[
    ("English", "en"),
    ("Polski", "pl"),
    ("Čeština", "cs"),
    ("Slovenčina", "sk"),
    ("Deutsch", "de"),
    ("Español", "es"),
    ("Українська", "uk"),
    ("Italiano", "it"),
    ("Français", "fr"),
    ("Română", "ro"),
    ("Magyar", "hu"),
    ("Беларуская", "be"),
    ("Български", "bg"),
    ("Русский", "ru"),
    ("Српски", "sr"),
];

const ROW_SIZES: [&str; 3] = ["normal", "compact", "tiny"];
const OPEN_TARGETS: [&str; 4] = ["active", "opposite", "left", "right"];

pub(super) fn build(
    page_box: &Box,
    config: client_config::AppConfig,
    on_connections_changed: Rc<dyn Fn() + 'static>,
) {
    let page_title = Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_interface")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .margin_bottom(16)
        .build();
    page_box.append(&page_title);

    let restart_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_top(12)
        .halign(Align::Start)
        .visible(false)
        .build();
    let restart_warning = Label::builder().use_markup(true).halign(Align::Start).build();
    restart_warning.set_markup(&format!(
        "<span foreground='orange'><b>{}</b></span>",
        crate::i18n::tr("restart_required")
    ));
    let restart_btn = gtk::Button::builder()
        .label(&*crate::i18n::tr("settings.restart_btn"))
        .valign(Align::Center)
        .build();
    restart_btn.add_css_class("suggested-action");
    restart_btn.set_cursor_from_name(Some("pointer"));
    restart_btn.connect_clicked(|_| crate::utils::restart_app());
    restart_box.append(&restart_warning);
    restart_box.append(&restart_btn);

    let appearance = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.group_appearance"))
        .build();

    let names: Vec<&str> = LANGUAGES.iter().map(|(name, _)| *name).collect();
    let lang_row = adw::ComboRow::builder()
        .title(&*crate::i18n::tr("language_select"))
        .subtitle(&*crate::i18n::tr("settings.desc_lang"))
        .model(&gtk::StringList::new(&names))
        .build();
    let current_lang = config
        .get::<String>("ui.language")
        .unwrap_or_else(|| "en".to_string());
    lang_row.set_selected(
        LANGUAGES.iter().position(|(_, c)| *c == current_lang).unwrap_or(0) as u32,
    );
    {
        let config = config.clone();
        let restart_box = restart_box.clone();
        lang_row.connect_selected_notify(move |row| {
            let Some((_, code)) = LANGUAGES.get(row.selected() as usize) else {
                return;
            };
            if config.get::<String>("ui.language").unwrap_or_default() == *code {
                return;
            }
            config.set("ui.language", *code);
            config.save();
            crate::i18n::set_lang(code);
            restart_box.set_visible(true);
        });
    }
    appearance.add(&lang_row);

    let theme_row = adw::ComboRow::builder()
        .title(&*crate::i18n::tr("settings.theme_label"))
        .subtitle(&*crate::i18n::tr("settings.desc_theme"))
        .model(&gtk::StringList::new(&[
            &crate::i18n::tr("settings.theme_default"),
            &crate::i18n::tr("settings.theme_light"),
            &crate::i18n::tr("settings.theme_dark"),
        ]))
        .selected(config.get::<u32>("ui.theme_index").unwrap_or(0))
        .build();
    {
        let config = config.clone();
        theme_row.connect_selected_notify(move |row| {
            let selected = row.selected();
            let sm = adw::StyleManager::default();
            match selected {
                1 => sm.set_color_scheme(adw::ColorScheme::ForceLight),
                2 => sm.set_color_scheme(adw::ColorScheme::ForceDark),
                _ => sm.set_color_scheme(adw::ColorScheme::Default),
            }
            config.set("ui.theme_index", selected);
            config.save();
        });
    }
    appearance.add(&theme_row);

    let rowsize_row = adw::ComboRow::builder()
        .title(&*crate::i18n::tr("settings.list_row_size_title"))
        .subtitle(&*crate::i18n::tr("settings.list_row_size_desc"))
        .model(&gtk::StringList::new(&[
            &crate::i18n::tr("settings.list_row_size_normal"),
            &crate::i18n::tr("settings.list_row_size_compact"),
            &crate::i18n::tr("settings.list_row_size_tiny"),
        ]))
        .build();
    let current_rowsize = config
        .get::<String>("ui.fm_list_row_size")
        .unwrap_or_else(|| "normal".to_string());
    rowsize_row
        .set_selected(ROW_SIZES.iter().position(|s| *s == current_rowsize).unwrap_or(0) as u32);
    {
        let config = config.clone();
        let on_changed = on_connections_changed.clone();
        rowsize_row.connect_selected_notify(move |row| {
            let value = ROW_SIZES.get(row.selected() as usize).copied().unwrap_or("normal");
            config.set("ui.fm_list_row_size", value.to_string());
            config.save();
            on_changed();
        });
    }
    appearance.add(&rowsize_row);
    page_box.append(&appearance);

    let panels = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.group_panels"))
        .margin_top(24)
        .build();

    let drives_row = adw::ComboRow::builder()
        .title(&*crate::i18n::tr("settings.drives_toolbar"))
        .subtitle(&format!(
            "{} {}",
            crate::i18n::tr("settings.desc_drives_toolbar"),
            crate::i18n::tr("settings.shift_hint")
        ))
        .model(&gtk::StringList::new(&[
            &crate::i18n::tr("settings.drives_option_all"),
            &crate::i18n::tr("settings.drives_option_fav"),
        ]))
        .selected(u32::from(crate::favorites::is_favorites_only(&config)))
        .build();
    {
        let config = config.clone();
        let on_changed = on_connections_changed.clone();
        drives_row.connect_selected_notify(move |row| {
            crate::favorites::set_favorites_only(&config, row.selected() == 1);
            on_changed();
        });
    }
    panels.add(&drives_row);

    let target_row = adw::ComboRow::builder()
        .title(&*crate::i18n::tr("settings.open_target_title"))
        .subtitle(&*crate::i18n::tr("settings.open_target_desc"))
        .model(&gtk::StringList::new(&[
            &crate::i18n::tr("settings.open_target_active"),
            &crate::i18n::tr("settings.open_target_opposite"),
            &crate::i18n::tr("settings.open_target_left"),
            &crate::i18n::tr("settings.open_target_right"),
        ]))
        .build();
    let current_target = config
        .get::<String>("ui.open_connection_target")
        .unwrap_or_else(|| "active".to_string());
    target_row.set_selected(
        OPEN_TARGETS.iter().position(|t| *t == current_target).unwrap_or(0) as u32,
    );
    {
        let config = config.clone();
        target_row.connect_selected_notify(move |row| {
            let value = OPEN_TARGETS.get(row.selected() as usize).copied().unwrap_or("active");
            config.set("ui.open_connection_target", value.to_string());
            config.save();
        });
    }
    panels.add(&target_row);

    panels.add(&switch_row(
        "settings.shared_hidden_files",
        "settings.desc_shared_hidden_files",
        "ui.show_hidden_files_shared",
        true,
        &config,
        Some(on_connections_changed.clone()),
    ));
    panels.add(&switch_row(
        "settings.show_thumbnails",
        "settings.desc_show_thumbnails",
        "ui.show_thumbnails",
        true,
        &config,
        Some(on_connections_changed.clone()),
    ));
    panels.add(&switch_row(
        "settings.new_tab_focus",
        "settings.desc_new_tab_focus",
        "ui.new_tab_focus_new",
        true,
        &config,
        None,
    ));

    page_box.append(&panels);
    page_box.append(&restart_box);
}

fn switch_row(
    title_key: &str,
    subtitle_key: &str,
    config_key: &'static str,
    default: bool,
    config: &client_config::AppConfig,
    on_changed: Option<Rc<dyn Fn() + 'static>>,
) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(&*crate::i18n::tr(title_key))
        .subtitle(&*crate::i18n::tr(subtitle_key))
        .active(config.get::<bool>(config_key).unwrap_or(default))
        .build();
    let config = config.clone();
    row.connect_active_notify(move |row| {
        config.set(config_key, row.is_active());
        config.save();
        if let Some(cb) = &on_changed {
            cb();
        }
    });
    row
}
