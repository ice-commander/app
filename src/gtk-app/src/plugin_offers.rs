//! The plugins in the folder together with the ones the site offers for this version: what each is, and whether it
//! can be installed or updated. The settings page and the setup wizard both list them.

use ic_plugin_host::loader;
use ic_plugin_host::updates::{self, OfferedPlugin, Standing};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct Listed {
    /// The name the plugin is enabled under: `ic_sftp_fs`.
    pub library: String,
    /// The installed library; none for a plugin only the site has.
    pub path: Option<PathBuf>,
    pub name: String,
    pub description: String,
    /// The installed version, or the offered one for a plugin not installed.
    pub version: String,
    pub offered: Option<OfferedPlugin>,
    /// `Current` for a plugin the site does not offer.
    pub standing: Standing,
    /// What installing or updating it downloads.
    pub size: u64,
}

fn or(text: &str, fallback: &str) -> String {
    if text.is_empty() {
        fallback.to_string()
    } else {
        text.to_string()
    }
}

/// Opens every installed library and reads every offered file's checksum: off the main thread when there are offers.
pub(crate) fn listing(offers: &BTreeMap<String, OfferedPlugin>) -> Vec<Listed> {
    let folder = loader::plugin_directory();
    let mut listed: Vec<Listed> = loader::installed_with_about()
        .into_iter()
        .map(|held| {
            let about = held.about.unwrap_or_default();
            let offered = offers.get(&held.library).cloned();
            let (standing, size) = match (&offered, &folder) {
                (Some(offer), Some(folder)) => {
                    let version = (!about.version.is_empty()).then_some(about.version.as_str());
                    let standing = updates::standing(folder, offer, Some(version));
                    (standing, fetched_size(folder, offer, standing))
                }
                _ => (Standing::Current, 0),
            };
            let (offered_name, offered_description) = offered
                .as_ref()
                .map(|offer| (offer.name.as_str(), offer.description.as_str()))
                .unwrap_or_default();
            Listed {
                name: or(&about.name, &or(offered_name, &held.library)),
                description: or(&about.description, offered_description),
                version: about.version,
                library: held.library,
                path: Some(held.path),
                offered,
                standing,
                size,
            }
        })
        .collect();
    for (library, offer) in offers {
        if listed.iter().any(|held| &held.library == library) {
            continue;
        }
        let Some(folder) = &folder else {
            continue;
        };
        let standing = updates::standing(folder, offer, None);
        listed.push(Listed {
            library: library.clone(),
            path: None,
            name: or(&offer.name, library),
            description: offer.description.clone(),
            version: offer.version.clone(),
            size: fetched_size(folder, offer, standing),
            offered: Some(offer.clone()),
            standing,
        });
    }
    listed.sort_by_key(|held| held.name.to_lowercase());
    listed
}

fn fetched_size(folder: &std::path::Path, offer: &OfferedPlugin, standing: Standing) -> u64 {
    match standing {
        Standing::Absent | Standing::Behind => updates::missing(folder, offer)
            .iter()
            .map(|file| file.size)
            .sum(),
        Standing::Current | Standing::Waiting => 0,
    }
}

/// Asks the site, then lists: what is installed alone when the site cannot be reached, with why.
pub(crate) async fn with_offers() -> (Vec<Listed>, Option<String>) {
    let (offers, failed) = match crate::updater::offered_plugins().await {
        Ok(Ok(offers)) => (offers, None),
        Ok(Err(e)) => (BTreeMap::new(), Some(e)),
        Err(_) => (BTreeMap::new(), Some("cancelled".to_string())),
    };
    let listed = crate::updater::spawn_on_tokio(async move {
        tokio::task::spawn_blocking(move || listing(&offers)).await
    });
    match listed.await {
        Ok(Ok(listed)) => (listed, failed),
        _ => (listing(&BTreeMap::new()), failed),
    }
}

/// Downloads what a plugin lacks — all of it when it is not installed — for the next start.
pub(crate) async fn fetch(listed: &Listed) -> Result<(), String> {
    let offer = listed.offered.clone().ok_or("the site does not offer it")?;
    match crate::updater::fetch_plugin(offer).await {
        Ok(result) => result,
        Err(_) => Err("cancelled".to_string()),
    }
}

/// `Description  ·  0.1.0  ·  26.3 MB`, leaving out what is empty.
pub(crate) fn subtitle(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("  \u{b7}  ")
}

pub(crate) fn size_text(bytes: u64) -> String {
    if bytes == 0 {
        String::new()
    } else {
        gtk::glib::format_size(bytes).to_string()
    }
}
