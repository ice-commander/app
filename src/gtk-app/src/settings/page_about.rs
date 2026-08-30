use gtk::prelude::*;
use gtk::{Align, Box, Button, Image, Label, Orientation};

pub(super) fn build(page_box: &Box, parent: &gtk::Window) {
    let page_title = Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_about")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .margin_bottom(16)
        .build();
    page_box.append(&page_title);

    let about_box = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .halign(Align::Center)
        .valign(Align::Start)
        .hexpand(true)
        .vexpand(true)
        .build();

    let logo_img = Image::from_resource("/com/icecommander/gtk/app-logo.svg");
    logo_img.set_pixel_size(256);
    about_box.append(&logo_img);

    let app_name_lbl = Label::builder()
        .label("<span size='xx-large' weight='bold'>Ice Commander</span>")
        .use_markup(true)
        .build();
    about_box.append(&app_name_lbl);

    let version_lbl = Label::builder()
        .label(&format!(
            "<span color='gray' size='large'>{} {} ({})</span>",
            crate::i18n::tr("settings.version"),
            common::version::APP_VERSION,
            common::version::BUILD_TYPE
        ))
        .use_markup(true)
        .build();
    about_box.append(&version_lbl);

    let update_btn = Button::builder()
        .label(&*crate::i18n::tr("settings.check_update"))
        .margin_top(16)
        .halign(Align::Center)
        .css_classes(vec!["suggested-action"])
        .build();
    update_btn.set_cursor_from_name(Some("pointer"));

    let update_btn_clone = update_btn.clone();
    let parent_clone = parent.clone();
    update_btn.connect_clicked(move |_| {
        update_btn_clone.set_sensitive(false);
        update_btn_clone.set_label(&crate::i18n::tr("settings.checking_update"));

        let btn_inner = update_btn_clone.clone();
        let dialog_parent = parent_clone.clone();

        crate::updater::manual_check(dialog_parent, move || {
            btn_inner.set_label(&crate::i18n::tr("settings.check_update"));
            btn_inner.set_sensitive(true);
        });
    });
    about_box.append(&update_btn);

    page_box.append(&about_box);
}
