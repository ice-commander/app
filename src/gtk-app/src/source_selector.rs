use adw::prelude::*;
use gtk::{Align, Box, Button, Label, ListBox, Orientation, Stack};
use panel_router::PanelRouter;
use std::rc::Rc;

use crate::connection_manager::{
    show_error, show_manage_ftp_dialog, FtpConnection,
};

pub fn connect_to_connection(conn: FtpConnection, router: &Rc<PanelRouter>) {
    let conn = crate::secret_store::opened(&conn);
    router.switch_to_selector(false);
    let rpath = conn.remote_path.clone().unwrap_or_else(|| "/".to_string());

    if conn.protocol.to_uppercase() == "FTP" {
        let ftp_rpc = std::rc::Rc::new(virtualfs::ftp_rpc::LocalFtpRpc {
            name: conn.name.clone(),
            host: conn.host.clone(),
            port: conn.port,
            user: conn.user.clone(),
            pass: conn.pass.clone().unwrap_or_default(),
            ftp_session: std::sync::Arc::new(std::sync::Mutex::new(None)),
        });
        router.mount_provider(ftp_rpc, "ftp", rpath.clone());
    } else if conn.protocol.to_uppercase() == "WEBDAV" {
        let webdav_rpc = std::rc::Rc::new(virtualfs::webdav_rpc::LocalWebDavRpc {
            name: conn.name.clone(),
            url: conn.host.clone(),
            user: if conn.user.is_empty() { None } else { Some(conn.user.clone()) },
            pass: conn.pass.clone(),
            remote_path: conn.remote_path.clone(),
        });
        router.mount_provider(webdav_rpc, "webdav", rpath.clone());
    } else if conn.protocol.to_uppercase() == "SFTP" {
        let sftp_rpc = std::rc::Rc::new(virtualfs::sftp_rpc::LocalSftpRpc {
            name: conn.name.clone(),
            host: conn.host.clone(),
            port: conn.port,
            user: conn.user.clone(),
            pass: conn.pass.clone(),
            auth_type: conn.auth_type.clone().unwrap_or_else(|| "password".to_string()),
            key_path: conn.key_path.clone(),
            passphrase: conn.passphrase.clone(),
            use_tunnel: conn.use_tunnel,
            tunnel_host: conn.tunnel_host.clone(),
            tunnel_port: conn.tunnel_port,
            tunnel_user: conn.tunnel_user.clone(),
            tunnel_auth_type: conn.tunnel_auth_type.clone(),
            tunnel_pass: conn.tunnel_pass.clone(),
            tunnel_key_path: conn.tunnel_key_path.clone(),
            tunnel_passphrase: conn.tunnel_passphrase.clone(),
            sftp_session: std::sync::Arc::new(std::sync::Mutex::new(None)),
            tunnel: std::sync::Arc::new(std::sync::Mutex::new(None)),
        });
        router.mount_provider(sftp_rpc, "sftp", rpath.clone());
    }
}

