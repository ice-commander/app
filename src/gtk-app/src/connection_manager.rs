use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};
use gtk::{
    Align, Box, Button, Image, Label, ListView, Orientation, ScrolledWindow,
    SignalListItemFactory, SingleSelection, TreeExpander, TreeListModel,
};
use std::rc::Rc;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Default)]
pub struct FtpConnection {
    pub name: String,
    #[serde(default)]
    pub folder: Option<String>,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub pass: Option<String>,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
    #[serde(default)]
    pub remote_path: Option<String>,
    #[serde(default)]
    pub use_tunnel: Option<bool>,
    #[serde(default)]
    pub tunnel_host: Option<String>,
    #[serde(default)]
    pub tunnel_port: Option<u16>,
    #[serde(default)]
    pub tunnel_user: Option<String>,
    #[serde(default)]
    pub tunnel_auth_type: Option<String>,
    #[serde(default)]
    pub tunnel_pass: Option<String>,
    #[serde(default)]
    pub tunnel_key_path: Option<String>,
    #[serde(default)]
    pub tunnel_passphrase: Option<String>,
}

fn connection_folder_paths(conns: &[FtpConnection], explicit: &[String]) -> Vec<String> {
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
    Connection { index: usize, conn: FtpConnection },
    NewFolder,
}

#[derive(Clone)]
struct PendingFolder {
    parent: Option<String>,
    generation: u64,
}

fn build_children_map(
    conns: &[FtpConnection],
    explicit: &[String],
    pending: Option<&PendingFolder>,
) -> std::collections::HashMap<Option<String>, Vec<ConnNode>> {
    let mut map: std::collections::HashMap<Option<String>, Vec<ConnNode>> =
        std::collections::HashMap::new();
    for p in connection_folder_paths(conns, explicit) {
        map.entry(parent_folder(&p)).or_default().push(ConnNode::Folder(p));
    }
    for (index, conn) in conns.iter().enumerate() {
        let key = conn.folder.as_deref().filter(|f| !f.is_empty()).map(str::to_string);
        map.entry(key)
            .or_default()
            .push(ConnNode::Connection { index, conn: conn.clone() });
    }
    if let Some(p) = pending {
        map.entry(p.parent.clone()).or_default().push(ConnNode::NewFolder);
    }
    map
}

