use gtk::glib;
use gtk_updater_ui::{UpdaterInit, UpdaterInput, UpdaterModel, UpdaterOutput};
use ic_plugin_host::updates::{self, Fetch, Offered, OfferedPlugin};
use relm4::prelude::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

const DEFAULT_UPDATE_SERVER: &str = "https://www.icecommander.com";
const APP_NAME: &str = "Ice Commander";
const FILE_PREFIX: &str = "icecommander";

pub(crate) fn update_server() -> String {
    let raw = std::env::var("IC_UPDATE_SERVER").unwrap_or_default();
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        DEFAULT_UPDATE_SERVER.to_string()
    } else {
        raw.to_string()
    }
}

pub fn auto_check(parent: gtk::Window) {
    run_check(parent, None);
}

pub fn manual_check(parent: gtk::Window, on_complete: impl Fn() + 'static) {
    run_check(parent, Some(Box::new(on_complete)));
}

fn run_check(parent: gtk::Window, on_complete: Option<Box<dyn Fn()>>) {
    let (out_tx, out_rx) = relm4::channel::<UpdaterOutput>();
    let ctrl = Rc::new(
        UpdaterModel::builder()
            .launch(UpdaterInit {
                app_name: APP_NAME.to_string(),
                current_version: common::version::APP_VERSION.to_string(),
                parent,
            })
            .forward(&out_tx, |o| o),
    );

    let found_pkg: Rc<RefCell<Option<common::UpdatePackage>>> = Rc::new(RefCell::new(None));

    {
        let ctrl = ctrl.clone();
        let found_pkg = found_pkg.clone();
        glib::spawn_future_local(async move {
            let rx = spawn_on_tokio(perform_update_check());
            let manual = on_complete.is_some();
            let finish = |on_complete: &Option<Box<dyn Fn()>>| {
                if let Some(cb) = on_complete {
                    cb();
                }
            };
            match rx.await {
                Ok(Ok((Some(found), plugins))) => {
                    finish(&on_complete);
                    let version = found.pkg.version.clone();
                    let notes = found.notes;
                    *found_pkg.borrow_mut() = Some(found.pkg);
                    let _ = ctrl
                        .sender()
                        .send(UpdaterInput::UpdateAvailable { version, notes });
                    // Quietly: the restart after installing the application puts them in place, and one dialog is enough.
                    tokio::spawn(async move {
                        if let Err(e) = stage_plugin_updates(plugins).await {
                            eprintln!("[updater] plugin updates: {e}");
                        }
                    });
                }
                Ok(Ok((None, plugins))) => {
                    let staged = spawn_on_tokio(stage_plugin_updates(plugins)).await;
                    finish(&on_complete);
                    match staged {
                        Ok(Ok(names)) if !names.is_empty() => {
                            let _ = ctrl.sender().send(UpdaterInput::PluginsReady(names));
                        }
                        Ok(Ok(_)) => {
                            if manual {
                                let _ = ctrl.sender().send(UpdaterInput::UpToDate);
                            }
                        }
                        Ok(Err(e)) => {
                            if manual {
                                let _ = ctrl.sender().send(UpdaterInput::Failed(e));
                            } else {
                                eprintln!("[updater] plugin updates: {e}");
                            }
                        }
                        Err(_) => {}
                    }
                }
                Ok(Err(e)) => {
                    finish(&on_complete);
                    if manual {
                        let _ = ctrl.sender().send(UpdaterInput::Failed(e));
                    } else {
                        eprintln!("[updater] auto check failed: {e}");
                    }
                }
                Err(_) => {
                    finish(&on_complete);
                    if manual {
                        let _ = ctrl
                            .sender()
                            .send(UpdaterInput::Failed("Update check cancelled".to_string()));
                    }
                }
            }
        });
    }

    glib::spawn_future_local(async move {
        while let Some(out) = out_rx.recv().await {
            match out {
                UpdaterOutput::Declined => break,
                UpdaterOutput::Restart => ic_utils::app::restart_app(),
                UpdaterOutput::Install => {
                    let Some(pkg) = found_pkg.borrow().clone() else {
                        continue;
                    };
                    let _ = ctrl.sender().send(UpdaterInput::Downloading {
                        version: pkg.version.clone(),
                    });
                    let rx =
                        spawn_on_tokio(
                            async move { download_update_package(&pkg, FILE_PREFIX).await },
                        );
                    match rx.await {
                        Ok(Ok((path, _version, _md5))) => install_downloaded(&ctrl, path).await,
                        Ok(Err(e)) => {
                            let _ = ctrl.sender().send(UpdaterInput::Failed(e));
                        }
                        Err(_) => {
                            let _ = ctrl
                                .sender()
                                .send(UpdaterInput::Failed("Download cancelled".to_string()));
                        }
                    }
                }
            }
        }
    });
}

