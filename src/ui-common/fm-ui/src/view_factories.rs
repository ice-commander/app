use gtk::prelude::*;
use relm4::ComponentSender;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::fm_view::{FmPanelModel, FmPanelOutput, Shared, ThumbnailFn};
use crate::utils::build_path_string;

thread_local! {
    static PAINTABLE_CACHE: RefCell<HashMap<String, gtk::gdk::Paintable>> = RefCell::new(HashMap::new());
}

pub fn generated_svg_paintable(svg_content: String, size: i32) -> Option<gtk::gdk::Paintable> {
    if let Some(paintable) = PAINTABLE_CACHE.with(|cache| cache.borrow().get(&svg_content).cloned())
    {
        return Some(paintable);
    }
    let paintable = svg_paintable_uncached(&svg_content, size)?;
    PAINTABLE_CACHE.with(|cache| {
        cache.borrow_mut().insert(svg_content, paintable.clone());
    });
    Some(paintable)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn svg_paintable_uncached(svg_content: &str, size: i32) -> Option<gtk::gdk::Paintable> {
    use resvg::{tiny_skia, usvg};

    thread_local! {
        static USVG_OPTIONS: usvg::Options<'static> = {
            let mut options = usvg::Options::default();
            options.fontdb_mut().load_system_fonts();
            options
        };
    }

    let tree = USVG_OPTIONS.with(|options| usvg::Tree::from_str(svg_content, options).ok())?;
    let pixels = (size.max(1) as u32) * 2;
    let mut pixmap = tiny_skia::Pixmap::new(pixels, pixels)?;
    let transform = tiny_skia::Transform::from_scale(
        pixels as f32 / tree.size().width(),
        pixels as f32 / tree.size().height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let stride = pixels as usize * 4;
    let bytes = gtk::glib::Bytes::from_owned(pixmap.take());
    let texture = gtk::gdk::MemoryTexture::new(
        pixels as i32,
        pixels as i32,
        gtk::gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &bytes,
        stride,
    );
    Some(texture.upcast())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn svg_paintable_uncached(svg_content: &str, size: i32) -> Option<gtk::gdk::Paintable> {
    let bytes = gtk::glib::Bytes::from(svg_content.as_bytes());
    let stream = gtk::gio::MemoryInputStream::from_bytes(&bytes);
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        size,
        size,
        true,
        gtk::gio::Cancellable::NONE,
    )
    .ok()?;
    Some(gtk::gdk::Texture::for_pixbuf(&pixbuf).upcast())
}

fn set_fixed_resource_icon(picture: &gtk::Picture, resource_path: &str, size: u32) {
    let cache_key = format!("res:{resource_path}@{size}");
    if let Some(p) = PAINTABLE_CACHE.with(|c| c.borrow().get(&cache_key).cloned()) {
        picture.set_paintable(Some(&p));
        return;
    }
    let sz = size as i32;
    match gtk::gdk_pixbuf::Pixbuf::from_resource_at_scale(resource_path, sz, sz, true) {
        Ok(pixbuf) => {
            let paintable: gtk::gdk::Paintable = gtk::gdk::Texture::for_pixbuf(&pixbuf).upcast();
            PAINTABLE_CACHE.with(|c| {
                c.borrow_mut().insert(cache_key, paintable.clone());
            });
            picture.set_paintable(Some(&paintable));
        }
        Err(_) => picture.set_resource(Some(resource_path)),
    }
}

fn to_gtk_ordering(o: std::cmp::Ordering) -> gtk::Ordering {
    match o {
        std::cmp::Ordering::Less => gtk::Ordering::Smaller,
        std::cmp::Ordering::Equal => gtk::Ordering::Equal,
        std::cmp::Ordering::Greater => gtk::Ordering::Larger,
    }
}

fn compare_directories(
    entry1: &crate::file_entry::FileEntry,
    entry2: &crate::file_entry::FileEntry,
    is_descending: bool,
) -> Option<gtk::Ordering> {
    if entry1.name() == ".." {
        return Some(if is_descending {
            gtk::Ordering::Larger
        } else {
            gtk::Ordering::Smaller
        });
    }
    if entry2.name() == ".." {
        return Some(if is_descending {
            gtk::Ordering::Smaller
        } else {
            gtk::Ordering::Larger
        });
    }
    if entry1.is_dir() != entry2.is_dir() {
        return Some(if entry1.is_dir() != is_descending {
            gtk::Ordering::Smaller
        } else {
            gtk::Ordering::Larger
        });
    }
    None
}

fn is_column_descending(column: &gtk::ColumnViewColumn, sorter: &gtk::ColumnViewSorter) -> bool {
    let n = sorter.n_sort_columns();
    for i in 0..n {
        if let (Some(col), order) = sorter.nth_sort_column(i) {
            if col == *column {
                return order == gtk::SortType::Descending;
            }
        }
    }
    false
}

fn add_right_click_gesture<W: IsA<gtk::Widget>>(
    widget: &W,
    list_item: &gtk::ListItem,
    shared: &Shared,
    sender: &ComponentSender<FmPanelModel>,
    btn_delete: &gtk::Button,
    selection_model: &gtk::MultiSelection,
    btn_refresh: &gtk::Button,
    btn_chmod: &gtk::Button,
) {
    let gesture = gtk::GestureClick::builder().button(3).build();
    let shared_c = shared.clone();
    let sender_c = sender.clone();
    let item_clone = list_item.clone();
    let selection_model_c = selection_model.clone();
    let btn_delete_c = btn_delete.clone();
    let btn_refresh_c = btn_refresh.clone();
    let btn_chmod_c = btn_chmod.clone();

    gesture.connect_pressed(move |g, _, x, y| {
        if let Some(obj) = item_clone.item() {
            if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                if entry.name() == ".." {
                    return;
                }
                let pos = item_clone.position();
                if pos != gtk::INVALID_LIST_POSITION {
                    let selection = selection_model_c.selection();
                    if !selection.contains(pos) {
                        selection_model_c.select_item(pos, true);
                    }
                }
                let path_str = build_path_string(&shared_c.current_path.borrow());
                let popover = crate::context_menu::create_context_menu(
                    &entry.name(),
                    entry.size(),
                    entry.is_dir(),
                    path_str,
                    &shared_c,
                    shared_c.root_path.borrow().clone(),
                    &sender_c,
                    &btn_delete_c,
                    &btn_refresh_c,
                    &btn_chmod_c,
                );
                if let Some(widget) = g.widget() {
                    popover.set_parent(&widget);
                    g.set_state(gtk::EventSequenceState::Claimed);
                    let rect = gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
                    popover.set_pointing_to(Some(&rect));
                    popover.popup();
                }
            }
        }
    });
    widget.add_controller(gesture);
}

