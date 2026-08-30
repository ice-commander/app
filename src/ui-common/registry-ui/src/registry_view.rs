#![allow(deprecated)] // TreeView/TreeStore/CellRenderer* + adw::MessageDialog — modernization tracked separately

use adw::prelude::*;
use ic_model::{RegistryValueData, RegistryValueInfo};
use gdk_pixbuf::Pixbuf;
use gtk::glib;
use relm4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct RegistryInit {
    pub show_toolbar: bool,
}

impl Default for RegistryInit {
    fn default() -> Self {
        Self { show_toolbar: true }
    }
}

#[derive(Debug)]
pub enum RegistryInput {
    Entries {
        path: String,
        subkeys: Vec<String>,
        values: Vec<RegistryValueInfo>,
    },
    Reset,
    KeySelected { path: String },
    KeyExpanded { path: String },
    ValueSelected(Option<String>),
}

#[derive(Debug)]
pub enum RegistryOutput {
    RequestKeys { path: String },
    SetValue {
        path: String,
        value_name: String,
        data: RegistryValueData,
    },
    DeleteEntry {
        path: String,
        is_key: bool,
        value_name: Option<String>,
    },
    Back,
}

struct EntriesPayload {
    path: String,
    subkeys: Vec<String>,
}

pub struct RegistryModel {
    show_toolbar: bool,
    selected_path: String,
    selected_path_mirror: Rc<RefCell<String>>,
    path_parts: Vec<String>,
    values: Vec<RegistryValueInfo>,
    values_mirror: Rc<RefCell<Vec<RegistryValueInfo>>>,
    selected_value: Option<String>,
    selected_value_mirror: Rc<RefCell<Option<String>>>,
    last_entries: Option<EntriesPayload>,
    entries_generation: u64,
    pending_clear: bool,
    paned_idle_armed: Rc<Cell<bool>>,
    folder_pixbuf: Option<Pixbuf>,
}

