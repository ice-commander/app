use adw::prelude::*;
use gtk::{Align, Box, Label};

pub(super) fn build(page_box: &Box, config: client_config::AppConfig) {
    let page_title = Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_transfer")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .margin_bottom(16)
        .build();
    page_box.append(&page_title);

    page_box.append(&build_timeouts(&config));
}

fn build_timeouts(config: &client_config::AppConfig) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.transfer_all_connections"))
        .description(&*crate::i18n::tr("settings.desc_timeouts"))
        .build();

    let row_connect = timeout_row(
        config,
        "net.connect_timeout_secs",
        20,
        localfs::set_connect_timeout_secs,
    );
    row_connect.set_title(&crate::i18n::tr("settings.connect_timeout"));
    row_connect.set_subtitle(&crate::i18n::tr("settings.desc_connect_timeout"));
    group.add(&row_connect);

    let row_request = timeout_row(
        config,
        "net.request_timeout_secs",
        20,
        localfs::set_request_timeout_secs,
    );
    row_request.set_title(&crate::i18n::tr("settings.request_timeout"));
    row_request.set_subtitle(&crate::i18n::tr("settings.desc_request_timeout"));
    group.add(&row_request);

    group
}

fn timeout_row(
    config: &client_config::AppConfig,
    key: &'static str,
    default: u64,
    apply: fn(u64),
) -> adw::SpinRow {
    let current = config.get::<u64>(key).unwrap_or(default);
    let adjustment = gtk::Adjustment::new(current as f64, 0.0, 3600.0, 1.0, 10.0, 0.0);
    let row = adw::SpinRow::builder().adjustment(&adjustment).build();
    let config = config.clone();
    row.connect_value_notify(move |r| {
        let secs = r.value() as u64;
        config.set(key, secs);
        config.save();
        apply(secs);
    });
    row
}