pub fn list_row_metrics(config: &client_config::AppConfig) -> (i32, i32) {
    match config.get::<String>("ui.fm_list_row_size").as_deref() {
        Some("compact") => (24, 3),
        Some("tiny") => (20, 2),
        _ => (30, 4),
    }
}

fn commit_inline_rename(
    shared: &Shared,
    sender: &ComponentSender<FmPanelModel>,
    old_name: &str,
    new_name: &str,
) {
    if new_name.is_empty() || new_name == old_name || new_name.contains('/') {
        return;
    }
    let parent = shared.current_path.borrow().clone();
    let old_path = {
        let mut p = parent.clone();
        p.push(old_name.to_string());
        build_path_string(&p)
    };
    let new_path = {
        let mut p = parent.clone();
        p.push(new_name.to_string());
        build_path_string(&p)
    };
    let _ = sender.output(FmPanelOutput::Rename { old_path, new_path });
}

pub fn setup_column_view(
    column_view: &gtk::ColumnView,
    shared: &Shared,
    sender: &ComponentSender<FmPanelModel>,
    btn_delete: gtk::Button,
    btn_refresh: gtk::Button,
    btn_chmod: gtk::Button,
    _list_store: gtk::gio::ListStore,
    selection_model: gtk::MultiSelection,
) -> (
    gtk::ColumnViewColumn,
    gtk::ColumnViewColumn,
    gtk::ColumnViewColumn,
) {
    let config = shared.config.clone();

    let name_factory = gtk::SignalListItemFactory::new();
    {
        let shared = shared.clone();
        let sender = sender.clone();
        let btn_delete = btn_delete.clone();
        let btn_refresh = btn_refresh.clone();
        let btn_chmod = btn_chmod.clone();
        let selection_model = selection_model.clone();
        name_factory.connect_setup(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let cell_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(12)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(8)
                .margin_end(12)
                .build();
            let image_wg = gtk::Image::builder().pixel_size(30).build();
            let name_label = gtk::Label::builder()
                .halign(gtk::Align::Start)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            let name_entry = gtk::Entry::builder().hexpand(true).visible(false).build();
            let name_vbox = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .hexpand(true)
                .build();
            name_vbox.append(&name_label);
            name_vbox.append(&name_entry);
            cell_box.append(&image_wg);
            cell_box.append(&name_vbox);
            list_item.set_child(Some(&cell_box));

            add_right_click_gesture(
                &cell_box,
                list_item,
                &shared,
                &sender,
                &btn_delete,
                &selection_model,
                &btn_refresh,
                &btn_chmod,
            );

            let list_item_inner = list_item.clone();
            let shared_inner = shared.clone();
            let sender_inner = sender.clone();
            let entry_clone = name_entry.clone();
            let name_label_inner = name_label.clone();
            name_entry.connect_activate(move |_| {
                if let Some(obj) = list_item_inner.item() {
                    if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                        let old_name = entry.name();
                        let is_editing =
                            { *shared_inner.editing_name.borrow() == Some(old_name.clone()) };
                        if is_editing {
                            let new_name = entry_clone.text().to_string();
                            commit_inline_rename(
                                &shared_inner,
                                &sender_inner,
                                &old_name,
                                &new_name,
                            );
                        }
                    }
                }
                *shared_inner.editing_name.borrow_mut() = None;
                name_label_inner.set_visible(true);
                entry_clone.set_visible(false);
            });

            let key = gtk::EventControllerKey::new();
            let list_item_key = list_item.clone();
            let shared_key = shared.clone();
            let entry_esc = name_entry.clone();
            let label_esc = name_label.clone();
            key.connect_key_pressed(move |_, keyval, _, _| {
                if keyval == gtk::gdk::Key::Escape {
                    if let Some(entry) = list_item_key
                        .item()
                        .and_downcast::<crate::file_entry::FileEntry>()
                    {
                        let is_editing =
                            { *shared_key.editing_name.borrow() == Some(entry.name()) };
                        if is_editing {
                            *shared_key.editing_name.borrow_mut() = None;
                            label_esc.set_visible(true);
                            entry_esc.set_visible(false);
                            return gtk::glib::Propagation::Stop;
                        }
                    }
                }
                gtk::glib::Propagation::Proceed
            });
            name_entry.add_controller(key);
        });
    }
    {
        let shared = shared.clone();
        let config = config.clone();
        name_factory.connect_bind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let item = list_item
                .item()
                .and_downcast::<crate::file_entry::FileEntry>()
                .unwrap();
            let cell_box = list_item.child().unwrap().downcast::<gtk::Box>().unwrap();
            let image_wg = cell_box
                .first_child()
                .unwrap()
                .downcast::<gtk::Image>()
                .unwrap();
            let name_vbox = image_wg
                .next_sibling()
                .unwrap()
                .downcast::<gtk::Box>()
                .unwrap();
            let name_label = name_vbox
                .first_child()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap();
            let name_entry = name_label
                .next_sibling()
                .unwrap()
                .downcast::<gtk::Entry>()
                .unwrap();

            let (icon_px, margin_v) = list_row_metrics(&config);
            cell_box.set_margin_top(margin_v);
            cell_box.set_margin_bottom(margin_v);
            image_wg.set_pixel_size(icon_px);

            let name = item.name();
            let is_dir = item.is_dir();
            name_label.set_text(&name);
            name_label.set_tooltip_text(Some(&name));

            {
                let mut map = shared.active_widgets.borrow_mut();
                map.retain(|_, (l, _)| l != &name_label);
                map.insert(
                    (true, name.clone()),
                    (name_label.clone(), name_entry.clone()),
                );
            }
            let is_editing = { *shared.editing_name.borrow() == Some(name.clone()) };
            if is_editing {
                name_label.set_visible(false);
                name_entry.set_visible(true);
                name_entry.set_text(&name);
                let entry_c = name_entry.clone();
                gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                    let _ = entry_c.grab_focus();
                    entry_c.select_region(0, -1);
                    gtk::glib::ControlFlow::Break
                });
            } else {
                name_label.set_visible(true);
                name_entry.set_visible(false);
            }

            match crate::utils::get_file_icon(&name, is_dir, icon_px as u32, item.permissions()) {
                crate::utils::FileIcon::Resource(res) => {
                    image_wg.set_property("resource", res);
                }
                crate::utils::FileIcon::IconName(icon) => {
                    image_wg.set_icon_name(Some(&icon));
                }
                crate::utils::FileIcon::GeneratedSvg(svg) => {
                    if let Some(p) = generated_svg_paintable(svg, icon_px) {
                        image_wg.set_paintable(Some(&p));
                    }
                }
            }
        });
    }
    let name_column = gtk::ColumnViewColumn::builder()
        .title("Name")
        .factory(&name_factory)
        .expand(true)
        .resizable(true)
        .build();
    let name_column_weak = name_column.downgrade();
    let cv_w = column_view.downgrade();
    let name_sorter = gtk::CustomSorter::new(move |o1, o2| {
        let e1 = o1.downcast_ref::<crate::file_entry::FileEntry>().unwrap();
        let e2 = o2.downcast_ref::<crate::file_entry::FileEntry>().unwrap();
        let desc = column_descending(&name_column_weak, &cv_w);
        if let Some(ord) = compare_directories(e1, e2, desc) {
            return ord;
        }
        to_gtk_ordering(e1.name().to_lowercase().cmp(&e2.name().to_lowercase()))
    });
    name_column.set_sorter(Some(&name_sorter));
    column_view.append_column(&name_column);

    let date_factory = gtk::SignalListItemFactory::new();
    {
        let shared = shared.clone();
        let sender = sender.clone();
        let btn_delete = btn_delete.clone();
        let btn_refresh = btn_refresh.clone();
        let btn_chmod = btn_chmod.clone();
        let selection_model = selection_model.clone();
        date_factory.connect_setup(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let date_label = gtk::Label::builder()
                .halign(gtk::Align::Start)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(8)
                .margin_end(12)
                .css_classes(vec!["dim-label"])
                .build();
            list_item.set_child(Some(&date_label));
            add_right_click_gesture(
                &date_label,
                list_item,
                &shared,
                &sender,
                &btn_delete,
                &selection_model,
                &btn_refresh,
                &btn_chmod,
            );
        });
    }
    {
        let config = config.clone();
        date_factory.connect_bind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let item = list_item
                .item()
                .and_downcast::<crate::file_entry::FileEntry>()
                .unwrap();
            let date_label = list_item.child().unwrap().downcast::<gtk::Label>().unwrap();
            let (_, margin_v) = list_row_metrics(&config);
            date_label.set_margin_top(margin_v);
            date_label.set_margin_bottom(margin_v);
            let date = item.date();
            date_label.set_text(&date);
            date_label.set_tooltip_text(Some(&date));
        });
    }
    let date_column = gtk::ColumnViewColumn::builder()
        .title("Date Modified")
        .factory(&date_factory)
        .resizable(true)
        .build();
    let date_column_weak = date_column.downgrade();
    let cv_w = column_view.downgrade();
    let date_sorter = gtk::CustomSorter::new(move |o1, o2| {
        let e1 = o1.downcast_ref::<crate::file_entry::FileEntry>().unwrap();
        let e2 = o2.downcast_ref::<crate::file_entry::FileEntry>().unwrap();
        let desc = column_descending(&date_column_weak, &cv_w);
        if let Some(ord) = compare_directories(e1, e2, desc) {
            return ord;
        }
        to_gtk_ordering(e1.date().cmp(&e2.date()))
    });
    date_column.set_sorter(Some(&date_sorter));
    column_view.append_column(&date_column);

    let size_factory = gtk::SignalListItemFactory::new();
    {
        let shared = shared.clone();
        let sender = sender.clone();
        let btn_delete = btn_delete.clone();
        let btn_refresh = btn_refresh.clone();
        let btn_chmod = btn_chmod.clone();
        let selection_model = selection_model.clone();
        size_factory.connect_setup(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let size_label = gtk::Label::builder()
                .halign(gtk::Align::End)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(8)
                .margin_end(12)
                .css_classes(vec!["dim-label"])
                .build();
            list_item.set_child(Some(&size_label));
            add_right_click_gesture(
                &size_label,
                list_item,
                &shared,
                &sender,
                &btn_delete,
                &selection_model,
                &btn_refresh,
                &btn_chmod,
            );
        });
    }
    {
        let config = config.clone();
        size_factory.connect_bind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let item = list_item
                .item()
                .and_downcast::<crate::file_entry::FileEntry>()
                .unwrap();
            let size_label = list_item.child().unwrap().downcast::<gtk::Label>().unwrap();
            let (_, margin_v) = list_row_metrics(&config);
            size_label.set_margin_top(margin_v);
            size_label.set_margin_bottom(margin_v);
            if item.is_dir() {
                size_label.set_text("");
            } else {
                size_label.set_text(&crate::utils::format_size(item.size()));
            }
        });
    }
    let size_column = gtk::ColumnViewColumn::builder()
        .title("Size")
        .factory(&size_factory)
        .resizable(true)
        .build();
    let size_column_weak = size_column.downgrade();
    let cv_w = column_view.downgrade();
    let size_sorter = gtk::CustomSorter::new(move |o1, o2| {
        let e1 = o1.downcast_ref::<crate::file_entry::FileEntry>().unwrap();
        let e2 = o2.downcast_ref::<crate::file_entry::FileEntry>().unwrap();
        let desc = column_descending(&size_column_weak, &cv_w);
        if let Some(ord) = compare_directories(e1, e2, desc) {
            return ord;
        }
        if e1.is_dir() && e2.is_dir() {
            to_gtk_ordering(e1.name().to_lowercase().cmp(&e2.name().to_lowercase()))
        } else {
            to_gtk_ordering(e1.size().cmp(&e2.size()))
        }
    });
    size_column.set_sorter(Some(&size_sorter));
    column_view.append_column(&size_column);

    (name_column, date_column, size_column)
}

