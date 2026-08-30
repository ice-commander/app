#![allow(deprecated)] // adw::MessageDialog / FileDialog — modernization tracked separately
use adw::prelude::*;
use relm4::ComponentSender;

use crate::fm_view::{FmPanelModel, FmPanelOutput, Shared};
use crate::utils::build_path_string;

pub fn present_error(window: Option<&gtk::Window>, title: &str, body: &str) {
    let dialog = adw::MessageDialog::builder()
        .modal(true)
        .heading(title)
        .body(body)
        .build();
    if let Some(w) = window {
        dialog.set_transient_for(Some(w));
    }
    dialog.add_response("ok", "OK");
    dialog.present();
}

pub fn show_create_dir_dialog(
    window: Option<&gtk::Window>,
    shared: &Shared,
    sender: ComponentSender<FmPanelModel>,
) {
    let Some(window) = window else { return };
    let dialog = adw::MessageDialog::new(Some(window), None, None);

    let entry = gtk::Entry::builder()
        .placeholder_text(&*crate::i18n::tr("fm.dir_name_placeholder"))
        .activates_default(true)
        .build();

    let (drive_name, drive_icon, is_fav) = current_drive_info(shared);

    let extra_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();

    let title_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::Start)
        .build();
    let title_icon = gtk::Image::from_resource("/com/fm-ui/gtk/add-folder.svg");
    title_icon.set_pixel_size(24);
    title_box.append(&title_icon);
    title_box.append(
        &gtk::Label::builder()
            .label(format!(
                "<span weight='bold' size='large'>{}</span>",
                crate::i18n::tr("fm.create_dir_title")
            ))
            .use_markup(true)
            .halign(gtk::Align::Start)
            .build(),
    );

    let info_vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    info_vbox.append(&drive_info_widget(&drive_name, &drive_icon, is_fav));
    let parent_path = build_path_string(&shared.current_path.borrow());
    info_vbox.append(
        &gtk::Label::builder()
            .label(format!(
                "<b>{}</b> <span color='gray'>{}</span>",
                crate::i18n::tr("fm.create_in"),
                parent_path
            ))
            .use_markup(true)
            .halign(gtk::Align::Start)
            .build(),
    );

    extra_box.append(&title_box);
    extra_box.append(&info_vbox);
    extra_box.append(&entry);

    dialog.set_extra_child(Some(&extra_box));
    dialog.add_response("cancel", &crate::i18n::tr("fm.cancel"));
    dialog.add_response("create", &crate::i18n::tr("fm.create_btn"));
    dialog.set_default_response(Some("create"));
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);

    let parent_for_cb = parent_path;
    dialog.connect_response(None, move |d, response| {
        if response == "create" {
            let name = entry.text().to_string();
            if !name.is_empty() && !name.contains('/') {
                let _ = sender.output(FmPanelOutput::Mkdir {
                    parent: parent_for_cb.clone(),
                    name,
                });
            }
        }
        d.close();
    });
    dialog.present();
}

pub fn show_delete_dialog(
    window: Option<&gtk::Window>,
    shared: &Shared,
    sender: ComponentSender<FmPanelModel>,
) {
    let Some(window) = window else { return };
    let files = shared.selected_files.borrow().clone();
    if files.is_empty() {
        return;
    }
    let parent = shared.current_path.borrow().clone();

    let body_text = if files.len() == 1 {
        let mut p = parent.clone();
        p.push(files[0].0.clone());
        crate::i18n::trf("fm.confirm_delete_single", &[("path", &*(gtk::glib::markup_escape_text(&build_path_string(&p))).to_string())])
    } else {
        crate::i18n::trf("fm.confirm_delete_multiple", &[("count", &*(files.len()).to_string())])
    };

    let extra_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    let title_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::Start)
        .build();
    let title_icon = gtk::Image::from_resource("/com/fm-ui/gtk/close.svg");
    title_icon.set_pixel_size(24);
    title_box.append(&title_icon);
    title_box.append(
        &gtk::Label::builder()
            .label(format!(
                "<span weight='bold' size='large'>{}</span>",
                crate::i18n::tr("fm.confirm_deletion_title")
            ))
            .use_markup(true)
            .halign(gtk::Align::Start)
            .build(),
    );

    let info_vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    let (drive_name, drive_icon, is_fav) = current_drive_info(shared);
    info_vbox.append(&drive_info_widget(&drive_name, &drive_icon, is_fav));
    info_vbox.append(
        &gtk::Label::builder()
            .label(&*body_text)
            .use_markup(true)
            .halign(gtk::Align::Start)
            .wrap(true)
            .build(),
    );
    extra_box.append(&title_box);
    extra_box.append(&info_vbox);

    let dialog = adw::MessageDialog::builder()
        .transient_for(window)
        .modal(true)
        .extra_child(&extra_box)
        .build();
    dialog.add_response("cancel", &crate::i18n::tr("fm.cancel"));
    dialog.add_response("delete", &crate::i18n::tr("fm.context.delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    dialog.connect_response(None, move |d, response| {
        if response == "delete" {
            let paths: Vec<String> = files
                .iter()
                .map(|(name, _)| {
                    let mut p = parent.clone();
                    p.push(name.clone());
                    build_path_string(&p)
                })
                .collect();
            let names: Vec<String> = files.iter().map(|(n, _)| n.clone()).collect();
            let _ = sender.output(FmPanelOutput::Delete { paths, names });
        }
        d.close();
    });
    dialog.present();
}

