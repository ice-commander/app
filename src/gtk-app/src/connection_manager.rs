use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};
use gtk::{
    Align, Box, Button, Image, Label, ListView, Orientation, ScrolledWindow, SignalListItemFactory,
    SingleSelection, TreeExpander, TreeListModel,
};
use std::rc::Rc;

pub use connection_form::{
    collect_plugin_form, kind_index, kind_protocol, kind_table, plugin_mount_settings,
    plugin_state_from_record, translate_optional, Connection,
};

fn connection_folder_paths(conns: &[Connection], explicit: &[String]) -> Vec<String> {
    fn add_with_ancestors(set: &mut std::collections::BTreeSet<String>, path: &str) {
        let mut acc = String::new();
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(seg);
            set.insert(acc.clone());
        }
    }
    let mut set = std::collections::BTreeSet::new();
    for f in explicit {
        add_with_ancestors(&mut set, f);
    }
    for c in conns {
        if let Some(f) = c.folder.as_deref() {
            add_with_ancestors(&mut set, f);
        }
    }
    set.into_iter().collect()
}

fn parent_folder(path: &str) -> Option<String> {
    path.rsplit_once('/').map(|(p, _)| p.to_string())
}

fn folder_leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[derive(Clone)]
enum ConnNode {
    Folder(String),
    Connection { index: usize, conn: Connection },
    NewFolder,
    Pinned(ic_plugin_host::PinnedConnection),
}

enum CommitError {
    Empty,
    Taken,
}

#[derive(Clone)]
enum TreeEdit {
    NewFolder { parent: Option<String> },
    RenameFolder { path: String },
    RenameConnection { index: usize },
}

#[derive(Clone)]
struct PendingEdit {
    edit: TreeEdit,
    generation: u64,
}

impl PendingEdit {
    fn renaming_folder(&self, path: &str) -> bool {
        matches!(&self.edit, TreeEdit::RenameFolder { path: p } if p == path)
    }

    fn renaming_connection(&self, index: usize) -> bool {
        matches!(&self.edit, TreeEdit::RenameConnection { index: i } if *i == index)
    }

    fn is_rename(&self) -> bool {
        !matches!(&self.edit, TreeEdit::NewFolder { .. })
    }
}

fn build_children_map(
    conns: &[Connection],
    explicit: &[String],
    pending: Option<&PendingEdit>,
) -> std::collections::HashMap<Option<String>, Vec<ConnNode>> {
    let mut map: std::collections::HashMap<Option<String>, Vec<ConnNode>> =
        std::collections::HashMap::new();
    for p in connection_folder_paths(conns, explicit) {
        map.entry(parent_folder(&p))
            .or_default()
            .push(ConnNode::Folder(p));
    }
    for (index, conn) in conns.iter().enumerate() {
        let key = conn
            .folder
            .as_deref()
            .filter(|f| !f.is_empty())
            .map(str::to_string);
        map.entry(key).or_default().push(ConnNode::Connection {
            index,
            conn: conn.clone(),
        });
    }
    if let Some(PendingEdit {
        edit: TreeEdit::NewFolder { parent },
        ..
    }) = pending
    {
        map.entry(parent.clone())
            .or_default()
            .push(ConnNode::NewFolder);
    }
    map
}

const PRIMARY_BUTTON_GAP: i32 = 12;

fn folder_contains(conn: &Connection, path: &str) -> bool {
    let prefix = format!("{}/", path);
    conn.folder.as_deref() == Some(path)
        || conn
            .folder
            .as_deref()
            .map(|f| f.starts_with(&prefix))
            .unwrap_or(false)
}

fn connections_in_folder(config: &client_config::AppConfig, path: &str) -> Vec<String> {
    let conns: Vec<Connection> = connection_form::stored_connections(&config);
    conns
        .into_iter()
        .filter(|c| folder_contains(c, path))
        .map(|c| c.name)
        .collect()
}

fn rename_folder(config: &client_config::AppConfig, old_path: &str, new_leaf: &str) {
    let leaf = new_leaf.replace('/', "-");
    let new_path = match parent_folder(old_path) {
        Some(p) => format!("{}/{}", p, leaf),
        None => leaf,
    };
    if new_path.is_empty() || new_path == old_path {
        return;
    }
    let old_prefix = format!("{}/", old_path);
    let new_prefix = format!("{}/", new_path);
    let remap = |f: &str| -> Option<String> {
        if f == old_path {
            Some(new_path.clone())
        } else if let Some(rest) = f.strip_prefix(old_prefix.as_str()) {
            Some(format!("{}{}", new_prefix, rest))
        } else {
            None
        }
    };
    let mut conns: Vec<Connection> = connection_form::stored_connections(&config);
    for c in conns.iter_mut() {
        if let Some(f) = c.folder.clone() {
            if let Some(nf) = remap(&f) {
                c.folder = Some(nf);
            }
        }
    }
    connection_form::save_connections(&config, conns);
    let mut folders: Vec<String> = config.get("ui.ftp_connection_folders").unwrap_or_default();
    for f in folders.iter_mut() {
        if let Some(nf) = remap(f) {
            *f = nf;
        }
    }
    config.set("ui.ftp_connection_folders", folders);
    config.save();
}

fn delete_folder(config: &client_config::AppConfig, path: &str) {
    let prefix = format!("{}/", path);
    let mut conns: Vec<Connection> = connection_form::stored_connections(&config);
    conns.retain(|c| !folder_contains(c, path));
    connection_form::save_connections(&config, conns);
    let mut folders: Vec<String> = config.get("ui.ftp_connection_folders").unwrap_or_default();
    folders.retain(|f| f != path && !f.starts_with(&prefix));
    config.set("ui.ftp_connection_folders", folders);
    config.save();
}

