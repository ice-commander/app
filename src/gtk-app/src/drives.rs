use crate::connection_manager::FtpConnection;
use virtualfs::utils::{get_drives, DriveInfo};
use panel_router::PanelRouter;
use gtk::prelude::*;
use std::rc::Rc;

#[derive(Clone)]
pub enum AppDriveItem {
    RootFs,
    UserHome,
    LocalDrive(String),
    Volume(gtk::gio::Volume),
    NetConnection(FtpConnection),
}

#[derive(Clone)]
pub struct AppDrive {
    pub item: AppDriveItem,
    pub name: String,
    pub subtitle: String,
    pub icon: String,      // e.g., "/com/icecommander/gtk/ssd.svg"
    pub key: String,       // e.g., "local_fs:/", "ftp://..."
    pub is_favorite: bool,
    pub is_online: bool,
    #[allow(dead_code)]
    pub drive_info: Option<DriveInfo>,
}

pub enum DriveActivation {
    Shown,
    NeedsAsyncMount(gtk::gio::Volume),
}

impl AppDrive {
    #[allow(dead_code)] // distinct from the `DriveInfo::is_mounted` field the other crates read
    pub fn is_mounted(&self) -> bool {
        match &self.item {
            AppDriveItem::RootFs | AppDriveItem::UserHome | AppDriveItem::LocalDrive(_) => true,
            AppDriveItem::Volume(vol) => vol.get_mount().is_some(),
            AppDriveItem::NetConnection(_) => false,
        }
    }
}

pub fn get_all_app_drives(config: &client_config::AppConfig) -> Vec<AppDrive> {
    let mut drives = Vec::new();
    let favorites = config.get::<Vec<String>>("ui.favorites").unwrap_or_default();

    let root_key = "local_fs:/".to_string();
    drives.push(AppDrive {
        item: AppDriveItem::RootFs,
        name: crate::i18n::tr("drives.system_root").to_string(),
        subtitle: "/".to_string(),
        icon: "/com/icecommander/gtk/home.svg".to_string(),
        key: root_key.clone(),
        is_favorite: favorites.contains(&root_key),
        is_online: true,
        drive_info: None,
    });

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
        drive_info: None,
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
                key,
                drive_info: Some(drive),
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
                key,
                drive_info: Some(DriveInfo {
                    path: name.clone(),
                    name,
                    is_mounted: false,
                    can_eject: volume.can_eject(),
                    volume: Some(volume),
                    mount: None,
                }),
            });
        }
    }

    let all_conns: Vec<FtpConnection> = config.get("ui.ftp_connections").unwrap_or_default();
    for conn in &all_conns {
        let is_webdav = conn.protocol.to_uppercase() == "WEBDAV";
        let key = if is_webdav {
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
        let icon = if is_webdav {
            "/com/icecommander/gtk/netdrive.svg".to_string()
        } else {
            "/com/icecommander/gtk/ftp.svg".to_string()
        };
        let subtitle = format!("{}://{}", conn.protocol.to_lowercase(), conn.host);

        drives.push(AppDrive {
            item: AppDriveItem::NetConnection(conn.clone()),
            name: conn.name.clone(),
            subtitle,
            icon,
            is_favorite: favorites.contains(&key),
            is_online: true,
            key,
            drive_info: None,
        });
    }

    drives
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
        AppDriveItem::NetConnection(conn) => {
            let conn = &crate::secret_store::opened(conn);
            let rpath = conn.remote_path.clone().unwrap_or_else(|| "/".to_string());

            match conn.protocol.to_uppercase().as_str() {
                "FTP" => {
                    let ftp_rpc = Rc::new(virtualfs::ftp_rpc::LocalFtpRpc {
                        name: conn.name.clone(),
                        host: conn.host.clone(),
                        port: conn.port,
                        user: conn.user.clone(),
                        pass: conn.pass.clone().unwrap_or_default(),
                        ftp_session: std::sync::Arc::new(std::sync::Mutex::new(None)),
                    });
                    router.mount_provider(ftp_rpc, "ftp", rpath);
                }
                "WEBDAV" => {
                    let webdav_rpc = Rc::new(virtualfs::webdav_rpc::LocalWebDavRpc {
                        name: conn.name.clone(),
                        url: conn.host.clone(),
                        user: if conn.user.is_empty() {
                            None
                        } else {
                            Some(conn.user.clone())
                        },
                        pass: conn.pass.clone(),
                        remote_path: conn.remote_path.clone(),
                    });
                    router.mount_provider(webdav_rpc, "webdav", rpath);
                }
                _ => {
                    let sftp_rpc = Rc::new(virtualfs::sftp_rpc::LocalSftpRpc {
                        name: conn.name.clone(),
                        host: conn.host.clone(),
                        port: conn.port,
                        user: conn.user.clone(),
                        pass: conn.pass.clone(),
                        auth_type: conn
                            .auth_type
                            .clone()
                            .unwrap_or_else(|| "password".to_string()),
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
                    router.mount_provider(sftp_rpc, "sftp", rpath);
                }
            }
            DriveActivation::Shown
        }
    }
}
