//! Plugin updates, the part that is files: which of the files the site offers differ from what is installed, a
//! verified download waiting beside the file it replaces as `<path>.new`, and putting it in place at the next start,
//! before anything is loaded. The network is the frontend's.

use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Downloads are taken from these releases only, whatever a listing says.
pub const TRUSTED: &str = "https://github.com/ice-commander/";

const WAITING: &str = "new";
const PARTIAL: &str = "part";

/// A file of a plugin as the site lists it: where it goes inside the plugin folder, what it must be, where it is.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Offered {
    pub path: String,
    pub md5: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OfferedPlugin {
    pub version: String,
    pub files: Vec<Offered>,
}

/// One file to fetch, and the plugin it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetch {
    pub plugin: String,
    pub file: Offered,
}

/// The system as the releases name it, after `uname -m`: `macos-arm64`, `linux-x86_64`, `windows-x86_64`.
pub fn platform() -> String {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" if cfg!(target_os = "macos") => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

fn order(a: &str, b: &str) -> Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        let ordered = a
            .get(i)
            .copied()
            .unwrap_or(0)
            .cmp(&b.get(i).copied().unwrap_or(0));
        if ordered != Ordering::Equal {
            return ordered;
        }
    }
    Ordering::Equal
}

/// A path the listing may name: inside the plugin folder and nowhere else.
fn inside(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut named = path.as_os_str().to_owned();
    named.push(".");
    named.push(suffix);
    PathBuf::from(named)
}

pub fn md5_of(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut context = md5::Context::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        context.consume(&buffer[..read]);
    }
    Some(format!("{:x}", context.finalize()))
}

/// What to fetch for the plugins installed in `folder`: each file whose bytes differ from the offer, unless the
/// installed plugin says it is newer than the offer — a developer's own build, left alone together with what it
/// carries. A plugin that is not installed is not installed from here. `installed` is each library's name and the
/// version it reports, none when it could not be asked.
pub fn plan(
    folder: &Path,
    offered: &BTreeMap<String, OfferedPlugin>,
    installed: &[(String, Option<String>)],
) -> Vec<Fetch> {
    let mut fetch = Vec::new();
    for (plugin, offer) in offered {
        let Some((_, version)) = installed.iter().find(|(name, _)| name == plugin) else {
            continue;
        };
        if version
            .as_deref()
            .is_some_and(|version| order(version, &offer.version) == Ordering::Greater)
        {
            continue;
        }
        for file in &offer.files {
            if !inside(&file.path) || !file.url.starts_with(TRUSTED) {
                continue;
            }
            let at = folder.join(&file.path);
            let matches =
                |path: &Path| md5_of(path).is_some_and(|held| held.eq_ignore_ascii_case(&file.md5));
            if !matches(&at) && !matches(&with_suffix(&at, WAITING)) {
                fetch.push(Fetch {
                    plugin: plugin.clone(),
                    file: file.clone(),
                });
            }
        }
    }
    fetch
}

/// Puts a download beside the file it replaces as `<path>.new`, once its size and md5 are what the listing said.
pub fn stage(folder: &Path, file: &Offered, bytes: &[u8]) -> Result<(), String> {
    if !inside(&file.path) {
        return Err(format!(
            "{:?} is not a path inside the plugin folder",
            file.path
        ));
    }
    if bytes.len() as u64 != file.size {
        return Err(format!(
            "{}: {} bytes arrived, {} were listed",
            file.path,
            bytes.len(),
            file.size
        ));
    }
    let arrived = format!("{:x}", md5::compute(bytes));
    if !arrived.eq_ignore_ascii_case(&file.md5) {
        return Err(format!(
            "{}: md5 {arrived}, {} was listed",
            file.path, file.md5
        ));
    }
    let at = folder.join(&file.path);
    if let Some(parent) = at.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let waiting = with_suffix(&at, WAITING);
    let partial = with_suffix(&waiting, PARTIAL);
    std::fs::write(&partial, bytes).map_err(|e| format!("{}: {e}", partial.display()))?;
    std::fs::rename(&partial, &waiting).map_err(|e| format!("{}: {e}", waiting.display()))
}