fn popup_actions(
    anchor: &impl IsA<gtk::Widget>,
    x: f64,
    y: f64,
    items: Vec<(String, std::boxed::Box<dyn Fn()>)>,
) {
    let bx = Box::builder().orientation(Orientation::Vertical).build();
    let popover = gtk::Popover::builder().has_arrow(false).build();
    popover.set_parent(anchor);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    for (label, cb) in items {
        let btn = Button::builder()
            .label(&label)
            .css_classes(vec!["flat"])
            .build();
        if let Some(child) = btn.child().and_downcast::<Label>() {
            child.set_xalign(0.0);
        }
        let pop = popover.downgrade();
        btn.connect_clicked(move |_| {
            cb();
            if let Some(p) = pop.upgrade() {
                p.popdown();
            }
        });
        bx.append(&btn);
    }
    popover.set_child(Some(&bx));
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

fn node_under(tree_view: &ListView, x: f64, y: f64) -> Option<(ConnNode, u32)> {
    let mut w = tree_view.pick(x, y, gtk::PickFlags::DEFAULT)?;
    let expander = loop {
        if let Ok(e) = w.clone().downcast::<TreeExpander>() {
            break e;
        }
        w = w.parent()?;
    };
    let row = expander.list_row()?;
    let pos = row.position();
    let node = row
        .item()?
        .downcast::<glib::BoxedAnyObject>()
        .ok()?
        .borrow::<ConnNode>()
        .clone();
    Some((node, pos))
}

type PluginEvent = dyn Fn(&str, Option<&str>, Option<&str>, Option<serde_json::Value>);
type PluginChange = dyn Fn(&str, &str, serde_json::Value, u32);
type PluginRebuild = dyn Fn(&str, &ic_view::Document, ic_view::State);
type PluginRemount = Rc<std::cell::RefCell<Option<Rc<dyn Fn()>>>>;

/// What a kind's form asks to be heard from. A kind whose plugin takes no
/// events hears nothing, however its document is written.
fn plugin_form_watches(listens: bool, document: &ic_view::Document) -> Vec<ic_view_session::Watch> {
    if listens {
        ic_view_session::watches(document)
    } else {
        Vec::new()
    }
}

/// One spelling for the kind, so the session, the lookups and the slot the form
/// is remembered under all agree with each other.
fn plugin_kind_id(protocol: &str) -> String {
    protocol.to_lowercase()
}

/// Every kind is seeded from the record being edited, never from what the kind
/// shown before it left behind under the same binds.
fn seeded_from_record(
    document: &ic_view::Document,
    kind: &str,
    record: Option<&Connection>,
) -> ic_view::State {
    let Some(record) = record else {
        let mut fresh = ic_view::State::default();
        fresh.set_view("mode", serde_json::Value::String("new".to_string()));
        return fresh;
    };
    let mut state = plugin_state_from_record(document, record);
    state.set_view("mode", serde_json::Value::String("edit".to_string()));
    for bind in connection_form::stored_secrets(record, kind) {
        state.stored_secrets.insert(bind);
    }
    state
}

/// What a rebuilt form cannot seed back from the state: a value a widget holds
/// that never reached it.
fn carried_into_rebuild(
    document: &ic_view::Document,
    state: &ic_view::State,
    held: &dyn Fn(&str) -> Option<serde_json::Value>,
) -> Vec<(String, serde_json::Value)> {
    let mut carried = Vec::new();
    document.form.walk(&mut |node| {
        let (Some(id), Some(bind)) = (node.id.as_ref(), node.bind.as_ref()) else {
            return;
        };
        if state.state.contains_key(bind) {
            return;
        }
        let Some(value) = held(id) else {
            return;
        };
        if value.as_str().is_some_and(str::is_empty) {
            return;
        }
        carried.push((bind.clone(), value));
    });
    carried
}

/// Tickets for the debounced changes still ticking: a timer cannot be recalled,
/// so a rebuild forgets every ticket it handed out and the numbering never restarts.
#[derive(Default)]
struct Debounces {
    issued: u64,
    live: std::collections::BTreeMap<String, u64>,
}

impl Debounces {
    fn arm(&mut self, node: &str) -> u64 {
        self.issued += 1;
        self.live.insert(node.to_string(), self.issued);
        self.issued
    }

    fn still_armed(&self, node: &str, ticket: u64) -> bool {
        self.live.get(node).copied() == Some(ticket)
    }

    fn forget(&mut self) {
        self.live.clear();
    }
}

pub fn create_manage_ftp_widget(
    parent: &gtk::Window,
    on_change: Rc<dyn Fn() + 'static>,
    config: client_config::AppConfig,
    on_connect: Option<Rc<dyn Fn(Connection) + 'static>>,
) -> gtk::Widget {
    let main_hbox = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_top(16)
        .margin_bottom(16)
        .build();

    let left_vbox = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .width_request(240)
        .vexpand(true)
        .build();

    let list_label = Label::builder()
        .label(&format!(
            "<b>{}</b>",
            crate::i18n::tr("conn_manager.saved_connections")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .build();
    left_vbox.append(&list_label);

    let tb_btn = |resource: &str, tooltip: String| {
        let b = Button::builder()
            .child(&Image::from_resource(resource))
            .tooltip_text(&tooltip)
            .css_classes(vec!["flat"])
            .build();
        b.set_cursor_from_name(Some("pointer"));
        b
    };
    let tb_sep = || {
        gtk::Separator::builder()
            .orientation(Orientation::Vertical)
            .margin_top(4)
            .margin_bottom(4)
            .build()
    };
    let btn_tb_new_conn = tb_btn(
        "/com/icecommander/gtk/add.svg",
        crate::i18n::tr("conn_manager.toolbar_new_conn").to_string(),
    );
    let btn_tb_new_folder = tb_btn(
        "/com/icecommander/gtk/add-folder.svg",
        crate::i18n::tr("conn_manager.toolbar_new_folder").to_string(),
    );
    let btn_tb_rename = tb_btn(
        "/com/icecommander/gtk/edit-pencil.svg",
        crate::i18n::tr("conn_manager.toolbar_rename").to_string(),
    );
    btn_tb_rename.set_sensitive(false);
    let btn_tb_delete = tb_btn(
        "/com/icecommander/gtk/delete-file.svg",
        crate::i18n::tr("conn_manager.toolbar_delete").to_string(),
    );
    btn_tb_delete.set_sensitive(false);
    let list_toolbar = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(2)
        .build();
    list_toolbar.append(&btn_tb_new_conn);
    list_toolbar.append(&tb_sep());
    list_toolbar.append(&btn_tb_rename);
    list_toolbar.append(&btn_tb_new_folder);
    list_toolbar.append(&tb_sep());
    list_toolbar.append(&btn_tb_delete);
    left_vbox.append(&list_toolbar);

    let selection = SingleSelection::new(None::<gio::ListStore>);
    selection.set_can_unselect(true);
    selection.set_autoselect(false);
    let tree_scroller = ScrolledWindow::builder().vexpand(true).build();
    left_vbox.append(&tree_scroller);

    let pending_edit: Rc<std::cell::RefCell<Option<PendingEdit>>> =
        Rc::new(std::cell::RefCell::new(None));
    let folder_generation = Rc::new(std::cell::Cell::new(0u64));

    let new_conn_folder: Rc<std::cell::RefCell<Option<String>>> =
        Rc::new(std::cell::RefCell::new(None));

    let btn_new_conn = Button::builder()
        .label(&*crate::i18n::tr("conn_manager.add_new_connection"))
        .css_classes(vec!["suggested-action"])
        .build();
    let io_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .homogeneous(true)
        .build();
    let btn_export = Button::builder()
        .label(&*crate::i18n::tr("conn_manager.export"))
        .build();
    let btn_import = Button::builder()
        .label(&*crate::i18n::tr("conn_manager.import"))
        .build();
    io_box.append(&btn_export);
    io_box.append(&btn_import);
    left_vbox.append(&io_box);

    main_hbox.append(&left_vbox);

    let separator = gtk::Separator::new(Orientation::Vertical);
    main_hbox.append(&separator);

    let right_vbox = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .vexpand(true)
        .build();

    let form_label = Label::builder()
        .label(&format!(
            "<b>{}</b>",
            crate::i18n::tr("conn_manager.add_new_connection")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .build();
    right_vbox.append(&form_label);

    let plugin_box = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();
    let plugin_scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&plugin_box)
        .vexpand(true)
        .build();
    right_vbox.append(&plugin_scrolled);

    let editing_index = Rc::new(std::cell::Cell::new(Option::<usize>::None));

    let kinds = std::rc::Rc::new(kind_table());
    let kind_labels: Vec<&str> = kinds.iter().map(|entry| entry.label.as_str()).collect();
    let protocol_dropdown = gtk::DropDown::from_strings(&kind_labels);
    let row_proto = adw::ActionRow::builder()
        .title(&*crate::i18n::tr("conn_manager.protocol"))
        .build();
    row_proto.add_suffix(&protocol_dropdown);

    let proto_group = adw::PreferencesGroup::new();
    proto_group.add(&row_proto);
    right_vbox.insert_child_after(&proto_group, Some(&form_label));

    let is_view_mode = Rc::new(std::cell::Cell::new(false));
    let previously_selected_index = Rc::new(std::cell::Cell::new(Option::<usize>::None));

    let btn_connect = gtk::Button::builder()
        .label(&*crate::i18n::tr("conn_manager.connect_btn"))
        .css_classes(vec!["suggested-action"])
        .width_request(140)
        .build();
    btn_connect.set_visible(false);

    let btn_add = gtk::Button::builder()
        .label(&*crate::i18n::tr("conn_manager.add_connection"))
        .css_classes(vec!["suggested-action"])
        .width_request(140)
        .build();

    let btn_cancel = gtk::Button::builder()
        .label(&*crate::i18n::tr("common.cancel"))
        .build();
    btn_cancel.set_visible(false);

    let buttons_hbox = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::End)
        .build();
    buttons_hbox.append(&btn_cancel);
    buttons_hbox.append(&btn_add);
    buttons_hbox.append(&btn_connect);
    right_vbox.append(&buttons_hbox);

    let plugin_renderer = ic_view_gtk::Renderer::new(Rc::new(translate_optional));
    let plugin_session: Rc<std::cell::RefCell<Option<ic_view_session::Session>>> =
        Rc::new(std::cell::RefCell::new(None));
    let plugin_view: Rc<std::cell::RefCell<Option<Rc<ic_view_gtk::BuiltView>>>> =
        Rc::new(std::cell::RefCell::new(None));
    let plugin_kind: Rc<std::cell::RefCell<Option<String>>> =
        Rc::new(std::cell::RefCell::new(None));
    let plugin_debounces: Rc<std::cell::RefCell<Debounces>> =
        Rc::new(std::cell::RefCell::new(Debounces::default()));
    let remount_plugin_view: PluginRemount = Rc::new(std::cell::RefCell::new(None));

    {
        let plugin_session = plugin_session.clone();
        let plugin_view = plugin_view.clone();
        let plugin_kind = plugin_kind.clone();
        let remount = remount_plugin_view.clone();
        ic_plugin_host::set_form_changed_handler(Rc::new(move |kind: &str| {
            if plugin_kind
                .borrow()
                .as_deref()
                .is_none_or(|held| !held.eq_ignore_ascii_case(kind))
            {
                return;
            }
            let Some(built) = plugin_view.borrow().clone() else {
                return;
            };
            let facts = crate::plugin_view::facts();
            {
                let mut held = plugin_session.borrow_mut();
                let Some(session) = held.as_mut() else {
                    return;
                };
                if session.redescribe(built.as_ref(), &ic_plugin_host::Kinds, &facts)
                    != ic_view_session::Redescribed::Rebuilt
                {
                    return;
                }
            }
            let again = remount.borrow().clone();
            if let Some(again) = again {
                again();
            }
        }));
    }

    let send_plugin_event: Rc<PluginEvent> = {
        let plugin_session = plugin_session.clone();
        let plugin_view = plugin_view.clone();
        let remount = remount_plugin_view.clone();
        Rc::new(
            move |event: &str,
                  node: Option<&str>,
                  bind: Option<&str>,
                  value: Option<serde_json::Value>| {
                let facts = crate::plugin_view::facts();
                let Some(built) = plugin_view.borrow().clone() else {
                    return;
                };
                let outcome = {
                    let mut held = plugin_session.borrow_mut();
                    let Some(session) = held.as_mut() else {
                        return;
                    };
                    session.send(
                        built.as_ref(),
                        &ic_plugin_host::Kinds,
                        &facts,
                        event,
                        node,
                        bind,
                        value,
                    )
                };
                if let Some(text) = outcome.clipboard {
                    if let Some(display) = gdk::Display::default() {
                        display.clipboard().set_text(&text);
                    }
                }
                if !outcome.redescribe {
                    return;
                }
                {
                    let mut held = plugin_session.borrow_mut();
                    let Some(session) = held.as_mut() else {
                        return;
                    };
                    if session.redescribe(built.as_ref(), &ic_plugin_host::Kinds, &facts)
                        != ic_view_session::Redescribed::Rebuilt
                    {
                        return;
                    }
                    let carried = carried_into_rebuild(&session.document, &session.state, &|id| {
                        built.value(id)
                    });
                    for (bind, value) in carried {
                        session.state.set_state(&bind, value);
                    }
                }
                let again = remount.borrow().clone();
                if let Some(again) = again {
                    again();
                }
            },
        )
    };

    let schedule_plugin_change: Rc<PluginChange> = {
        let plugin_debounces = plugin_debounces.clone();
        let send = send_plugin_event.clone();
        Rc::new(
            move |node: &str, bind: &str, value: serde_json::Value, debounce: u32| {
                let ticket = plugin_debounces.borrow_mut().arm(node);
                let pending = plugin_debounces.clone();
                let send = send.clone();
                let target = node.to_string();
                let bound = bind.to_string();
                let fire = move || {
                    let still = pending.borrow().still_armed(&target, ticket);
                    if still {
                        send("change", Some(&target), Some(&bound), Some(value.clone()));
                    }
                };
                if debounce == 0 {
                    fire();
                } else {
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(u64::from(debounce)),
                        fire,
                    );
                }
            },
        )
    };

    let mount_plugin_view: Rc<dyn Fn()> = {
        let plugin_box = plugin_box.clone();
        let plugin_session = plugin_session.clone();
        let plugin_view = plugin_view.clone();
        let plugin_debounces = plugin_debounces.clone();
        let picker_parent = parent.clone();
        let send = send_plugin_event.clone();
        let schedule = schedule_plugin_change.clone();
        Rc::new(move || {
            let Some((kind, document, state, applying)) =
                plugin_session.borrow().as_ref().map(|session| {
                    (
                        session.id.clone(),
                        session.document.clone(),
                        session.state.clone(),
                        session.applying(),
                    )
                })
            else {
                return;
            };
            plugin_debounces.borrow_mut().forget();
            while let Some(child) = plugin_box.first_child() {
                plugin_box.remove(&child);
            }
            let built = Rc::new(plugin_renderer.build(&document, &state));
            plugin_box.append(&built.root);
            for id in built.ids() {
                let owned = id.to_string();
                let sink = plugin_session.clone();
                let again = built.clone();
                let guard = applying.clone();
                built.on_change(&owned, move |bind, value| {
                    if guard.is_set() {
                        return;
                    }
                    let mut held = sink.borrow_mut();
                    let Some(session) = held.as_mut() else {
                        return;
                    };
                    session.state.set_state(&bind, value);
                    session.state.touched.insert(bind);
                    again.refresh(&session.state);
                });
            }
            let mut pickers = Vec::new();
            document.form.walk(&mut |node| {
                if let (Some(id), Some(picker)) = (node.id.as_ref(), node.picker.as_ref()) {
                    let title = picker
                        .title
                        .as_ref()
                        .map(|text| text.resolve(&translate_optional))
                        .unwrap_or_default();
                    pickers.push((id.clone(), title, picker.mode));
                }
            });
            for (id, title, mode) in pickers {
                let Some(field) = built.entry(&id) else {
                    continue;
                };
                let window = picker_parent.clone();
                built.on_activate(&id, move |_| {
                    let dialog = gtk::FileDialog::builder().title(&title).build();
                    let target = field.clone();
                    let chosen = move |result: Result<gtk::gio::File, gtk::glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                target.set_text(&path.to_string_lossy());
                            }
                        }
                    };
                    match mode {
                        ic_view::PickMode::Folder => {
                            dialog.select_folder(Some(&window), gtk::gio::Cancellable::NONE, chosen)
                        }
                        ic_view::PickMode::File => {
                            dialog.open(Some(&window), gtk::gio::Cancellable::NONE, chosen)
                        }
                    }
                });
            }
            let listens = ic_plugin_host::connection_takes_events(&kind);
            for watch in plugin_form_watches(listens, &document) {
                let target = watch.node.clone();
                let guard = applying.clone();
                if watch.on_value {
                    let schedule = schedule.clone();
                    let debounce = watch.debounce_ms;
                    built.on_change(&watch.node, move |bind, value| {
                        if guard.is_set() {
                            return;
                        }
                        schedule(&target, &bind, value, debounce);
                    });
                } else {
                    let send = send.clone();
                    built.on_activate(&watch.node, move |_| {
                        if guard.is_set() {
                            return;
                        }
                        send("activate", Some(&target), None, None);
                    });
                }
            }
            *plugin_view.borrow_mut() = Some(built);
        })
    };
    *remount_plugin_view.borrow_mut() = Some(mount_plugin_view.clone());

    let rebuild_plugin_view: Rc<PluginRebuild> = {
        let plugin_session = plugin_session.clone();
        let plugin_kind = plugin_kind.clone();
        let mount = mount_plugin_view.clone();
        let send = send_plugin_event.clone();
        Rc::new(
            move |kind: &str, document: &ic_view::Document, carried: ic_view::State| {
                let kind = plugin_kind_id(kind);
                let listens = ic_plugin_host::connection_takes_events(&kind);
                // The table holds the form as it was when the dialog opened; ask the kind again.
                let document =
                    ic_plugin_host::connection_document(&kind).unwrap_or_else(|| document.clone());
                let session = ic_view_session::Session::over(
                    &kind,
                    document,
                    &carried,
                    &crate::plugin_view::facts(),
                );
                *plugin_session.borrow_mut() = Some(session);
                *plugin_kind.borrow_mut() = Some(kind);
                mount();
                if listens {
                    send("opened", None, None, None);
                }
            },
        )
    };

    let load_conn = {
        let protocol_dropdown = protocol_dropdown.clone();
        let kinds_load = kinds.clone();
        let rebuild_load = rebuild_plugin_view.clone();

        move |conn: &Connection| {
            let conn = &crate::secret_store::opened(conn);
            let selected = kind_index(&kinds_load, &conn.kind);
            protocol_dropdown.set_selected(selected);
            if let Some(entry) = kinds_load.get(selected as usize) {
                let kind = plugin_kind_id(&entry.protocol);
                let state = seeded_from_record(&entry.document, &kind, Some(conn));
                rebuild_load(&kind, &entry.document, state);
            }
        }
    };
    let load_conn = Rc::new(load_conn);

    let clear_fields = {
        let kinds_clear = kinds.clone();
        let protocol_dropdown_clear = protocol_dropdown.clone();
        let rebuild_clear = rebuild_plugin_view.clone();

        move || {
            if let Some(entry) = kinds_clear.get(protocol_dropdown_clear.selected() as usize) {
                let kind = plugin_kind_id(&entry.protocol);
                let state = seeded_from_record(&entry.document, &kind, None);
                rebuild_clear(&kind, &entry.document, state);
            }
        }
    };
    let clear_fields = Rc::new(clear_fields);

    let right_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    right_stack.add_named(&right_vbox, Some("form"));
    main_hbox.append(&right_stack);

    let update_visibility = Rc::new({
        let protocol_dropdown = protocol_dropdown.clone();
        let kinds_visible = kinds.clone();
        let plugin_kind_visible = plugin_kind.clone();
        let rebuild_visible = rebuild_plugin_view.clone();
        let editing_index_visible = editing_index.clone();
        let config_visible = config.clone();
        move || {
            let selected_proto = protocol_dropdown.selected();
            let Some(entry) = kinds_visible.get(selected_proto as usize) else {
                return;
            };
            let kind = plugin_kind_id(&entry.protocol);
            let already_built = plugin_kind_visible.borrow().as_deref() == Some(kind.as_str());
            if !already_built {
                let edited = editing_index_visible.get().and_then(|at| {
                    let conns: Vec<Connection> =
                        connection_form::stored_connections(&config_visible);
                    conns.get(at).map(crate::secret_store::opened)
                });
                let state = seeded_from_record(&entry.document, &kind, edited.as_ref());
                rebuild_visible(&kind, &entry.document, state);
            }
        }
    });

    protocol_dropdown.connect_selected_notify({
        let update = update_visibility.clone();
        move |_| update()
    });

    update_visibility();

    let set_form_editable = {
        let plugin_view_editable = plugin_view.clone();
        let plugin_session_editable = plugin_session.clone();
        let protocol_dropdown = protocol_dropdown.clone();

        move |editable: bool| {
            protocol_dropdown.set_sensitive(editable);

            let mut held = plugin_session_editable.borrow_mut();
            let Some(session) = held.as_mut() else {
                return;
            };
            session.state.set_view(
                "mode",
                serde_json::Value::String(if editable { "edit" } else { "view" }.to_string()),
            );
            if let Some(view) = plugin_view_editable.borrow().as_ref() {
                view.refresh(&session.state);
            }
        }
    };

    let update_ui_state = Rc::new({
        let editing_index = editing_index.clone();
        let is_view_mode = is_view_mode.clone();
        let btn_connect = btn_connect.clone();
        let btn_add = btn_add.clone();
        let btn_cancel = btn_cancel.clone();
        let set_form_editable = set_form_editable.clone();
        let update_visibility = update_visibility.clone();
        let has_on_connect = on_connect.is_some();
        let btn_new_conn = btn_new_conn.clone();
        let form_label = form_label.clone();

        move || {
            let idx_opt = editing_index.get();
            let view_mode = is_view_mode.get();

            match idx_opt {
                None => {
                    set_form_editable(true);
                    btn_connect.set_visible(false);
                    btn_connect.remove_css_class("suggested-action");
                    btn_add.set_visible(true);
                    btn_add.set_label(&*crate::i18n::tr("conn_manager.add_connection"));
                    btn_add.add_css_class("suggested-action");
                    btn_cancel.set_visible(true);
                    btn_cancel.set_label(&*crate::i18n::tr("common.cancel"));
                    btn_new_conn.set_visible(false);
                    form_label.set_markup(&format!(
                        "<b>{}</b>",
                        crate::i18n::tr("conn_manager.add_new_connection")
                    ));
                }
                Some(_) => {
                    if view_mode {
                        set_form_editable(false);
                        btn_connect.set_visible(has_on_connect);
                        if has_on_connect {
                            btn_connect.add_css_class("suggested-action");
                            btn_add.remove_css_class("suggested-action");
                        } else {
                            btn_connect.remove_css_class("suggested-action");
                            btn_add.add_css_class("suggested-action");
                        }
                        btn_add.set_visible(true);
                        btn_add.set_label(&*crate::i18n::tr("conn_manager.edit_btn"));
                        btn_cancel.set_visible(has_on_connect);
                        btn_cancel.set_label(&*crate::i18n::tr("common.cancel"));
                        btn_new_conn.set_visible(true);
                        form_label.set_markup(&format!(
                            "<b>{}</b>",
                            crate::i18n::tr("conn_manager.view_title")
                        ));
                    } else {
                        set_form_editable(true);
                        btn_connect.set_visible(false);
                        btn_connect.remove_css_class("suggested-action");
                        btn_add.set_visible(true);
                        btn_add.set_label(&*crate::i18n::tr("conn_manager.save_connection"));
                        btn_add.add_css_class("suggested-action");
                        btn_cancel.set_visible(true);
                        btn_cancel.set_label(&*crate::i18n::tr("common.cancel"));
                        btn_new_conn.set_visible(false);
                        form_label.set_markup(&format!(
                            "<b>{}</b>",
                            crate::i18n::tr("conn_manager.edit_title")
                        ));
                    }
                }
            }

            let primary_is_connect = btn_connect.has_css_class("suggested-action");
            btn_connect.set_margin_start(if primary_is_connect {
                PRIMARY_BUTTON_GAP
            } else {
                0
            });
            btn_add.set_margin_start(if primary_is_connect {
                0
            } else {
                PRIMARY_BUTTON_GAP
            });

            update_visibility();
        }
    });

    let conns: Vec<Connection> = connection_form::stored_connections(&config);
    if !conns.is_empty() {
        editing_index.set(Some(0));
        previously_selected_index.set(Some(0));
        is_view_mode.set(true);
        load_conn(&conns[0]);
    } else {
        editing_index.set(None);
        is_view_mode.set(false);
    }
    update_ui_state();

    let clear_form = {
        let editing_index = editing_index.clone();
        let is_view_mode = is_view_mode.clone();
        let clear_fields = clear_fields.clone();
        let update_ui_state = update_ui_state.clone();
        let selection = selection.clone();
        let right_stack = right_stack.clone();
        let new_conn_folder = new_conn_folder.clone();

        move || {
            editing_index.set(None);
            clear_fields();
            is_view_mode.set(false);
            *new_conn_folder.borrow_mut() = None;
            selection.set_selected(gtk::INVALID_LIST_POSITION);
            update_ui_state();
            right_stack.set_visible_child_name("form");
        }
    };
    let clear_form = Rc::new(clear_form);

    let add_conn_in_folder: Rc<dyn Fn(Option<String>)> = {
        let clear_form = clear_form.clone();
        let new_conn_folder = new_conn_folder.clone();
        Rc::new(move |path: Option<String>| {
            clear_form();
            *new_conn_folder.borrow_mut() = path;
        })
    };

    let load_conn_cancel = load_conn.clone();
    let editing_index_cancel = editing_index.clone();
    let previously_selected_index_cancel = previously_selected_index.clone();
    let is_view_mode_cancel = is_view_mode.clone();
    let update_ui_state_cancel = update_ui_state.clone();
    let config_cancel = config.clone();
    let has_on_connect_cancel = on_connect.is_some();

    btn_cancel.connect_clicked(move |btn| {
        let idx_opt = editing_index_cancel.get();
        let view_mode = is_view_mode_cancel.get();

        if let Some(idx) = idx_opt {
            if !view_mode {
                let conns: Vec<Connection> = connection_form::stored_connections(&config_cancel);
                if idx < conns.len() {
                    load_conn_cancel(&conns[idx]);
                }
                is_view_mode_cancel.set(true);
                update_ui_state_cancel();
            } else {
                if has_on_connect_cancel {
                    let root_win = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
                    if let Some(win) = root_win {
                        win.close();
                    }
                }
            }
        } else {
            let prev_idx_opt = previously_selected_index_cancel.get();
            let conns: Vec<Connection> = connection_form::stored_connections(&config_cancel);
            if let Some(prev_idx) = prev_idx_opt {
                if prev_idx < conns.len() {
                    editing_index_cancel.set(Some(prev_idx));
                    load_conn_cancel(&conns[prev_idx]);
                    is_view_mode_cancel.set(true);
                    update_ui_state_cancel();
                    return;
                }
            }
            if !conns.is_empty() {
                editing_index_cancel.set(Some(0));
                previously_selected_index_cancel.set(Some(0));
                load_conn_cancel(&conns[0]);
                is_view_mode_cancel.set(true);
                update_ui_state_cancel();
            } else {
                if has_on_connect_cancel {
                    let root_win = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
                    if let Some(win) = root_win {
                        win.close();
                    }
                }
            }
        }
    });

    let clear_form_new = clear_form.clone();
    btn_new_conn.connect_clicked(move |_| {
        clear_form_new();
    });

    let folder_page = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();
    let folder_title = Label::builder()
        .use_markup(true)
        .halign(Align::Start)
        .build();
    folder_page.append(&folder_title);
    let folder_search = gtk::SearchEntry::builder()
        .placeholder_text(&*crate::i18n::tr("conn_manager.search"))
        .build();
    folder_page.append(&folder_search);
    let folder_list = gtk::ListBox::builder()
        .css_classes(vec!["boxed-list"])
        .build();
    let folder_scroll = ScrolledWindow::builder()
        .vexpand(true)
        .child(&folder_list)
        .build();
    folder_page.append(&folder_scroll);

    let folder_empty = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .halign(Align::Center)
        .valign(Align::Center)
        .vexpand(true)
        .visible(false)
        .build();
    let folder_empty_icon = Image::from_resource("/com/icecommander/gtk/folder.svg");
    folder_empty_icon.set_pixel_size(48);
    folder_empty_icon.add_css_class("dim-label");
    let folder_empty_label = Label::builder()
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(vec!["dim-label"])
        .build();
    let btn_folder_add_conn = Button::builder()
        .label(&*crate::i18n::tr("conn_manager.add_new_connection"))
        .css_classes(vec!["suggested-action", "pill"])
        .halign(Align::Center)
        .build();
    folder_empty.append(&folder_empty_icon);
    folder_empty.append(&folder_empty_label);
    folder_empty.append(&btn_folder_add_conn);
    folder_page.append(&folder_empty);

    right_stack.add_named(&folder_page, Some("folder"));

    let pinned_page = Box::builder()
        .orientation(Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();
    right_stack.add_named(&pinned_page, Some("pinned"));
    let pinned_shown: Rc<std::cell::RefCell<Option<String>>> =
        Rc::new(std::cell::RefCell::new(None));
    let show_pinned: Rc<dyn Fn(&ic_plugin_host::PinnedConnection)> = {
        let pinned_page = pinned_page.clone();
        let pinned_shown = pinned_shown.clone();
        let right_stack = right_stack.clone();
        Rc::new(move |pin: &ic_plugin_host::PinnedConnection| {
            while let Some(child) = pinned_page.first_child() {
                pinned_page.remove(&child);
            }
            if let Some(shown) = crate::plugin_view::embed(&pin.view, &pin.id) {
                pinned_page.append(&shown);
            }
            *pinned_shown.borrow_mut() = Some(pin.id.clone());
            right_stack.set_visible_child_name("pinned");
        })
    };

    let folder_state: Rc<std::cell::RefCell<Option<String>>> =
        Rc::new(std::cell::RefCell::new(None));

    let populate_folder: Rc<dyn Fn()> = {
        let config = config.clone();
        let on_connect = on_connect.clone();
        let load_conn = load_conn.clone();
        let update_ui_state = update_ui_state.clone();
        let editing_index = editing_index.clone();
        let previously_selected_index = previously_selected_index.clone();
        let is_view_mode = is_view_mode.clone();
        let right_stack = right_stack.clone();
        let folder_list = folder_list.clone();
        let folder_search = folder_search.clone();
        let folder_title = folder_title.clone();
        let folder_state = folder_state.clone();
        let folder_scroll = folder_scroll.clone();
        let folder_empty = folder_empty.clone();
        let folder_empty_label = folder_empty_label.clone();
        Rc::new(move || {
            let Some(path) = folder_state.borrow().clone() else {
                return;
            };
            folder_title.set_markup(&format!("<b>{}</b>", folder_leaf(&path)));
            while let Some(child) = folder_list.first_child() {
                folder_list.remove(&child);
            }
            let query = folder_search.text().to_lowercase();
            let prefix = format!("{}/", path);
            let conns: Vec<Connection> = connection_form::stored_connections(&config);
            let mut total_in_folder = 0usize;
            for (idx, conn) in conns.into_iter().enumerate() {
                let in_folder = conn.folder.as_deref() == Some(path.as_str())
                    || conn
                        .folder
                        .as_deref()
                        .map(|f| f.starts_with(&prefix))
                        .unwrap_or(false);
                if !in_folder {
                    continue;
                }
                total_in_folder += 1;
                if !query.is_empty() && !conn.name.to_lowercase().contains(&query) {
                    continue;
                }
                let subtitle = connection_form::kind_summary(&conn).unwrap_or_default();
                let row = adw::ActionRow::builder()
                    .title(&conn.name)
                    .subtitle(&subtitle)
                    .title_lines(1)
                    .subtitle_lines(1)
                    .activatable(true)
                    .build();
                let icon = Image::new();
                draw_kind(&icon, &conn.kind, 20);
                row.add_prefix(&icon);

                let on_connect = on_connect.clone();
                let load_conn = load_conn.clone();
                let update_ui_state = update_ui_state.clone();
                let editing_index = editing_index.clone();
                let previously_selected_index = previously_selected_index.clone();
                let is_view_mode = is_view_mode.clone();
                let right_stack = right_stack.clone();
                let conn_c = conn.clone();
                row.connect_activated(move |_| {
                    if let Some(cb) = &on_connect {
                        cb(crate::secret_store::opened(&conn_c));
                    } else {
                        editing_index.set(Some(idx));
                        previously_selected_index.set(Some(idx));
                        is_view_mode.set(true);
                        load_conn(&conn_c);
                        update_ui_state();
                        right_stack.set_visible_child_name("form");
                    }
                });
                folder_list.append(&row);
            }
            let empty = total_in_folder == 0;
            folder_empty_label.set_label(&crate::i18n::trf(
                "conn_manager.folder_empty",
                &[("name", folder_leaf(&path))],
            ));
            folder_search.set_visible(!empty);
            folder_scroll.set_visible(!empty);
            folder_empty.set_visible(empty);
        })
    };
    {
        let populate_folder = populate_folder.clone();
        folder_search.connect_search_changed(move |_| populate_folder());
    }

    {
        let add_conn_in_folder = add_conn_in_folder.clone();
        let folder_state = folder_state.clone();
        btn_folder_add_conn.connect_clicked(move |_| {
            let path = folder_state.borrow().clone();
            add_conn_in_folder(path);
        });
    }

    let show_folder: Rc<dyn Fn(String)> = {
        let folder_state = folder_state.clone();
        let right_stack = right_stack.clone();
        let populate_folder = populate_folder.clone();
        Rc::new(move |path: String| {
            *folder_state.borrow_mut() = Some(path);
            right_stack.set_visible_child_name("folder");
            populate_folder();
        })
    };

    let refresh_list_rc: Rc<std::cell::RefCell<Option<std::boxed::Box<dyn Fn()>>>> =
        Rc::new(std::cell::RefCell::new(None));
    let refresh_weak = Rc::downgrade(&refresh_list_rc);

    let factory = SignalListItemFactory::new();
    {
        let config = config.clone();
        let on_change = on_change.clone();
        let refresh_weak = refresh_weak.clone();
        let on_connect = on_connect.clone();
        let editing_index = editing_index.clone();
        let previously_selected_index = previously_selected_index.clone();
        let pending_edit = pending_edit.clone();
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let icon = Image::new();
            icon.set_pixel_size(20);
            let label = Label::builder()
                .xalign(0.0)
                .ellipsize(pango::EllipsizeMode::End)
                .build();
            let entry = gtk::Entry::builder().hexpand(true).visible(false).build();
            let content = Box::builder()
                .orientation(Orientation::Horizontal)
                .spacing(6)
                .hexpand(true)
                .build();
            label.set_hexpand(true);
            content.append(&icon);
            content.append(&label);
            content.append(&entry);
            let expander = TreeExpander::new();
            expander.set_child(Some(&content));
            item.set_child(Some(&expander));

            let cancel_pending = {
                let pending_edit = pending_edit.clone();
                let refresh_weak = refresh_weak.clone();
                Rc::new(move || {
                    if pending_edit.borrow().is_none() {
                        return;
                    }
                    *pending_edit.borrow_mut() = None;
                    if let Some(rc) = refresh_weak.upgrade() {
                        if let Some(f) = rc.borrow().as_ref() {
                            f();
                        }
                    }
                })
            };

            let commit_pending: Rc<dyn Fn(&str) -> Result<(), CommitError>> = {
                let config = config.clone();
                let on_change = on_change.clone();
                let pending_edit = pending_edit.clone();
                let refresh_weak = refresh_weak.clone();
                Rc::new(move |text: &str| {
                    let pending = pending_edit.borrow().clone().ok_or(CommitError::Empty)?;
                    let typed = text.trim();
                    if typed.is_empty() {
                        return Err(CommitError::Empty);
                    }

                    match &pending.edit {
                        TreeEdit::RenameConnection { index } => {
                            let mut conns: Vec<Connection> =
                                connection_form::stored_connections(&config);
                            let conn = conns.get_mut(*index).ok_or(CommitError::Empty)?;
                            if conn.name != typed {
                                conn.name = typed.to_string();
                                connection_form::save_connections(&config, conns);
                                config.save();
                                on_change();
                            }
                        }
                        _ => {
                            let leaf = typed.replace('/', "-");
                            if leaf.is_empty() {
                                return Err(CommitError::Empty);
                            }
                            let base = match &pending.edit {
                                TreeEdit::NewFolder { parent } => parent.clone(),
                                TreeEdit::RenameFolder { path } => parent_folder(path),
                                TreeEdit::RenameConnection { .. } => unreachable!(),
                            };
                            let path = match &base {
                                Some(p) => format!("{}/{}", p, leaf),
                                None => leaf.clone(),
                            };
                            let unchanged = matches!(
                                &pending.edit,
                                TreeEdit::RenameFolder { path: old } if *old == path
                            );
                            if !unchanged {
                                let conns: Vec<Connection> =
                                    connection_form::stored_connections(&config);
                                let folders: Vec<String> =
                                    config.get("ui.ftp_connection_folders").unwrap_or_default();
                                if connection_folder_paths(&conns, &folders)
                                    .iter()
                                    .any(|f| f == &path)
                                {
                                    return Err(CommitError::Taken);
                                }
                                match &pending.edit {
                                    TreeEdit::NewFolder { .. } => {
                                        let mut folders = folders;
                                        folders.push(path);
                                        config.set("ui.ftp_connection_folders", folders);
                                        config.save();
                                    }
                                    TreeEdit::RenameFolder { path: old } => {
                                        rename_folder(&config, old, &leaf);
                                        on_change();
                                    }
                                    TreeEdit::RenameConnection { .. } => unreachable!(),
                                }
                            }
                        }
                    }

                    *pending_edit.borrow_mut() = None;
                    if let Some(rc) = refresh_weak.upgrade() {
                        if let Some(f) = rc.borrow().as_ref() {
                            f();
                        }
                    }
                    Ok(())
                })
            };

            {
                let commit_pending = commit_pending.clone();
                entry.connect_activate(move |e| {
                    if let Err(CommitError::Taken) = commit_pending(&e.text()) {
                        e.add_css_class("error");
                        e.set_tooltip_text(Some(&crate::i18n::tr("conn_manager.folder_exists")));
                    }
                });
            }

            entry.connect_changed(|e| {
                e.remove_css_class("error");
                e.set_tooltip_text(None);
            });

            {
                let cancel_pending = cancel_pending.clone();
                let keys = gtk::EventControllerKey::new();
                keys.connect_key_pressed(move |_, key, _, _| {
                    if key == gdk::Key::Escape {
                        cancel_pending();
                        glib::Propagation::Stop
                    } else {
                        glib::Propagation::Proceed
                    }
                });
                entry.add_controller(keys);
            }

            {
                let pending_edit = pending_edit.clone();
                let cancel_pending = cancel_pending.clone();
                let commit_pending = commit_pending.clone();
                let focus = gtk::EventControllerFocus::new();
                focus.connect_leave(move |ctrl| {
                    let Some(pending) = pending_edit.borrow().clone() else {
                        return;
                    };
                    let generation = pending.generation;
                    let typed = ctrl
                        .widget()
                        .and_downcast::<gtk::Entry>()
                        .map(|e| e.text().to_string())
                        .unwrap_or_default();
                    let pending_edit = pending_edit.clone();
                    let cancel_pending = cancel_pending.clone();
                    let commit_pending = commit_pending.clone();
                    glib::idle_add_local_once(move || {
                        let still_current = pending_edit
                            .borrow()
                            .as_ref()
                            .map(|p| p.generation == generation)
                            .unwrap_or(false);
                        if !still_current {
                            return;
                        }
                        if pending.is_rename() {
                            if commit_pending(&typed).is_err() {
                                cancel_pending();
                            }
                        } else {
                            cancel_pending();
                        }
                    });
                });
                entry.add_controller(focus);
            }

            let drag = gtk::DragSource::builder()
                .actions(gdk::DragAction::MOVE)
                .build();
            {
                let exp_w = expander.downgrade();
                drag.connect_prepare(move |_, _, _| {
                    let exp = exp_w.upgrade()?;
                    let obj = exp.list_row()?.item()?;
                    let node = obj.downcast::<glib::BoxedAnyObject>().ok()?;
                    let idx = match &*node.borrow::<ConnNode>() {
                        ConnNode::Connection { index, .. } => *index,
                        _ => return None,
                    };
                    Some(gdk::ContentProvider::for_value(&(idx as u32).to_value()))
                });
            }
            content.add_controller(drag);

            let drop = gtk::DropTarget::new(glib::Type::U32, gdk::DragAction::MOVE);
            {
                let exp_w = expander.downgrade();
                let config = config.clone();
                let on_change = on_change.clone();
                let refresh_weak = refresh_weak.clone();
                drop.connect_drop(move |_, value, _, _| {
                    let Ok(from) = value.get::<u32>() else {
                        return false;
                    };
                    let Some(exp) = exp_w.upgrade() else {
                        return false;
                    };
                    let Some(obj) = exp.list_row().and_then(|r| r.item()) else {
                        return false;
                    };
                    let Ok(node) = obj.downcast::<glib::BoxedAnyObject>() else {
                        return false;
                    };
                    let target = match &*node.borrow::<ConnNode>() {
                        ConnNode::Folder(p) => Some(p.clone()),
                        ConnNode::Connection { conn, .. } => conn.folder.clone(),
                        ConnNode::NewFolder | ConnNode::Pinned(_) => return false,
                    };
                    let mut conns: Vec<Connection> = connection_form::stored_connections(&config);
                    if (from as usize) >= conns.len() {
                        return false;
                    }
                    conns[from as usize].folder = target;
                    connection_form::save_connections(&config, conns);
                    config.save();
                    on_change();
                    if let Some(rc) = refresh_weak.upgrade() {
                        if let Some(f) = rc.borrow().as_ref() {
                            f();
                        }
                    }
                    true
                });
            }
            content.add_controller(drop);

            if let Some(cb) = on_connect.clone() {
                let click = gtk::GestureClick::new();
                let exp_w = expander.downgrade();
                let editing_index = editing_index.clone();
                let previously_selected_index = previously_selected_index.clone();
                click.connect_pressed(move |g, n, _, _| {
                    if n != 2 {
                        return;
                    }
                    let Some(exp) = exp_w.upgrade() else {
                        return;
                    };
                    let Some(obj) = exp.list_row().and_then(|r| r.item()) else {
                        return;
                    };
                    let Ok(node) = obj.downcast::<glib::BoxedAnyObject>() else {
                        return;
                    };
                    let node_ref = node.borrow::<ConnNode>();
                    if let ConnNode::Connection { index, conn } = &*node_ref {
                        editing_index.set(Some(*index));
                        previously_selected_index.set(Some(*index));
                        cb(crate::secret_store::opened(conn));
                        g.set_state(gtk::EventSequenceState::Claimed);
                    }
                });
                content.add_controller(click);
            }
        });
    }

    let pending_edit_bind = pending_edit.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
            return;
        };
        let Some(expander) = item.child().and_downcast::<TreeExpander>() else {
            return;
        };
        expander.set_list_row(Some(&row));
        let Some(content) = expander.child().and_downcast::<Box>() else {
            return;
        };
        let Some(icon) = content.first_child().and_downcast::<Image>() else {
            return;
        };
        let Some(label) = icon.next_sibling().and_downcast::<Label>() else {
            return;
        };
        let Some(entry) = label.next_sibling().and_downcast::<gtk::Entry>() else {
            return;
        };
        let Some(obj) = row.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        let node = obj.borrow::<ConnNode>();
        let renaming = {
            let pending = pending_edit_bind.borrow();
            match (&*node, pending.as_ref()) {
                (ConnNode::Folder(path), Some(p)) => p.renaming_folder(path),
                (ConnNode::Connection { index, .. }, Some(p)) => p.renaming_connection(*index),
                _ => false,
            }
        };
        let begin_edit = |text: &str| {
            label.set_visible(false);
            entry.set_text(text);
            entry.remove_css_class("error");
            entry.set_tooltip_text(None);
            entry.set_visible(true);
            let entry = entry.clone();
            glib::idle_add_local_once(move || {
                entry.grab_focus();
                entry.select_region(0, -1);
            });
        };
        if !matches!(&*node, ConnNode::NewFolder) && !renaming {
            entry.set_visible(false);
            label.set_visible(true);
        }
        match &*node {
            ConnNode::Folder(path) => {
                icon.set_resource(Some("/com/icecommander/gtk/folder.svg"));
                label.set_text(folder_leaf(path));
                if renaming {
                    begin_edit(folder_leaf(path));
                }
            }
            ConnNode::Connection { conn, .. } => {
                draw_kind(&icon, &conn.kind, 20);
                label.set_text(&conn.name);
                if renaming {
                    begin_edit(&conn.name);
                }
            }
            ConnNode::Pinned(pin) => {
                match crate::plugin_host::texture_from_svg(&pin.svg, 20) {
                    Some(texture) => icon.set_paintable(Some(&texture)),
                    None => icon.set_resource(Some(NO_PICTURE)),
                }
                label.set_text(&pin.title);
            }
            ConnNode::NewFolder => {
                icon.set_resource(Some("/com/icecommander/gtk/add-folder.svg"));
                label.set_visible(false);
                entry.set_text("");
                entry.remove_css_class("error");
                entry.set_tooltip_text(None);
                entry.set_visible(true);
                let entry = entry.clone();
                glib::idle_add_local_once(move || {
                    entry.grab_focus();
                });
            }
        }
    });

    let tree_view = ListView::builder()
        .model(&selection)
        .factory(&factory)
        .build();
    tree_view.add_css_class("navigation-sidebar");
    tree_scroller.set_child(Some(&tree_view));

    {
        let make_root_drop = || {
            let target = gtk::DropTarget::new(glib::Type::U32, gdk::DragAction::MOVE);
            let config = config.clone();
            let on_change = on_change.clone();
            let refresh_list_rc = refresh_list_rc.clone();
            target.connect_drop(move |_, value, _, _| {
                let Ok(from) = value.get::<u32>() else {
                    return false;
                };
                let mut conns: Vec<Connection> = connection_form::stored_connections(&config);
                let Some(conn) = conns.get_mut(from as usize) else {
                    return false;
                };
                if conn.folder.is_none() {
                    return true;
                }
                conn.folder = None;
                connection_form::save_connections(&config, conns);
                config.save();
                on_change();
                if let Some(f) = refresh_list_rc.borrow().as_ref() {
                    f();
                }
                true
            });
            target
        };
        tree_view.add_controller(make_root_drop());
        tree_scroller.add_controller(make_root_drop());
    }

    {
        let editing_index = editing_index.clone();
        let previously_selected_index = previously_selected_index.clone();
        let is_view_mode = is_view_mode.clone();
        let load_conn = load_conn.clone();
        let update_ui_state = update_ui_state.clone();
        let right_stack = right_stack.clone();
        let show_folder = show_folder.clone();
        let show_pinned = show_pinned.clone();
        selection.connect_selection_changed(move |sel, _, _| {
            let Some(obj) = sel.selected_item() else {
                return;
            };
            let Some(row) = obj.downcast_ref::<gtk::TreeListRow>() else {
                return;
            };
            let Some(node_obj) = row.item() else {
                return;
            };
            let Ok(node) = node_obj.downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let node_ref = node.borrow::<ConnNode>();
            match &*node_ref {
                ConnNode::Connection { index, conn } => {
                    editing_index.set(Some(*index));
                    previously_selected_index.set(Some(*index));
                    is_view_mode.set(true);
                    load_conn(conn);
                    update_ui_state();
                    right_stack.set_visible_child_name("form");
                }
                ConnNode::Folder(path) => {
                    show_folder(path.clone());
                }
                ConnNode::Pinned(pin) => show_pinned(pin),
                ConnNode::NewFolder => {}
            }
        });
    }

    let selected_node = {
        let selection = selection.clone();
        Rc::new(move || -> Option<ConnNode> {
            let row = selection
                .selected_item()?
                .downcast::<gtk::TreeListRow>()
                .ok()?;
            let node = row.item()?.downcast::<glib::BoxedAnyObject>().ok()?;
            let node = node.borrow::<ConnNode>().clone();
            Some(node)
        })
    };

    let update_toolbar_state = {
        let selected_node = selected_node.clone();
        let btn_tb_delete = btn_tb_delete.clone();
        let btn_tb_rename = btn_tb_rename.clone();
        Rc::new(move || {
            let node = selected_node();
            btn_tb_rename.set_sensitive(matches!(
                node,
                Some(ConnNode::Folder(_)) | Some(ConnNode::Connection { .. })
            ));
            match node {
                Some(ConnNode::Connection { .. }) => {
                    btn_tb_delete.set_sensitive(true);
                    if let Some(img) = btn_tb_delete.child().and_downcast::<Image>() {
                        img.set_resource(Some("/com/icecommander/gtk/delete-file.svg"));
                    }
                }
                Some(ConnNode::Folder(_)) => {
                    btn_tb_delete.set_sensitive(true);
                    if let Some(img) = btn_tb_delete.child().and_downcast::<Image>() {
                        img.set_resource(Some("/com/icecommander/gtk/delete-folder.svg"));
                    }
                }
                _ => btn_tb_delete.set_sensitive(false),
            }
        })
    };

    {
        let update_toolbar_state = update_toolbar_state.clone();
        selection.connect_selection_changed(move |_, _, _| update_toolbar_state());
    }

    let refresh_list = {
        let config = config.clone();
        let selection = selection.clone();
        let pending_edit = pending_edit.clone();
        let update_toolbar_state = update_toolbar_state.clone();
        let pinned_shown = pinned_shown.clone();
        let pinned_page = pinned_page.clone();
        let clear_form = clear_form.clone();
        move || {
            let conns: Vec<Connection> = connection_form::stored_connections(&config);
            let folders: Vec<String> = config.get("ui.ftp_connection_folders").unwrap_or_default();
            let pending = pending_edit.borrow().clone();
            let map = Rc::new(build_children_map(&conns, &folders, pending.as_ref()));
            let root = gio::ListStore::new::<glib::BoxedAnyObject>();
            let pins = ic_plugin_host::pinned_connections();
            let shown = pinned_shown.borrow().clone();
            if let Some(shown) = shown {
                if !pins.iter().any(|pin| pin.id == shown) {
                    *pinned_shown.borrow_mut() = None;
                    while let Some(child) = pinned_page.first_child() {
                        pinned_page.remove(&child);
                    }
                    clear_form();
                }
            }
            for pin in pins {
                root.append(&glib::BoxedAnyObject::new(ConnNode::Pinned(pin)));
            }
            if let Some(children) = map.get(&None) {
                for node in children {
                    root.append(&glib::BoxedAnyObject::new(node.clone()));
                }
            }
            let map_cf = map.clone();
            let tree = TreeListModel::new(root, false, true, move |item| {
                let bo = item.downcast_ref::<glib::BoxedAnyObject>()?;
                let node = bo.borrow::<ConnNode>();
                let ConnNode::Folder(path) = &*node else {
                    return None;
                };
                let children = map_cf.get(&Some(path.clone()))?;
                if children.is_empty() {
                    return None;
                }
                let store = gio::ListStore::new::<glib::BoxedAnyObject>();
                for n in children {
                    store.append(&glib::BoxedAnyObject::new(n.clone()));
                }
                Some(store.upcast::<gio::ListModel>())
            });
            selection.set_model(Some(&tree));
            update_toolbar_state();
        }
    };
    *refresh_list_rc.borrow_mut() =
        Some(std::boxed::Box::new(refresh_list.clone()) as std::boxed::Box<dyn Fn()>);
    refresh_list();
    {
        let refresh_weak = Rc::downgrade(&refresh_list_rc);
        ic_plugin_host::set_pinned_changed_handler(Rc::new(move || {
            if let Some(rc) = refresh_weak.upgrade() {
                if let Some(f) = rc.borrow().as_ref() {
                    f();
                }
            }
        }));
    }

    let start_tree_edit: Rc<dyn Fn(TreeEdit)> = {
        let pending_edit = pending_edit.clone();
        let folder_generation = folder_generation.clone();
        let refresh_list = refresh_list.clone();
        Rc::new(move |edit: TreeEdit| {
            folder_generation.set(folder_generation.get() + 1);
            *pending_edit.borrow_mut() = Some(PendingEdit {
                edit,
                generation: folder_generation.get(),
            });
            refresh_list();
        })
    };
    let start_new_folder: Rc<dyn Fn(Option<String>)> = {
        let start_tree_edit = start_tree_edit.clone();
        Rc::new(move |parent| start_tree_edit(TreeEdit::NewFolder { parent }))
    };
    let start_rename_folder: Rc<dyn Fn(String)> = {
        let start_tree_edit = start_tree_edit.clone();
        Rc::new(move |path| start_tree_edit(TreeEdit::RenameFolder { path }))
    };
    let start_rename_selected: Rc<dyn Fn()> = {
        let start_tree_edit = start_tree_edit.clone();
        let selected_node = selected_node.clone();
        Rc::new(move || match selected_node() {
            Some(ConnNode::Folder(path)) => start_tree_edit(TreeEdit::RenameFolder { path }),
            Some(ConnNode::Connection { index, .. }) => {
                start_tree_edit(TreeEdit::RenameConnection { index })
            }
            _ => {}
        })
    };

    let confirm_delete_folder: Rc<dyn Fn(&gtk::Widget, String)> = {
        let config = config.clone();
        let on_change = on_change.clone();
        let refresh_list = refresh_list.clone();
        Rc::new(move |anchor: &gtk::Widget, path: String| {
            let doomed = connections_in_folder(&config, &path);
            let apply = {
                let config = config.clone();
                let on_change = on_change.clone();
                let refresh_list = refresh_list.clone();
                let path = path.clone();
                move || {
                    delete_folder(&config, &path);
                    on_change();
                    refresh_list();
                }
            };
            if doomed.is_empty() {
                apply();
                return;
            }

            let dialog = adw::AlertDialog::builder()
                .heading(&*crate::i18n::tr("conn_manager.delete"))
                .body(&crate::i18n::trf(
                    "conn_manager.delete_folder_body",
                    &[("name", folder_leaf(&path))],
                ))
                .build();

            let list = Box::builder()
                .orientation(Orientation::Vertical)
                .spacing(4)
                .build();
            for name in &doomed {
                let row = Box::builder()
                    .orientation(Orientation::Horizontal)
                    .spacing(6)
                    .build();
                let icon = Image::new();
                icon.set_resource(Some(NO_PICTURE));
                icon.set_pixel_size(16);
                row.append(&icon);
                row.append(
                    &Label::builder()
                        .label(name)
                        .xalign(0.0)
                        .ellipsize(pango::EllipsizeMode::End)
                        .build(),
                );
                list.append(&row);
            }
            let scroller = ScrolledWindow::builder()
                .child(&list)
                .propagate_natural_height(true)
                .max_content_height(180)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .build();
            dialog.set_extra_child(Some(&scroller));

            dialog.add_response("cancel", &crate::i18n::tr("common.cancel"));
            dialog.add_response("delete", &crate::i18n::tr("conn_manager.delete"));
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            dialog.connect_response(None, move |d, resp| {
                if resp == "delete" {
                    apply();
                }
                d.close();
            });
            dialog.present(Some(anchor));
        })
    };

    {
        let clear_form = clear_form.clone();
        btn_tb_new_conn.connect_clicked(move |_| clear_form());
    }

    {
        let start_new_folder = start_new_folder.clone();
        let selected_node = selected_node.clone();
        btn_tb_new_folder.connect_clicked(move |_| {
            let parent = match selected_node() {
                Some(ConnNode::Folder(p)) => Some(p),
                Some(ConnNode::Connection { conn, .. }) => conn.folder.clone(),
                _ => None,
            };
            start_new_folder(parent);
        });
    }

    {
        let start_rename_selected = start_rename_selected.clone();
        btn_tb_rename.connect_clicked(move |_| start_rename_selected());
    }

    {
        let config = config.clone();
        let on_change = on_change.clone();
        let clear_form = clear_form.clone();
        let refresh_list = refresh_list.clone();
        let selected_node = selected_node.clone();
        let confirm_delete_folder = confirm_delete_folder.clone();
        btn_tb_delete.connect_clicked(move |btn| {
            let (index, name) = match selected_node() {
                Some(ConnNode::Folder(path)) => {
                    confirm_delete_folder(btn.upcast_ref(), path);
                    return;
                }
                Some(ConnNode::Connection { index, conn }) => (index, conn.name),
                _ => return,
            };
            let dialog = adw::AlertDialog::builder()
                .heading(&*crate::i18n::tr("conn_manager.delete"))
                .body(&crate::i18n::trf(
                    "conn_manager.delete_conn_body",
                    &[("name", name.as_str())],
                ))
                .build();
            dialog.add_response("cancel", &crate::i18n::tr("common.cancel"));
            dialog.add_response("delete", &crate::i18n::tr("conn_manager.delete"));
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            let config = config.clone();
            let on_change = on_change.clone();
            let clear_form = clear_form.clone();
            let refresh_list = refresh_list.clone();
            dialog.connect_response(None, move |d, resp| {
                if resp == "delete" {
                    let mut conns: Vec<Connection> = connection_form::stored_connections(&config);
                    if index < conns.len() {
                        conns.remove(index);
                        connection_form::save_connections(&config, conns);
                        config.save();
                        on_change();
                        clear_form();
                        refresh_list();
                    }
                }
                d.close();
            });
            dialog.present(Some(btn));
        });
    }

    {
        let config = config.clone();
        let refresh: Rc<dyn Fn()> = Rc::new(refresh_list.clone());
        let on_change = on_change.clone();
        let clear_form = clear_form.clone();
        let on_connect = on_connect.clone();
        let selection_ctx = selection.clone();
        let start_new_folder = start_new_folder.clone();
        let start_rename_folder = start_rename_folder.clone();
        let start_tree_edit_ctx = start_tree_edit.clone();
        let confirm_delete_folder = confirm_delete_folder.clone();
        let add_conn_in_folder = add_conn_in_folder.clone();
        let tree_view_w = tree_view.downgrade();
        let gesture = gtk::GestureClick::new();
        gesture.set_button(gdk::BUTTON_SECONDARY);
        gesture.connect_pressed(move |g, _, x, y| {
            let Some(tv) = tree_view_w.upgrade() else {
                return;
            };
            let found = node_under(&tv, x, y);
            if let Some((_, pos)) = &found {
                selection_ctx.set_selected(*pos);
            }
            let mut items: Vec<(String, std::boxed::Box<dyn Fn()>)> = Vec::new();
            match found.map(|(n, _)| n) {
                Some(ConnNode::Folder(path)) => {
                    {
                        let start_new_folder = start_new_folder.clone();
                        let path = path.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.new_subfolder").to_string(),
                            std::boxed::Box::new(move || start_new_folder(Some(path.clone()))),
                        ));
                    }
                    {
                        let start_rename_folder = start_rename_folder.clone();
                        let path = path.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.rename").to_string(),
                            std::boxed::Box::new(move || start_rename_folder(path.clone())),
                        ));
                    }
                    {
                        let confirm_delete_folder = confirm_delete_folder.clone();
                        let tv = tv.clone();
                        let path = path.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.delete").to_string(),
                            std::boxed::Box::new(move || {
                                confirm_delete_folder(tv.upcast_ref(), path.clone())
                            }),
                        ));
                    }
                    {
                        let add_conn_in_folder = add_conn_in_folder.clone();
                        let path = path.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.add_new_connection").to_string(),
                            std::boxed::Box::new(move || add_conn_in_folder(Some(path.clone()))),
                        ));
                    }
                }
                Some(ConnNode::Connection { index, conn }) => {
                    if let Some(cb) = on_connect.clone() {
                        let conn = conn.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.connect_btn").to_string(),
                            std::boxed::Box::new(move || cb(crate::secret_store::opened(&conn))),
                        ));
                    }
                    {
                        let start_tree_edit = start_tree_edit_ctx.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.rename").to_string(),
                            std::boxed::Box::new(move || {
                                start_tree_edit(TreeEdit::RenameConnection { index })
                            }),
                        ));
                    }
                    {
                        let config = config.clone();
                        let refresh = refresh.clone();
                        let on_change = on_change.clone();
                        let clear_form = clear_form.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.delete").to_string(),
                            std::boxed::Box::new(move || {
                                let mut conns: Vec<Connection> =
                                    connection_form::stored_connections(&config);
                                if index < conns.len() {
                                    conns.remove(index);
                                    connection_form::save_connections(&config, conns);
                                    config.save();
                                    on_change();
                                    clear_form();
                                    refresh();
                                }
                            }),
                        ));
                    }
                }
                Some(ConnNode::NewFolder) | Some(ConnNode::Pinned(_)) => {}
                None => {
                    {
                        let start_new_folder = start_new_folder.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.new_folder_title").to_string(),
                            std::boxed::Box::new(move || start_new_folder(None)),
                        ));
                    }
                    {
                        let clear_form = clear_form.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.add_new_connection").to_string(),
                            std::boxed::Box::new(move || clear_form()),
                        ));
                    }
                }
            }
            if !items.is_empty() {
                popup_actions(&tv, x, y, items);
            }
            g.set_state(gtk::EventSequenceState::Claimed);
        });
        tree_view.add_controller(gesture);
    }

    {
        let config_exp = config.clone();
        let parent_exp = parent.clone();
        btn_export.connect_clicked(move |_| {
            let conns: Vec<Connection> = connection_form::stored_connections(&config_exp);
            if conns.is_empty() {
                show_error(
                    &parent_exp,
                    &crate::i18n::tr("conn_manager.export"),
                    &crate::i18n::tr("conn_manager.export_empty"),
                );
                return;
            }
            let parent2 = parent_exp.clone();
            prompt_password(
                &parent_exp,
                &crate::i18n::tr("conn_manager.export_password_title"),
                &crate::i18n::tr("conn_manager.export_password_body"),
                std::rc::Rc::new(move |pw| {
                    let Some(pw) = pw else { return };
                    let json = crate::secret_store::export_connections(
                        &conns,
                        if pw.is_empty() {
                            None
                        } else {
                            Some(pw.as_str())
                        },
                    );
                    let plain = pw.is_empty();
                    let parent3 = parent2.clone();
                    let fd = gtk::FileDialog::builder()
                        .initial_name("ice-commander-connections.json")
                        .build();
                    fd.save(Some(&parent2), gtk::gio::Cancellable::NONE, move |res| {
                        let Ok(file) = res else { return };
                        let Some(path) = file.path() else { return };
                        match std::fs::write(&path, &json) {
                            Ok(()) => {
                                ::secret_store::harden_file_permissions(&path);
                                let body = if plain {
                                    crate::i18n::tr("conn_manager.export_done_plain")
                                } else {
                                    crate::i18n::tr("conn_manager.export_done_encrypted")
                                };
                                show_error(
                                    &parent3,
                                    &crate::i18n::tr("conn_manager.export_done_title"),
                                    &body,
                                );
                            }
                            Err(e) => show_error(
                                &parent3,
                                &crate::i18n::tr("conn_manager.export_failed"),
                                &e.to_string(),
                            ),
                        }
                    });
                }),
            );
        });
    }

    {
        let config_imp = config.clone();
        let parent_imp = parent.clone();
        let on_change_imp = on_change.clone();
        let refresh_imp = refresh_list_rc.clone();
        let do_import: std::rc::Rc<dyn Fn(String, Option<String>)> = {
            let config = config_imp.clone();
            let parent = parent_imp.clone();
            let on_change = on_change_imp.clone();
            let refresh = refresh_imp.clone();
            std::rc::Rc::new(move |text: String, pw: Option<String>| {
                match crate::secret_store::parse_import(&text, pw.as_deref()) {
                    Ok(list) => {
                        let mut conns: Vec<Connection> =
                            connection_form::stored_connections(&config);
                        let (mut added, mut updated) = (0usize, 0usize);
                        for mut c in list {
                            crate::secret_store::seal_connection(&config, &mut c);
                            match conns.iter_mut().find(|e| e.name == c.name) {
                                Some(e) => {
                                    *e = c;
                                    updated += 1;
                                }
                                None => {
                                    conns.push(c);
                                    added += 1;
                                }
                            }
                        }
                        connection_form::save_connections(&config, conns);
                        config.save();
                        on_change();
                        if let Some(f) = refresh.borrow().as_ref() {
                            f();
                        }
                        show_error(
                            &parent,
                            &crate::i18n::tr("conn_manager.import_done_title"),
                            &format!(
                                "{}\n\n{}",
                                crate::i18n::trf(
                                    "conn_manager.import_done_body",
                                    &[
                                        ("added", &*(added).to_string()),
                                        ("updated", &*(updated).to_string())
                                    ]
                                ),
                                crate::i18n::tr("conn_manager.import_delete_reminder")
                            ),
                        );
                    }
                    Err(crate::secret_store::ImportError::WrongPassword)
                    | Err(crate::secret_store::ImportError::NeedsPassword) => show_error(
                        &parent,
                        &crate::i18n::tr("conn_manager.import_failed"),
                        &crate::i18n::tr("conn_manager.import_wrong_password"),
                    ),
                    Err(crate::secret_store::ImportError::Malformed) => show_error(
                        &parent,
                        &crate::i18n::tr("conn_manager.import_failed"),
                        &crate::i18n::tr("conn_manager.import_malformed"),
                    ),
                }
            })
        };
        btn_import.connect_clicked(move |_| {
            let parent2 = parent_imp.clone();
            let do_import = do_import.clone();
            let fd = gtk::FileDialog::new();
            fd.open(Some(&parent_imp), gtk::gio::Cancellable::NONE, move |res| {
                let Ok(file) = res else { return };
                let Some(path) = file.path() else { return };
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => {
                        show_error(
                            &parent2,
                            &crate::i18n::tr("conn_manager.import_failed"),
                            &e.to_string(),
                        );
                        return;
                    }
                };
                if crate::secret_store::import_needs_password(&text) {
                    let do_import = do_import.clone();
                    prompt_password(
                        &parent2,
                        &crate::i18n::tr("conn_manager.import_password_title"),
                        &crate::i18n::tr("conn_manager.import_password_body"),
                        std::rc::Rc::new(move |pw| {
                            if let Some(pw) = pw.filter(|p| !p.is_empty()) {
                                do_import(text.clone(), Some(pw));
                            }
                        }),
                    );
                } else {
                    do_import(text, None);
                }
            });
        });
    }

    let refresh_list_add = refresh_list_rc.clone();
    let on_change_add = on_change.clone();
    let editing_index_add = editing_index.clone();
    let previously_selected_index_add = previously_selected_index.clone();
    let config_add = config.clone();
    let is_view_mode_add = is_view_mode.clone();
    let update_ui_state_add = update_ui_state.clone();
    let load_conn_add = load_conn.clone();

    let config_seal = config.clone();
    let new_conn_folder_add = new_conn_folder.clone();
    let kinds_add = kinds.clone();
    let plugin_view_add = plugin_view.clone();
    let plugin_session_add = plugin_session.clone();
    btn_add.connect_clicked(move |_| {
        let is_view = is_view_mode_add.get();
        if is_view && editing_index_add.get().is_some() {
            is_view_mode_add.set(false);
            update_ui_state_add();
            return;
        }

        let open = plugin_session_add
            .borrow()
            .as_ref()
            .map(|session| (session.document.clone(), session.state.clone()));
        let Some((document, held)) = open else {
            return;
        };
        let form = {
            let shown = plugin_view_add.borrow();
            let Some(view) = shown.as_ref() else {
                return;
            };
            let values = connection_form::values_to_commit(&held, view.values());
            let Some(collected) =
                collect_plugin_form(&document, values, view.touched(), &held.stored_secrets)
            else {
                return;
            };
            collected
        };

        let protocol = kind_protocol(&kinds_add, protocol_dropdown.selected());
        let mut conns: Vec<Connection> = connection_form::stored_connections(&config_add);
        let folder = match editing_index_add.get() {
            Some(idx) => conns.get(idx).and_then(|c| c.folder.clone()),
            None => new_conn_folder_add.borrow().clone(),
        };
        let mut new_conn = connection_form::record_from_form(&form, &protocol, folder);
        // A secret the form did not ask again for stays as it was stored.
        if let Some(previous) = editing_index_add.get().and_then(|at| conns.get(at)) {
            let opened = crate::secret_store::opened(previous);
            let kind = new_conn.kind.clone();
            connection_form::carry_secrets(&mut new_conn, &opened, &kind);
        }
        crate::secret_store::seal_connection(&config_seal, &mut new_conn);

        let saved_idx = if let Some(idx) = editing_index_add.get() {
            if idx < conns.len() {
                conns[idx] = new_conn;
            } else {
                conns.push(new_conn);
            }
            idx
        } else {
            conns.push(new_conn);
            conns.len() - 1
        };
        connection_form::save_connections(&config_add, conns.clone());
        config_add.save();
        *new_conn_folder_add.borrow_mut() = None;

        editing_index_add.set(Some(saved_idx));
        previously_selected_index_add.set(Some(saved_idx));
        is_view_mode_add.set(true);
        load_conn_add(&conns[saved_idx]);
        update_ui_state_add();

        on_change_add();
        if let Some(f) = refresh_list_add.borrow().as_ref() {
            f();
        }
    });

    if let Some(on_connect_cb) = on_connect.clone() {
        let editing_index_conn = editing_index.clone();
        let config_conn = config.clone();
        btn_connect.connect_clicked(move |_| {
            if let Some(idx) = editing_index_conn.get() {
                let conns: Vec<Connection> = connection_form::stored_connections(&config_conn);
                if idx < conns.len() {
                    on_connect_cb(crate::secret_store::opened(&conns[idx]));
                }
            }
        });
    }

    // A timer armed as the dialog closes would otherwise reach the plugin with the form gone.
    main_hbox.connect_unmap(move |_| plugin_debounces.borrow_mut().forget());

    main_hbox.upcast::<gtk::Widget>()
}

