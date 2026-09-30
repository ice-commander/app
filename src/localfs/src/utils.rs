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

/// What gio answers for the desktop build, worked out from `/proc/mounts` for the frontends
/// with no toolkit to ask. The two lists have to agree, or a machine looks different in each.
const NOT_A_PLACE_FOR_FILES: &[&str] = &[
    "/", "/boot", "/proc", "/sys", "/dev", "/run", "/tmp", "/var", "/usr", "/etc", "/snap",
];

fn shown_by_the_toolkit(device: &str, mount_point: &str) -> bool {
    if !device.starts_with("/dev/") || device.starts_with("/dev/loop") {
        return false;
    }
    !NOT_A_PLACE_FOR_FILES
        .iter()
        .any(|held| mount_point == *held || mount_point.starts_with(&format!("{held}/")))
}

/// Label by device, as the partitions on this machine are named.
pub fn partition_labels() -> std::collections::BTreeMap<String, String> {
    let mut named = std::collections::BTreeMap::new();
    let Ok(entries) = std::fs::read_dir("/dev/disk/by-label") else {
        return named;
    };
    for entry in entries.flatten() {
        if let Ok(device) = std::fs::canonicalize(entry.path()) {
            named.insert(
                device.to_string_lossy().into_owned(),
                entry.file_name().to_string_lossy().replace("\\x20", " "),
            );
        }
    }
    named
}

pub fn mounted_partitions(
    proc_mounts: &str,
    labels: &std::collections::BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for line in proc_mounts.lines() {
        let mut parts = line.split_whitespace();
        let (Some(device), Some(mount_point)) = (parts.next(), parts.next()) else {
            continue;
        };
        if !shown_by_the_toolkit(device, mount_point) {
            continue;
        }
        let mount_point = mount_point.replace("\\040", " ");
        if found.iter().any(|(held, _)| *held == mount_point) {
            continue;
        }
        let name = labels.get(device).cloned().unwrap_or_else(|| {
            std::path::Path::new(&mount_point)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| mount_point.clone())
        });
        found.push((mount_point, name));
    }
    found
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
    #[cfg(all(
        not(target_os = "windows"),
        not(target_os = "macos"),
        not(feature = "gtk")
    ))]
    {
        if let Ok(contents) = std::fs::read_to_string("/proc/mounts") {
            for (path, name) in mounted_partitions(&contents, &partition_labels()) {
                drives.push(DriveInfo {
                    path,
                    name,
                    is_mounted: true,
                    can_eject: false,
                });
            }
        }
    }
    drives
}

pub fn log_debug(_msg: &str) {}

#[cfg(test)]
mod tests {
    use super::{mounted_partitions, partition_labels};
    use std::collections::BTreeMap;

    const MOUNTS: &str = "sysfs /sys sysfs rw 0 0\n\
/dev/nvme1n1p2 / ext4 rw 0 0\n\
/dev/nvme1n1p1 /boot/efi vfat rw 0 0\n\
/dev/nvme1n1p12 /development ext4 rw 0 0\n\
/dev/nvme1n1p11 /storage ext4 rw 0 0\n\
/dev/loop3 /snap/core squashfs ro 0 0\n\
/dev/sdb1 /media/usb\\040stick vfat rw 0 0\n\
proc /proc proc rw 0 0\n\
/dev/nvme1n1p11 /storage ext4 rw 0 0\n";

    fn labels() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("/dev/nvme1n1p12".to_string(), "Development".to_string()),
            ("/dev/nvme1n1p11".to_string(), "Storage".to_string()),
            ("/dev/nvme1n1p1".to_string(), "EFI".to_string()),
        ])
    }

    /// What gio answers on the owner's machine: by label, and not the root or the EFI partition.
    #[test]
    fn the_same_partitions_the_toolkit_offers_and_no_others() {
        let found = mounted_partitions(MOUNTS, &labels());
        assert_eq!(
            found,
            [
                ("/development".to_string(), "Development".to_string()),
                ("/storage".to_string(), "Storage".to_string()),
                ("/media/usb stick".to_string(), "usb stick".to_string()),
            ]
        );
    }

    #[test]
    fn a_partition_with_no_label_is_named_after_where_it_is_mounted() {
        let found = mounted_partitions(MOUNTS, &BTreeMap::new());
        let names: Vec<&str> = found.iter().map(|(_, name)| name.as_str()).collect();
        assert_eq!(names, ["development", "storage", "usb stick"]);
    }

    #[test]
    fn labels_are_read_from_the_machine_without_falling_over() {
        let _ = partition_labels();
    }
}