pub fn show_rename_dialog(
    window: Option<&gtk::Window>,
    shared: &Shared,
    selection_model: &gtk::MultiSelection,
    sender: ComponentSender<FmPanelModel>,
) {
    let Some(window) = window else { return };
    let bitset = selection_model.selection();
    if bitset.size() != 1 {
        return;
    }
    let Some(obj) = selection_model.item(bitset.nth(0)) else {
        return;
    };
    let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() else {
        return;
    };
    let old_name = entry.name();
    if old_name == ".." {
        return;
    }
    let parent = build_path_string(&shared.current_path.borrow());
    rename_dialog_for(window, &parent, &old_name, sender);
}

pub fn rename_dialog_for(
    window: &gtk::Window,
    parent_path: &str,
    old_name: &str,
    sender: ComponentSender<FmPanelModel>,
) {
    let safe_parent = if parent_path.ends_with('/') {
        parent_path.to_string()
    } else {
        format!("{}/", parent_path)
    };
    let old_path = format!("{}{}", safe_parent, old_name);

    let dialog =
        adw::MessageDialog::new(Some(window), Some(&*crate::i18n::tr("fm.rename_title")), None);
    let entry = gtk::Entry::builder()
        .text(old_name)
        .activates_default(true)
        .build();
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", &crate::i18n::tr("fm.cancel"));
    dialog.add_response("rename", &crate::i18n::tr("fm.rename_btn"));
    dialog.set_default_response(Some("rename"));
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);

    let old_name_c = old_name.to_string();
    dialog.connect_response(None, move |d, response| {
        if response == "rename" {
            let new_name = entry.text().to_string();
            if !new_name.is_empty() && !new_name.contains('/') && new_name != old_name_c {
                let _ = sender.output(FmPanelOutput::Rename {
                    old_path: old_path.clone(),
                    new_path: format!("{}{}", safe_parent, new_name),
                });
            }
        }
        d.close();
    });
    dialog.present();
}

fn mode_from_checks(checks: &[(gtk::CheckButton, u32)]) -> u32 {
    checks
        .iter()
        .filter(|(cb, _)| cb.is_active())
        .fold(0, |m, (_, mask)| m | mask)
}

fn apply_mode_to_checks(mode: u32, checks: &[(gtk::CheckButton, u32)]) {
    for (cb, mask) in checks {
        cb.set_active((mode & mask) != 0);
    }
}

pub fn show_chmod_dialog(
    window: Option<&gtk::Window>,
    shared: &Shared,
    sender: ComponentSender<FmPanelModel>,
) {
    let Some(window) = window else { return };
    let name = {
        let sel = shared.selected_files.borrow();
        match sel.first() {
            Some((n, _)) if n != ".." => n.clone(),
            _ => return,
        }
    };
    let current = shared
        .cached_entries
        .borrow()
        .iter()
        .find(|e| e.0 == name)
        .and_then(|e| e.4)
        .unwrap_or(0o644);
    let mut parts = shared.current_path.borrow().clone();
    parts.push(name.clone());
    let path = build_path_string(&parts);
    chmod_dialog_for(window, &path, &name, current, sender);
}