fn prompt_folder_name(
    parent: &gtk::Window,
    title: &str,
    initial: &str,
    on_ok: impl Fn(String) + 'static,
) {
    let dialog = adw::AlertDialog::builder().heading(title).build();
    let entry = gtk::Entry::builder().text(initial).build();
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", &*crate::i18n::tr("account.cancel"));
    dialog.add_response("ok", &*crate::i18n::tr("conn_manager.ok"));
    dialog.set_default_response(Some("ok"));
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    let entry_r = entry.clone();
    dialog.connect_response(None, move |d, resp| {
        if resp == "ok" {
            let name = entry_r.text().trim().to_string();
            if !name.is_empty() {
                on_ok(name);
            }
        }
        d.close();
    });
    dialog.present(Some(parent));
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
    let mut conns: Vec<FtpConnection> = config.get("ui.ftp_connections").unwrap_or_default();
    for c in conns.iter_mut() {
        if let Some(f) = c.folder.clone() {
            if let Some(nf) = remap(&f) {
                c.folder = Some(nf);
            }
        }
    }
    config.set("ui.ftp_connections", conns);
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
    let mut conns: Vec<FtpConnection> = config.get("ui.ftp_connections").unwrap_or_default();
    for c in conns.iter_mut() {
        let inside = c.folder.as_deref() == Some(path)
            || c.folder.as_deref().map(|f| f.starts_with(&prefix)).unwrap_or(false);
        if inside {
            c.folder = None;
        }
    }
    config.set("ui.ftp_connections", conns);
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
        let btn = Button::builder().label(&label).css_classes(vec!["flat"]).build();
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

pub fn create_manage_ftp_widget(
    parent: &gtk::Window,
    on_change: Rc<dyn Fn() + 'static>,
    config: client_config::AppConfig,
    on_connect: Option<Rc<dyn Fn(FtpConnection) + 'static>>,
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
        .label(&format!("<b>{}</b>", crate::i18n::tr("conn_manager.saved_connections")))
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
    list_toolbar.append(&btn_tb_new_folder);
    list_toolbar.append(&tb_sep());
    list_toolbar.append(&btn_tb_delete);
    left_vbox.append(&list_toolbar);

    let selection = SingleSelection::new(None::<gio::ListStore>);
    selection.set_can_unselect(true);
    selection.set_autoselect(false);
    let tree_scroller = ScrolledWindow::builder().vexpand(true).build();
    left_vbox.append(&tree_scroller);

    let pending_folder: Rc<std::cell::RefCell<Option<PendingFolder>>> =
        Rc::new(std::cell::RefCell::new(None));
    let folder_generation = Rc::new(std::cell::Cell::new(0u64));

    // Folder a connection created from an empty-folder page must land in; consumed on save.
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
        .label(&format!("<b>{}</b>", crate::i18n::tr("conn_manager.add_new_connection")))
        .use_markup(true)
        .halign(Align::Start)
        .build();
    right_vbox.append(&form_label);

    let form_box = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();

    let form_scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&form_box)
        .vexpand(true)
        .build();
    right_vbox.append(&form_scrolled);

    let entry_name = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.conn_name_placeholder").as_str())
        .build();
    let entry_host = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.host_placeholder").as_str())
        .build();
    let entry_port = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.port_placeholder").as_str())
        .build();
    let entry_user = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.username_placeholder").as_str())
        .build();
    let entry_pass = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.password_placeholder").as_str())
        .visibility(false)
        .build();
    let entry_remote_path = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.remote_path_placeholder").as_str())
        .build();

    let editing_index = Rc::new(std::cell::Cell::new(Option::<usize>::None));

    let protocol_dropdown = gtk::DropDown::from_strings(&["FTP", "SFTP", "WebDAV"]);
    let row_proto = adw::ActionRow::builder().title(&*crate::i18n::tr("conn_manager.protocol")).build();
    row_proto.add_suffix(&protocol_dropdown);

    let auth_dropdown = gtk::DropDown::from_strings(&[
        &*crate::i18n::tr("conn_manager.auth_password"),
        &*crate::i18n::tr("conn_manager.auth_ssh_key"),
    ]);
    let row_auth = adw::ActionRow::builder()
        .title(&*crate::i18n::tr("conn_manager.auth_type"))
        .build();
    row_auth.add_suffix(&auth_dropdown);

    let pref_group = adw::PreferencesGroup::new();
    pref_group.add(&row_proto);
    pref_group.add(&row_auth);

    form_box.append(&entry_name);
    form_box.append(&pref_group);

    let host_port_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();
    entry_host.set_hexpand(true);
    entry_port.set_width_request(100);
    host_port_box.append(&entry_host);
    host_port_box.append(&entry_port);
    form_box.append(&host_port_box);

    let user_pass_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();
    entry_user.set_hexpand(true);
    entry_pass.set_hexpand(true);
    user_pass_box.append(&entry_user);
    user_pass_box.append(&entry_pass);
    form_box.append(&user_pass_box);

    form_box.append(&entry_remote_path);

    let row_key_path = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    let entry_key_path = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.key_path_placeholder").as_str())
        .hexpand(true)
        .build();
    let btn_browse = Button::builder().label(&*crate::i18n::tr("conn_manager.browse")).build();
    row_key_path.append(&entry_key_path);
    row_key_path.append(&btn_browse);
    form_box.append(&row_key_path);

    let entry_passphrase = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.passphrase_placeholder").as_str())
        .visibility(false)
        .build();
    form_box.append(&entry_passphrase);

    let switch_use_tunnel = gtk::Switch::builder()
        .valign(Align::Center)
        .build();
    let row_use_tunnel = adw::ActionRow::builder()
        .title(&*crate::i18n::tr("conn_manager.use_tunnel"))
        .build();
    row_use_tunnel.add_suffix(&switch_use_tunnel);

    let tunnel_box = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();

    let tunnel_sep = gtk::Separator::new(Orientation::Horizontal);
    tunnel_box.append(&tunnel_sep);

    let entry_tunnel_host = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.tunnel_host_placeholder").as_str())
        .hexpand(true)
        .build();
    let entry_tunnel_port = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.tunnel_port_placeholder").as_str())
        .build();
    let tunnel_host_port_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();
    entry_tunnel_port.set_width_request(100);
    tunnel_host_port_box.append(&entry_tunnel_host);
    tunnel_host_port_box.append(&entry_tunnel_port);
    tunnel_box.append(&tunnel_host_port_box);

    let entry_tunnel_user = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.tunnel_user_placeholder").as_str())
        .build();
    tunnel_box.append(&entry_tunnel_user);

    let tunnel_auth_dropdown = gtk::DropDown::from_strings(&[
        &*crate::i18n::tr("conn_manager.auth_password"),
        &*crate::i18n::tr("conn_manager.auth_ssh_key"),
    ]);
    let row_tunnel_auth = adw::ActionRow::builder()
        .title(&*crate::i18n::tr("conn_manager.tunnel_auth_type"))
        .build();
    row_tunnel_auth.add_suffix(&tunnel_auth_dropdown);

    let tunnel_pref_group = adw::PreferencesGroup::new();
    tunnel_pref_group.add(&row_tunnel_auth);
    tunnel_box.append(&tunnel_pref_group);

    let entry_tunnel_pass = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.tunnel_password_placeholder").as_str())
        .visibility(false)
        .build();
    tunnel_box.append(&entry_tunnel_pass);

    let row_tunnel_key_path = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    let entry_tunnel_key_path = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.tunnel_key_path_placeholder").as_str())
        .hexpand(true)
        .build();
    let btn_tunnel_browse = Button::builder().label(&*crate::i18n::tr("conn_manager.browse")).build();
    row_tunnel_key_path.append(&entry_tunnel_key_path);
    row_tunnel_key_path.append(&btn_tunnel_browse);
    tunnel_box.append(&row_tunnel_key_path);

    let entry_tunnel_passphrase = gtk::Entry::builder()
        .placeholder_text(crate::i18n::tr("conn_manager.tunnel_passphrase_placeholder").as_str())
        .visibility(false)
        .build();
    tunnel_box.append(&entry_tunnel_passphrase);

    form_box.append(&row_use_tunnel);
    form_box.append(&tunnel_box);

    let is_view_mode = Rc::new(std::cell::Cell::new(false));
    let previously_selected_index = Rc::new(std::cell::Cell::new(Option::<usize>::None));

    let btn_connect = gtk::Button::builder()
        .label(&*crate::i18n::tr("conn_manager.connect_btn"))
        .css_classes(vec!["suggested-action"])
        .width_request(140)
        .margin_start(12)
        .build();
    btn_connect.set_visible(false);

    let btn_add = gtk::Button::builder()
        .label(&*crate::i18n::tr("conn_manager.add_connection"))
        .css_classes(vec!["suggested-action"])
        .width_request(140)
        .build();

    let btn_cancel = gtk::Button::builder().label(&*crate::i18n::tr("common.cancel")).build();
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

    let load_conn = {
        let entry_name = entry_name.clone();
        let entry_host = entry_host.clone();
        let entry_port = entry_port.clone();
        let entry_user = entry_user.clone();
        let entry_pass = entry_pass.clone();
        let entry_remote_path = entry_remote_path.clone();
        let entry_key_path = entry_key_path.clone();
        let entry_passphrase = entry_passphrase.clone();
        let switch_use_tunnel = switch_use_tunnel.clone();
        let entry_tunnel_host = entry_tunnel_host.clone();
        let entry_tunnel_port = entry_tunnel_port.clone();
        let entry_tunnel_user = entry_tunnel_user.clone();
        let tunnel_auth_dropdown = tunnel_auth_dropdown.clone();
        let entry_tunnel_pass = entry_tunnel_pass.clone();
        let entry_tunnel_key_path = entry_tunnel_key_path.clone();
        let entry_tunnel_passphrase = entry_tunnel_passphrase.clone();
        let protocol_dropdown = protocol_dropdown.clone();
        let auth_dropdown = auth_dropdown.clone();
        
        move |conn: &FtpConnection| {
            let conn = &crate::secret_store::opened(conn);
            entry_name.set_text(&conn.name);
            entry_host.set_text(&conn.host);
            entry_port.set_text(&conn.port.to_string());
            entry_user.set_text(&conn.user);
            entry_pass.set_text(conn.pass.as_deref().unwrap_or(""));
            entry_remote_path.set_text(conn.remote_path.as_deref().unwrap_or(""));
            entry_key_path.set_text(conn.key_path.as_deref().unwrap_or(""));
            entry_passphrase.set_text(conn.passphrase.as_deref().unwrap_or(""));

            let use_tunnel = conn.use_tunnel.unwrap_or(false);
            switch_use_tunnel.set_active(use_tunnel);
            entry_tunnel_host.set_text(conn.tunnel_host.as_deref().unwrap_or(""));
            entry_tunnel_port.set_text(&conn.tunnel_port.map(|p| p.to_string()).unwrap_or_else(|| "".to_string()));
            entry_tunnel_user.set_text(conn.tunnel_user.as_deref().unwrap_or(""));
            tunnel_auth_dropdown.set_selected(if conn.tunnel_auth_type.as_deref() == Some("key") { 1 } else { 0 });
            entry_tunnel_pass.set_text(conn.tunnel_pass.as_deref().unwrap_or(""));
            entry_tunnel_key_path.set_text(conn.tunnel_key_path.as_deref().unwrap_or(""));
            entry_tunnel_passphrase.set_text(conn.tunnel_passphrase.as_deref().unwrap_or(""));

            let proto_idx = if conn.protocol.to_uppercase() == "FTP" {
                0
            } else if conn.protocol.to_uppercase() == "SFTP" {
                1
            } else {
                2
            };
            protocol_dropdown.set_selected(proto_idx);

            let auth_idx = if conn.auth_type.as_deref() == Some("key") {
                1
            } else {
                0
            };
            auth_dropdown.set_selected(auth_idx);
        }
    };
    let load_conn = Rc::new(load_conn);

    let clear_fields = {
        let entry_name = entry_name.clone();
        let entry_host = entry_host.clone();
        let entry_port = entry_port.clone();
        let entry_user = entry_user.clone();
        let entry_pass = entry_pass.clone();
        let entry_remote_path = entry_remote_path.clone();
        let entry_key_path = entry_key_path.clone();
        let entry_passphrase = entry_passphrase.clone();
        let switch_use_tunnel = switch_use_tunnel.clone();
        let entry_tunnel_host = entry_tunnel_host.clone();
        let entry_tunnel_port = entry_tunnel_port.clone();
        let entry_tunnel_user = entry_tunnel_user.clone();
        let tunnel_auth_dropdown = tunnel_auth_dropdown.clone();
        let entry_tunnel_pass = entry_tunnel_pass.clone();
        let entry_tunnel_key_path = entry_tunnel_key_path.clone();
        let entry_tunnel_passphrase = entry_tunnel_passphrase.clone();
        
        move || {
            entry_name.set_text("");
            entry_host.set_text("");
            entry_port.set_text("");
            entry_user.set_text("");
            entry_pass.set_text("");
            entry_remote_path.set_text("");
            entry_key_path.set_text("");
            entry_passphrase.set_text("");
            switch_use_tunnel.set_active(false);
            entry_tunnel_host.set_text("");
            entry_tunnel_port.set_text("");
            entry_tunnel_user.set_text("");
            tunnel_auth_dropdown.set_selected(0);
            entry_tunnel_pass.set_text("");
            entry_tunnel_key_path.set_text("");
            entry_tunnel_passphrase.set_text("");
        }
    };
    let clear_fields = Rc::new(clear_fields);

    let right_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    right_stack.add_named(&right_vbox, Some("form"));
    main_hbox.append(&right_stack);

    let update_visibility = Rc::new({
        let protocol_dropdown = protocol_dropdown.clone();
        let auth_dropdown = auth_dropdown.clone();
        let row_auth = row_auth.clone();
        let entry_host = entry_host.clone();
        let entry_port = entry_port.clone();
        let entry_pass = entry_pass.clone();
        let row_key_path = row_key_path.clone();
        let entry_passphrase = entry_passphrase.clone();
        let row_use_tunnel = row_use_tunnel.clone();
        let switch_use_tunnel = switch_use_tunnel.clone();
        let tunnel_box = tunnel_box.clone();
        let tunnel_auth_dropdown = tunnel_auth_dropdown.clone();
        let entry_tunnel_pass = entry_tunnel_pass.clone();
        let row_tunnel_key_path = row_tunnel_key_path.clone();
        let entry_tunnel_passphrase = entry_tunnel_passphrase.clone();
        move || {
            let selected_proto = protocol_dropdown.selected();
            if selected_proto == 2 {
                row_auth.set_visible(false);
                let webdav_placeholder = crate::i18n::tr("conn_manager.webdav_url_placeholder");
                entry_host.set_placeholder_text(Some(&*webdav_placeholder));
                entry_port.set_visible(false);
                entry_pass.set_visible(true);
                row_key_path.set_visible(false);
                entry_passphrase.set_visible(false);
                row_use_tunnel.set_visible(false);
                tunnel_box.set_visible(false);
            } else {
                let is_sftp = selected_proto == 1;
                row_auth.set_visible(is_sftp);
                row_use_tunnel.set_visible(is_sftp);
                let host_placeholder = if is_sftp {
                    crate::i18n::tr("conn_manager.host_sftp_placeholder")
                } else {
                    crate::i18n::tr("conn_manager.host_ftp_placeholder")
                };
                entry_host.set_placeholder_text(Some(&*host_placeholder));
                entry_port.set_visible(true);
                if is_sftp {
                    let is_key_auth = auth_dropdown.selected() == 1;
                    entry_pass.set_visible(!is_key_auth);
                    row_key_path.set_visible(is_key_auth);
                    entry_passphrase.set_visible(is_key_auth);

                    let use_tunnel = switch_use_tunnel.is_active();
                    tunnel_box.set_visible(use_tunnel);
                    if use_tunnel {
                        let is_tunnel_key_auth = tunnel_auth_dropdown.selected() == 1;
                        entry_tunnel_pass.set_visible(!is_tunnel_key_auth);
                        row_tunnel_key_path.set_visible(is_tunnel_key_auth);
                        entry_tunnel_passphrase.set_visible(is_tunnel_key_auth);
                    }
                } else {
                    entry_pass.set_visible(true);
                    row_key_path.set_visible(false);
                    entry_passphrase.set_visible(false);
                    tunnel_box.set_visible(false);
                }
            }
        }
    });

    protocol_dropdown.connect_selected_notify({
        let update = update_visibility.clone();
        move |_| update()
    });

    auth_dropdown.connect_selected_notify({
        let update = update_visibility.clone();
        move |_| update()
    });

    switch_use_tunnel.connect_active_notify({
        let update = update_visibility.clone();
        move |_| update()
    });

    tunnel_auth_dropdown.connect_selected_notify({
        let update = update_visibility.clone();
        move |_| update()
    });

    update_visibility();

    let set_form_editable = {
        let entry_name = entry_name.clone();
        let entry_host = entry_host.clone();
        let entry_port = entry_port.clone();
        let entry_user = entry_user.clone();
        let entry_pass = entry_pass.clone();
        let entry_remote_path = entry_remote_path.clone();
        let entry_key_path = entry_key_path.clone();
        let entry_passphrase = entry_passphrase.clone();
        let protocol_dropdown = protocol_dropdown.clone();
        let auth_dropdown = auth_dropdown.clone();
        let btn_browse = btn_browse.clone();
        let switch_use_tunnel = switch_use_tunnel.clone();
        let entry_tunnel_host = entry_tunnel_host.clone();
        let entry_tunnel_port = entry_tunnel_port.clone();
        let entry_tunnel_user = entry_tunnel_user.clone();
        let tunnel_auth_dropdown = tunnel_auth_dropdown.clone();
        let entry_tunnel_pass = entry_tunnel_pass.clone();
        let entry_tunnel_key_path = entry_tunnel_key_path.clone();
        let entry_tunnel_passphrase = entry_tunnel_passphrase.clone();
        let btn_tunnel_browse = btn_tunnel_browse.clone();
        
        move |editable: bool| {
            entry_name.set_sensitive(editable);
            entry_host.set_sensitive(editable);
            entry_port.set_sensitive(editable);
            entry_user.set_sensitive(editable);
            entry_pass.set_sensitive(editable);
            entry_remote_path.set_sensitive(editable);
            entry_key_path.set_sensitive(editable);
            entry_passphrase.set_sensitive(editable);
            
            protocol_dropdown.set_sensitive(editable);
            auth_dropdown.set_sensitive(editable);
            btn_browse.set_sensitive(editable);
            
            switch_use_tunnel.set_sensitive(editable);
            entry_tunnel_host.set_sensitive(editable);
            entry_tunnel_port.set_sensitive(editable);
            entry_tunnel_user.set_sensitive(editable);
            tunnel_auth_dropdown.set_sensitive(editable);
            entry_tunnel_pass.set_sensitive(editable);
            entry_tunnel_key_path.set_sensitive(editable);
            entry_tunnel_passphrase.set_sensitive(editable);
            btn_tunnel_browse.set_sensitive(editable);
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
                    form_label.set_markup(&format!("<b>{}</b>", crate::i18n::tr("conn_manager.add_new_connection")));
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
                        form_label.set_markup(&format!("<b>{}</b>", crate::i18n::tr("conn_manager.view_title")));
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
                        form_label.set_markup(&format!("<b>{}</b>", crate::i18n::tr("conn_manager.edit_title")));
                    }
                }
            }
            update_visibility();
        }
    });

    let conns: Vec<FtpConnection> = config.get("ui.ftp_connections").unwrap_or_default();
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

    let parent_clone = parent.clone();
    let entry_key_path_clone = entry_key_path.clone();
    btn_browse.connect_clicked(move |_| {
        let entry_key_path_inner = entry_key_path_clone.clone();
        let dialog = gtk::FileDialog::builder()
            .title(&*crate::i18n::tr("conn_manager.select_ssh_key"))
            .build();
        dialog.open(
            Some(&parent_clone),
            gtk::gio::Cancellable::NONE,
            move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        entry_key_path_inner.set_text(&path.to_string_lossy());
                    }
                }
            },
        );
    });

    let parent_clone2 = parent.clone();
    let entry_tunnel_key_path_clone = entry_tunnel_key_path.clone();
    btn_tunnel_browse.connect_clicked(move |_| {
        let entry_key_path_inner = entry_tunnel_key_path_clone.clone();
        let dialog = gtk::FileDialog::builder()
            .title(&*crate::i18n::tr("conn_manager.select_tunnel_ssh_key"))
            .build();
        dialog.open(
            Some(&parent_clone2),
            gtk::gio::Cancellable::NONE,
            move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        entry_key_path_inner.set_text(&path.to_string_lossy());
                    }
                }
            },
        );
    });


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
                let conns: Vec<FtpConnection> = config_cancel.get("ui.ftp_connections").unwrap_or_default();
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
            let conns: Vec<FtpConnection> = config_cancel.get("ui.ftp_connections").unwrap_or_default();
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
    let folder_title = Label::builder().use_markup(true).halign(Align::Start).build();
    folder_page.append(&folder_title);
    let folder_search = gtk::SearchEntry::builder()
        .placeholder_text(&*crate::i18n::tr("conn_manager.search"))
        .build();
    folder_page.append(&folder_search);
    let folder_list = gtk::ListBox::builder().css_classes(vec!["boxed-list"]).build();
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
            let conns: Vec<FtpConnection> = config.get("ui.ftp_connections").unwrap_or_default();
            let mut total_in_folder = 0usize;
            for (idx, conn) in conns.into_iter().enumerate() {
                let in_folder = conn.folder.as_deref() == Some(path.as_str())
                    || conn.folder.as_deref().map(|f| f.starts_with(&prefix)).unwrap_or(false);
                if !in_folder {
                    continue;
                }
                total_in_folder += 1;
                if !query.is_empty() && !conn.name.to_lowercase().contains(&query) {
                    continue;
                }
                let subtitle = if conn.protocol.eq_ignore_ascii_case("webdav") {
                    conn.host.clone()
                } else {
                    format!(
                        "{}://{}@{}:{}",
                        conn.protocol.to_lowercase(),
                        conn.user,
                        conn.host,
                        conn.port
                    )
                };
                let row = adw::ActionRow::builder()
                    .title(&conn.name)
                    .subtitle(&subtitle)
                    .title_lines(1)
                    .subtitle_lines(1)
                    .activatable(true)
                    .build();
                let res = if conn.protocol.eq_ignore_ascii_case("webdav") {
                    "/com/icecommander/gtk/netdrive.svg"
                } else {
                    "/com/icecommander/gtk/ftp.svg"
                };
                let icon = Image::from_resource(res);
                icon.set_pixel_size(20);
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
            // An empty result under a live query is not an empty folder — keep the search box.
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
        let pending_folder = pending_folder.clone();
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
                .build();
            content.append(&icon);
            content.append(&label);
            content.append(&entry);
            let expander = TreeExpander::new();
            expander.set_child(Some(&content));
            item.set_child(Some(&expander));

            let cancel_pending = {
                let pending_folder = pending_folder.clone();
                let refresh_weak = refresh_weak.clone();
                Rc::new(move || {
                    if pending_folder.borrow().is_none() {
                        return;
                    }
                    *pending_folder.borrow_mut() = None;
                    if let Some(rc) = refresh_weak.upgrade() {
                        if let Some(f) = rc.borrow().as_ref() {
                            f();
                        }
                    }
                })
            };

            {
                let config = config.clone();
                let pending_folder = pending_folder.clone();
                let refresh_weak = refresh_weak.clone();
                entry.connect_activate(move |e| {
                    let Some(pending) = pending_folder.borrow().clone() else {
                        return;
                    };
                    let leaf = e.text().trim().replace('/', "-");
                    if leaf.is_empty() {
                        return;
                    }
                    let path = match &pending.parent {
                        Some(p) => format!("{}/{}", p, leaf),
                        None => leaf,
                    };
                    let conns: Vec<FtpConnection> =
                        config.get("ui.ftp_connections").unwrap_or_default();
                    let mut folders: Vec<String> =
                        config.get("ui.ftp_connection_folders").unwrap_or_default();
                    if connection_folder_paths(&conns, &folders).iter().any(|f| f == &path) {
                        e.add_css_class("error");
                        e.set_tooltip_text(Some(&crate::i18n::tr(
                            "conn_manager.folder_exists",
                        )));
                        return;
                    }
                    folders.push(path);
                    config.set("ui.ftp_connection_folders", folders);
                    config.save();
                    *pending_folder.borrow_mut() = None;
                    if let Some(rc) = refresh_weak.upgrade() {
                        if let Some(f) = rc.borrow().as_ref() {
                            f();
                        }
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
                // Clicking away abandons the row, but a refresh triggered by starting a *new*
                // edit also fires leave — the generation check keeps that from killing it.
                let pending_folder = pending_folder.clone();
                let cancel_pending = cancel_pending.clone();
                let focus = gtk::EventControllerFocus::new();
                focus.connect_leave(move |_| {
                    let Some(generation) =
                        pending_folder.borrow().as_ref().map(|p| p.generation)
                    else {
                        return;
                    };
                    let pending_folder = pending_folder.clone();
                    let cancel_pending = cancel_pending.clone();
                    glib::idle_add_local_once(move || {
                        let still_current = pending_folder
                            .borrow()
                            .as_ref()
                            .map(|p| p.generation == generation)
                            .unwrap_or(false);
                        if still_current {
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
                        ConnNode::NewFolder => return false,
                    };
                    let mut conns: Vec<FtpConnection> =
                        config.get("ui.ftp_connections").unwrap_or_default();
                    if (from as usize) >= conns.len() {
                        return false;
                    }
                    conns[from as usize].folder = target;
                    config.set("ui.ftp_connections", conns);
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
        if !matches!(&*node, ConnNode::NewFolder) {
            entry.set_visible(false);
            label.set_visible(true);
        }
        match &*node {
            ConnNode::Folder(path) => {
                icon.set_resource(Some("/com/icecommander/gtk/folder.svg"));
                label.set_text(folder_leaf(path));
            }
            ConnNode::Connection { conn, .. } => {
                let res = if conn.protocol.eq_ignore_ascii_case("webdav") {
                    "/com/icecommander/gtk/netdrive.svg"
                } else {
                    "/com/icecommander/gtk/ftp.svg"
                };
                icon.set_resource(Some(res));
                label.set_text(&conn.name);
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
        let editing_index = editing_index.clone();
        let previously_selected_index = previously_selected_index.clone();
        let is_view_mode = is_view_mode.clone();
        let load_conn = load_conn.clone();
        let update_ui_state = update_ui_state.clone();
        let right_stack = right_stack.clone();
        let show_folder = show_folder.clone();
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
                ConnNode::NewFolder => {}
            }
        });
    }

    let selected_node = {
        let selection = selection.clone();
        Rc::new(move || -> Option<ConnNode> {
            let row = selection.selected_item()?.downcast::<gtk::TreeListRow>().ok()?;
            let node = row.item()?.downcast::<glib::BoxedAnyObject>().ok()?;
            let node = node.borrow::<ConnNode>().clone();
            Some(node)
        })
    };

    let update_toolbar_state = {
        let selected_node = selected_node.clone();
        let btn_tb_delete = btn_tb_delete.clone();
        Rc::new(move || match selected_node() {
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
        })
    };

    {
        let update_toolbar_state = update_toolbar_state.clone();
        selection.connect_selection_changed(move |_, _, _| update_toolbar_state());
    }

    let refresh_list = {
        let config = config.clone();
        let selection = selection.clone();
        let pending_folder = pending_folder.clone();
        let update_toolbar_state = update_toolbar_state.clone();
        move || {
            let conns: Vec<FtpConnection> =
                config.get("ui.ftp_connections").unwrap_or_default();
            let folders: Vec<String> =
                config.get("ui.ftp_connection_folders").unwrap_or_default();
            let pending = pending_folder.borrow().clone();
            let map = Rc::new(build_children_map(&conns, &folders, pending.as_ref()));
            let root = gio::ListStore::new::<glib::BoxedAnyObject>();
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

    let start_new_folder: Rc<dyn Fn(Option<String>)> = {
        let pending_folder = pending_folder.clone();
        let folder_generation = folder_generation.clone();
        let refresh_list = refresh_list.clone();
        Rc::new(move |parent: Option<String>| {
            folder_generation.set(folder_generation.get() + 1);
            *pending_folder.borrow_mut() = Some(PendingFolder {
                parent,
                generation: folder_generation.get(),
            });
            refresh_list();
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
        let config = config.clone();
        let on_change = on_change.clone();
        let clear_form = clear_form.clone();
        let refresh_list = refresh_list.clone();
        let selected_node = selected_node.clone();
        btn_tb_delete.connect_clicked(move |btn| {
            let Some(node) = selected_node() else {
                return;
            };
            let (body, name) = match &node {
                ConnNode::Connection { conn, .. } => {
                    ("conn_manager.delete_conn_body", conn.name.clone())
                }
                ConnNode::Folder(path) => (
                    "conn_manager.delete_folder_body",
                    folder_leaf(path).to_string(),
                ),
                ConnNode::NewFolder => return,
            };
            let dialog = adw::AlertDialog::builder()
                .heading(&*crate::i18n::tr("conn_manager.delete"))
                .body(&crate::i18n::trf(body, &[("name", name.as_str())]))
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
                    match &node {
                        ConnNode::Connection { index, .. } => {
                            let mut conns: Vec<FtpConnection> =
                                config.get("ui.ftp_connections").unwrap_or_default();
                            if *index < conns.len() {
                                conns.remove(*index);
                                config.set("ui.ftp_connections", conns);
                                config.save();
                                on_change();
                                clear_form();
                                refresh_list();
                            }
                        }
                        ConnNode::Folder(path) => {
                            delete_folder(&config, path);
                            on_change();
                            refresh_list();
                        }
                        ConnNode::NewFolder => {}
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
        let parent = parent.clone();
        let on_connect = on_connect.clone();
        let selection_ctx = selection.clone();
        let start_new_folder = start_new_folder.clone();
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
                            crate::i18n::tr("conn_manager.new_subfolder")
                                .to_string(),
                            std::boxed::Box::new(move || {
                                start_new_folder(Some(path.clone()))
                            }),
                        ));
                    }
                    {
                        let config = config.clone();
                        let refresh = refresh.clone();
                        let parent = parent.clone();
                        let path = path.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.rename").to_string(),
                            std::boxed::Box::new(move || {
                                let config = config.clone();
                                let refresh = refresh.clone();
                                let path = path.clone();
                                let initial = folder_leaf(&path).to_string();
                                prompt_folder_name(
                                    &parent,
                                    &crate::i18n::tr("conn_manager.rename_folder"),
                                    &initial,
                                    move |leaf| {
                                        rename_folder(&config, &path, &leaf);
                                        refresh();
                                    },
                                );
                            }),
                        ));
                    }
                    {
                        let config = config.clone();
                        let refresh = refresh.clone();
                        let on_change = on_change.clone();
                        let path = path.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.delete").to_string(),
                            std::boxed::Box::new(move || {
                                delete_folder(&config, &path);
                                on_change();
                                refresh();
                            }),
                        ));
                    }
                    {
                        let add_conn_in_folder = add_conn_in_folder.clone();
                        let path = path.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.add_new_connection").to_string(),
                            std::boxed::Box::new(move || {
                                add_conn_in_folder(Some(path.clone()))
                            }),
                        ));
                    }
                }
                Some(ConnNode::Connection { index, conn }) => {
                    if let Some(cb) = on_connect.clone() {
                        let conn = conn.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.connect_btn")
                                .to_string(),
                            std::boxed::Box::new(move || cb(crate::secret_store::opened(&conn))),
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
                                let mut conns: Vec<FtpConnection> =
                                    config.get("ui.ftp_connections").unwrap_or_default();
                                if index < conns.len() {
                                    conns.remove(index);
                                    config.set("ui.ftp_connections", conns);
                                    config.save();
                                    on_change();
                                    clear_form();
                                    refresh();
                                }
                            }),
                        ));
                    }
                }
                Some(ConnNode::NewFolder) => {}
                None => {
                    {
                        let start_new_folder = start_new_folder.clone();
                        items.push((
                            crate::i18n::tr("conn_manager.new_folder_title")
                                .to_string(),
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
            let conns: Vec<FtpConnection> =
                config_exp.get("ui.ftp_connections").unwrap_or_default();
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
                        if pw.is_empty() { None } else { Some(pw.as_str()) },
                    );
                    let plain = pw.is_empty();
                    let parent3 = parent2.clone();
                    let fd = gtk::FileDialog::builder()
                        .initial_name("ice-commander-connections.json")
                        .build();
                    fd.save(
                        Some(&parent2),
                        gtk::gio::Cancellable::NONE,
                        move |res| {
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
                        },
                    );
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
                        let mut conns: Vec<FtpConnection> =
                            config.get("ui.ftp_connections").unwrap_or_default();
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
                        config.set("ui.ftp_connections", conns);
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
                                crate::i18n::trf("conn_manager.import_done_body", &[("added", &*(added).to_string()), ("updated", &*(updated).to_string())]),
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
    let entry_remote_path_add = entry_remote_path.clone();
    let switch_use_tunnel_add = switch_use_tunnel.clone();
    let entry_tunnel_host_add = entry_tunnel_host.clone();
    let entry_tunnel_port_add = entry_tunnel_port.clone();
    let entry_tunnel_user_add = entry_tunnel_user.clone();
    let tunnel_auth_dropdown_add = tunnel_auth_dropdown.clone();
    let entry_tunnel_pass_add = entry_tunnel_pass.clone();
    let entry_tunnel_key_path_add = entry_tunnel_key_path.clone();
    let entry_tunnel_passphrase_add = entry_tunnel_passphrase.clone();
    let config_add = config.clone();
    let is_view_mode_add = is_view_mode.clone();
    let update_ui_state_add = update_ui_state.clone();
    let load_conn_add = load_conn.clone();

    let config_seal = config.clone();
    let new_conn_folder_add = new_conn_folder.clone();
    btn_add.connect_clicked(move |_| {
        let is_view = is_view_mode_add.get();
        if is_view && editing_index_add.get().is_some() {
            is_view_mode_add.set(false);
            update_ui_state_add();
            return;
        }

        let name = entry_name.text().to_string();
        let host = entry_host.text().to_string();
        let port_str = entry_port.text().to_string();
        let user = entry_user.text().to_string();
        let protocol = match protocol_dropdown.selected() {
            0 => "FTP".to_string(),
            1 => "SFTP".to_string(),
            _ => "WEBDAV".to_string(),
        };

        if name.is_empty() || host.is_empty() || user.is_empty() {
            return;
        }

        let default_port = if protocol == "FTP" {
            21
        } else if protocol == "SFTP" {
            22
        } else {
            80
        };
        let port = port_str.parse::<u16>().unwrap_or(default_port);

        let pass = if protocol == "FTP" || auth_dropdown.selected() == 0 {
            let p = entry_pass.text().to_string();
            if p.is_empty() {
                None
            } else {
                Some(p)
            }
        } else {
            None
        };

        let auth_type = if protocol == "SFTP" {
            Some(match auth_dropdown.selected() {
                0 => "password".to_string(),
                _ => "key".to_string(),
            })
        } else {
            None
        };

        let key_path = if protocol == "SFTP" && auth_dropdown.selected() == 1 {
            let p = entry_key_path.text().to_string();
            if p.is_empty() {
                None
            } else {
                Some(p)
            }
        } else {
            None
        };

        let passphrase = if protocol == "SFTP" && auth_dropdown.selected() == 1 {
            let p = entry_passphrase.text().to_string();
            if p.is_empty() {
                None
            } else {
                Some(p)
            }
        } else {
            None
        };

        let remote_path = {
            let p = entry_remote_path_add.text().to_string();
            if p.is_empty() {
                None
            } else {
                Some(p)
            }
        };

        let use_tunnel = if protocol == "SFTP" {
            Some(switch_use_tunnel_add.is_active())
        } else {
            None
        };

        let tunnel_host = if protocol == "SFTP" && switch_use_tunnel_add.is_active() {
            let p = entry_tunnel_host_add.text().to_string();
            if p.is_empty() { None } else { Some(p) }
        } else {
            None
        };

        let tunnel_port = if protocol == "SFTP" && switch_use_tunnel_add.is_active() {
            let p = entry_tunnel_port_add.text().to_string();
            Some(p.parse::<u16>().unwrap_or(22))
        } else {
            None
        };

        let tunnel_user = if protocol == "SFTP" && switch_use_tunnel_add.is_active() {
            let p = entry_tunnel_user_add.text().to_string();
            if p.is_empty() { None } else { Some(p) }
        } else {
            None
        };

        let tunnel_auth_type = if protocol == "SFTP" && switch_use_tunnel_add.is_active() {
            Some(if tunnel_auth_dropdown_add.selected() == 1 {
                "key".to_string()
            } else {
                "password".to_string()
            })
        } else {
            None
        };

        let tunnel_pass = if protocol == "SFTP" && switch_use_tunnel_add.is_active() && tunnel_auth_dropdown_add.selected() == 0 {
            let p = entry_tunnel_pass_add.text().to_string();
            if p.is_empty() { None } else { Some(p) }
        } else {
            None
        };

        let tunnel_key_path = if protocol == "SFTP" && switch_use_tunnel_add.is_active() && tunnel_auth_dropdown_add.selected() == 1 {
            let p = entry_tunnel_key_path_add.text().to_string();
            if p.is_empty() { None } else { Some(p) }
        } else {
            None
        };

        let tunnel_passphrase = if protocol == "SFTP" && switch_use_tunnel_add.is_active() && tunnel_auth_dropdown_add.selected() == 1 {
            let p = entry_tunnel_passphrase_add.text().to_string();
            if p.is_empty() { None } else { Some(p) }
        } else {
            None
        };

        let mut conns: Vec<FtpConnection> =
            config_add.get("ui.ftp_connections").unwrap_or_default();
        let folder = match editing_index_add.get() {
            Some(idx) => conns.get(idx).and_then(|c| c.folder.clone()),
            None => new_conn_folder_add.borrow().clone(),
        };
        let mut new_conn = FtpConnection {
            name,
            folder,
            protocol,
            host,
            port,
            user,
            pass,
            auth_type,
            key_path,
            passphrase,
            remote_path,
            use_tunnel,
            tunnel_host,
            tunnel_port,
            tunnel_user,
            tunnel_auth_type,
            tunnel_pass,
            tunnel_key_path,
            tunnel_passphrase,
        };
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
        config_add.set("ui.ftp_connections", conns.clone());
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
                let conns: Vec<FtpConnection> = config_conn.get("ui.ftp_connections").unwrap_or_default();
                if idx < conns.len() {
                    on_connect_cb(crate::secret_store::opened(&conns[idx]));
                }
            }
        });
    }

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
    on_connect: Option<Rc<dyn Fn(FtpConnection) + 'static>>,
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
        Rc::new(move |conn: FtpConnection| {
            cb(conn);
            win.close();
        }) as Rc<dyn Fn(FtpConnection)>
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
    let dialog = adw::AlertDialog::builder().heading(heading).body(body).build();
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

pub fn show_error(parent: &impl IsA<gtk::Widget>, title: &str, msg: &str) {
    let dialog = adw::AlertDialog::builder().heading(title).body(msg).build();
    dialog.add_response("ok", &*crate::i18n::tr("common.ok"));
    dialog.present(Some(parent));
}