/// Every `<path>.new` under `folder` put in place: the old file removed, the new one renamed. Runs before any plugin
/// is loaded, the same on every system. One whose file cannot be removed — on Windows a library another running
/// instance holds — waits for the next start. A download cut off half way is thrown away. Returns what was replaced.
pub fn apply_pending(folder: &Path) -> Vec<PathBuf> {
    let mut applied = Vec::new();
    let mut folders = vec![(folder.to_path_buf(), 0)];
    while let Some((at, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else {
            continue;
        };
        for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
            if path.is_dir() {
                if depth < 4 {
                    folders.push((path, depth + 1));
                }
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.ends_with(&format!(".{WAITING}.{PARTIAL}")) {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let Some(target) = name.strip_suffix(&format!(".{WAITING}")) else {
                continue;
            };
            let target = at.join(target);
            if target.exists() {
                if let Err(e) = remove_when_free(&target) {
                    ic_logging::warn!("plugin update for {} waits: {e}", target.display());
                    continue;
                }
            }
            match std::fs::rename(&path, &target) {
                Ok(()) => {
                    ic_logging::info!("plugin update put in place: {}", target.display());
                    applied.push(target);
                }
                Err(e) => ic_logging::warn!("plugin update for {} waits: {e}", target.display()),
            }
        }
    }
    applied
}

/// A restart starts the new instance before the old one has gone, and on Windows the old one still holds the library
/// for a moment: it is given that moment before the update is put off to the next start.
fn remove_when_free(path: &Path) -> std::io::Result<()> {
    let mut tries = 20;
    loop {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(e) if tries == 0 || e.kind() == std::io::ErrorKind::NotFound => return Err(e),
            Err(_) => {
                tries -= 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let at =
            std::env::temp_dir().join(format!("ic-plugin-updates-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(&at).unwrap();
        at
    }

    fn offered(path: &str, bytes: &[u8]) -> Offered {
        Offered {
            path: path.to_string(),
            md5: format!("{:x}", md5::compute(bytes)),
            size: bytes.len() as u64,
            url: format!(
                "{TRUSTED}ic-plugin-video/releases/download/v0.2.0/{}",
                path.rsplit('/').next().unwrap()
            ),
        }
    }

    fn video(version: &str, library: &[u8], codecs: &[u8]) -> BTreeMap<String, OfferedPlugin> {
        BTreeMap::from([(
            "ic_video_view".to_string(),
            OfferedPlugin {
                version: version.to_string(),
                files: vec![
                    offered("libic_video_view.dylib", library),
                    offered("video-codecs/libmpv.2.dylib", codecs),
                ],
            },
        )])
    }

    #[test]
    fn only_what_differs_is_fetched_and_a_newer_installed_plugin_is_left_alone() {
        let at = folder("plan");
        std::fs::create_dir_all(at.join("video-codecs")).unwrap();
        std::fs::write(at.join("libic_video_view.dylib"), b"old library").unwrap();
        std::fs::write(at.join("video-codecs/libmpv.2.dylib"), b"same codecs").unwrap();
        let offer = video("0.2.0", b"new library", b"same codecs");

        let fetch = plan(
            &at,
            &offer,
            &[("ic_video_view".to_string(), Some("0.1.0".to_string()))],
        );
        assert_eq!(fetch.len(), 1);
        assert_eq!(fetch[0].file.path, "libic_video_view.dylib");

        let developer = plan(
            &at,
            &offer,
            &[("ic_video_view".to_string(), Some("0.3.0".to_string()))],
        );
        assert!(developer.is_empty());

        let unknown = plan(&at, &offer, &[("ic_video_view".to_string(), None)]);
        assert_eq!(
            unknown.len(),
            1,
            "a library that could not be asked is judged by its bytes"
        );

        assert!(
            plan(&at, &offer, &[]).is_empty(),
            "nothing is installed from here"
        );
        let _ = std::fs::remove_dir_all(&at);
    }

    #[test]
    fn a_staged_download_is_put_in_place_at_the_next_start_and_not_fetched_again() {
        let at = folder("stage");
        std::fs::write(at.join("libic_video_view.dylib"), b"old library").unwrap();
        let offer = video("0.2.0", b"new library", b"new codecs");
        let installed = [("ic_video_view".to_string(), Some("0.1.0".to_string()))];

        for fetch in plan(&at, &offer, &installed) {
            let bytes: &[u8] = if fetch.file.path.contains('/') {
                b"new codecs"
            } else {
                b"new library"
            };
            stage(&at, &fetch.file, bytes).unwrap();
        }
        assert!(
            plan(&at, &offer, &installed).is_empty(),
            "what waits is not fetched twice"
        );
        assert_eq!(
            std::fs::read(at.join("libic_video_view.dylib")).unwrap(),
            b"old library"
        );

        let applied = apply_pending(&at);
        assert_eq!(applied.len(), 2);
        assert_eq!(
            std::fs::read(at.join("libic_video_view.dylib")).unwrap(),
            b"new library"
        );
        assert_eq!(
            std::fs::read(at.join("video-codecs/libmpv.2.dylib")).unwrap(),
            b"new codecs"
        );
        assert!(!at.join("libic_video_view.dylib.new").exists());
        let _ = std::fs::remove_dir_all(&at);
    }

    #[test]
    fn a_download_that_is_not_what_was_listed_is_refused() {
        let at = folder("refuse");
        let file = offered("libic_video_view.dylib", b"right bytes");
        assert!(stage(&at, &file, b"wrong bytes").is_err());
        assert!(stage(&at, &file, b"short").is_err());
        let outside = Offered {
            path: "../escape.dylib".to_string(),
            ..file.clone()
        };
        assert!(stage(&at, &outside, b"right bytes").is_err());
        assert!(
            std::fs::read_dir(&at).unwrap().next().is_none(),
            "nothing was written"
        );
        let _ = std::fs::remove_dir_all(&at);
    }

    #[test]
    fn a_listing_cannot_send_files_outside_the_folder_or_from_elsewhere() {
        let at = folder("listing");
        std::fs::write(at.join("libic_video_view.dylib"), b"old").unwrap();
        let mut offer = video("0.2.0", b"new", b"codecs");
        let files = &mut offer.get_mut("ic_video_view").unwrap().files;
        files[0].url = "https://example.com/libic_video_view.dylib".to_string();
        files[1].path = "../../evil.dylib".to_string();
        assert!(plan(&at, &offer, &[("ic_video_view".to_string(), None)]).is_empty());
        let _ = std::fs::remove_dir_all(&at);
    }

    #[test]
    fn a_download_cut_off_half_way_is_thrown_away() {
        let at = folder("partial");
        std::fs::write(at.join("libic_video_view.dylib.new.part"), b"half").unwrap();
        assert!(apply_pending(&at).is_empty());
        assert!(!at.join("libic_video_view.dylib.new.part").exists());
        let _ = std::fs::remove_dir_all(&at);
    }

    #[test]
    fn the_platform_is_named_as_the_releases_name_it() {
        let named = platform();
        assert!(
            [
                "macos-arm64",
                "macos-x86_64",
                "linux-x86_64",
                "linux-aarch64",
                "windows-x86_64"
            ]
            .contains(&named.as_str()),
            "{named}"
        );
    }
}