thread_local! {
    static OPEN_DIALOG: std::cell::RefCell<Option<gtk::Window>> =
        const { std::cell::RefCell::new(None) };
}

pub fn connections_dialog_open() -> bool {
    OPEN_DIALOG.with(|slot| slot.borrow().is_some())
}

pub fn close_manage_ftp_dialog() {
    if let Some(win) = OPEN_DIALOG.with(|slot| slot.borrow_mut().take()) {
        win.close();
    }
}

pub fn show_manage_ftp_dialog(
    parent: &impl IsA<gtk::Window>,
    on_change: Rc<dyn Fn() + 'static>,
    config: client_config::AppConfig,
    on_connect: Option<Rc<dyn Fn(Connection) + 'static>>,
) {
    let window = gtk::Window::builder()
        .title(&*crate::i18n::tr("conn_manager.title"))
        .default_width(750)
        .default_height(500)
        .resizable(true)
        .modal(true)
        .transient_for(parent)
        .build();

    let key_controller = gtk::EventControllerKey::new();
    let win_clone = window.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gtk::gdk::Key::Escape {
            win_clone.close();
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    window.add_controller(key_controller);

    let win_connect = window.clone();
    let on_connect_wrapped = on_connect.map(|cb| {
        let win = win_connect.clone();
        Rc::new(move |conn: Connection| {
            cb(conn);
            win.close();
        }) as Rc<dyn Fn(Connection)>
    });
    let widget = create_manage_ftp_widget(&window, on_change, config, on_connect_wrapped);
    window.set_child(Some(&widget));
    OPEN_DIALOG.with(|slot| *slot.borrow_mut() = Some(window.clone()));
    crate::api::notify_connections_dialog(true);
    window.connect_close_request(move |_| {
        OPEN_DIALOG.with(|slot| *slot.borrow_mut() = None);
        crate::api::notify_connections_dialog(false);
        gtk::glib::Propagation::Proceed
    });

    window.present();
}

fn prompt_password(
    parent: &gtk::Window,
    heading: &str,
    body: &str,
    on_done: Rc<dyn Fn(Option<String>)>,
) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    let entry = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .activates_default(true)
        .build();
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", &crate::i18n::tr("common.cancel"));
    dialog.add_response("ok", &crate::i18n::tr("common.ok"));
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.connect_response(None, move |_, resp| {
        if resp == "ok" {
            on_done(Some(entry.text().to_string()));
        } else {
            on_done(None);
        }
    });
    dialog.present(Some(parent));
}

