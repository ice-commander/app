use adw::prelude::*;
use gtk::{Align, Box, Label};
use std::rc::Rc;

pub(super) fn build(
    page_box: &Box,
    parent: &gtk::Window,
    config: client_config::AppConfig,
    on_connections_changed: Rc<dyn Fn() + 'static>,
) {
    let page_title = Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_connections")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .margin_bottom(16)
        .build();
    page_box.append(&page_title);

    let widget = crate::connection_manager::create_manage_ftp_widget(
        parent,
        on_connections_changed,
        config.clone(),
        None,
    );
    page_box.append(&widget);

    page_box.append(&build_timeouts(&config));
}

fn build_timeouts(config: &client_config::AppConfig) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.timeouts_group"))
        .description(&*crate::i18n::tr("settings.desc_timeouts"))
        .margin_top(24)
        .build();

    let spin_connect = timeout_spin(
        config,
        "net.connect_timeout_secs",
        20,
        virtualfs::set_connect_timeout_secs,
    );
    let row_connect = adw::ActionRow::builder()
        .title(&*crate::i18n::tr("settings.connect_timeout"))
        .subtitle(&*crate::i18n::tr("settings.desc_connect_timeout"))
        .build();
    row_connect.add_suffix(&spin_connect);
    group.add(&row_connect);

    let spin_request = timeout_spin(
        config,
        "net.request_timeout_secs",
        20,
        virtualfs::set_request_timeout_secs,
    );
    let row_request = adw::ActionRow::builder()
        .title(&*crate::i18n::tr("settings.request_timeout"))
        .subtitle(&*crate::i18n::tr("settings.desc_request_timeout"))
        .build();
    row_request.add_suffix(&spin_request);
    group.add(&row_request);

    group
}

fn timeout_spin(
    config: &client_config::AppConfig,
    key: &'static str,
    default: u64,
    apply: fn(u64),
) -> gtk::SpinButton {
    let current = config.get::<u64>(key).unwrap_or(default);
    let adjustment = gtk::Adjustment::new(current as f64, 0.0, 3600.0, 1.0, 10.0, 0.0);
    let spin = gtk::SpinButton::new(Some(&adjustment), 1.0, 0);
    spin.set_valign(Align::Center);
    let config = config.clone();
    spin.connect_value_changed(move |s| {
        let secs = s.value() as u64;
        config.set(key, secs);
        config.save();
        apply(secs);
    });
    spin
}
