use gtk::glib;
use gtk_updater_ui::{UpdaterInit, UpdaterInput, UpdaterModel, UpdaterOutput};
use relm4::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

const DEFAULT_UPDATE_SERVER: &str = "https://www.icecommander.com";
const APP_NAME: &str = "Ice Commander";
const FILE_PREFIX: &str = "icecommander";

fn update_server() -> String {
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
                Ok(Ok(Some(found))) => {
                    finish(&on_complete);
                    let version = found.pkg.version.clone();
                    let notes = found.notes;
                    *found_pkg.borrow_mut() = Some(found.pkg);
                    let _ = ctrl
                        .sender()
                        .send(UpdaterInput::UpdateAvailable { version, notes });
                }
                Ok(Ok(None)) => {
                    finish(&on_complete);
                    if manual {
                        let _ = ctrl.sender().send(UpdaterInput::UpToDate);
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
                    let rx = spawn_on_tokio(async move {
                        download_update_package(&pkg, FILE_PREFIX).await
                    });
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

fn spawn_on_tokio<F, T>(future: F) -> tokio::sync::oneshot::Receiver<T>
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

async fn perform_update_check() -> Result<Option<AvailableUpdate>, String> {
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
    let check = resp
        .json::<common::UpdateCheck>()
        .await
        .map_err(|e| e.to_string())?;

    if !check.update || check.url.is_empty() {
        return Ok(None);
    }
    // The server compares versions too; this refuses a downgrade should the two ever disagree.
    if !is_newer_version(common::version::APP_VERSION, &check.version) {
        return Ok(None);
    }

    let pkg = common::UpdatePackage {
        app_type: common::version::APP_TYPE.to_string(),
        build_type: common::version::BUILD_TYPE.to_string(),
        version: check.version,
        url: check.url,
        md5: check.md5,
    };
    Ok(Some(AvailableUpdate { pkg, notes: check.notes }))
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
    use super::{is_newer_version, verify_checksum};

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