pub fn chmod_dialog_for(
    window: &gtk::Window,
    path: &str,
    file_name: &str,
    initial_mode: u32,
    sender: ComponentSender<FmPanelModel>,
) {
    let dialog = adw::MessageDialog::new(
        Some(window),
        Some("Permissions"),
        Some(&format!("Permissions for: {file_name}")),
    );

    let grid = gtk::Grid::builder()
        .row_spacing(8)
        .column_spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .build();
    for (col, title) in ["Class", "Read", "Write", "Exec", "Special"].iter().enumerate() {
        grid.attach(&gtk::Label::new(Some(title)), col as i32, 0, 1, 1);
    }

    let rows: [(&str, [(&str, u32); 4]); 3] = [
        ("Owner", [("", 0o400), ("", 0o200), ("", 0o100), ("SUID", 0o4000)]),
        ("Group", [("", 0o040), ("", 0o020), ("", 0o010), ("SGID", 0o2000)]),
        ("Others", [("", 0o004), ("", 0o002), ("", 0o001), ("Sticky", 0o1000)]),
    ];
    let mut checks: Vec<(gtk::CheckButton, u32)> = Vec::with_capacity(12);
    for (row_idx, (class_label, cells)) in rows.iter().enumerate() {
        let r = (row_idx + 1) as i32;
        let lbl = gtk::Label::builder().label(*class_label).halign(gtk::Align::Start).build();
        grid.attach(&lbl, 0, r, 1, 1);
        for (col_idx, (cb_label, mask)) in cells.iter().enumerate() {
            let cb = gtk::CheckButton::builder().label(*cb_label).build();
            grid.attach(&cb, (col_idx + 1) as i32, r, 1, 1);
            checks.push((cb, *mask));
        }
    }

    let octal = gtk::Entry::builder()
        .text(format!("{:04o}", initial_mode & 0o7777))
        .max_length(4)
        .width_chars(6)
        .halign(gtk::Align::Start)
        .build();
    apply_mode_to_checks(initial_mode, &checks);

    let updating = std::rc::Rc::new(std::cell::Cell::new(false));
    for (cb, _) in &checks {
        let checks_c = checks.clone();
        let octal_c = octal.clone();
        let updating_c = updating.clone();
        cb.connect_toggled(move |_| {
            if updating_c.get() {
                return;
            }
            updating_c.set(true);
            octal_c.set_text(&format!("{:04o}", mode_from_checks(&checks_c)));
            updating_c.set(false);
        });
    }
    let focus = gtk::EventControllerFocus::new();
    {
        let checks_c = checks.clone();
        let octal_c = octal.clone();
        let updating_c = updating.clone();
        focus.connect_leave(move |_| {
            if updating_c.get() {
                return;
            }
            let text = octal_c.text();
            if let Ok(mode) = u32::from_str_radix(text.trim(), 8) {
                updating_c.set(true);
                octal_c.set_text(&format!("{:04o}", mode & 0o7777));
                apply_mode_to_checks(mode, &checks_c);
                updating_c.set(false);
            }
        });
    }
    octal.add_controller(focus);
    grid.attach(
        &gtk::Label::builder().label("Octal:").halign(gtk::Align::Start).build(),
        0,
        4,
        1,
        1,
    );
    grid.attach(&octal, 1, 4, 2, 1);

    dialog.set_extra_child(Some(&grid));
    dialog.add_response("cancel", &crate::i18n::tr("fm.cancel"));
    dialog.add_response("apply", "Apply");
    dialog.set_default_response(Some("apply"));
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);

    let path = path.to_string();
    dialog.connect_response(None, move |d, response| {
        if response == "apply" {
            let text = octal.text();
            let mode = u32::from_str_radix(text.trim(), 8)
                .map(|m| m & 0o7777)
                .unwrap_or_else(|_| mode_from_checks(&checks));
            let _ = sender.output(FmPanelOutput::Chmod {
                path: path.clone(),
                mode,
            });
        }
        d.close();
    });
    dialog.present();
}

fn current_drive_info(shared: &Shared) -> (String, String, bool) {
    let conn_id = shared.source.borrow().connection_id.clone();
    let key = if let Some(cid) = conn_id {
        cid
    } else {
        format!(
            "local_fs:{}",
            build_path_string(&shared.current_path.borrow())
        )
    };
    resolve_drive_details(&key, &shared.config)
}

