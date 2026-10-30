use crate::connection_manager::Connection;
use gtk::prelude::*;
use localfs::utils::get_drives;
use panel_router::PanelRouter;
use std::rc::Rc;

#[derive(Clone)]
pub enum AppDriveItem {
    RootFs,
    UserHome,
    LocalDrive(String),
    Volume(gtk::gio::Volume),
    NetConnection(Connection),
    /// Offered by a plugin rather than saved by the user — a peer's share, for
    /// instance. Mounted through the connection kind that published it.
    Offered {
        kind: String,
        settings: std::collections::BTreeMap<String, String>,
        /// How the row showed itself, so the path begins with the same name
        /// and picture the user picked.
        shown: fm_core::plugin_fs::Shown,
    },
}

/// What a saved connection is drawn with when its kind brought no picture.
const NO_PICTURE: &str = "/com/icecommander/gtk/connect.svg";

#[derive(Clone)]
pub struct AppDrive {
    pub item: AppDriveItem,
    pub name: String,
    pub subtitle: String,
    pub icon: String, // e.g., "/com/icecommander/gtk/ssd.svg"
    /// An icon the entry brought with it, for a drive a plugin offers. Empty
    /// means draw `icon` from the application's own resources.
    pub svg: Vec<u8>,
    pub key: String, // e.g., "local_fs:/", "ftp://..."
    pub is_favorite: bool,
    pub is_online: bool,
}

pub enum DriveActivation {
    Shown,
    NeedsAsyncMount(gtk::gio::Volume),
}

pub fn get_all_app_drives(config: &client_config::AppConfig) -> Vec<AppDrive> {
    let mut drives = Vec::new();
    let favorites = config
        .get::<Vec<String>>("ui.favorites")
        .unwrap_or_default();

    // Android lets no app read "/"; "~" is the phone's own storage there.
    #[cfg(not(target_os = "android"))]
    {
        let root_key = "local_fs:/".to_string();
        drives.push(AppDrive {
            item: AppDriveItem::RootFs,
            name: crate::i18n::tr("drives.system_root").to_string(),
            subtitle: "/".to_string(),
            icon: "/com/icecommander/gtk/home.svg".to_string(),
            key: root_key.clone(),
            is_favorite: favorites.contains(&root_key),
            is_online: true,
            svg: Vec::new(),
        });
    }

    let home_key = "local_fs:~".to_string();
    let home_path = dirs::home_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    drives.push(AppDrive {
        item: AppDriveItem::UserHome,
        name: crate::i18n::tr("drives.user_home").to_string(),
        subtitle: home_path,
        icon: "/com/icecommander/gtk/at-home.svg".to_string(),
        key: home_key.clone(),
        is_favorite: favorites.contains(&home_key),
        is_online: true,
        svg: Vec::new(),
    });

    for drive in get_drives() {
        if drive.is_mounted {
            let key = format!("local_fs:{}", drive.path);
            drives.push(AppDrive {
                item: AppDriveItem::LocalDrive(drive.path.clone()),
                name: drive.name.clone(),
                subtitle: drive.path.clone(),
                icon: "/com/icecommander/gtk/ssd.svg".to_string(),
                is_favorite: favorites.contains(&key),
                is_online: true,
                svg: Vec::new(),
                key,
            });
        }
    }

    let monitor = gtk::gio::VolumeMonitor::get();
    for volume in monitor.volumes() {
        if volume.can_mount() && volume.get_mount().is_none() {
            let name = volume.name().to_string();
            let key = name.clone();
            drives.push(AppDrive {
                item: AppDriveItem::Volume(volume.clone()),
                name: name.clone(),
                subtitle: crate::i18n::tr("drives.not_mounted").to_string(),
                icon: "/com/icecommander/gtk/ssd.svg".to_string(),
                is_favorite: favorites.contains(&key),
                is_online: false,
                svg: Vec::new(),
                key,
            });
        }
    }

    let all_conns: Vec<Connection> = connection_form::stored_connections(&config);
    for conn in &all_conns {
        let key = connection_form::connection_key(conn);
        let svg = match connection_form::kind_picture(&conn.kind) {
            connection_form::KindPicture::Svg(bytes) => bytes,
            _ => Vec::new(),
        };
        let subtitle = connection_form::kind_summary(conn).unwrap_or_default();

        drives.push(AppDrive {
            item: AppDriveItem::NetConnection(conn.clone()),
            name: conn.name.clone(),
            subtitle,
            icon: NO_PICTURE.to_string(),
            is_favorite: favorites.contains(&key),
            is_online: true,
            svg,
            key,
        });
    }

    drives.extend(offered_drives(&favorites));

    drives
}

/// What the plugins offer right now. Ported from the fork's `p2p_app_drives`:
/// each of a peer's shares is its own row, named after the share, and the key
/// is the plugin's so a favourite survives a restart.
fn offered_drives(favorites: &[String]) -> Vec<AppDrive> {
    ic_plugin_host::plugin_drives()
        .into_iter()
        .map(|drive| AppDrive {
            name: drive.name.clone(),
            subtitle: drive.subtitle,
            icon: if drive.online {
                "/com/icecommander/gtk/connect.svg".to_string()
            } else {
                "/com/icecommander/gtk/disconnect.svg".to_string()
            },
            is_favorite: favorites.contains(&drive.key),
            is_online: drive.online,
            svg: drive.svg.clone(),
            key: drive.key.clone(),
            item: AppDriveItem::Offered {
                kind: drive.kind,
                settings: drive.settings,
                shown: fm_core::plugin_fs::Shown {
                    name: drive.name.clone(),
                    icon_svg: String::from_utf8(drive.svg.clone()).ok(),
                    key: drive.key,
                },
            },
        })
        .collect()
}

pub fn activate_drive_item(item: &AppDriveItem, router: &Rc<PanelRouter>) -> DriveActivation {
    match item {
        AppDriveItem::RootFs => {
            router.open_local_path("/".to_string());
            DriveActivation::Shown
        }
        AppDriveItem::UserHome => {
            let home_path = dirs::home_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            router.open_local_path(home_path);
            DriveActivation::Shown
        }
        AppDriveItem::LocalDrive(path) => {
            router.open_local_path(path.clone());
            DriveActivation::Shown
        }
        AppDriveItem::Volume(vol) => DriveActivation::NeedsAsyncMount(vol.clone()),
        AppDriveItem::Offered {
            kind,
            settings,
            shown,
        } => {
            if let Some(served) =
                ic_plugin_host::mount_connection_shown(kind, settings, Some(shown.clone()))
            {
                let at = connection_form::opening_path_in(kind, settings)
                    .unwrap_or_else(|| "/".to_string());
                router.mount_provider(served, kind, at);
            }
            DriveActivation::Shown
        }
        AppDriveItem::NetConnection(conn) => {
            let conn = &crate::secret_store::opened(conn);
            let at = connection_form::opening_path(conn).unwrap_or_else(|| "/".to_string());
            if let Some(served) = crate::connection_manager::mount_through_plugin(conn) {
                router.mount_provider(served, &conn.kind, at);
            }
            DriveActivation::Shown
        }
    }
}