async fn install_downloaded(ctrl: &Rc<Controller<UpdaterModel>>, path: PathBuf) {
    #[cfg(target_os = "linux")]
    {
        match common::installer::spawn_linux_gui_update(&path) {
            Ok(mut child) => {
                let _ = ctrl.sender().send(UpdaterInput::Installing);
                let wait = spawn_on_tokio(async move {
                    tokio::task::spawn_blocking(move || child.wait())
                        .await
                        .unwrap_or_else(|e| Err(std::io::Error::other(e)))
                });
                match wait.await {
                    Ok(Ok(status)) if status.success() => {
                        let _ = ctrl.sender().send(UpdaterInput::Installed);
                    }
                    Ok(Ok(status)) => {
                        let _ = ctrl.sender().send(UpdaterInput::Failed(format!(
                            "Installer exit code: {:?}",
                            status.code()
                        )));
                    }
                    Ok(Err(e)) => {
                        let _ = ctrl.sender().send(UpdaterInput::Failed(format!(
                            "Failed to wait for installer: {e}"
                        )));
                    }
                    Err(_) => {
                        let _ = ctrl
                            .sender()
                            .send(UpdaterInput::Failed("Install task cancelled".to_string()));
                    }
                }
            }
            Err(e) => {
                let _ = ctrl.sender().send(UpdaterInput::Failed(e));
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        if let Err(e) = common::installer::install_update(&path, true) {
            let _ = ctrl.sender().send(UpdaterInput::Failed(e));
        }
    }
}

pub(crate) fn spawn_on_tokio<F, T>(future: F) -> tokio::sync::oneshot::Receiver<T>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = tx.send(future.await);
    });
    rx
}

struct AvailableUpdate {
    pkg: common::UpdatePackage,
    notes: String,
}

/// What `POST /api/checkUpdates` answers: the application's update, as the old check did, and the plugins that go
/// with this version, each with every file this system needs.
#[derive(serde::Deserialize, Default)]
struct Checked {
    #[serde(default)]
    update: bool,
    #[serde(default)]
    version: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    md5: String,
    #[serde(default)]
    plugins: BTreeMap<String, OfferedPlugin>,
}

async fn ask_site() -> Result<Checked, String> {
    let asked = serde_json::json!({
        "version": common::version::APP_VERSION,
        "app_type": common::version::APP_TYPE,
        "build_type": common::version::BUILD_TYPE,
        "platform": updates::platform(),
    });
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/api/checkUpdates?app_type={}&build_type={}&version={}",
            update_server(),
            common::version::APP_TYPE,
            common::version::BUILD_TYPE,
            common::version::APP_VERSION,
        ))
        .json(&asked)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    if status.is_success() {
        return resp.json::<Checked>().await.map_err(|e| e.to_string());
    }
    if status != reqwest::StatusCode::NOT_FOUND && status != reqwest::StatusCode::METHOD_NOT_ALLOWED
    {
        return Err(format!("HTTP status {status}"));
    }
    // A site from before the version registry knows the application alone.
    let url = format!(
        "{}/api/checkUpdate?app_type={}&build_type={}&version={}",
        update_server(),
        common::version::APP_TYPE,
        common::version::BUILD_TYPE,
        common::version::APP_VERSION,
    );
    let resp = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP status {}", resp.status()));
    }
    let old = resp
        .json::<common::UpdateCheck>()
        .await
        .map_err(|e| e.to_string())?;
    Ok(Checked {
        update: old.update,
        version: old.version,
        notes: old.notes,
        url: old.url,
        md5: old.md5,
        plugins: BTreeMap::new(),
    })
}

/// The plugins the site offers this version on this system, by the name each is enabled under.
pub(crate) fn offered_plugins(
) -> tokio::sync::oneshot::Receiver<Result<BTreeMap<String, OfferedPlugin>, String>> {
    spawn_on_tokio(async { Ok(ask_site().await?.plugins) })
}

/// Installs or updates one plugin: what differs of it — all of it when it is not there — is fetched and left as
/// `<path>.new` for the next start.
pub(crate) fn fetch_plugin(
    offer: OfferedPlugin,
) -> tokio::sync::oneshot::Receiver<Result<(), String>> {
    spawn_on_tokio(async move {
        let folder =
            ic_plugin_host::loader::plugin_directory().ok_or("no plugin folder on this system")?;
        let files = {
            let folder = folder.clone();
            tokio::task::spawn_blocking(move || updates::missing(&folder, &offer))
                .await
                .map_err(|e| e.to_string())?
        };
        download_and_stage(folder, files).await
    })
}