fn column_descending(
    column_weak: &gtk::glib::WeakRef<gtk::ColumnViewColumn>,
    cv_weak: &gtk::glib::WeakRef<gtk::ColumnView>,
) -> bool {
    if let (Some(col), Some(view)) = (column_weak.upgrade(), cv_weak.upgrade()) {
        if let Some(sorter) = view.sorter() {
            if let Ok(cv_sorter) = sorter.downcast::<gtk::ColumnViewSorter>() {
                return is_column_descending(&col, &cv_sorter);
            }
        }
    }
    false
}

pub fn create_grid_factory(
    shared: &Shared,
    sender: &ComponentSender<FmPanelModel>,
    btn_delete: gtk::Button,
    btn_refresh: gtk::Button,
    btn_chmod: gtk::Button,
    _list_store: gtk::gio::ListStore,
    selection_model: gtk::MultiSelection,
    thumbnailer: Option<ThumbnailFn>,
) -> gtk::SignalListItemFactory {
    let grid_factory = gtk::SignalListItemFactory::new();
    let panel_id = shared.panel_id.clone();
    let config = shared.config.clone();

    {
        let shared = shared.clone();
        let sender = sender.clone();
        let btn_delete = btn_delete.clone();
        let btn_refresh = btn_refresh.clone();
        let btn_chmod = btn_chmod.clone();
        let selection_model = selection_model.clone();
        grid_factory.connect_setup(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let child_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .valign(gtk::Align::Center)
                .halign(gtk::Align::Center)
                .build();
            let overlay = gtk::Overlay::builder().build();
            let picture_wg = gtk::Picture::builder()
                .halign(gtk::Align::Center)
                .valign(gtk::Align::Center)
                .content_fit(gtk::ContentFit::ScaleDown)
                .build();
            overlay.set_child(Some(&picture_wg));
            child_box.append(&overlay);
            let info_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .valign(gtk::Align::Center)
                .hexpand(true)
                .build();
            let name_label = gtk::Label::builder()
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::Char)
                .hexpand(true)
                .build();
            let name_entry = gtk::Entry::builder().visible(false).build();
            let details_label = gtk::Label::builder()
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .visible(false)
                .css_classes(vec!["caption", "dim-label"])
                .build();
            let time_label = gtk::Label::builder()
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .visible(false)
                .css_classes(vec!["caption", "dim-label"])
                .build();
            info_box.append(&name_label);
            info_box.append(&name_entry);
            info_box.append(&details_label);
            info_box.append(&time_label);
            child_box.append(&info_box);
            list_item.set_child(Some(&child_box));

            add_right_click_gesture(
                &child_box,
                list_item,
                &shared,
                &sender,
                &btn_delete,
                &selection_model,
                &btn_refresh,
                &btn_chmod,
            );

            let list_item_inner = list_item.clone();
            let shared_inner = shared.clone();
            let sender_inner = sender.clone();
            let entry_clone = name_entry.clone();
            let name_label_c = name_label.clone();
            name_entry.connect_activate(move |_| {
                if let Some(obj) = list_item_inner.item() {
                    if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                        let old_name = entry.name();
                        let is_editing =
                            { *shared_inner.editing_name.borrow() == Some(old_name.clone()) };
                        if is_editing {
                            let new_name = entry_clone.text().to_string();
                            commit_inline_rename(
                                &shared_inner,
                                &sender_inner,
                                &old_name,
                                &new_name,
                            );
                        }
                    }
                }
                *shared_inner.editing_name.borrow_mut() = None;
                name_label_c.set_visible(true);
                entry_clone.set_visible(false);
            });

            let key = gtk::EventControllerKey::new();
            let list_item_key = list_item.clone();
            let shared_key = shared.clone();
            let name_entry_esc = name_entry.clone();
            let name_label_esc = name_label.clone();
            key.connect_key_pressed(move |_, keyval, _, _| {
                if keyval == gtk::gdk::Key::Escape {
                    if let Some(entry) = list_item_key
                        .item()
                        .and_downcast::<crate::file_entry::FileEntry>()
                    {
                        let is_editing =
                            { *shared_key.editing_name.borrow() == Some(entry.name()) };
                        if is_editing {
                            *shared_key.editing_name.borrow_mut() = None;
                            name_label_esc.set_visible(true);
                            name_entry_esc.set_visible(false);
                            return gtk::glib::Propagation::Stop;
                        }
                    }
                }
                gtk::glib::Propagation::Proceed
            });
            name_entry.add_controller(key);
        });
    }

    let config_key = if panel_id == "default" {
        "ui.fm_grid_icon_size".to_string()
    } else {
        format!("ui.fm_grid_icon_size_{}", panel_id)
    };

    {
        let shared = shared.clone();
        let config = config.clone();
        let thumbnailer = thumbnailer.clone();
        grid_factory.connect_bind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let item = list_item
                .item()
                .and_downcast::<crate::file_entry::FileEntry>()
                .unwrap();
            let child_box = list_item.child().unwrap().downcast::<gtk::Box>().unwrap();
            let first = child_box.first_child().unwrap();
            let second = first.next_sibling().unwrap();
            let (overlay, info_box) = if first.is::<gtk::Overlay>() {
                (
                    first.downcast::<gtk::Overlay>().unwrap(),
                    second.downcast::<gtk::Box>().unwrap(),
                )
            } else {
                (
                    second.downcast::<gtk::Overlay>().unwrap(),
                    first.downcast::<gtk::Box>().unwrap(),
                )
            };
            let picture_wg = overlay.child().unwrap().downcast::<gtk::Picture>().unwrap();
            let name_label = info_box
                .first_child()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap();
            let name_entry = name_label
                .next_sibling()
                .unwrap()
                .downcast::<gtk::Entry>()
                .unwrap();
            let details_label = name_entry
                .next_sibling()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap();
            let time_label = details_label
                .next_sibling()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap();

            let fm_grid_icon_size = config.get::<u32>(&config_key).unwrap_or(80);

            child_box.set_orientation(gtk::Orientation::Vertical);
            child_box.set_halign(gtk::Align::Center);
            child_box.set_valign(gtk::Align::Center);
            child_box.set_width_request(110);
            child_box.set_height_request(-1);
            child_box.set_spacing(6);
            child_box.set_margin_start(0);
            child_box.set_margin_end(0);
            child_box.set_margin_top(0);
            child_box.set_margin_bottom(0);
            info_box.set_spacing(0);
            info_box.set_halign(gtk::Align::Center);
            info_box.set_valign(gtk::Align::Center);
            name_label.set_halign(gtk::Align::Center);
            name_label.set_valign(gtk::Align::Center);
            name_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            name_label.set_width_chars(15);
            name_label.set_max_width_chars(15);
            name_label.set_lines(2);
            name_label.set_wrap(true);
            name_entry.set_halign(gtk::Align::Center);
            details_label.set_visible(false);
            time_label.set_visible(false);
            let name = item.name();
            let is_dir = item.is_dir();
            name_label.set_text(&name);
            name_label.set_tooltip_text(Some(&name));


            {
                let mut map = shared.active_widgets.borrow_mut();
                map.retain(|_, (l, _)| l != &name_label);
                map.insert(
                    (false, name.clone()),
                    (name_label.clone(), name_entry.clone()),
                );
            }
            let is_editing = { *shared.editing_name.borrow() == Some(name.clone()) };
            if is_editing {
                name_label.set_visible(false);
                name_entry.set_visible(true);
                name_entry.set_text(&name);
                let entry_c = name_entry.clone();
                gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                    let _ = entry_c.grab_focus();
                    entry_c.select_region(0, -1);
                    gtk::glib::ControlFlow::Break
                });
            } else {
                name_label.set_visible(true);
                name_entry.set_visible(false);
            }

            let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
            let show_thumbs = config.get::<bool>("ui.show_thumbnails").unwrap_or(true);
            let is_image = !is_dir
                && show_thumbs
                && matches!(
                    ext.as_str(),
                    "png"
                        | "jpg"
                        | "jpeg"
                        | "gif"
                        | "webp"
                        | "svg"
                        | "nef"
                        | "cr2"
                        | "cr3"
                        | "arw"
                        | "dng"
                        | "raf"
                        | "orf"
                        | "rw2"
                        | "pef"
                );

            let svg_size_u32 = match (fm_grid_icon_size, is_image) {
                (30, _) => 30,
                (40, _) => 40,
                _ => 80,
            };
            let dim = fm_grid_icon_size as i32;
            picture_wg.set_size_request(dim, dim);
            overlay.set_halign(gtk::Align::Center);
            overlay.set_valign(gtk::Align::Center);
            overlay.set_size_request(dim, dim);

            match crate::utils::get_file_icon(&name, is_dir, svg_size_u32, item.permissions()) {
                crate::utils::FileIcon::Resource(res) => {
                    set_fixed_resource_icon(&picture_wg, &res, svg_size_u32);
                }
                crate::utils::FileIcon::IconName(_) => {
                    set_fixed_resource_icon(&picture_wg, "/com/fm-ui/gtk/file.svg", svg_size_u32);
                }
                crate::utils::FileIcon::GeneratedSvg(svg) => {
                    if let Some(p) = generated_svg_paintable(svg, svg_size_u32 as i32) {
                        picture_wg.set_paintable(Some(&p));
                    }
                }
            }

            if !is_dir {
                let lower_name = name.to_lowercase();
                let is_tar_compound =
                    lower_name.ends_with(".tar.gz") || lower_name.ends_with(".tar.bz2");
                let is_archive_or_pdf = is_tar_compound
                    || matches!(
                        ext.as_str(),
                        "zip" | "tar" | "tgz" | "gz" | "rar" | "7z" | "pdf" | "bz2" | "tbz2"
                            | "tbz"
                    );
                if !is_archive_or_pdf {
                    if let Some(tn) = &thumbnailer {
                        tn(&item.path(), &picture_wg);
                    }
                }
            }
        });
    }

    grid_factory
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_entry::FileEntry;
    use std::cmp::Ordering;

    fn file(name: &str) -> FileEntry {
        FileEntry::new(name, &format!("/{name}"), false, 10, "2026-01-01", None)
    }

    fn dir(name: &str) -> FileEntry {
        FileEntry::new(name, &format!("/{name}"), true, 0, "2026-01-01", None)
    }

    fn config(suffix: &str) -> client_config::AppConfig {
        client_config::AppConfig::new(&format!("_ice_test_fm_ui_{suffix}"))
    }

    #[test]
    fn rust_orderings_map_onto_gtk_orderings() {
        assert_eq!(to_gtk_ordering(Ordering::Less), gtk::Ordering::Smaller);
        assert_eq!(to_gtk_ordering(Ordering::Equal), gtk::Ordering::Equal);
        assert_eq!(to_gtk_ordering(Ordering::Greater), gtk::Ordering::Larger);
    }

    #[test]
    fn the_parent_entry_is_pinned_ahead_of_everything_when_ascending() {
        assert_eq!(
            compare_directories(&dir(".."), &dir("apps"), false),
            Some(gtk::Ordering::Smaller)
        );
        assert_eq!(
            compare_directories(&file("a.txt"), &dir(".."), false),
            Some(gtk::Ordering::Larger)
        );
    }

    #[test]
    fn the_parent_entry_inverts_when_descending_so_the_view_keeps_it_on_top() {
        assert_eq!(
            compare_directories(&dir(".."), &dir("apps"), true),
            Some(gtk::Ordering::Larger)
        );
        assert_eq!(
            compare_directories(&file("a.txt"), &dir(".."), true),
            Some(gtk::Ordering::Smaller)
        );
    }

    #[test]
    fn the_parent_entry_wins_over_another_parent_entry_deterministically() {
        assert_eq!(
            compare_directories(&dir(".."), &dir(".."), false),
            Some(gtk::Ordering::Smaller)
        );
    }

    #[test]
    fn directories_group_before_files_in_both_directions() {
        assert_eq!(
            compare_directories(&dir("apps"), &file("a.txt"), false),
            Some(gtk::Ordering::Smaller)
        );
        assert_eq!(
            compare_directories(&file("a.txt"), &dir("apps"), false),
            Some(gtk::Ordering::Larger)
        );
        assert_eq!(
            compare_directories(&dir("apps"), &file("a.txt"), true),
            Some(gtk::Ordering::Larger)
        );
        assert_eq!(
            compare_directories(&file("a.txt"), &dir("apps"), true),
            Some(gtk::Ordering::Smaller)
        );
    }

    #[test]
    fn entries_of_the_same_kind_are_left_to_the_column_comparator() {
        assert_eq!(compare_directories(&file("a.txt"), &file("b.txt"), false), None);
        assert_eq!(compare_directories(&dir("apps"), &dir("bin"), false), None);
        assert_eq!(compare_directories(&dir("apps"), &dir("bin"), true), None);
    }

    #[test]
    fn a_file_literally_named_dot_dot_is_not_treated_as_the_parent_row() {
        assert_eq!(
            compare_directories(&file(".."), &file("a.txt"), false),
            Some(gtk::Ordering::Smaller)
        );
    }

    #[test]
    fn list_row_metrics_default_to_the_normal_size() {
        let cfg = config("rows_default");
        assert_eq!(list_row_metrics(&cfg), (30, 4));
        cfg.set("ui.fm_list_row_size", "normal");
        assert_eq!(list_row_metrics(&cfg), (30, 4));
    }

    #[test]
    fn list_row_metrics_shrink_for_compact_and_tiny() {
        let cfg = config("rows_small");
        cfg.set("ui.fm_list_row_size", "compact");
        assert_eq!(list_row_metrics(&cfg), (24, 3));
        cfg.set("ui.fm_list_row_size", "tiny");
        assert_eq!(list_row_metrics(&cfg), (20, 2));
    }

    #[test]
    fn an_unknown_row_size_falls_back_to_the_normal_metrics() {
        let cfg = config("rows_unknown");
        cfg.set("ui.fm_list_row_size", "gigantic");
        assert_eq!(list_row_metrics(&cfg), (30, 4));
        cfg.set("ui.fm_list_row_size", 42);
        assert_eq!(list_row_metrics(&cfg), (30, 4));
    }
}