const NO_PICTURE: &str = "/com/icecommander/gtk/connect.svg";

/// Draws a connection the way its own kind says it looks.
pub fn draw_kind(icon: &Image, kind: &str, size: i32) {
    icon.set_pixel_size(size);
    match connection_form::kind_picture(kind) {
        connection_form::KindPicture::Svg(bytes) => {
            match crate::plugin_host::texture_from_svg(&bytes, size as u32) {
                Some(drawn) => icon.set_paintable(Some(&drawn)),
                None => icon.set_resource(Some(NO_PICTURE)),
            }
        }
        connection_form::KindPicture::Theme(named) => icon.set_icon_name(Some(&named)),
        connection_form::KindPicture::None => icon.set_resource(Some(NO_PICTURE)),
    }
}

pub fn mount_through_plugin(
    conn: &Connection,
) -> Option<std::rc::Rc<dyn fm_core::rpc::FileSystemRpc>> {
    let document = ic_plugin_host::connection_document(&conn.kind)?;
    let opened = crate::secret_store::opened(conn);
    let icon_svg = match connection_form::kind_picture(&conn.kind) {
        connection_form::KindPicture::Svg(bytes) => String::from_utf8(bytes).ok(),
        _ => None,
    };
    ic_plugin_host::mount_connection_shown(
        &conn.kind,
        &plugin_mount_settings(&document, &opened),
        Some(fm_core::plugin_fs::Shown {
            name: conn.name.clone(),
            icon_svg,
        }),
    )
}

