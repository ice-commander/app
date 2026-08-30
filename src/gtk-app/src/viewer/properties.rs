use adw::prelude::*;

pub(super) fn show_directory_properties(parent_window: &impl IsA<gtk::Window>, entry: &gtk_fm_ui::FileEntry) {
    let folder_icon = gtk::Image::builder()
        .resource("/com/icecommander/gtk/folder.svg")
        .pixel_size(80)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .build();

    let grid = gtk::Grid::builder()
        .row_spacing(8)
        .column_spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    let type_label = gtk::Label::builder()
        .label(&*crate::i18n::tr("player.dir_prop_type"))
        .halign(gtk::Align::Start)
        .css_classes(vec!["dim-label"])
        .build();
    let type_value = gtk::Label::builder()
        .label(&*crate::i18n::tr("player.dir_prop_directory"))
        .halign(gtk::Align::Start)
        .build();
    grid.attach(&type_label, 0, 0, 1, 1);
    grid.attach(&type_value, 1, 0, 1, 1);

    let name_label = gtk::Label::builder()
        .label(&*crate::i18n::tr("player.dir_prop_name"))
        .halign(gtk::Align::Start)
        .css_classes(vec!["dim-label"])
        .build();
    let name_value = gtk::Label::builder()
        .label(&entry.name())
        .halign(gtk::Align::Start)
        .selectable(true)
        .build();
    grid.attach(&name_label, 0, 1, 1, 1);
    grid.attach(&name_value, 1, 1, 1, 1);

    let path_label = gtk::Label::builder()
        .label(&*crate::i18n::tr("player.dir_prop_path"))
        .halign(gtk::Align::Start)
        .css_classes(vec!["dim-label"])
        .build();
    let path_value = gtk::Label::builder()
        .label(&entry.path())
        .halign(gtk::Align::Start)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();
    grid.attach(&path_label, 0, 2, 1, 1);
    grid.attach(&path_value, 1, 2, 1, 1);

    let date = entry.date();
    if !date.is_empty() {
        let date_label = gtk::Label::builder()
            .label(&*crate::i18n::tr("player.dir_prop_date"))
            .halign(gtk::Align::Start)
            .css_classes(vec!["dim-label"])
            .build();
        let date_value = gtk::Label::builder()
            .label(&date)
            .halign(gtk::Align::Start)
            .selectable(true)
            .build();
        grid.attach(&date_label, 0, 3, 1, 1);
        grid.attach(&date_value, 1, 3, 1, 1);
    }

    let content_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    content_box.append(&folder_icon);
    content_box.append(&grid);

    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr("player.directory_properties_title"))
        .extra_child(&content_box)
        .build();

    dialog.add_response("copy", &*crate::i18n::tr("player.copy_path_btn"));
    dialog.add_response("close", &*crate::i18n::tr("player.close_btn"));
    dialog.set_default_response(Some("close"));
    dialog.set_response_appearance("copy", adw::ResponseAppearance::Suggested);

    let path_str = entry.path();
    dialog.connect_response(None, move |d, response| {
        if response == "copy" {
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&path_str);
            }
        }
        d.close();
    });
    dialog.present(Some(parent_window.upcast_ref::<gtk::Window>()));
}
