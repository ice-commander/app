use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where the list of plugins for this release comes from. A local path or a
/// `file://` URL is read directly; an `http(s)://` one needs a transport this
/// crate deliberately does not carry, so it is reported rather than fetched.
pub const SOURCE_KEY: &str = "plugins.catalog";
pub const SOURCE_OVERRIDE: &str = "IC_PLUGIN_CATALOG";

/// One plugin a catalogue offers, whether or not it is installed here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    /// Matches `About::id` so an installed plugin can be recognised.
    #[serde(default)]
    pub id: String,
    /// The library's file name without prefix or extension, e.g. `ic_sftp_fs`.
    /// An entry without one is dropped: it names nothing that can be installed.
    #[serde(default)]
    pub library: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Where the library itself would be fetched from.
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// No catalogue is configured for this build.
    NotConfigured,
    /// The address needs a network transport the application does not have.
    NoTransport(String),
    Unreadable(String),
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::NotConfigured => write!(f, "no plugin catalogue is configured"),
            CatalogError::NoTransport(what) => {
                write!(f, "{what} needs a downloader this build does not have")
            }
            CatalogError::Unreadable(why) => write!(f, "{why}"),
        }
    }
}

fn local_path(address: &str) -> Option<PathBuf> {
    if address.is_empty() {
        return None;
    }
    if let Some(rest) = address.strip_prefix("file://") {
        return Some(PathBuf::from(rest));
    }
    if address.starts_with("http://") || address.starts_with("https://") {
        return None;
    }
    Some(PathBuf::from(address))
}

pub fn source(config: &client_config::AppConfig) -> String {
    if let Some(chosen) = std::env::var_os(SOURCE_OVERRIDE) {
        let held = chosen.to_string_lossy().into_owned();
        if !held.is_empty() {
            return held;
        }
    }
    config.get::<String>(SOURCE_KEY).unwrap_or_default()
}

