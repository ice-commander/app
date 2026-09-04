use adw::prelude::*;
use gtk::{Align, Box, DropDown, Label, Orientation, Stack};
use panel_router::PanelRouter;
use std::cell::RefCell;
use std::rc::Rc;

fn is_currently_active(
    router: &Rc<PanelRouter>,
    item: &crate::drives::AppDriveItem,
    stack: &Stack,
) -> bool {
    if stack.visible_child_name().as_deref() != Some("filemanager") {
        return false;
    }

    let resource_id = router.current_resource_id();
    let current_path_str = router.current_path_string();
    let current_norm = current_path_str.replace('\\', "/");

    match item {
        crate::drives::AppDriveItem::RootFs => {
            if resource_id != "local_fs" {
                return false;
            }
            #[cfg(target_os = "windows")]
            {
                return current_norm == "/" || current_norm.is_empty();
            }
            #[cfg(not(target_os = "windows"))]
            {
                let home_path = dirs::home_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if current_norm.starts_with(&home_path) {
                    return false;
                }
                #[cfg(target_os = "macos")]
                {
                    if current_norm.starts_with("/Volumes/") {
                        return false;
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let monitor = gtk::gio::VolumeMonitor::get();
                    for vol in monitor.volumes() {
                        if let Some(mount) = vol.get_mount() {
                            let root = mount.root();
                            let path_opt = root.path();
                            if let Some(path) = path_opt {
                                let vol_path = path.to_string_lossy().to_string();
                                if current_norm.starts_with(&vol_path) {
                                    return false;
                                }
                            }
                        }
                    }
                }
                true
            }
        }
        crate::drives::AppDriveItem::UserHome => {
            if resource_id != "local_fs" {
                return false;
            }
            let home_path = dirs::home_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
                .replace('\\', "/");
            current_norm.starts_with(&home_path)
        }
        crate::drives::AppDriveItem::LocalDrive(drive) => {
            if resource_id != "local_fs" {
                return false;
            }
            let drive_norm = drive.replace('\\', "/");
            current_norm.starts_with(&drive_norm)
        }
        crate::drives::AppDriveItem::Volume(vol) => {
            if resource_id != "local_fs" {
                return false;
            }
            if let Some(mount) = vol.get_mount() {
                let root = mount.root();
                let path_opt = root.path();
                if let Some(path) = path_opt {
                    let vol_path = path.to_string_lossy().to_string().replace('\\', "/");
                    return current_norm.starts_with(&vol_path);
                }
            }
            false
        }
        crate::drives::AppDriveItem::NetConnection(conn) => {
            if let Some(conn_id) = router.provider().connection_id() {
                let target_id = if conn.protocol.to_uppercase() == "WEBDAV" {
                    format!("webdav://{}@{}", conn.user, conn.host)
                } else {
                    format!(
                        "{}://{}@{}:{}",
                        conn.protocol.to_lowercase(),
                        conn.user,
                        conn.host,
                        conn.port
                    )
                };
                return conn_id == target_id;
            }
            false
        }
    }
}

pub fn create_drives_toolbar(
    router: Rc<PanelRouter>,
    stack: Stack,
    selector_updaters: Rc<std::cell::RefCell<Vec<Rc<dyn Fn()>>>>,
    shift_held: Rc<std::cell::Cell<bool>>,
    config: client_config::AppConfig,
    nav_hook: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> (Box, DropDown) {
    let is_syncing = Rc::new(std::cell::Cell::new(false));
    let monitor = gtk::gio::VolumeMonitor::get();
    let string_list = gtk::StringList::new(&[]);
    let drive_items = Rc::new(RefCell::new(Vec::<crate::drives::AppDrive>::new()));

    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, obj| {
        let list_item = obj.downcast_ref::<gtk::ListItem>().expect("factory always provides ListItem");
        let hbox = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .build();
        let img = gtk::Image::new();
        img.set_pixel_size(24);
        let lbl = Label::builder().halign(Align::Start).hexpand(true).build();
        let star = Label::builder().halign(Align::End).build();
        hbox.append(&img);
        hbox.append(&lbl);
        hbox.append(&star);
        list_item.set_child(Some(&hbox));
    });

    let drive_items_bind = drive_items.clone();
    factory.connect_bind(move |_, obj| {
        let list_item = obj.downcast_ref::<gtk::ListItem>().expect("factory always provides ListItem");
        if let Some(child) = list_item.child() {
            if let Some(hbox) = child.downcast_ref::<Box>() {
                if let Some(img) = hbox
                    .first_child()
                    .and_then(|c| c.downcast::<gtk::Image>().ok())
                {
                    if let Some(lbl) = img.next_sibling().and_then(|c| c.downcast::<Label>().ok()) {
                        let star = lbl.next_sibling().and_then(|c| c.downcast::<Label>().ok());
                        let pos = list_item.position() as usize;
                        let item_opt = drive_items_bind.borrow().get(pos).cloned();
                        if let Some(item) = item_opt {
                            lbl.set_text(&item.name);
                            img.set_resource(Some(&item.icon));
                            if let Some(star) = star {
                                star.set_text(if item.is_favorite { "⭐" } else { "" });
                            }
                        } else {
                            println!("[DrivesToolbar] Bind warning: position {} is out of bounds for drive_items length {}", pos, drive_items_bind.borrow().len());
                        }
                    }
                }
            }
        }
    });

    let button_factory = gtk::SignalListItemFactory::new();
    button_factory.connect_setup(|_, obj| {
        let list_item = obj.downcast_ref::<gtk::ListItem>().expect("factory always provides ListItem");
        list_item.set_child(Some(&Label::new(None)));
    });

    let drive_dropdown = DropDown::builder()
        .model(&string_list)
        .factory(&button_factory)
        .list_factory(&factory)
        .build();

    let switch_icon = gtk::Image::new();
    switch_icon.set_pixel_size(20);
    let switch_label = Label::builder()
        .halign(Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let switch_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    switch_box.append(&switch_icon);
    switch_box.append(&switch_label);

    let update_switch: Rc<dyn Fn()> = {
        let dd = drive_dropdown.clone();
        let items = drive_items.clone();
        let icon = switch_icon.clone();
        let lbl = switch_label.clone();
        Rc::new(move || {
            let sel = dd.selected();
            if let Some(item) = items.borrow().get(sel as usize) {
                lbl.set_text(&item.name);
                icon.set_resource(Some(&item.icon));
            }
        })
    };
    drive_dropdown.connect_selected_notify({
        let update = update_switch.clone();
        move |_| update()
    });


    let update_volumes_ui = {
        let string_list = string_list.clone();
        let drive_items = drive_items.clone();
        let router = router.clone();
        let stack = stack.clone();
        let is_syncing_vol = is_syncing.clone();
        let shift_held = shift_held.clone();
        let drive_dropdown_vol = drive_dropdown.clone();

        Rc::new(move || {
            is_syncing_vol.set(true);

            let shift_active = shift_held.get();
            let fav_only = config.get::<bool>("ui.drives_toolbar_favorites_only")
                .unwrap_or(false)
                && !shift_active;

            let all_drives = crate::drives::get_all_app_drives(&config);

            let mut populated = all_drives.clone();

            if fav_only {
                let router_check = router.clone();
                let stack_check = stack.clone();
                populated.retain(|p| {
                    p.is_favorite || is_currently_active(&router_check, &p.item, &stack_check)
                });
            } else {
                populated.sort_by(|a, b| b.is_favorite.cmp(&a.is_favorite));
            }

            let mut items = Vec::new();
            let mut dropdown_strings = Vec::new();
            for p in populated {
                items.push(p.clone());
                if p.is_favorite {
                    dropdown_strings.push(format!("⭐ {}", p.name));
                } else {
                    dropdown_strings.push(p.name);
                }
            }

            let mut active_idx = 0;
            for (idx, item) in items.iter().enumerate() {
                if is_currently_active(&router, &item.item, &stack) {
                    active_idx = idx;
                }
            }

            let dropdown_refs: Vec<&str> = dropdown_strings.iter().map(|s| s.as_str()).collect();

            is_syncing_vol.set(true);
            *drive_items.borrow_mut() = items;
            string_list.splice(0, string_list.n_items(), &dropdown_refs);
            drive_dropdown_vol.set_selected(active_idx as u32);
            is_syncing_vol.set(false);

        })
    };

    selector_updaters
        .borrow_mut()
        .push(update_volumes_ui.clone());

    update_volumes_ui();
    update_switch();

    {
        let update1 = update_volumes_ui.clone();
        monitor.connect_volume_added(move |_, _| update1());
        let update2 = update_volumes_ui.clone();
        monitor.connect_volume_removed(move |_, _| update2());
        let update3 = update_volumes_ui.clone();
        monitor.connect_mount_added(move |_, _| update3());
        let update4 = update_volumes_ui.clone();
        monitor.connect_mount_removed(move |_, _| update4());
    }

    let router_dd = router.clone();
    let stack_dd = stack.clone();
    let drive_items_dd = drive_items.clone();

    let is_syncing_dd = is_syncing.clone();
    drive_dropdown.connect_selected_notify(move |dd| {
        if is_syncing_dd.get() {
            return;
        }
        let idx = dd.selected();
        if idx == u32::MAX {
            return;
        }
        let item_opt = drive_items_dd.borrow().get(idx as usize).cloned();
        if let Some(item) = item_opt {
            if is_currently_active(&router_dd, &item.item, &stack_dd) {
                return;
            }

            match crate::drives::activate_drive_item(&item.item, &router_dd) {
                crate::drives::DriveActivation::Shown => {
                    stack_dd.set_visible_child_name("filemanager");
                }
                crate::drives::DriveActivation::NeedsAsyncMount(vol_inner) => {
                    let router_inner = router_dd.clone();
                    let stack_inner = stack_dd.clone();
                    let dd_inner = dd.clone();

                    gtk::glib::spawn_future_local(async move {
                        let mut mount_success = true;
                        if vol_inner.get_mount().is_none() {
                            let root_opt = stack_inner.root();
                            let parent_win = root_opt.clone().and_then(|r| r.downcast::<gtk::Window>().ok());
                            let mount_op = gtk::MountOperation::new(parent_win.as_ref());
                            match vol_inner.mount_future(gtk::gio::MountMountFlags::NONE, Some(&mount_op)).await {
                                Ok(_) => {}
                                 Err(e) => {
                                     let msg = crate::i18n::trf("toolbar.mount_failed_body", &[("device", &*(vol_inner.name().to_string()).to_string()), ("error", &*(e.to_string()).to_string())]);
                                     show_error(&dd_inner, &*crate::i18n::tr("toolbar.mount_failed_title"), &msg);
                                     mount_success = false;
                                 }
                            }
                        }
                        if mount_success {
                            if let Some(mount) = vol_inner.get_mount() {
                                let root = mount.root();
                                let path_opt = root.path();
                                 if let Some(path) = path_opt {
                                     let path_str = path.to_string_lossy().to_string();
                                     router_inner.open_local_path(path_str);
                                     stack_inner.set_visible_child_name("filemanager");
                                 } else {
                                      let msg = crate::i18n::trf("selector.path_resolution_failed_body", &[("device", &*(vol_inner.name().to_string()).to_string()), ("uri", &*(root.uri().to_string()).to_string())]);
                                      show_error(&dd_inner, &*crate::i18n::tr("selector.path_resolution_failed_title"), &msg);
                                  }
                             } else {
                                 let msg = crate::i18n::trf("selector.mount_details_unavailable_body", &[("device", &*(vol_inner.name().to_string()).to_string())]);
                                 show_error(&dd_inner, &*crate::i18n::tr("selector.mount_details_unavailable_title"), &msg);
                             }
                        }
                    });
                }
            }
        }
    });

    let drive_dropdown_sync = drive_dropdown.clone();
    let drive_items_sync = drive_items.clone();
    let router_sync = router.clone();
    let stack_sync = stack.clone();

    let update_vol_sync = update_volumes_ui.clone();
    let is_syncing_sync = is_syncing.clone();
    let sync_selection = Rc::new(move || {
        let items = drive_items_sync.borrow();
        let mut matched_idx = None;
        for (idx, item) in items.iter().enumerate() {
            if is_currently_active(&router_sync, &item.item, &stack_sync) {
                matched_idx = Some(idx);
                break;
            }
        }

        if let Some(idx) = matched_idx {
            if drive_dropdown_sync.selected() != idx as u32 {
                is_syncing_sync.set(true);
                drive_dropdown_sync.set_selected(idx as u32);
                is_syncing_sync.set(false);
            }
        } else {
            let is_in_fm = stack_sync.visible_child_name().as_deref() == Some("filemanager");
            if is_in_fm {
                drop(items);
                update_vol_sync();
            } else {
                if drive_dropdown_sync.selected() != 0 {
                    is_syncing_sync.set(true);
                    drive_dropdown_sync.set_selected(0);
                    is_syncing_sync.set(false);
                }
            }
        }
    });

    router.set_on_navigated({
        let sync = sync_selection.clone();
        let nav_hook = nav_hook.clone();
        move || {
            sync();
            if let Some(f) = nav_hook.borrow().as_ref() {
                f();
            }
        }
    });

    stack.connect_visible_child_name_notify({
        let sync = sync_selection.clone();
        let nav_hook = nav_hook.clone();
        move |st| {
            sync();
            let on_selector = st.visible_child_name().map(|n| n == "selector").unwrap_or(false);
            if on_selector {
                if let Some(f) = nav_hook.borrow().as_ref() {
                    f();
                }
            }
        }
    });

    (switch_box, drive_dropdown)
}

fn show_error(parent: &impl IsA<gtk::Widget>, title: &str, msg: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading(title)
        .body(msg)
        .build();
    dialog.add_response("ok", &*crate::i18n::tr("common.ok"));
    dialog.present(Some(parent));
}