pub fn show_error(parent: &impl IsA<gtk::Widget>, title: &str, msg: &str) {
    let dialog = adw::AlertDialog::builder().heading(title).body(msg).build();
    dialog.add_response("ok", &*crate::i18n::tr("common.ok"));
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::{
        carried_into_rebuild, plugin_form_watches, plugin_kind_id, seeded_from_record, Connection,
        Debounces,
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    fn grouped_record() -> Connection {
        Connection {
            name: "cloud".to_string(),
            folder: Some("work/servers".to_string()),
            kind: "webdav".to_string(),
            settings: BTreeMap::from([
                (
                    "url".to_string(),
                    "https://dav.example.org/remote.php/dav".to_string(),
                ),
                ("user".to_string(), "ivan".to_string()),
            ]),
        }
    }

    #[test]
    fn a_folder_and_a_settings_map_survive_the_round_trip_through_the_api_record() {
        let stored = grouped_record();
        let as_api: panel_server::ApiConnection =
            serde_json::from_value(serde_json::to_value(&stored).expect("encodes"))
                .expect("the api record reads what the dialog wrote");
        let back: Connection =
            serde_json::from_value(serde_json::to_value(&as_api).expect("encodes"))
                .expect("the dialog reads what the api record wrote");
        assert_eq!(back.folder, stored.folder);
        assert_eq!(back.kind, stored.kind);
        assert_eq!(back.settings, stored.settings);
    }

    fn a_form_that_wants_to_be_heard() -> ic_view::Document {
        serde_json::from_value(serde_json::json!({
            "schema": 1,
            "kind": "example",
            "fields": [
                { "bind": "token", "type": "text" },
                { "bind": "key_path", "type": "text" },
            ],
            "form": { "t": "column", "children": [
                { "t": "input", "id": "token", "bind": "token", "emit": "change", "debounce_ms": 300 },
                { "t": "input", "id": "key_path", "bind": "key_path", "picker": { "mode": "file" } },
                { "t": "button", "id": "login", "intent": { "do": "emit", "node": "login" } },
                { "t": "button", "id": "cancel", "intent": { "do": "close" } },
            ]},
        }))
        .expect("a document")
    }

    #[test]
    fn a_kind_whose_plugin_takes_no_events_is_wired_to_nothing_however_its_form_is_written() {
        assert!(plugin_form_watches(false, &a_form_that_wants_to_be_heard()).is_empty());
    }

    #[test]
    fn a_listening_kind_is_wired_to_its_emitting_buttons_and_to_the_fields_that_ask_to_be_heard() {
        let watched = plugin_form_watches(true, &a_form_that_wants_to_be_heard());
        assert_eq!(
            watched,
            vec![
                ic_view_session::Watch {
                    node: "token".to_string(),
                    debounce_ms: 300,
                    on_value: true,
                },
                ic_view_session::Watch {
                    node: "login".to_string(),
                    debounce_ms: 0,
                    on_value: false,
                },
            ]
        );
    }

    #[test]
    fn only_the_last_keystroke_of_a_debounced_node_is_still_armed_when_its_timer_fires() {
        let mut waiting = Debounces::default();
        let first = waiting.arm("token");
        let second = waiting.arm("token");
        let other = waiting.arm("key_path");
        assert!(!waiting.still_armed("token", first));
        assert!(waiting.still_armed("token", second));
        assert!(waiting.still_armed("key_path", other));
    }

    #[test]
    fn a_change_armed_before_a_rebuild_is_dropped_even_when_the_same_node_is_typed_in_again() {
        let mut waiting = Debounces::default();
        let before = waiting.arm("token");
        waiting.forget();
        assert!(!waiting.still_armed("token", before));
        let after = waiting.arm("token");
        assert_ne!(before, after, "a ticket is never handed out twice");
        assert!(!waiting.still_armed("token", before));
        assert!(waiting.still_armed("token", after));
    }

    #[test]
    fn every_change_still_ticking_when_the_dialog_goes_away_is_dropped_instead_of_being_sent() {
        let mut waiting = Debounces::default();
        let token = waiting.arm("token");
        let key_path = waiting.arm("key_path");
        waiting.forget();
        assert!(!waiting.still_armed("token", token));
        assert!(!waiting.still_armed("key_path", key_path));
    }

    fn facts() -> ic_view_session::HostFacts {
        ic_view_session::HostFacts::new("gtk", "en", &["copy"])
    }

    // The application knows no protocol: a kind exists only because something declared it.
    fn declare(kind: &str, document: &serde_json::Value) {
        let encoded = document.to_string();
        let host = ic_plugin_host::host_table();
        let id = std::ffi::CString::new(kind).expect("a kind id");
        assert_eq!(
            (host.register_connection_kind)(
                id.as_ptr(),
                encoded.as_ptr(),
                encoded.len() as u64,
                std::ptr::null(),
                std::ptr::null_mut(),
            ),
            ic_plugin_api::IC_OK
        );
    }

    fn a_form_with_a_default_port(port: u16) -> ic_view::Document {
        serde_json::from_value(json!({
            "schema": 1,
            "kind": "example",
            "fields": [
                { "bind": "host", "type": "text" },
                { "bind": "port", "type": "integer", "default": port },
            ],
            "form": { "t": "column", "children": [
                { "t": "input", "id": "host", "bind": "host" },
                { "t": "input", "id": "port", "bind": "port" },
            ]},
        }))
        .expect("a document")
    }

    #[test]
    fn the_view_a_kind_is_told_it_is_carries_the_id_the_plugin_registered_not_the_table_spelling() {
        assert_eq!(plugin_kind_id("SFTP"), "sftp");
        let session = ic_view_session::Session::over(
            &plugin_kind_id("SFTP"),
            a_form_that_wants_to_be_heard(),
            &ic_view::State::default(),
            &facts(),
        );
        let event = session.envelope(
            "change",
            Some("token"),
            Some("token"),
            None,
            json!({}),
            &facts(),
        );
        assert_eq!(
            event["view"],
            json!("sftp"),
            "a plugin comparing the view it is given against its own id must see one spelling"
        );
    }

    #[test]
    fn a_kind_shown_after_another_is_seeded_from_the_record_so_its_own_default_still_applies() {
        let first = a_form_with_a_default_port(21);
        let previous = ic_view_session::Session::over(
            "first",
            first.clone(),
            &seeded_from_record(&first, "first", None),
            &facts(),
        );
        assert_eq!(previous.state.state.get("port"), Some(&json!(21)));

        let second = a_form_with_a_default_port(22);
        let session = ic_view_session::Session::over(
            "second",
            second.clone(),
            &seeded_from_record(&second, "second", None),
            &facts(),
        );
        assert_eq!(
            session.state.state.get("port"),
            Some(&json!(22)),
            "what the kind shown before it was seeded with must not survive the switch"
        );
        assert_eq!(session.state.view.get("mode"), Some(&json!("new")));
    }

    #[test]
    fn the_kind_being_switched_to_is_seeded_from_the_record_the_dialog_is_editing() {
        let document = a_form_with_a_default_port(22);
        let mut record = Connection::new("second");
        record.put("host", "files.example.org".to_string());
        record.put("port", "2222".to_string());
        let state = seeded_from_record(&document, "second", Some(&record));
        assert_eq!(state.state.get("host"), Some(&json!("files.example.org")));
        assert_eq!(state.state.get("port"), Some(&json!("2222")));
        assert_eq!(state.view.get("mode"), Some(&json!("edit")));
    }

    #[test]
    fn a_secret_the_record_already_holds_is_marked_stored_so_the_form_offers_to_keep_it() {
        let declared = json!({
            "schema": 1,
            "kind": "gtkstored",
            "fields": [
                { "bind": "pass", "type": "text", "secret": true },
                { "bind": "passphrase", "type": "text", "secret": true },
            ],
            "form": { "t": "column", "children": [
                { "t": "input", "id": "pass", "bind": "pass" },
            ]},
        });
        declare("gtkstored", &declared);
        let document: ic_view::Document =
            serde_json::from_value(declared).expect("the declared document");

        let mut record = Connection::new("gtkstored");
        record.put("pass", "hunter2".to_string());
        let state = seeded_from_record(&document, "gtkstored", Some(&record));
        assert!(state.stored_secrets.contains("pass"));
        assert!(
            !state.stored_secrets.contains("passphrase"),
            "only a secret the record holds a value for is marked"
        );
        assert!(
            seeded_from_record(&document, "gtkstored", None)
                .stored_secrets
                .is_empty(),
            "a connection being added holds nothing yet"
        );
    }

    #[test]
    fn a_value_the_reply_put_into_the_widgets_is_carried_into_the_form_the_rebuild_renders() {
        let document = a_form_that_wants_to_be_heard();
        let mut state = ic_view::State::default();
        state.set_state("key_path", json!("/home/ivan/.ssh/id_ed25519"));
        let carried = carried_into_rebuild(&document, &state, &|id| match id {
            "token" => Some(json!("put-by-the-plugin")),
            "key_path" => Some(json!("/tmp/stale")),
            _ => None,
        });
        assert_eq!(
            carried,
            vec![("token".to_string(), json!("put-by-the-plugin"))],
            "only what the state cannot seed back is carried"
        );
    }

    #[test]
    fn an_empty_widget_carries_nothing_into_the_rebuild_and_leaves_the_new_default_alone() {
        let document = a_form_that_wants_to_be_heard();
        let carried =
            carried_into_rebuild(&document, &ic_view::State::default(), &|_| Some(json!("")));
        assert!(carried.is_empty());
    }
}