pub fn create_source_selector(
    config: client_config::AppConfig,
    router: Rc<PanelRouter>,
    stack: Stack,
    selector_updaters: Rc<std::cell::RefCell<Vec<Rc<dyn Fn()>>>>,
    on_open_registry: Rc<dyn Fn()>,
    on_open_process_manager: Rc<dyn Fn()>,
) -> Box {
    #[cfg(not(target_os = "windows"))]
    let _ = &on_open_registry;

    let container = Box::builder()
        .orientation(Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();

    let empty_title = Box::new(Orientation::Horizontal, 0);
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .title_widget(&empty_title)
        .build();
    container.append(&header);

    let btn_ftp_box = Box::new(Orientation::Horizontal, 6);
    let btn_ftp_img = gtk::Image::from_resource("/com/icecommander/gtk/ftp.svg");
    btn_ftp_img.set_pixel_size(20);
    btn_ftp_box.append(&btn_ftp_img);
    btn_ftp_box.append(&Label::new(Some(&*crate::i18n::tr("selector.btn_connections"))));

    let btn_ftp = Button::builder()
        .child(&btn_ftp_box)
        .tooltip_text(&*crate::i18n::tr("selector.tooltip_connections"))
        .build();
    btn_ftp.add_css_class("flat");
    btn_ftp.set_cursor_from_name(Some("pointer"));
    header.pack_start(&btn_ftp);

    #[cfg(target_os = "windows")]
    let _btn_reg = {
        let btn_reg_box = Box::new(Orientation::Horizontal, 6);
        let btn_reg_img = gtk::Image::from_resource("/com/icecommander/gtk/registry.svg");
        btn_reg_img.set_pixel_size(20);
        btn_reg_box.append(&btn_reg_img);
        btn_reg_box.append(&Label::new(Some(&*crate::i18n::tr("selector.btn_registry"))));

        let btn_reg = Button::builder()
            .child(&btn_reg_box)
            .tooltip_text(&*crate::i18n::tr("selector.tooltip_registry"))
            .build();
        btn_reg.add_css_class("flat");
        btn_reg.set_cursor_from_name(Some("pointer"));
        let on_open_registry_activated = on_open_registry.clone();
        btn_reg.connect_clicked(move |_| {
            on_open_registry_activated();
        });
        header.pack_end(&btn_reg);
        btn_reg
    };

    let btn_sysinfo_box = Box::new(Orientation::Horizontal, 6);
    let btn_sysinfo_img = gtk::Image::from_resource("/com/icecommander/gtk/processes.svg");
    btn_sysinfo_img.set_pixel_size(20);
    btn_sysinfo_box.append(&btn_sysinfo_img);
    btn_sysinfo_box.append(&Label::new(Some(&*crate::i18n::tr("selector.btn_processes"))));

    let btn_sysinfo = Button::builder()
        .child(&btn_sysinfo_box)
        .tooltip_text(&*crate::i18n::tr("selector.tooltip_processes"))
        .build();
    btn_sysinfo.add_css_class("flat");
    btn_sysinfo.set_cursor_from_name(Some("pointer"));
    let on_open_process_manager_activated = on_open_process_manager.clone();
    btn_sysinfo.connect_clicked(move |_| {
        on_open_process_manager_activated();
    });
    header.pack_end(&btn_sysinfo);

    let selector_box = Box::builder()
        .orientation(Orientation::Vertical)
        .halign(Align::Center)
        .valign(Align::Center)
        .spacing(16)
        .hexpand(true)
        .vexpand(true)
        .build();
    container.append(&selector_box);

    let selector_title = Label::builder()
        .label(&format!("<b>{}</b>", crate::i18n::tr("selector.title")))
        .use_markup(true)
        .halign(Align::Center)
        .margin_top(16)
        .build();
    selector_box.append(&selector_title);

    let list_box = ListBox::builder()
        .width_request(450)
        .css_classes(vec!["boxed-list"])
        .build();

    let scrolled_list = gtk::ScrolledWindow::builder()
        .width_request(450)
        .propagate_natural_height(true)
        .max_content_height(800)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&list_box)
        .build();
    selector_box.append(&scrolled_list);

    let favorites_only_check = gtk::CheckButton::builder()
        .label(&*crate::i18n::tr("selector.favorites_only_label"))
        .halign(Align::Center)
        .build();

    let favorites_hint_label = Label::builder()
        .label(&format!("<span size='small' color='gray'>{}</span>", crate::i18n::tr("selector.shift_hint")))
        .use_markup(true)
        .halign(Align::Center)
        .margin_bottom(16)
        .build();

    let hint_lbl_clone = favorites_hint_label.clone();
    let updaters_check = selector_updaters.clone();
    let config_check = config.clone();
    favorites_only_check.connect_toggled(move |cb| {
        let active = cb.is_active();
        hint_lbl_clone.set_visible(active);
        let current = crate::favorites::is_favorites_only(&config_check);
        if active != current {
            crate::favorites::set_favorites_only(&config_check, active);
            for updater in updaters_check.borrow().iter() {
                updater();
            }
        }
    });

    selector_box.append(&favorites_only_check);
    selector_box.append(&favorites_hint_label);

    let add_source_row = {
        let config = config.clone();
        move |title: &str,
              subtitle: &str,
              icon_name: &str,
              key: Option<&str>,
              updaters: &Rc<std::cell::RefCell<Vec<Rc<dyn Fn()>>>>|
              -> adw::ActionRow {
            let row = adw::ActionRow::builder()
                .title(title)
                .subtitle(subtitle)
                .activatable(true)
                .build();
            let icon = gtk::Image::from_resource(&format!("/com/icecommander/gtk/{}", icon_name));
            icon.set_pixel_size(30);
            row.add_prefix(&icon);

            if let Some(k) = key {
                let is_fav = crate::favorites::is_favorite(&config, k);
                let star_icon = if is_fav {
                    gtk::Image::from_resource("/com/icecommander/gtk/star.svg")
                } else {
                    gtk::Image::from_resource("/com/icecommander/gtk/empty-star.svg")
                };
                star_icon.set_pixel_size(20);

                let btn = Button::builder()
                    .child(&star_icon)
                    .valign(Align::Center)
                    .build();
                btn.add_css_class("flat");
                btn.set_cursor_from_name(Some("pointer"));
                btn.set_focusable(false);

                if is_fav {
                    btn.add_css_class("star-active");
                } else {
                    btn.add_css_class("star-inactive");
                }

                let key_clone = k.to_string();
                let updaters_clone = updaters.clone();
                let config_star = config.clone();
                btn.connect_clicked(move |_| {
                    crate::favorites::toggle_favorite(&config_star, &key_clone);
                    for updater in updaters_clone.borrow().iter() {
                        updater();
                    }
                });
                row.add_suffix(&btn);
            }

            let go_icon = gtk::Image::from_icon_name("go-next-symbolic");            go_icon.set_pixel_size(16);
            row.add_suffix(&go_icon);
            row
        }
    };

    let monitor = gtk::gio::VolumeMonitor::get();

    let update_selector_ui = {
        let list_box = list_box.clone();
        let router = router.clone();
        let stack = stack.clone();
        let _monitor = monitor.clone();
        let favorites_only_check = favorites_only_check.clone();
        let favorites_hint_label = favorites_hint_label.clone();
        let selector_updaters_clone = selector_updaters.clone();
        #[cfg(target_os = "windows")]
        let on_open_registry_clone = on_open_registry.clone();
        let config = config.clone();

        Rc::new(move || {
            let favorites_only = crate::favorites::is_favorites_only(&config);
            if favorites_only_check.is_active() != favorites_only {
                favorites_only_check.set_active(favorites_only);
            }
            favorites_hint_label.set_visible(favorites_only);

            while let Some(child) = list_box.first_child() {
                list_box.remove(&child);
            }

            let all_drives = crate::drives::get_all_app_drives(&config);

            for p in &all_drives {
                match &p.item {
                    crate::drives::AppDriveItem::RootFs => {
                        let row_root = add_source_row(&p.name, &p.subtitle, "home.svg", Some(&p.key), &selector_updaters_clone);
                        let router_root = router.clone();
                        let stack_root = stack.clone();
                        let item_root = p.item.clone();
                        row_root.connect_activated(move |_| {
                            if let crate::drives::DriveActivation::Shown =
                                crate::drives::activate_drive_item(&item_root, &router_root)
                            {
                                stack_root.set_visible_child_name("filemanager");
                            }
                        });
                        list_box.append(&row_root);
                        list_box.select_row(Some(&row_root));
                    }
                    crate::drives::AppDriveItem::UserHome => {
                        let row_home = add_source_row(&p.name, &p.subtitle, "at-home.svg", Some(&p.key), &selector_updaters_clone);
                        let router_home = router.clone();
                        let stack_home = stack.clone();
                        let item_home = p.item.clone();
                        row_home.connect_activated(move |_| {
                            if let crate::drives::DriveActivation::Shown =
                                crate::drives::activate_drive_item(&item_home, &router_home)
                            {
                                stack_home.set_visible_child_name("filemanager");
                            }
                        });
                        list_box.append(&row_home);
                    }
                    _ => {}
                }
            }

            for p in &all_drives {
                match &p.item {
                    crate::drives::AppDriveItem::LocalDrive(_path) => {
                        let row_drive = add_source_row(&p.name, &p.subtitle, "ssd.svg", Some(&p.key), &selector_updaters_clone);
                        let router_drive = router.clone();
                        let stack_drive = stack.clone();
                        let item_drive = p.item.clone();
                        row_drive.connect_activated(move |_| {
                            if let crate::drives::DriveActivation::Shown =
                                crate::drives::activate_drive_item(&item_drive, &router_drive)
                            {
                                stack_drive.set_visible_child_name("filemanager");
                            }
                        });
                        list_box.append(&row_drive);
                    }
                    crate::drives::AppDriveItem::Volume(vol) => {
                        let row_vol = add_source_row(&p.name, &p.subtitle, "ssd.svg", Some(&p.key), &selector_updaters_clone);
                        let vol_clone = vol.clone();
                        let router_vol = router.clone();
                        let stack_vol = stack.clone();
                        let row_vol_inner = row_vol.clone();
                        row_vol.connect_activated(move |_| {
                            let vol_inner = vol_clone.clone();
                            let router_inner = router_vol.clone();
                            let stack_inner = stack_vol.clone();
                            let row_inner = row_vol_inner.clone();
                            gtk::glib::spawn_future_local(async move {
                                let mut mount_success = true;
                                if vol_inner.get_mount().is_none() {
                                    let root_opt = stack_inner.root();
                                    let parent_win = root_opt.clone().and_then(|r| r.downcast::<gtk::Window>().ok());
                                    let mount_op = gtk::MountOperation::new(parent_win.as_ref());
                                    match vol_inner.mount_future(gtk::gio::MountMountFlags::NONE, Some(&mount_op)).await {
                                        Ok(_) => {}
                                        Err(e) => {
                                            let msg = crate::i18n::trf("selector.mount_failed_body", &[("device", &*(vol_inner.name().to_string()).to_string()), ("error", &*(e.to_string()).to_string())]);
                                            show_error(&row_inner, &*crate::i18n::tr("selector.mount_failed_title"), &msg);
                                            mount_success = false;
                                        }
                                    }
                                }
                                if mount_success {
                                    if let Some(mount) = vol_inner.get_mount() {
                                        let root = mount.root();
                                        if let Some(path) = root.path() {
                                            let path_str = path.to_string_lossy().to_string();
                                            router_inner.open_local_path(path_str);
                                            stack_inner.set_visible_child_name("filemanager");
                                        } else {
                                            let msg = crate::i18n::trf("selector.path_resolution_failed_body", &[("device", &*(vol_inner.name().to_string()).to_string()), ("uri", &*(root.uri().to_string()).to_string())]);
                                            show_error(&row_inner, &*crate::i18n::tr("selector.path_resolution_failed_title"), &msg);
                                        }
                                    } else {
                                        let msg = crate::i18n::trf("selector.mount_details_unavailable_body", &[("device", &*(vol_inner.name().to_string()).to_string())]);
                                        show_error(&row_inner, &*crate::i18n::tr("selector.mount_details_unavailable_title"), &msg);
                                    }
                                }
                            });
                        });
                        list_box.append(&row_vol);
                    }
                    _ => {}
                }
            }

            let conn_header = adw::ActionRow::builder()
                .title(&format!("<b>{}</b>", crate::i18n::tr("selector.connections_section")))
                .selectable(false)
                .activatable(false)
                .build();

            let add_conn_img = gtk::Image::from_resource("/com/icecommander/gtk/add.svg");
            add_conn_img.set_pixel_size(16);
            let add_conn_btn = gtk::Button::builder()
                .child(&add_conn_img)
                .css_classes(vec!["flat"])
                .tooltip_text(&*crate::i18n::tr("selector.tooltip_add_connection"))
                .valign(Align::Center)
                .build();
            add_conn_btn.set_cursor_from_name(Some("pointer"));
            add_conn_btn.set_focusable(false);

            let stack_ftp_header = stack.clone();
            let selector_updaters_ftp_header = selector_updaters_clone.clone();
            let config_manage_ftp = config.clone();
            let router_for_btn = router.clone();
            let _stack_for_btn = stack.clone();
            add_conn_btn.connect_clicked(move |_| {
                if let Some(root) = stack_ftp_header.root() {
                    if let Some(win) = root.downcast_ref::<gtk::Window>() {
                        let updaters = selector_updaters_ftp_header.clone();
                        let on_change = Rc::new(move || {
                            for updater in updaters.borrow().iter() {
                                updater();
                            }
                        });
                        let router_clone = router_for_btn.clone();
                        show_manage_ftp_dialog(win, on_change, config_manage_ftp.clone(), Some(Rc::new(move |conn| {
                            connect_to_connection(conn, &router_clone);
                        })));
                    }
                }
            });
            conn_header.add_suffix(&add_conn_btn);
            list_box.append(&conn_header);

            for p in &all_drives {
                if let crate::drives::AppDriveItem::NetConnection(conn) = &p.item {
                    let icon_file = if conn.protocol.to_uppercase() == "WEBDAV" {
                        "netdrive.svg"
                    } else {
                        "ftp.svg"
                    };
                    let row_vol = add_source_row(&p.name, &p.subtitle, icon_file, Some(&p.key), &selector_updaters_clone);
                    let stack_vol = stack.clone();
                    let router_clone = router.clone();
                    let item_conn = p.item.clone();
                    row_vol.connect_activated(move |_| {
                        if let crate::drives::DriveActivation::Shown =
                            crate::drives::activate_drive_item(&item_conn, &router_clone)
                        {
                            stack_vol.set_visible_child_name("filemanager");
                        }
                    });
                    list_box.append(&row_vol);
                }
            }
        })
    };

    selector_updaters
        .borrow_mut()
        .push(update_selector_ui.clone());

    update_selector_ui();

    let update1 = update_selector_ui.clone();
    monitor.connect_volume_added(move |_, _| update1());
    let update2 = update_selector_ui.clone();
    monitor.connect_volume_removed(move |_, _| update2());
    let update3 = update_selector_ui.clone();
    monitor.connect_mount_added(move |_, _| update3());
    let update4 = update_selector_ui.clone();
    monitor.connect_mount_removed(move |_, _| update4());

    let stack_ftp = stack.clone();
    let selector_updaters_ftp = selector_updaters.clone();
    let config_btn_ftp = config.clone();
    btn_ftp.connect_clicked(move |_| {
        if let Some(root) = stack_ftp.root() {
            if let Some(win) = root.downcast_ref::<gtk::Window>() {
                let updaters = selector_updaters_ftp.clone();
                let on_change = Rc::new(move || {
                    for updater in updaters.borrow().iter() {
                        updater();
                    }
                });
                let router_clone = router.clone();
                show_manage_ftp_dialog(win, on_change, config_btn_ftp.clone(), Some(Rc::new(move |conn| {
                    connect_to_connection(conn, &router_clone);
                })));
            }
        }
    });

    container
}
