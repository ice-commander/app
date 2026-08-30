use crate::encoding::{encode_windows_1251, get_text_for_format};
use crate::metadata::query_file_metadata;
use crate::{Format, Mode};
use adw::prelude::*;
use gtk::{Align, Button, Label, Orientation, ScrolledWindow, TextView, Window};
use fm_core::rpc::FileSystemRpc;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

type RawHook = Option<Rc<dyn Fn(&[u8]) -> Option<Vec<u8>>>>;

pub(crate) fn load_image_paintable(pic: &gtk::Picture, data: &[u8], raw: &RawHook) {
    let bytes_glib = if let Some(extract) = raw {
        if let Some(raw_jpeg) = extract(data) {
            gtk::glib::Bytes::from(&raw_jpeg)
        } else {
            gtk::glib::Bytes::from(data)
        }
    } else {
        gtk::glib::Bytes::from(data)
    };
    let stream = gtk::gio::MemoryInputStream::from_bytes(&bytes_glib);
    if let Ok(pixbuf) =
        gtk::gdk_pixbuf::Pixbuf::from_stream(&stream, gtk::gio::Cancellable::NONE)
    {
        let pixbuf = pixbuf.apply_embedded_orientation().unwrap_or(pixbuf);
        let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
        pic.set_paintable(Some(&texture));
    } else {
        pic.set_paintable(None::<&gtk::gdk::Texture>);
    }
}