/// A plugin's files are written all or none: half of them would pair a new plugin with old codecs.
async fn download_and_stage(folder: PathBuf, files: Vec<Offered>) -> Result<(), String> {
    let mut arrived = Vec::new();
    for file in files {
        let bytes = fetch_bytes(&file.url).await?;
        arrived.push((file, bytes));
    }
    tokio::task::spawn_blocking(move || {
        arrived
            .iter()
            .try_for_each(|(file, bytes)| updates::stage(&folder, file, bytes))
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn perform_update_check(
) -> Result<(Option<AvailableUpdate>, BTreeMap<String, OfferedPlugin>), String> {
    let check = ask_site().await?;
    let plugins = check.plugins;

    if !check.update || check.url.is_empty() {
        return Ok((None, plugins));
    }
    // The server compares versions too; this refuses a downgrade should the two ever disagree.
    if !is_newer_version(common::version::APP_VERSION, &check.version) {
        return Ok((None, plugins));
    }

    let pkg = common::UpdatePackage {
        app_type: common::version::APP_TYPE.to_string(),
        build_type: common::version::BUILD_TYPE.to_string(),
        version: check.version,
        url: check.url,
        md5: check.md5,
    };
    Ok((
        Some(AvailableUpdate {
            pkg,
            notes: check.notes,
        }),
        plugins,
    ))
}

/// Fetches what differs for the installed plugins and leaves it beside the old files as `<path>.new`, put in place at
/// the next start. Answers the names of the plugins that will change.
async fn stage_plugin_updates(
    offered: BTreeMap<String, OfferedPlugin>,
) -> Result<Vec<String>, String> {
    if offered.is_empty() {
        return Ok(Vec::new());
    }
    let folder =
        ic_plugin_host::loader::plugin_directory().ok_or("no plugin folder on this system")?;
    let (fetches, names) = {
        let folder = folder.clone();
        tokio::task::spawn_blocking(move || {
            let installed = ic_plugin_host::loader::installed_with_about();
            let names: BTreeMap<String, String> = installed
                .iter()
                .map(|held| {
                    let name = held
                        .about
                        .as_ref()
                        .map(|about| about.name.clone())
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| held.library.clone());
                    (held.library.clone(), name)
                })
                .collect();
            let versions: Vec<(String, Option<String>)> = installed
                .iter()
                .map(|held| {
                    (
                        held.library.clone(),
                        held.about.as_ref().map(|about| about.version.clone()),
                    )
                })
                .collect();
            (updates::plan(&folder, &offered, &versions), names)
        })
        .await
        .map_err(|e| e.to_string())?
    };

    let mut by_plugin: BTreeMap<String, Vec<Fetch>> = BTreeMap::new();
    for fetch in fetches {
        by_plugin
            .entry(fetch.plugin.clone())
            .or_default()
            .push(fetch);
    }
    let mut staged = Vec::new();
    let mut failed = Vec::new();
    for (plugin, fetches) in by_plugin {
        let files = fetches.into_iter().map(|fetch| fetch.file).collect();
        match download_and_stage(folder.clone(), files).await {
            Ok(()) => staged.push(names.get(&plugin).cloned().unwrap_or(plugin)),
            Err(e) => failed.push(format!("{plugin}: {e}")),
        }
    }
    if staged.is_empty() && !failed.is_empty() {
        return Err(failed.join("; "));
    }
    for why in &failed {
        eprintln!("[updater] plugin update: {why}");
    }
    Ok(staged)
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = reqwest::get(url).await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP status {}", resp.status()));
    }
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

fn is_newer_version(current: &str, latest: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split('.')
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect()
    };
    let (cur, lat) = (parse(current), parse(latest));
    for i in 0..std::cmp::max(cur.len(), lat.len()) {
        let c = cur.get(i).copied().unwrap_or(0);
        let l = lat.get(i).copied().unwrap_or(0);
        if l > c {
            return true;
        }
        if c > l {
            return false;
        }
    }
    false
}

fn verify_checksum(expected: &str, bytes: &[u8]) -> Result<String, String> {
    let actual = format!("{:x}", md5::compute(bytes));
    if expected.is_empty() {
        return Err("the update listing carries no checksum for this package".to_string());
    }
    if !expected.eq_ignore_ascii_case(&actual) {
        return Err(format!(
            "checksum mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(actual)
}

async fn download_update_package(
    pkg: &common::UpdatePackage,
    file_prefix: &str,
) -> Result<(PathBuf, String, String), String> {
    let download_url = &pkg.url;
    let full_url = if download_url.starts_with('/') {
        format!("{}{}", update_server(), download_url)
    } else {
        download_url.clone()
    };
    let resp = reqwest::get(&full_url).await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP status {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let file_ext = download_url.split('.').next_back().unwrap_or("bin");
    let file_path = std::env::temp_dir().join(format!(
        "{}-update-{}.{}",
        file_prefix, pkg.version, file_ext
    ));
    let hash = verify_checksum(&pkg.md5, &bytes)?;

    std::fs::write(&file_path, &bytes).map_err(|e| e.to_string())?;
    Ok((file_path, pkg.version.clone(), hash))
}

#[cfg(test)]
mod tests {
    use super::{is_newer_version, verify_checksum, Checked};

    /// What the site answered for 0.8.130 on macOS, from the registry of the real builds, two plugins kept.
    const ANSWERED: &str = r#"{"latest_version": "0.8.130", "plugins": {"ic_video_view": {"files": [{"md5": "c97a69f257e4fe68aaf2a128ee6c43bf", "path": "libic_video_view.dylib", "size": 419200, "url": "https://github.com/ice-commander/ic-plugin-videocodecs/releases/download/v0.1.0/libic_video_view.dylib"}, {"md5": "4edba30dc8f6e4ba8297047a2eb27396", "path": "video-codecs/THIRD-PARTY-macos.txt", "size": 48798, "url": "https://github.com/ice-commander/ic-plugin-videocodecs/releases/download/v0.1.0/THIRD-PARTY-macos.txt"}, {"md5": "0a5d046cd4d7b150d4127f4ae1a22e80", "path": "video-codecs/libmpv.2.dylib", "size": 26892608, "url": "https://github.com/ice-commander/ic-plugin-videocodecs/releases/download/v0.1.0/libmpv.2.dylib"}], "version": "0.1.0"}, "ic_sftp_fs": {"files": [{"md5": "c0baa9cbd8bcf96510898a45ce930146", "path": "libic_sftp_fs.dylib", "size": 3774320, "url": "https://github.com/ice-commander/ic-plugin-sftp/releases/download/v0.1.0/libic_sftp_fs.dylib"}], "version": "0.1.0"}}, "plugins_of": "0.8.130", "update": false}"#;

    #[test]
    fn the_sites_answer_reads_and_only_an_installed_plugin_is_fetched() {
        let checked: Checked = serde_json::from_str(ANSWERED).unwrap();
        assert!(!checked.update);
        let video = &checked.plugins["ic_video_view"];
        assert_eq!(video.version, "0.1.0");
        assert_eq!(video.files.len(), 3);
        assert_eq!(video.files[2].path, "video-codecs/libmpv.2.dylib");

        let folder = std::env::temp_dir().join(format!("ic-updater-answer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("libic_sftp_fs.dylib"), b"an older build").unwrap();
        let installed = [("ic_sftp_fs".to_string(), Some("0.0.9".to_string()))];
        let fetch = ic_plugin_host::updates::plan(&folder, &checked.plugins, &installed);
        let _ = std::fs::remove_dir_all(&folder);

        assert_eq!(fetch.len(), 1, "{fetch:?}");
        assert_eq!(fetch[0].plugin, "ic_sftp_fs");
        assert!(fetch[0]
            .file
            .url
            .starts_with(ic_plugin_host::updates::TRUSTED));
    }

    #[test]
    fn a_matching_checksum_passes_and_case_does_not_matter() {
        let bytes = b"package bytes";
        let md5 = format!("{:x}", md5::compute(bytes));
        assert_eq!(verify_checksum(&md5, bytes).unwrap(), md5);
        assert!(verify_checksum(&md5.to_uppercase(), bytes).is_ok());
    }

    #[test]
    fn a_tampered_package_is_rejected() {
        let md5 = format!("{:x}", md5::compute(b"package bytes"));
        let err = verify_checksum(&md5, b"package bytez").unwrap_err();
        assert!(err.contains("mismatch"), "{err}");
    }

    #[test]
    fn a_package_without_a_checksum_is_rejected_rather_than_trusted() {
        assert!(verify_checksum("", b"anything").is_err());
    }

    #[test]
    fn newer_patch_wins() {
        assert!(is_newer_version("1.2.3", "1.2.4"));
    }

    #[test]
    fn equal_is_not_newer() {
        assert!(!is_newer_version("1.2.3", "1.2.3"));
    }

    #[test]
    fn older_is_not_newer() {
        assert!(!is_newer_version("1.2.10", "1.2.9"));
    }

    #[test]
    fn shorter_side_pads_with_zero() {
        assert!(is_newer_version("1.2", "1.2.1"));
        assert!(!is_newer_version("1.2.0", "1.2"));
    }

    #[test]
    fn numeric_not_lexicographic() {
        assert!(is_newer_version("0.5.9", "0.5.80"));
    }
}
