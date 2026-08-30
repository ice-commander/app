#[cfg(target_os = "windows")]
extern "system" {
    fn GetLogicalDrives() -> u32;
}

#[cfg(feature = "gtk")]
use gtk::gio::prelude::*;

#[derive(Clone)]
pub struct DriveInfo {
    pub path: String,
    pub name: String,
    pub is_mounted: bool,
    pub can_eject: bool,
    #[cfg(feature = "gtk")]
    pub volume: Option<gtk::gio::Volume>,
    #[cfg(feature = "gtk")]
    pub mount: Option<gtk::gio::Mount>,
}

pub fn get_drives() -> Vec<DriveInfo> {
    let mut drives = Vec::new();
    #[cfg(target_os = "windows")]
    {
        let mask = unsafe { GetLogicalDrives() };
        for b in 0..26 {
            if (mask & (1 << b)) != 0 {
                let drive_letter = (b'A' + b) as char;
                let drive_path = format!("{}:\\", drive_letter);
                drives.push(DriveInfo {
                    path: drive_path.clone(),
                    name: format!("Disk {}:", drive_letter),
                    is_mounted: true,
                    can_eject: false,
                    #[cfg(feature = "gtk")]
                    volume: None,
                    #[cfg(feature = "gtk")]
                    mount: None,
                });
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = std::fs::read_dir("/Volumes") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(resolved) = std::fs::canonicalize(&path) {
                    if resolved == std::path::Path::new("/") {
                        continue;
                    }
                }
                let path_str = path.to_string_lossy().to_string();
                let name = entry.file_name().to_string_lossy().to_string();
                drives.push(DriveInfo {
                    path: path_str,
                    name,
                    is_mounted: true,
                    can_eject: true,
                    #[cfg(feature = "gtk")]
                    volume: None,
                    #[cfg(feature = "gtk")]
                    mount: None,
                });
            }
        }
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos"), feature = "gtk"))]
    {
        let monitor = gtk::gio::VolumeMonitor::get();
        let mut seen_paths = std::collections::HashSet::new();

        for mount in monitor.mounts() {
            if let Some(path) = mount.root().path() {
                let path_str = path.to_string_lossy().to_string();
                seen_paths.insert(path_str.clone());

                let vol = mount.volume();
                let can_eject =
                    mount.can_unmount() || vol.as_ref().map_or(false, |v| v.can_eject());

                drives.push(DriveInfo {
                    path: path_str,
                    name: mount.name().to_string(),
                    is_mounted: true,
                    can_eject,
                    volume: vol,
                    mount: Some(mount),
                });
            }
        }

        for volume in monitor.volumes() {
            if volume.can_mount() && volume.get_mount().is_none() {
                let path_str = volume.name().to_string();
                drives.push(DriveInfo {
                    path: path_str.clone(),
                    name: path_str,
                    is_mounted: false,
                    can_eject: volume.can_eject(),
                    volume: Some(volume.clone()),
                    mount: None,
                });
            }
        }
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos"), not(feature = "gtk")))]
    {
        if let Ok(contents) = std::fs::read_to_string("/proc/mounts") {
            for line in contents.lines() {
                let mut parts = line.split_whitespace();
                let _dev = parts.next();
                if let Some(mount_point) = parts.next() {
                    if mount_point.starts_with("/media/")
                        || mount_point.starts_with("/mnt/")
                        || mount_point.starts_with("/run/media/")
                    {
                        let mount_point = mount_point.replace("\\040", " ");
                        let name = std::path::Path::new(&mount_point)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| mount_point.clone());
                        drives.push(DriveInfo {
                            path: mount_point,
                            name,
                            is_mounted: true,
                            can_eject: false,
                        });
                    }
                }
            }
        }
    }
    drives
}

pub fn log_debug(_msg: &str) {}