pub(crate) fn build_editor_content(
    window: &Window,
    root_stack: &gtk::Stack,
    file_path_str: Option<&str>,
    file_name: &str,
    services: crate::HostServices,
    start_in_edit_mode: bool,
    bytes: Vec<u8>,
    provider: Option<Rc<dyn FileSystemRpc>>,
    meta_opt: Option<(u64, u64)>,
) {
    let ext = file_path_str
        .map(|p| {
            Path::new(p)
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
        })
        .unwrap_or_default();
    let ext_is_image = matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" |
        "nef" | "cr2" | "cr3" | "arw" | "dng" | "raf" | "orf" | "rw2" | "pef"
    );
    let is_image = decide_image(&bytes, &ext, ext_is_image);

    let initial_format = if is_image {
        Format::Image
    } else if std::str::from_utf8(&bytes).is_ok() && !bytes.contains(&0) {
        Format::Utf8
    } else {
        Format::Hex
    };

    let read_only = provider.as_ref().map(|p| p.is_read_only()).unwrap_or(false);

    let initial_mode = if is_image || read_only {
        Mode::View
    } else if start_in_edit_mode {
        Mode::Edit
    } else {
        Mode::View
    };

    let current_mode = Rc::new(Cell::new(initial_mode));
    let current_format = Rc::new(Cell::new(initial_format));
    let last_text_format = Rc::new(Cell::new(if initial_format == Format::Image {
        Format::Hex
    } else {
        initial_format
    }));
    let is_modified = Rc::new(Cell::new(false));
    let ignore_changes = Rc::new(Cell::new(false));
    let initial_metadata = Rc::new(Cell::new(meta_opt));

    let file_path = Rc::new(RefCell::new(file_path_str.map(|s| s.to_string())));
    let file_name_cell = Rc::new(RefCell::new(file_name.to_string()));

    let main_box = gtk::Box::builder().orientation(Orientation::Vertical).build();

    let toolbar = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .margin_top(5)
        .margin_bottom(5)
        .margin_start(10)
        .margin_end(10)
        .build();
    toolbar.add_css_class("editor-toolbar");
    crate::style::ensure_loaded();

    let btn_close = Button::builder().tooltip_text(&*crate::i18n::tr("editor.tooltip_close")).build();
    let btn_close_content = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .build();
    let img_close = gtk::Image::from_resource("/com/icecommander/gtk/close.svg");
    img_close.set_pixel_size(20);
    let lbl_close = Label::new(Some(&*crate::i18n::tr("editor.close")));
    btn_close_content.append(&img_close);
    btn_close_content.append(&lbl_close);
    btn_close.set_child(Some(&btn_close_content));

    let btn_view = Button::builder()
        .tooltip_text(&*crate::i18n::tr("editor.tooltip_view"))
        .build();
    let btn_view_content = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .build();
    let img_view = gtk::Image::from_resource("/com/icecommander/gtk/fileexplorer.svg");
    img_view.set_pixel_size(20);
    let lbl_view = Label::new(Some(&*crate::i18n::tr("editor.view")));
    btn_view_content.append(&img_view);
    btn_view_content.append(&lbl_view);
    btn_view.set_child(Some(&btn_view_content));

    let btn_edit = Button::builder().tooltip_text(&*crate::i18n::tr("editor.tooltip_edit")).build();
    let btn_edit_content = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .build();
    let img_edit = gtk::Image::from_resource("/com/icecommander/gtk/edit-pencil.svg");
    img_edit.set_pixel_size(20);
    let lbl_edit = Label::new(Some(&*crate::i18n::tr("editor.edit")));
    btn_edit_content.append(&img_edit);
    btn_edit_content.append(&lbl_edit);
    btn_edit.set_child(Some(&btn_edit_content));

    let btn_save = Button::builder().tooltip_text(&*crate::i18n::tr("editor.tooltip_save")).build();
    let btn_save_content = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .build();
    let img_save = gtk::Image::from_resource("/com/icecommander/gtk/save.svg");
    img_save.set_pixel_size(20);
    let lbl_save = Label::new(Some(&*crate::i18n::tr("editor.save")));
    btn_save_content.append(&img_save);
    btn_save_content.append(&lbl_save);
    btn_save.set_child(Some(&btn_save_content));

    toolbar.append(&btn_close);
    toolbar.append(&btn_view);
    toolbar.append(&btn_edit);
    toolbar.append(&btn_save);

    let dropdown = if is_image {
        gtk::DropDown::from_strings(&[
            &*crate::i18n::tr("editor.format_image"),
            &*crate::i18n::tr("editor.format_utf8"),
            &*crate::i18n::tr("editor.format_ansi"),
            &*crate::i18n::tr("editor.format_hex"),
        ])
    } else {
        gtk::DropDown::from_strings(&[
            &*crate::i18n::tr("editor.format_utf8"),
            &*crate::i18n::tr("editor.format_ansi"),
            &*crate::i18n::tr("editor.format_hex"),
        ])
    };

    let dropdown_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .halign(Align::End)
        .hexpand(true)
        .build();
    let lbl_format = Label::new(Some(&*crate::i18n::tr("editor.format_label")));
    lbl_format.set_valign(Align::Center);
    dropdown_box.append(&lbl_format);
    dropdown_box.append(&dropdown);
    toolbar.append(&dropdown_box);

    let format_to_index = |fmt: Format, is_img: bool| -> u32 {
        match (fmt, is_img) {
            (Format::Image, true) => 0,
            (Format::Utf8, true) => 1,
            (Format::Ansi, true) => 2,
            (Format::Hex, true) => 3,
            (Format::Utf8, false) => 0,
            (Format::Ansi, false) => 1,
            (Format::Hex, false) => 2,
            _ => 0,
        }
    };

    let index_to_format = |idx: u32, is_img: bool| -> Format {
        match (idx, is_img) {
            (0, true) => Format::Image,
            (1, true) => Format::Utf8,
            (2, true) => Format::Ansi,
            (3, true) => Format::Hex,
            (0, false) => Format::Utf8,
            (1, false) => Format::Ansi,
            (2, false) => Format::Hex,
            _ => Format::Utf8,
        }
    };

    dropdown.set_selected(format_to_index(initial_format, is_image));

    let picture = gtk::Picture::new();
    picture.set_halign(Align::Center);
    picture.set_valign(Align::Center);

    let is_raw_file = matches!(
        ext.as_str(),
        "nef" | "cr2" | "cr3" | "arw" | "dng" | "raf" | "orf" | "rw2" | "pef"
    );
    let raw_hook: RawHook = if is_raw_file { services.raw_thumbnail.clone() } else { None };
    let raw_hook_dropdown = raw_hook.clone();
    let raw_hook_reload = raw_hook.clone();
    let raw_hook_btn = raw_hook.clone();

    if initial_format == Format::Image {
        load_image_paintable(&picture, &bytes, &raw_hook);
    }

    let image_scrolled = ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&picture)
        .build();

    let text_view = TextView::builder().monospace(true).build();
    let text_scrolled = ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&text_view)
        .build();

    let stack = gtk::Stack::new();
    stack.add_named(&image_scrolled, Some("image"));
    stack.add_named(&text_scrolled, Some("text"));

    let is_modified_hex = is_modified.clone();
    let hex_changed_ui: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let hex_changed_ui_cb = hex_changed_ui.clone();
    let hex_view = crate::hex_view::HexView::new(
        bytes.clone(),
        Rc::new(move || {
            is_modified_hex.set(true);
            if let Some(cb) = hex_changed_ui_cb.borrow().as_ref() {
                cb();
            }
        }),
    );
    stack.add_named(&hex_view.widget, Some("hex"));

    let buffer = text_view.buffer();

    if let Some(ref observer) = services.observer {
        observer.opened(
            file_path_str.unwrap_or_default(),
            if start_in_edit_mode { "edit" } else { "view" },
            &buffer,
        );
    }

    if !matches!(initial_format, Format::Image | Format::Hex) {
        let initial_text = get_text_for_format(initial_format, &bytes);
        buffer.set_text(&initial_text);
    }

    let update_ui_state = {
        let window = window.clone();
        let file_name_cell_clone = file_name_cell.clone();
        let current_mode = current_mode.clone();
        let current_format = current_format.clone();
        let btn_view = btn_view.clone();
        let btn_edit = btn_edit.clone();
        let btn_save = btn_save.clone();
        let text_view = text_view.clone();
        let stack = stack.clone();
        let hex_view_ui = hex_view.clone();

        move || {
            let mode = current_mode.get();
            let format = current_format.get();

            let title_prefix = match mode {
                Mode::View => "Viewer",
                Mode::Edit => "Editor",
            };
            window.set_title(Some(&format!(
                "{} - {}{}",
                title_prefix,
                *file_name_cell_clone.borrow(),
                if read_only {
                    format!(" ({})", crate::i18n::tr("editor.read_only"))
                } else {
                    String::new()
                }
            )));

            match mode {
                Mode::View => {
                    btn_view.set_visible(false);
                    btn_edit.set_visible(format != Format::Image && !read_only);
                    btn_save.set_visible(false);
                    text_view.set_editable(false);
                }
                Mode::Edit => {
                    btn_view.set_visible(format != Format::Image);
                    btn_edit.set_visible(false);
                    btn_save.set_visible(format != Format::Image);
                    text_view.set_editable(format != Format::Image);
                }
            }

            match format {
                Format::Image => stack.set_visible_child_name("image"),
                Format::Hex => {
                    stack.set_visible_child_name("hex");
                    hex_view_ui.set_editable(mode == Mode::Edit);
                }
                _ => stack.set_visible_child_name("text"),
            }
        }
    };
    let update_save_sensitivity = {
        let btn_save = btn_save.clone();
        let is_modified = is_modified.clone();
        let file_path = file_path.clone();
        move || {
            let is_new = file_path.borrow().is_none();
            btn_save.set_sensitive(is_modified.get() || is_new);
        }
    };
    let update_save_sensitivity = Rc::new(update_save_sensitivity);

    update_ui_state();
    update_save_sensitivity();

    let current_mode_view = current_mode.clone();
    let update_ui_view = update_ui_state.clone();
    btn_view.connect_clicked(move |_| {
        current_mode_view.set(Mode::View);
        update_ui_view();
    });

    let current_mode_edit = current_mode.clone();
    let update_ui_edit = update_ui_state.clone();
    btn_edit.connect_clicked(move |_| {
        current_mode_edit.set(Mode::Edit);
        update_ui_edit();
    });

    let win_close = window.clone();
    btn_close.connect_clicked(move |_| {
        win_close.close();
    });

    let is_modified_changed = is_modified.clone();
    let ignore_changes_cb = ignore_changes.clone();
    let update_save_sensitivity_cb = update_save_sensitivity.clone();
    buffer.connect_changed(move |_| {
        if !ignore_changes_cb.get() {
            is_modified_changed.set(true);
            update_save_sensitivity_cb();
        }
    });

    let dropdown_cb = dropdown.clone();
    let current_format_cb = current_format.clone();
    let is_modified_cb = is_modified.clone();
    let ignore_changes_cb = ignore_changes.clone();
    let buffer_cb = buffer.clone();
    let update_ui_cb = update_ui_state.clone();
    let picture_cb = picture.clone();
    let last_text_format_cb = last_text_format.clone();

    let original_bytes = Rc::new(RefCell::new(bytes));
    let original_bytes_cb = original_bytes.clone();

    let hex_view_cb = hex_view.clone();
    let raw_hook_cb = raw_hook_dropdown.clone();
    dropdown.connect_selected_notify(move |_dd| {
        let new_fmt = index_to_format(dropdown_cb.selected(), is_image);
        let prev_fmt = current_format_cb.get();
        if new_fmt == prev_fmt {
            return;
        }

        if new_fmt != Format::Image {
            last_text_format_cb.set(new_fmt);
        }

        if is_modified_cb.get() {
            let start = buffer_cb.start_iter();
            let end = buffer_cb.end_iter();
            let text = buffer_cb.text(&start, &end, false).to_string();

            let bytes_converted = match prev_fmt {
                Format::Utf8 => text.into_bytes(),
                Format::Ansi => encode_windows_1251(&text),
                Format::Hex => hex_view_cb.bytes(),
                Format::Image => original_bytes_cb.borrow().clone(),
            };

            match new_fmt {
                Format::Image => load_image_paintable(&picture_cb, &bytes_converted, &raw_hook_cb),
                Format::Hex => hex_view_cb.set_bytes(&bytes_converted),
                _ => {
                    let new_text = get_text_for_format(new_fmt, &bytes_converted);
                    ignore_changes_cb.set(true);
                    buffer_cb.set_text(&new_text);
                    ignore_changes_cb.set(false);
                }
            }

            current_format_cb.set(new_fmt);
            update_ui_cb();
        } else {
            let reload_bytes = original_bytes_cb.borrow().clone();

            if new_fmt == Format::Hex {
                hex_view_cb.set_bytes(&reload_bytes);
            } else if new_fmt == Format::Image {
                load_image_paintable(&picture_cb, &reload_bytes, &raw_hook_cb);
            } else {
                let new_text = get_text_for_format(new_fmt, &reload_bytes);
                ignore_changes_cb.set(true);
                buffer_cb.set_text(&new_text);
                ignore_changes_cb.set(false);
                is_modified_cb.set(false);
            }

            current_format_cb.set(new_fmt);
            update_ui_cb();
        }
    });

    let hex_view_reload = hex_view.clone();
    let reload_file = {
        let root_stack = root_stack.clone();
        let file_path = file_path.clone();
        let provider = provider.clone();
        let initial_metadata = initial_metadata.clone();
        let is_modified = is_modified.clone();
        let ignore_changes = ignore_changes.clone();
        let current_format = current_format.clone();
        let original_bytes = original_bytes.clone();
        let buffer = buffer.clone();
        let picture = picture.clone();
        let window = window.clone();
        let update_save_sensitivity = update_save_sensitivity.clone();

        move || {
            root_stack.set_visible_child_name("loading");
            let path_opt = file_path.borrow().clone();
            if let Some(path) = path_opt {
                let path_c = path.clone();
                let provider_c = provider.clone();
                let root_stack_c = root_stack.clone();
                let initial_metadata_c = initial_metadata.clone();
                let is_modified_c = is_modified.clone();
                let ignore_changes_c = ignore_changes.clone();
                let current_format_c = current_format.clone();
                let original_bytes_c = original_bytes.clone();
                let buffer_c = buffer.clone();
                let picture_c = picture.clone();
                let window_c = window.clone();
                let hex_view_c = hex_view_reload.clone();
                let raw_hook_c = raw_hook_reload.clone();
                let update_save_sensitivity_inner = update_save_sensitivity.clone();

                gtk::glib::spawn_future_local(async move {
                    let res = if let Some(ref p) = provider_c {
                        p.read_file_opt(path_c.clone(), None, crate::read_blocking(&path_c)).await
                    } else {
                        std::fs::read(std::path::Path::new(&path_c)).map_err(common::AppError::from)
                    };
                    match res {
                        Ok(bytes) => {
                            let new_meta = query_file_metadata(path_c, provider_c).await;
                            initial_metadata_c.set(new_meta);
                            *original_bytes_c.borrow_mut() = bytes.clone();
                            let fmt = current_format_c.get();
                            match fmt {
                                Format::Image => load_image_paintable(&picture_c, &bytes, &raw_hook_c),
                                Format::Hex => hex_view_c.set_bytes(&bytes),
                                _ => {
                                    let text = get_text_for_format(fmt, &bytes);
                                    ignore_changes_c.set(true);
                                    buffer_c.set_text(&text);
                                    ignore_changes_c.set(false);
                                }
                            }
                            is_modified_c.set(false);
                            update_save_sensitivity_inner();
                            root_stack_c.set_visible_child_name("content");
                        }
                        Err(e) => {
                            root_stack_c.set_visible_child_name("content");
                            let dialog = adw::AlertDialog::builder()
                                .heading(&*crate::i18n::tr("editor.reload_error"))
                                .body(&*crate::i18n::trf("editor.failed_reload", &[("error", &*(e.to_string()).to_string())]))
                                .build();
                            dialog.add_response("ok", &*crate::i18n::tr("editor.ok"));
                            dialog.present(Some(&window_c));
                        }
                    }
                });
            } else {
                root_stack.set_visible_child_name("content");
            }
        }
    };
    let reload_file = Rc::new(reload_file);

    let file_path_save = file_path.clone();
    let file_name_save = file_name_cell.clone();
    let buffer_save = buffer.clone();
    let current_format_save = current_format.clone();
    let last_text_format_save = last_text_format.clone();
    let is_modified_save = is_modified.clone();
    let services_save = services.clone();
    let services_prompt = services.clone();
    let services_res = services.clone();
    let original_bytes_save = original_bytes.clone();
    let picture_save = picture.clone();
    let btn_save_cb = btn_save.clone();
    let window_save = window.clone();
    let provider_save = provider.clone();
    let update_ui_cb = update_ui_state.clone();
    let initial_metadata_save = initial_metadata.clone();
    let reload_file_save = reload_file.clone();
    let update_save_sensitivity_save = update_save_sensitivity.clone();
    let hex_view_save = hex_view.clone();

    btn_save.connect_clicked(move |_| {
        let fmt = current_format_save.get();
        let parse_fmt = if fmt == Format::Image {
            last_text_format_save.get()
        } else {
            fmt
        };

        let start = buffer_save.start_iter();
        let end = buffer_save.end_iter();
        let text = buffer_save.text(&start, &end, false).to_string();

        let save_bytes = match parse_fmt {
            Format::Utf8 => text.into_bytes(),
            Format::Ansi => encode_windows_1251(&text),
            Format::Hex => hex_view_save.bytes(),
            Format::Image => original_bytes_save.borrow().clone(),
        };

        let is_new = file_path_save.borrow().is_none();

        let file_path_save_inner = file_path_save.clone();
        let file_name_save_inner = file_name_save.clone();
        let update_ui_cb_inner = update_ui_cb.clone();
        let window_save_inner = window_save.clone();
        let provider_save_inner = provider_save.clone();
        let btn_save_cb_inner = btn_save_cb.clone();
        let is_modified_save_inner = is_modified_save.clone();
        let original_bytes_save_inner = original_bytes_save.clone();
        let picture_save_inner = picture_save.clone();
        let initial_metadata_save_inner = initial_metadata_save.clone();
        let update_save_sensitivity_save_inner = update_save_sensitivity_save.clone();

        let do_save = {
            let raw_hook_save = raw_hook_btn.clone();
            let services_res = services_res.clone();
            let initial_metadata_save_inner = initial_metadata_save_inner.clone();
            let provider_save_inner = provider_save_inner.clone();
            let update_save_sensitivity_save_inner = update_save_sensitivity_save_inner.clone();
            Rc::new(move |path: String, bytes: Vec<u8>| {
                btn_save_cb_inner.set_sensitive(false);

                let btn_save_res = btn_save_cb_inner.clone();
                let is_modified_res = is_modified_save_inner.clone();
                let original_bytes_res = original_bytes_save_inner.clone();
                let fmt_res = fmt;
                let picture_res = picture_save_inner.clone();
                let save_bytes_res = bytes.clone();
                let window_res = window_save_inner.clone();
                let file_path_save_cb = file_path_save_inner.clone();
                let file_name_save_cb = file_name_save_inner.clone();
                let update_ui_cb_cb = update_ui_cb_inner.clone();
                let path_for_callback = path.clone();
                let initial_metadata_res = initial_metadata_save_inner.clone();
                let provider_res = provider_save_inner.clone();
                let update_save_sensitivity_res = update_save_sensitivity_save_inner.clone();
                let raw_hook_res = raw_hook_save.clone();
                let on_saved_res = services_res.on_saved.clone();

                gtk::glib::spawn_future_local(async move {
                    let res = if let Some(ref p) = provider_res {
                        p.write_file(path_for_callback.clone(), save_bytes_res.clone(), None, None).await
                    } else {
                        std::fs::write(Path::new(&path_for_callback), &save_bytes_res).map_err(common::AppError::from)
                    };
                    btn_save_res.set_sensitive(true);
                    match res {
                        Ok(()) => {
                            is_modified_res.set(false);
                            update_save_sensitivity_res();
                            *original_bytes_res.borrow_mut() = save_bytes_res.clone();
                            if fmt_res == Format::Image {
                                load_image_paintable(&picture_res, &save_bytes_res, &raw_hook_res);
                            }

                            let new_name = Path::new(&path_for_callback)
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            *file_path_save_cb.borrow_mut() = Some(path_for_callback.clone());
                            *file_name_save_cb.borrow_mut() = new_name;
                            update_ui_cb_cb();

                            on_saved_res();

                            let new_meta = query_file_metadata(path_for_callback, provider_res).await;
                            initial_metadata_res.set(new_meta);
                        }
                        Err(e) => {
                            let dialog = adw::AlertDialog::builder()
                                .heading(&*crate::i18n::tr("editor.save_error"))
                                .body(&*crate::i18n::trf("editor.failed_save", &[("error", &*(e.to_string()).to_string())]))
                                .build();
                            dialog.add_response("ok", &*crate::i18n::tr("editor.ok"));
                            dialog.present(Some(&window_res));
                        }
                    }
                });
            })
        };

        if is_new {
            let dialog = adw::AlertDialog::builder()
                .heading(&*crate::i18n::tr("editor.save_new_file"))
                .body(&*crate::i18n::tr("editor.enter_new_filename"))
                .build();

            let entry = gtk::Entry::builder()
                .placeholder_text(&*crate::i18n::tr("editor.placeholder_filename"))
                .activates_default(true)
                .build();
            dialog.set_extra_child(Some(&entry));

            dialog.add_response("cancel", &*crate::i18n::tr("editor.cancel"));
            dialog.add_response("save", &*crate::i18n::tr("editor.save"));
            dialog.set_default_response(Some("save"));
            dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

            let save_bytes_prompt = save_bytes.clone();
            let current_dir_prompt = services_prompt.current_dir.clone();
            let do_save_prompt = do_save.clone();
            dialog.connect_response(None, move |d, response| {
                if response == "save" {
                    let filename = entry.text().to_string().trim().to_string();
                    if !filename.is_empty() {
                        let parent_dir = current_dir_prompt();
                        let full_path = if parent_dir.ends_with('/') || parent_dir.is_empty() {
                            format!("{}{}", parent_dir, filename)
                        } else {
                            format!("{}/{}", parent_dir, filename)
                        };
                        do_save_prompt(full_path, save_bytes_prompt.clone());
                    }
                }
                d.close();
            });
            dialog.present(Some(&window_save));
        } else {
            let Some(path) = file_path_save.borrow().clone() else { return; };
            let fast_save = services_save.fast_save;

            let baseline = initial_metadata_save.get();

            if !fast_save && baseline.is_some() {
                let path_save = path.clone();
                let save_bytes_run = save_bytes.clone();
                let provider_query = provider_save.clone();
                let window_query = window_save.clone();
                let do_save_c = do_save.clone();
                let reload_file_c = reload_file_save.clone();

                gtk::glib::spawn_future_local(async move {
                    let new_meta = query_file_metadata(path, provider_query).await;
                    if let Some((base_size, base_mtime)) = baseline {
                        if let Some((new_size, new_mtime)) = new_meta {
                            if new_size != base_size || new_mtime != base_mtime {
                                let dialog = adw::AlertDialog::builder()
                                    .heading(&*crate::i18n::tr("editor.file_changed_externally"))
                                    .body(&*crate::i18n::tr("editor.file_changed_externally_body"))
                                    .build();
                                dialog.add_response("overwrite", &*crate::i18n::tr("editor.overwrite_anyway"));
                                dialog.add_response("reload", &*crate::i18n::tr("editor.reload_from_disk"));
                                dialog.add_response("cancel", &*crate::i18n::tr("editor.cancel"));
                                dialog.set_default_response(Some("cancel"));
                                dialog.set_response_appearance("overwrite", adw::ResponseAppearance::Destructive);
                                dialog.set_response_appearance("reload", adw::ResponseAppearance::Suggested);

                                let do_save_dialog = do_save_c.clone();
                                let reload_file_dialog = reload_file_c.clone();
                                let path_save_inner = path_save.clone();
                                let save_bytes_inner = save_bytes_run.clone();

                                dialog.connect_response(None, move |d, response| {
                                    match response {
                                        "overwrite" => {
                                            do_save_dialog(path_save_inner.clone(), save_bytes_inner.clone());
                                        }
                                        "reload" => {
                                            reload_file_dialog();
                                        }
                                        _ => {}
                                    }
                                    d.close();
                                });
                                dialog.present(Some(&window_query));
                            } else {
                                do_save_c(path_save, save_bytes_run);
                            }
                        } else {
                            let dialog = adw::AlertDialog::builder()
                                .heading(&*crate::i18n::tr("editor.file_not_found"))
                                .body(&*crate::i18n::tr("editor.file_deleted_externally_body"))
                                .build();
                            dialog.add_response("overwrite", &*crate::i18n::tr("editor.save_anyway"));
                            dialog.add_response("cancel", &*crate::i18n::tr("editor.cancel"));
                            dialog.set_default_response(Some("cancel"));
                            dialog.set_response_appearance("overwrite", adw::ResponseAppearance::Suggested);

                            let do_save_dialog = do_save_c.clone();
                            let path_save_inner = path_save.clone();
                            let save_bytes_inner = save_bytes_run.clone();

                            dialog.connect_response(None, move |d, response| {
                                if response == "overwrite" {
                                    do_save_dialog(path_save_inner.clone(), save_bytes_inner.clone());
                                }
                                d.close();
                            });
                            dialog.present(Some(&window_query));
                        }
                    } else {
                        do_save_c(path_save, save_bytes_run);
                    }
                });
            } else {
                do_save(path, save_bytes);
            }
        }
    });

    let sep = gtk::Separator::builder()
        .orientation(Orientation::Horizontal)
        .build();

    main_box.append(&toolbar);
    main_box.append(&sep);
    main_box.append(&stack);

    root_stack.add_named(&main_box, Some("content"));
    root_stack.set_visible_child_name("content");

    let key_controller = gtk::EventControllerKey::new();
    let win_key = window.clone();
    let current_mode_key = current_mode.clone();
    let current_format_key = current_format.clone();
    let update_ui_key = update_ui_state.clone();
    let btn_save_key = btn_save.clone();
    let hex_view_key = hex_view.clone();
    let services_key = services.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, state| {
        if keyval == gtk::gdk::Key::Escape {
            win_key.close();
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::F3 {
            current_mode_key.set(Mode::View);
            update_ui_key();
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::g
            && state.contains(gtk::gdk::ModifierType::CONTROL_MASK)
            && current_format_key.get() == Format::Hex
        {
            crate::hex_view::ask_for_offset(&win_key, hex_view_key.clone());
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::F4 {
            if current_format_key.get() != Format::Image && !read_only {
                current_mode_key.set(Mode::Edit);
                update_ui_key();
            }
            gtk::glib::Propagation::Stop
        } else {
            let is_save = {
                let pressed = crate::accel_string(keyval, state);
                let expected = services_key.save_hotkey.as_deref().unwrap_or("Ctrl+S");
                pressed.eq_ignore_ascii_case(expected)
            };

            if is_save {
                if btn_save_key.is_visible() && btn_save_key.is_sensitive() {
                    btn_save_key.emit_clicked();
                }
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        }
    });
    window.add_controller(key_controller);

    let is_modified_close = is_modified.clone();
    let window_close_req = window.clone();
    window.connect_close_request(move |_| {
        if is_modified_close.get() {
            let dialog = adw::AlertDialog::builder()
                .heading(&*crate::i18n::tr("editor.unsaved_changes"))
                .body(&*crate::i18n::tr("editor.discard_changes_body"))
                .build();
            dialog.add_response("discard", &*crate::i18n::tr("editor.discard"));
            dialog.add_response("cancel", &*crate::i18n::tr("editor.cancel"));
            dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);

            let is_modified_dialog = is_modified_close.clone();
            let win_dialog = window_close_req.clone();
            dialog.connect_response(None, move |d, response| {
                if response == "discard" {
                    is_modified_dialog.set(false);
                    win_dialog.close();
                }
                d.close();
            });
            dialog.present(Some(&window_close_req));
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });

    window.present();
}


fn decide_image(bytes: &[u8], ext: &str, ext_is_image: bool) -> bool {
    if looks_like_image(bytes, ext) {
        return true;
    }
    if !ext_is_image {
        return false;
    }
    !(std::str::from_utf8(bytes).is_ok() && !bytes.contains(&0))
}

fn looks_like_image(bytes: &[u8], ext: &str) -> bool {
    const SIGNATURES: [&[u8]; 8] = [
        b"\x89PNG\r\n\x1a\n",
        b"\xff\xd8\xff",
        b"GIF87a",
        b"GIF89a",
        b"BM",
        b"II*\x00",       // TIFF little-endian, also most raw formats
        b"MM\x00*",       // TIFF big-endian
        b"FUJIFILM",
    ];
    if SIGNATURES.iter().any(|sig| bytes.starts_with(sig)) {
        return true;
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return true;
    }
    if bytes.starts_with(b"\x00\x00\x01\x00") || bytes.starts_with(b"\x00\x00\x02\x00") {
        return true;
    }
    ext == "svg"
        && bytes
            .iter()
            .take(1024)
            .collect::<Vec<_>>()
            .windows(4)
            .any(|w| w == [&b'<', &b's', &b'v', &b'g'])
}

#[cfg(test)]
mod tests {
    use super::{decide_image, looks_like_image};

    #[test]
    fn a_photo_with_a_wrong_extension_is_still_an_image() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert!(looks_like_image(png, "dat"));
        assert!(looks_like_image(b"\xff\xd8\xff\xe0JFIF", "bin"));
    }

    #[test]
    fn text_named_png_is_not_an_image() {
        assert!(!looks_like_image(b"just a note, not a picture", "png"));
        assert!(!decide_image(b"just a note, not a picture", "png", true));
    }

    #[test]
    fn camera_raws_stay_images() {
        assert!(decide_image(b"IIRO\x08\x00\x00\x00", "orf", true));
        assert!(decide_image(b"IIU\x00\x18\x00\x00\x00", "rw2", true));
        assert!(decide_image(b"\x00\x00\x00\x18ftypcrx \x00\x00\x00\x01", "cr3", true));
        assert!(looks_like_image(b"II*\x00\x08\x00\x00\x00", "nef"));
    }

    #[test]
    fn webp_needs_both_riff_and_webp() {
        assert!(looks_like_image(b"RIFF\x00\x00\x00\x00WEBPVP8 ", "webp"));
        assert!(!looks_like_image(b"RIFF\x00\x00\x00\x00WAVEfmt ", "wav"));
    }

    #[test]
    fn svg_is_recognised_only_by_its_own_extension() {
        let svg = b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>";
        assert!(looks_like_image(svg, "svg"));
        assert!(!looks_like_image(svg, "txt"));
    }
}