impl RegistryModel {
    fn arm_paned_center(&self, paned: &gtk::Paned) {
        if self.paned_idle_armed.get() {
            return;
        }
        self.paned_idle_armed.set(true);
        let armed = self.paned_idle_armed.clone();
        let paned = paned.clone();
        glib::idle_add_local(move || {
            if !armed.get() {
                return glib::ControlFlow::Break;
            }
            let width = paned.width();
            if width > 0 {
                paned.set_position(width / 2);
                armed.set(false);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
}

pub struct RegistryWidgets {
    header: adw::HeaderBar,
    edit_btn: gtk::Button,
    delete_btn: gtk::Button,
    paned: gtk::Paned,
    tree_view: gtk::TreeView,
    tree_store: gtk::TreeStore,
    list_box: gtk::ListBox,
    selection_handler: glib::SignalHandlerId,
    row_selected_handler: glib::SignalHandlerId,
    rendered_generation: u64,
}

impl SimpleComponent for RegistryModel {
    type Init = RegistryInit;
    type Input = RegistryInput;
    type Output = RegistryOutput;
    type Root = gtk::Box;
    type Widgets = RegistryWidgets;

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        crate::init_resources();

        let selected_path_mirror = Rc::new(RefCell::new("/".to_string()));
        let values_mirror: Rc<RefCell<Vec<RegistryValueInfo>>> = Rc::new(RefCell::new(Vec::new()));
        let selected_value_mirror: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

        let empty_title = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .title_widget(&empty_title)
            .build();

        let btn_back = gtk::Button::builder()
            .child(&gtk::Image::from_resource(
                "/com/registry-ui/gtk/arrow-left.svg",
            ))
            .tooltip_text(&*crate::i18n::tr("registry.back_tooltip"))
            .build();
        btn_back.set_cursor_from_name(Some("pointer"));
        let sender_back = sender.clone();
        btn_back.connect_clicked(move |_| {
            let _ = sender_back.output(RegistryOutput::Back);
        });
        header.pack_start(&btn_back);

        header.pack_start(
            &gtk::Separator::builder()
                .orientation(gtk::Orientation::Vertical)
                .margin_top(8)
                .margin_bottom(8)
                .build(),
        );

        let edit_btn = gtk::Button::builder()
            .child(&gtk::Image::from_resource(
                "/com/registry-ui/gtk/edit-pencil.svg",
            ))
            .tooltip_text(&*crate::i18n::tr("registry.edit_value_tooltip"))
            .sensitive(false)
            .build();
        edit_btn.set_cursor_from_name(Some("pointer"));
        header.pack_start(&edit_btn);

        let delete_btn = gtk::Button::builder()
            .child(&gtk::Image::from_resource("/com/registry-ui/gtk/close.svg"))
            .tooltip_text(&*crate::i18n::tr("registry.delete_value_tooltip"))
            .sensitive(false)
            .css_classes(vec!["destructive-action"])
            .build();
        delete_btn.set_cursor_from_name(Some("pointer"));
        header.pack_start(&delete_btn);

        header.pack_start(
            &gtk::Separator::builder()
                .orientation(gtk::Orientation::Vertical)
                .margin_top(8)
                .margin_bottom(8)
                .build(),
        );

        let btn_refresh = gtk::Button::builder()
            .child(&gtk::Image::from_resource(
                "/com/registry-ui/gtk/refresh.svg",
            ))
            .tooltip_text(&*crate::i18n::tr("registry.refresh_tooltip"))
            .build();
        btn_refresh.set_cursor_from_name(Some("pointer"));
        header.pack_start(&btn_refresh);

        header.set_visible(init.show_toolbar);
        root.append(&header);

        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .vexpand(true)
            .build();
        paned.set_shrink_start_child(false);
        paned.set_shrink_end_child(false);
        paned.set_resize_start_child(true);
        paned.set_resize_end_child(true);

        let tree_store = gtk::TreeStore::new(&[
            Pixbuf::static_type(), // 0: icon
            glib::Type::STRING,    // 1: display name
            glib::Type::STRING,    // 2: full path
            glib::Type::BOOL,      // 3: is loaded
        ]);

        let tree_view = gtk::TreeView::builder()
            .model(&tree_store)
            .headers_visible(false)
            .build();

        let icon_renderer = gtk::CellRendererPixbuf::new();
        icon_renderer.set_ypad(6);
        let column = gtk::TreeViewColumn::new();
        column.pack_start(&icon_renderer, false);
        column.add_attribute(&icon_renderer, "pixbuf", 0);

        let renderer = gtk::CellRendererText::new();
        renderer.set_ypad(6);
        column.pack_start(&renderer, true);
        column.add_attribute(&renderer, "text", 1);
        tree_view.append_column(&column);

        let scrolled_tree = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&tree_view)
            .build();
        scrolled_tree.set_size_request(250, -1);
        paned.set_start_child(Some(&scrolled_tree));

        let list_box = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .activate_on_single_click(false)
            .css_classes(vec!["boxed-list", "registry-list-box"])
            .build();
        let scrolled_list = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&list_box)
            .build();
        paned.set_end_child(Some(&scrolled_list));

        root.append(&paned);

        let sender_sel = sender.clone();
        let selection_handler = tree_view.selection().connect_changed(move |sel| {
            if let Some((model, iter)) = sel.selected() {
                let path_str: String = model.get(&iter, 2);
                sender_sel.input(RegistryInput::KeySelected { path: path_str });
            }
        });

        let sender_exp = sender.clone();
        let expansion_store = tree_store.clone();
        tree_view.connect_row_expanded(move |_, iter, _| {
            let is_loaded: bool = expansion_store.get(iter, 3);
            if !is_loaded {
                let path_str: String = expansion_store.get(iter, 2);
                sender_exp.input(RegistryInput::KeyExpanded { path: path_str });
            }
        });

        let sender_rowsel = sender.clone();
        let row_selected_handler = list_box.connect_row_selected(move |_, row_opt| {
            let selected = row_opt.and_then(|row| {
                row.widget_name()
                    .as_str()
                    .strip_prefix("val:")
                    .map(str::to_string)
            });
            sender_rowsel.input(RegistryInput::ValueSelected(selected));
        });

        let values_edit = values_mirror.clone();
        let path_edit = selected_path_mirror.clone();
        let sender_edit = sender.clone();
        list_box.connect_row_activated(move |list, row| {
            let widget_name = row.widget_name().to_string();
            if !widget_name.starts_with("val:") {
                return;
            }
            let val_name = widget_name.replacen("val:", "", 1);
            let values = values_edit.borrow();
            let Some(val_info) = values.iter().find(|v| v.name == val_name) else {
                return;
            };

            let window = list.root().and_downcast::<gtk::Window>().unwrap();
            let val_display_name = if val_info.name.is_empty() {
                crate::i18n::tr("registry.default_value")
            } else {
                val_info.name.clone()
            };
            let editing_body = crate::i18n::trf("registry.editing_format", &[("name", &*(val_display_name).to_string())]);
            let dialog = adw::MessageDialog::builder()
                .heading(&*crate::i18n::tr("registry.edit_value_title"))
                .body(&*editing_body)
                .transient_for(&window)
                .modal(true)
                .build();

            let type_string = match &val_info.data {
                RegistryValueData::String(_) => crate::i18n::tr("registry.type_sz"),
                RegistryValueData::DWord(_) => crate::i18n::tr("registry.type_dword"),
                RegistryValueData::QWord(_) => crate::i18n::tr("registry.type_qword"),
                RegistryValueData::Binary(_) => crate::i18n::tr("registry.type_binary"),
                RegistryValueData::MultiString(_) => crate::i18n::tr("registry.type_multi_sz"),
                RegistryValueData::ExpandString(_) => crate::i18n::tr("registry.type_expand_sz"),
                RegistryValueData::Unknown => crate::i18n::tr("registry.type_unknown"),
            };
            let type_label = gtk::Label::builder()
                .label(&*type_string)
                .halign(gtk::Align::Start)
                .css_classes(vec!["dim-label"])
                .margin_bottom(8)
                .build();

            let entry_text = match &val_info.data {
                RegistryValueData::String(s) | RegistryValueData::ExpandString(s) => s.clone(),
                RegistryValueData::DWord(d) => d.to_string(),
                RegistryValueData::QWord(q) => q.to_string(),
                RegistryValueData::Binary(b) => b
                    .iter()
                    .map(|byte| format!("{:02X}", byte))
                    .collect::<Vec<String>>()
                    .join(" "),
                RegistryValueData::MultiString(m) => m.join("\\n"),
                _ => String::new(),
            };

            let entry = gtk::Entry::builder()
                .text(&entry_text)
                .activates_default(true)
                .build();

            let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
            vbox.append(&type_label);
            vbox.append(&entry);

            dialog.set_extra_child(Some(&vbox));
            dialog.add_response("cancel", &crate::i18n::tr("registry.cancel_btn"));
            dialog.add_response("save", &crate::i18n::tr("registry.save_btn"));
            dialog.set_default_response(Some("save"));
            dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

            let sender_save = sender_edit.clone();
            let path = path_edit.borrow().clone();
            let val_clone = val_info.clone();

            dialog.connect_response(None, move |d, response| {
                if response == "save" {
                    let new_text = entry.text().to_string();
                    let new_data = match &val_clone.data {
                        RegistryValueData::DWord(_) => {
                            if let Ok(num) = new_text.parse::<u32>() {
                                RegistryValueData::DWord(num)
                            } else {
                                val_clone.data.clone()
                            }
                        }
                        RegistryValueData::QWord(_) => {
                            if let Ok(num) = new_text.parse::<u64>() {
                                RegistryValueData::QWord(num)
                            } else {
                                val_clone.data.clone()
                            }
                        }
                        RegistryValueData::String(_) => RegistryValueData::String(new_text),
                        RegistryValueData::ExpandString(_) => {
                            RegistryValueData::ExpandString(new_text)
                        }
                        RegistryValueData::MultiString(_) => RegistryValueData::MultiString(
                            new_text.split("\\n").map(|s| s.to_string()).collect(),
                        ),
                        RegistryValueData::Binary(_) => {
                            let bytes: Vec<u8> = new_text
                                .split_whitespace()
                                .filter_map(|s| u8::from_str_radix(s, 16).ok())
                                .collect();
                            RegistryValueData::Binary(bytes)
                        }
                        _ => val_clone.data.clone(),
                    };

                    let _ = sender_save.output(RegistryOutput::SetValue {
                        path: path.clone(),
                        value_name: val_clone.name.clone(),
                        data: new_data,
                    });
                }
                d.close();
            });

            dialog.present();
        });

        let list_box_edit = list_box.clone();
        edit_btn.connect_clicked(move |_| {
            if let Some(row) = list_box_edit.selected_row() {
                row.activate();
            }
        });

        let sender_delete = sender.clone();
        let path_delete = selected_path_mirror.clone();
        let selected_delete = selected_value_mirror.clone();
        delete_btn.connect_clicked(move |btn| {
            let Some(val_name) = selected_delete.borrow().clone() else {
                return;
            };
            let window = btn.root().and_downcast::<gtk::Window>().unwrap();
            let val_display_name = if val_name.is_empty() {
                crate::i18n::tr("registry.default_value")
            } else {
                val_name.clone()
            };
            let delete_confirm_body =
                crate::i18n::trf("registry.delete_confirm_format", &[("name", &*(val_display_name).to_string())]);
            let dialog = adw::MessageDialog::builder()
                .heading(&*crate::i18n::tr("registry.delete_value_title"))
                .body(&*delete_confirm_body)
                .transient_for(&window)
                .modal(true)
                .build();
            dialog.add_response("cancel", &crate::i18n::tr("registry.cancel_btn"));
            dialog.add_response("delete", &crate::i18n::tr("registry.delete_btn"));
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

            let sender_resp = sender_delete.clone();
            let path = path_delete.borrow().clone();
            dialog.connect_response(None, move |d, response| {
                if response == "delete" {
                    let _ = sender_resp.output(RegistryOutput::DeleteEntry {
                        path: path.clone(),
                        is_key: false,
                        value_name: Some(val_name.clone()),
                    });
                }
                d.close();
            });
            dialog.present();
        });

        let sender_refresh = sender.clone();
        let path_refresh = selected_path_mirror.clone();
        btn_refresh.connect_clicked(move |_| {
            let _ = sender_refresh.output(RegistryOutput::RequestKeys {
                path: path_refresh.borrow().clone(),
            });
        });

        let folder_pixbuf =
            Pixbuf::from_resource_at_scale("/com/registry-ui/gtk/folder.svg", 16, 16, true).ok();

        let model = RegistryModel {
            show_toolbar: init.show_toolbar,
            selected_path: "/".to_string(),
            selected_path_mirror,
            path_parts: Vec::new(),
            values: Vec::new(),
            values_mirror,
            selected_value: None,
            selected_value_mirror,
            last_entries: None,
            entries_generation: 0,
            pending_clear: false,
            paned_idle_armed: Rc::new(Cell::new(false)),
            folder_pixbuf,
        };
        let widgets = RegistryWidgets {
            header,
            edit_btn,
            delete_btn,
            paned,
            tree_view,
            tree_store,
            list_box,
            selection_handler,
            row_selected_handler,
            rendered_generation: 0,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            RegistryInput::Entries {
                path,
                subkeys,
                values,
            } => {
                *self.values_mirror.borrow_mut() = values.clone();
                self.values = values;
                if normalize_path(&path) == normalize_path(&self.selected_path) {
                    self.selected_value = None;
                    *self.selected_value_mirror.borrow_mut() = None;
                }
                self.last_entries = Some(EntriesPayload { path, subkeys });
                self.pending_clear = false;
                self.entries_generation += 1;
            }
            RegistryInput::Reset => {
                self.selected_path = "/".to_string();
                *self.selected_path_mirror.borrow_mut() = "/".to_string();
                self.path_parts.clear();
                self.values.clear();
                self.values_mirror.borrow_mut().clear();
                self.selected_value = None;
                *self.selected_value_mirror.borrow_mut() = None;
                self.last_entries = None;
                self.pending_clear = true;
                self.entries_generation += 1;
                let _ = sender.output(RegistryOutput::RequestKeys {
                    path: "/".to_string(),
                });
            }
            RegistryInput::KeySelected { path } => {
                self.selected_path = path.clone();
                *self.selected_path_mirror.borrow_mut() = path.clone();
                self.path_parts = path
                    .split(['/', '\\'])
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                let _ = sender.output(RegistryOutput::RequestKeys { path });
            }
            RegistryInput::KeyExpanded { path } => {
                let _ = sender.output(RegistryOutput::RequestKeys { path });
            }
            RegistryInput::ValueSelected(name) => {
                *self.selected_value_mirror.borrow_mut() = name.clone();
                self.selected_value = name;
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.header.set_visible(self.show_toolbar);

        let has_selection = self.selected_value.is_some();
        widgets.edit_btn.set_sensitive(has_selection);
        widgets.delete_btn.set_sensitive(has_selection);

        if widgets.rendered_generation == self.entries_generation {
            return;
        }
        widgets.rendered_generation = self.entries_generation;

        let selection = widgets.tree_view.selection();
        selection.block_signal(&widgets.selection_handler);
        widgets.list_box.block_signal(&widgets.row_selected_handler);

        if self.pending_clear {
            widgets.tree_store.clear();
            while let Some(child) = widgets.list_box.first_child() {
                widgets.list_box.remove(&child);
            }
            self.arm_paned_center(&widgets.paned);
        }

        if let Some(entries) = self.last_entries.as_ref() {
            let norm_path = normalize_path(&entries.path);
            apply_entries_to_tree(
                &widgets.tree_store,
                &widgets.tree_view,
                &self.folder_pixbuf,
                norm_path,
                &entries.subkeys,
            );
            if normalize_path(&self.selected_path) == norm_path {
                rebuild_value_list(&widgets.list_box, &self.values);
            }
        }

        selection.unblock_signal(&widgets.selection_handler);
        widgets
            .list_box
            .unblock_signal(&widgets.row_selected_handler);
    }
}

fn normalize_path(path: &str) -> &str {
    if path == "/" {
        "/"
    } else {
        path.trim_end_matches('/')
    }
}

fn insert_key_node(
    store: &gtk::TreeStore,
    parent: Option<&gtk::TreeIter>,
    folder_pixbuf: &Option<Pixbuf>,
    name: &str,
    full_path: &str,
) {
    let iter = store.insert_with_values(
        parent,
        None,
        &[(0, folder_pixbuf), (1, &name), (2, &full_path), (3, &false)],
    );
    store.insert_with_values(
        Some(&iter),
        None,
        &[(0, &None::<&Pixbuf>), (1, &""), (2, &""), (3, &false)],
    );
}

fn apply_entries_to_tree(
    store: &gtk::TreeStore,
    view: &gtk::TreeView,
    folder_pixbuf: &Option<Pixbuf>,
    norm_path: &str,
    subkeys: &[String],
) {
    if norm_path == "/" {
        store.clear();
        for subkey in subkeys {
            let node_path = format!("/{}", subkey);
            insert_key_node(store, None, folder_pixbuf, subkey, &node_path);
        }
    } else if let Some(iter) = find_node(store, norm_path) {
        let tree_path = store.path(&iter);
        let is_expanded = view.row_expanded(&tree_path);

        while let Some(child) = store.iter_children(Some(&iter)) {
            store.remove(&child);
        }

        for subkey in subkeys {
            let node_path = format!("{}/{}", norm_path, subkey);
            insert_key_node(store, Some(&iter), folder_pixbuf, subkey, &node_path);
        }

        store.set(&iter, &[(3, &true)]);

        if is_expanded {
            view.expand_row(&tree_path, false);
        }
    }
}

fn rebuild_value_list(list_box: &gtk::ListBox, values: &[RegistryValueInfo]) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    for val in values {
        let row_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_top(5)
            .margin_bottom(5)
            .margin_start(8)
            .margin_end(12)
            .build();
        row_box.append(
            &gtk::Image::builder()
                .resource("/com/registry-ui/gtk/file.svg")
                .pixel_size(24)
                .build(),
        );

        let default_val_text = crate::i18n::tr("registry.default_value");
        let display_name = if val.name.is_empty() {
            &*default_val_text
        } else {
            &val.name
        };
        let name_label = gtk::Label::builder()
            .label(display_name)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        row_box.append(&name_label);

        let data_str = format_value_data(&val.data);
        let value_label = gtk::Label::builder()
            .label(&data_str)
            .halign(gtk::Align::End)
            .css_classes(vec!["dim-label"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(50)
            .build();
        row_box.append(&value_label);

        let row = gtk::ListBoxRow::builder().child(&row_box).build();
        row.set_widget_name(&format!("val:{}", val.name));
        list_box.append(&row);
    }
}

fn format_value_data(data: &RegistryValueData) -> String {
    match data {
        RegistryValueData::String(s) | RegistryValueData::ExpandString(s) => s.clone(),
        RegistryValueData::MultiString(v) => v.join("\n"),
        RegistryValueData::DWord(d) => format!("0x{:08X} ({})", d, d),
        RegistryValueData::QWord(q) => format!("0x{:016X} ({})", q, q),
        RegistryValueData::Binary(b) => format!("{:02X?}", b),
        RegistryValueData::Unknown => "(Unknown Type)".to_string(),
    }
}

fn find_node_by_path(
    store: &gtk::TreeStore,
    iter: &gtk::TreeIter,
    target_path: &str,
) -> Option<gtk::TreeIter> {
    let path: String = store.get(iter, 2);
    if path == target_path {
        return Some(*iter);
    }

    if let Some(child_iter) = store.iter_children(Some(iter)) {
        let current_child = child_iter;
        loop {
            if let Some(found) = find_node_by_path(store, &current_child, target_path) {
                return Some(found);
            }
            if !store.iter_next(&current_child) {
                break;
            }
        }
    }
    None
}

fn find_node(store: &gtk::TreeStore, target_path: &str) -> Option<gtk::TreeIter> {
    if let Some(root_iter) = store.iter_first() {
        let current_root = root_iter;
        loop {
            if let Some(found) = find_node_by_path(store, &current_root, target_path) {
                return Some(found);
            }
            if !store.iter_next(&current_root) {
                break;
            }
        }
    }
    None
}