/// Reads the catalogue the source points at.
pub fn fetch(config: &client_config::AppConfig) -> Result<Vec<Offer>, CatalogError> {
    let address = source(config);
    if address.is_empty() {
        return Err(CatalogError::NotConfigured);
    }
    let Some(path) = local_path(&address) else {
        return Err(CatalogError::NoTransport(address));
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| CatalogError::Unreadable(format!("{}: {e}", path.display())))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Vec<Offer>, CatalogError> {
    #[derive(Deserialize)]
    struct Catalogue {
        #[serde(default)]
        plugins: Vec<Offer>,
    }
    let parsed: Catalogue =
        serde_json::from_str(text).map_err(|e| CatalogError::Unreadable(e.to_string()))?;
    Ok(parsed
        .plugins
        .into_iter()
        .filter(|offer| !offer.library.is_empty())
        .collect())
}

/// Puts an offered plugin into the plugin folder. Reachable only for a source
/// that is already on this machine; a download is not implemented.
pub fn install(offer: &Offer) -> Result<PathBuf, CatalogError> {
    let Some(from) = local_path(&offer.url) else {
        return Err(CatalogError::NoTransport(offer.url.clone()));
    };
    let Some(folder) = crate::loader::plugin_directory() else {
        return Err(CatalogError::Unreadable("no plugin folder".to_string()));
    };
    std::fs::create_dir_all(&folder)
        .map_err(|e| CatalogError::Unreadable(format!("{}: {e}", folder.display())))?;
    let into = folder.join(file_name_for(&offer.library));
    std::fs::copy(&from, &into).map_err(|e| {
        CatalogError::Unreadable(format!("{} -> {}: {e}", from.display(), into.display()))
    })?;
    Ok(into)
}

/// What this platform's linker would have called the library.
pub fn file_name_for(library: &str) -> String {
    let extension = crate::loader::library_extension();
    if cfg!(target_os = "windows") {
        format!("{library}.{extension}")
    } else {
        format!("lib{library}.{extension}")
    }
}

/// Is this offer newer than what is installed? Compares dotted numbers, and
/// falls back to "different means newer" for anything it cannot parse.
pub fn is_newer(offered: &str, installed: &str) -> bool {
    let parts = |text: &str| -> Option<Vec<u64>> {
        text.split('.')
            .map(|part| {
                part.trim_matches(|c: char| !c.is_ascii_digit())
                    .parse::<u64>()
                    .ok()
            })
            .collect()
    };
    match (parts(offered), parts(installed)) {
        (Some(left), Some(right)) => {
            let width = left.len().max(right.len());
            for at in 0..width {
                let a = left.get(at).copied().unwrap_or(0);
                let b = right.get(at).copied().unwrap_or(0);
                if a != b {
                    return a > b;
                }
            }
            false
        }
        _ => !offered.is_empty() && offered != installed,
    }
}

pub fn is_installed(offer: &Offer, present: &[PathBuf]) -> bool {
    present
        .iter()
        .any(|path| crate::loader::name_of_library(path) == offer.library)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_compared_number_by_number() {
        assert!(is_newer("0.8.2", "0.8.1"));
        assert!(is_newer("0.9.0", "0.8.12"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.8.1", "0.8.1"));
        assert!(!is_newer("0.8.1", "0.8.2"));
        assert!(!is_newer("0.8", "0.8.0"), "a missing part counts as zero");
        assert!(is_newer("0.8.1", ""), "anything beats an unknown version");
    }

    #[test]
    fn an_unparsable_version_is_newer_only_when_it_differs() {
        assert!(is_newer("nightly", "0.8.1"));
        assert!(!is_newer("nightly", "nightly"));
        assert!(!is_newer("", "0.8.1"));
    }

    #[test]
    fn a_catalogue_without_a_source_is_reported_not_guessed() {
        let config = client_config::AppConfig::new("ice-commander-catalog-empty");
        config.set(SOURCE_KEY, "");
        assert_eq!(fetch(&config), Err(CatalogError::NotConfigured));
    }

    #[test]
    fn a_web_address_says_plainly_that_it_cannot_be_fetched() {
        let config = client_config::AppConfig::new("ice-commander-catalog-web");
        config.set(SOURCE_KEY, "https://example.org/plugins.json");
        assert_eq!(
            fetch(&config),
            Err(CatalogError::NoTransport(
                "https://example.org/plugins.json".to_string()
            ))
        );
    }

    #[test]
    fn a_catalogue_on_disk_is_read_and_entries_without_a_library_are_dropped() {
        let folder = std::env::temp_dir().join("ic-catalog-probe");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a scratch folder");
        let listing = folder.join("plugins.json");
        std::fs::write(
            &listing,
            serde_json::json!({
                "plugins": [
                    { "id": "ic-node-in-net", "library": "ic_node_in_net",
                      "name": "Node In Net", "version": "0.8.0", "url": "/tmp/x.so" },
                    { "id": "broken", "name": "no library named" },
                ]
            })
            .to_string(),
        )
        .expect("a catalogue");

        let config = client_config::AppConfig::new("ice-commander-catalog-file");
        config.set(SOURCE_KEY, listing.to_string_lossy().into_owned());
        let offered = fetch(&config).expect("reads");
        assert_eq!(
            offered.len(),
            1,
            "an entry with no library cannot be installed"
        );
        assert_eq!(offered[0].library, "ic_node_in_net");
        assert_eq!(offered[0].version, "0.8.0");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn what_is_already_in_the_folder_is_recognised_by_its_library_name() {
        let present = vec![
            PathBuf::from("/plugins/libic_sftp_fs.so"),
            PathBuf::from("/plugins/libic_ftp_fs.so"),
        ];
        let mine = Offer {
            library: "ic_sftp_fs".to_string(),
            ..Offer::default()
        };
        let other = Offer {
            library: "ic_node_in_net".to_string(),
            ..Offer::default()
        };
        assert!(is_installed(&mine, &present) || cfg!(target_os = "windows"));
        assert!(!is_installed(&other, &present));
    }

    #[test]
    fn the_file_name_follows_what_this_platform_links() {
        let named = file_name_for("ic_sftp_fs");
        if cfg!(target_os = "windows") {
            assert_eq!(named, "ic_sftp_fs.dll");
        } else if cfg!(target_os = "macos") {
            assert_eq!(named, "libic_sftp_fs.dylib");
        } else {
            assert_eq!(named, "libic_sftp_fs.so");
        }
    }

    #[test]
    fn installing_from_a_web_address_is_refused_rather_than_half_done() {
        let offer = Offer {
            library: "ic_x".to_string(),
            url: "https://example.org/libic_x.so".to_string(),
            ..Offer::default()
        };
        assert!(matches!(install(&offer), Err(CatalogError::NoTransport(_))));
    }
}
