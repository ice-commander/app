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
        config,
        None,
    );
    widget.set_margin_start(0);
    widget.set_margin_end(0);
    widget.set_margin_top(0);
    widget.set_margin_bottom(0);
    page_box.append(&widget);
}