pub fn resolve_drive_details(
    key: &str,
    config: &client_config::AppConfig,
) -> (String, String, bool) {
    #[derive(serde::Deserialize, Clone, Debug)]
    struct DialogFtpConnection {
        name: String,
        protocol: String,
        host: String,
        port: u16,
        user: String,
    }

    let favorites = config
        .get::<Vec<String>>("ui.favorites")
        .unwrap_or_default();

    let mut name = "Local Filesystem".to_string();
    let mut icon = "/com/fm-ui/gtk/ssd.svg".to_string();
    let mut key_for_fav = key.to_string();

    if key.starts_with("webdav://") {
        icon = "/com/fm-ui/gtk/netdrive.svg".to_string();
        let all_conns = config
            .get::<Vec<DialogFtpConnection>>("ui.ftp_connections")
            .unwrap_or_default();
        name = all_conns
            .iter()
            .find(|c| format!("webdav://{}@{}", c.user, c.host) == key)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| key.to_string());
    } else if key.starts_with("ftp://") || key.starts_with("sftp://") {
        icon = "/com/fm-ui/gtk/ftp.svg".to_string();
        let all_conns = config
            .get::<Vec<DialogFtpConnection>>("ui.ftp_connections")
            .unwrap_or_default();
        name = all_conns
            .iter()
            .find(|c| {
                format!(
                    "{}://{}@{}:{}",
                    c.protocol.to_lowercase(),
                    c.user,
                    c.host,
                    c.port
                ) == key
            })
            .map(|c| c.name.clone())
            .unwrap_or_else(|| key.to_string());
    } else if key.starts_with("local_fs:") {
        let path = key.strip_prefix("local_fs:").unwrap_or("");
        let path_norm = path.replace('\\', "/");
        let home_path = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default()
            .replace('\\', "/");

        if path == "~"
            || path_norm == home_path
            || path_norm.starts_with(&format!("{}/", home_path))
        {
            name = "User Home".to_string();
            icon = "/com/fm-ui/gtk/at-home.svg".to_string();
            key_for_fav = "local_fs:~".to_string();
        } else {
            let monitor = gtk::gio::VolumeMonitor::get();
            let mut matched_mount: Option<(String, gtk::gio::Mount)> = None;
            for mount in monitor.mounts() {
                if let Some(m_path) = mount.root().path() {
                    let path_str = m_path.to_string_lossy().to_string();
                    if path_norm.starts_with(&path_str)
                        && matched_mount
                            .as_ref()
                            .is_none_or(|(p, _)| path_str.len() > p.len())
                    {
                        matched_mount = Some((path_str, mount));
                    }
                }
            }
            if let Some((mount_path, mount)) = matched_mount {
                key_for_fav = format!("local_fs:{}", mount_path);
                icon = "/com/fm-ui/gtk/ssd.svg".to_string();
                name = mount.name().to_string();
            } else {
                #[cfg(target_os = "windows")]
                let mut resolved = false;
                #[cfg(not(target_os = "windows"))]
                let resolved = false;
                #[cfg(target_os = "windows")]
                {
                    let parts: Vec<&str> = path.split(&['/', '\\'][..]).collect();
                    if let Some(first_part) = parts.first() {
                        if first_part.ends_with(':') {
                            name = format!(
                                "Disk {}:",
                                first_part.chars().next().unwrap_or('C').to_uppercase()
                            );
                            key_for_fav = format!("local_fs:{}", first_part);
                            icon = "/com/fm-ui/gtk/ssd.svg".to_string();
                            resolved = true;
                        }
                    }
                }
                if !resolved {
                    name = "System Root".to_string();
                    icon = "/com/fm-ui/gtk/home.svg".to_string();
                    key_for_fav = "local_fs:/".to_string();
                }
            }
        }
    }

    let is_fav = favorites.contains(&key_for_fav);
    (name, icon, is_fav)
}

fn drive_info_widget(name: &str, icon_res: &str, is_fav: bool) -> gtk::Box {
    let drive_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Start)
        .css_classes(vec!["dialog-drive-box"])
        .build();
    let drive_icon = gtk::Image::from_resource(icon_res);
    drive_icon.set_pixel_size(24);
    drive_box.append(&drive_icon);
    drive_box.append(
        &gtk::Label::builder()
            .label(name)
            .use_markup(true)
            .css_classes(vec!["dialog-drive-label"])
            .build(),
    );
    let star_res = if is_fav {
        "/com/fm-ui/gtk/star.svg"
    } else {
        "/com/fm-ui/gtk/empty-star.svg"
    };
    let star_icon = gtk::Image::from_resource(star_res);
    star_icon.set_pixel_size(16);
    star_icon.add_css_class(if is_fav {
        "star-active"
    } else {
        "star-inactive"
    });
    drive_box.append(&star_icon);
    drive_box
}
