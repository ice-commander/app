use crate::dialogs::{open_downloaded_file, show_error_dialog, show_info_dialog};
use common::AppError;
use fm_core::rpc::RemoteFileEntry;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

fn parse_date_str(date_str: &str) -> u64 {
    use chrono::TimeZone;
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
        if let chrono::LocalResult::Single(dt) = chrono::Local.from_local_datetime(&naive) {
            return dt.timestamp() as u64;
        }
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M") {
        if let chrono::LocalResult::Single(dt) = chrono::Local.from_local_datetime(&naive) {
            return dt.timestamp() as u64;
        }
    }
    0
}

pub struct LocalWebDavRpc {
    pub name: String,
    pub url: String,
    pub user: Option<String>,
    pub pass: Option<String>,
    pub remote_path: Option<String>,
}

#[async_trait::async_trait(?Send)]
impl fm_core::rpc::FileSystemRpc for LocalWebDavRpc {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn content_wait(&self) -> fm_core::rpc::ContentWait {
        crate::net_content_wait()
    }
    fn connection_id(&self) -> Option<String> {
        Some(format!(
            "webdav://{}@{}",
            self.user.as_deref().unwrap_or(""),
            self.url
        ))
    }

    fn display_name(&self) -> Option<String> {
        Some(if self.name.is_empty() { self.url.clone() } else { self.name.clone() })
    }

    fn get_icon(&self, path: &str) -> String {
        let path_norm = path.replace('\\', "/");
        if path_norm == "/" || path_norm.is_empty() {
            "/com/icecommander/gtk/netdrive.svg".to_string()
        } else {
            "/com/icecommander/gtk/folder.svg".to_string()
        }
    }

    async fn list_dir(&self, path: String) -> Result<Vec<RemoteFileEntry>, AppError> {
        let cfg = crate::fs_webdav::WebDAVConfig {
            url: self.url.clone(),
            user: self.user.clone(),
            pass: self.pass.clone(),
            base_path: self.remote_path.clone(),
        };

        let (dirs, files) = tokio::spawn(async move {
            crate::fs_webdav::list_webdav_directory(&cfg, &path).await
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))?;

        let mut entries = Vec::new();
        for d in dirs {
            entries.push(RemoteFileEntry { name: d.0, is_dir: true, size: 0, modified: parse_date_str(&d.1), permissions: None });
        }
        for f in files {
            entries.push(RemoteFileEntry { name: f.0, is_dir: false, size: f.1, modified: parse_date_str(&f.2), permissions: None });
        }
        Ok(entries)
    }

    async fn create_directory(&self, parent_path: String, dir_name: String, _permissions: Option<u32>) -> Result<(), AppError> {
        let cfg = crate::fs_webdav::WebDAVConfig {
            url: self.url.clone(),
            user: self.user.clone(),
            pass: self.pass.clone(),
            base_path: self.remote_path.clone(),
        };
        let mut path = PathBuf::from(&parent_path);
        path.push(&dir_name);
        let path_str = path.to_string_lossy().to_string();

        tokio::spawn(async move {
            crate::fs_webdav::create_webdav_directory(&cfg, &path_str).await
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))
    }

    async fn delete_entries(&self, paths: Vec<String>) -> Result<(), AppError> {
        let cfg = crate::fs_webdav::WebDAVConfig {
            url: self.url.clone(),
            user: self.user.clone(),
            pass: self.pass.clone(),
            base_path: self.remote_path.clone(),
        };

        tokio::spawn(async move {
            for path in paths {
                crate::fs_webdav::delete_webdav_entry(&cfg, &path).await.map_err(|e| AppError::Remote(e))?;
            }
            Ok::<(), AppError>(())
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
    }

    async fn rename_entry(&self, path: String, new_path: String) -> Result<(), AppError> {
        let cfg = crate::fs_webdav::WebDAVConfig {
            url: self.url.clone(),
            user: self.user.clone(),
            pass: self.pass.clone(),
            base_path: self.remote_path.clone(),
        };

        tokio::spawn(async move {
            crate::fs_webdav::rename_webdav_entry(&cfg, &path, &new_path).await
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))
    }

    fn request_file_download(&self, file_path: String, _transfer_id: uuid::Uuid) {
        let filename = Path::new(&file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("downloaded_file")
            .to_string();

        let mut dest_path = dirs::download_dir().unwrap_or_else(|| std::env::temp_dir());
        dest_path.push(&filename);
        let dest_path_clone = dest_path.clone();

        let cfg = crate::fs_webdav::WebDAVConfig {
            url: self.url.clone(),
            user: self.user.clone(),
            pass: self.pass.clone(),
            base_path: self.remote_path.clone(),
        };

        crate::spawn_local(async move {
            let (tx, mut rx) = mpsc::channel::<Result<Vec<u8>, String>>(100);

            let writer_handle = tokio::spawn(async move {
                let mut file = match std::fs::File::create(&dest_path_clone) {
                    Ok(f) => f,
                    Err(e) => return Err(e.to_string()),
                };
                while let Some(res) = rx.recv().await {
                    match res {
                        Ok(bytes) => {
                            if bytes.is_empty() {
                                break;
                            }
                            if let Err(e) = file.write_all(&bytes) {
                                return Err(e.to_string());
                            }
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(())
            });

            let download_res = tokio::spawn(async move {
                crate::fs_webdav::start_webdav_download(cfg, file_path, tx).await
            })
            .await;

            let result = match download_res {
                Ok(Ok(_)) => match writer_handle.await {
                    Ok(res) => res,
                    Err(e) => Err(e.to_string()),
                },
                Ok(Err(e)) => Err(e),
                Err(e) => Err(e.to_string()),
            };

            match result {
                Ok(()) => {
                    show_info_dialog(
                        &*crate::i18n::tr("connections.download_complete"),
                        &crate::i18n::trf("connections.download_success_body", &[("path", &*(dest_path.to_string_lossy()).to_string())]),
                    );
                    open_downloaded_file(&dest_path);
                }
                Err(e) => {
                    show_error_dialog(&*crate::i18n::tr("connections.download_error"), &e);
                }
            }
        });
    }

    fn trigger_file_upload(
        &self,
        target_path: String,
        file_name: String,
        local_file_path: std::path::PathBuf,
        _transfer_id: uuid::Uuid,
    ) {
        let mut dest_path = PathBuf::from(&target_path);
        dest_path.push(&file_name);
        let dest_path_str = dest_path.to_string_lossy().to_string();

        let cfg = crate::fs_webdav::WebDAVConfig {
            url: self.url.clone(),
            user: self.user.clone(),
            pass: self.pass.clone(),
            base_path: self.remote_path.clone(),
        };

        crate::spawn_local(async move {
            let res = tokio::task::spawn_blocking(move || {
                std::fs::read(&local_file_path).map_err(|e| e.to_string())
            })
            .await;

            match res {
                Ok(Ok(content)) => {
                    let upload_res = tokio::spawn(async move {
                        crate::fs_webdav::upload_webdav_file(&cfg, &dest_path_str, content)
                            .await
                    })
                    .await;

                    match upload_res {
                        Ok(Ok(())) => {
                        }
                        Ok(Err(e)) => show_error_dialog(&*crate::i18n::tr("connections.upload_error"), &e),
                        Err(e) => show_error_dialog(&*crate::i18n::tr("connections.upload_error"), &e.to_string()),
                    }
                }
                Ok(Err(e)) => show_error_dialog(&*crate::i18n::tr("connections.upload_error"), &e),
                Err(e) => show_error_dialog(&*crate::i18n::tr("connections.upload_error"), &e.to_string()),
            }
        });
    }

    async fn read_file(
        &self,
        path: String,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<Vec<u8>, AppError> {
        let cfg = crate::fs_webdav::WebDAVConfig {
            url: self.url.clone(),
            user: self.user.clone(),
            pass: self.pass.clone(),
            base_path: self.remote_path.clone(),
        };

        let (tx, mut rx) = mpsc::channel::<Result<Vec<u8>, String>>(100);
        let (progress_tx, mut progress_rx) = mpsc::channel::<u64>(100);

        let collector_handle = tokio::spawn(async move {
            let mut all_bytes = Vec::new();
            let mut total_read = 0u64;
            let mut last_sent = std::time::Instant::now();
            while let Some(res) = rx.recv().await {
                match res {
                    Ok(bytes) => {
                        if bytes.is_empty() { break; }
                        total_read += bytes.len() as u64;
                        if last_sent.elapsed().as_millis() >= 100 {
                            let _ = progress_tx.try_send(total_read);
                            last_sent = std::time::Instant::now();
                        }
                        all_bytes.extend_from_slice(&bytes);
                    }
                    Err(e) => return Err(e),
                }
            }
            Ok(all_bytes)
        });

        let download_res = tokio::spawn(async move {
            crate::fs_webdav::start_webdav_download(cfg, path, tx).await
        });

        let mut progress_done = false;
        while !progress_done {
            tokio::select! {
                Some(bytes) = progress_rx.recv() => {
                    if let Some(ref cb) = progress_callback { cb(bytes); }
                }
                else => { progress_done = true; }
            }
        }

        match download_res.await {
            Ok(Ok(_)) => collector_handle.await.map_err(|e| AppError::Remote(e.to_string()))?.map_err(|e| AppError::Remote(e)),
            Ok(Err(e)) => Err(AppError::Remote(e)),
            Err(e) => Err(AppError::Remote(e.to_string())),
        }
    }

    async fn write_file(
        &self,
        path: String,
        content: Vec<u8>,
        _permissions: Option<u32>,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<(), AppError> {
        let cfg = crate::fs_webdav::WebDAVConfig {
            url: self.url.clone(),
            user: self.user.clone(),
            pass: self.pass.clone(),
            base_path: self.remote_path.clone(),
        };

        let (progress_tx, mut raw_progress_rx) = mpsc::channel::<u64>(100);

        let upload_res = tokio::spawn(async move {
            crate::fs_webdav::upload_webdav_file_with_progress(
                &cfg, &path, content, progress_tx,
            )
            .await
        });

        let (progress_tx, mut progress_rx) = mpsc::channel::<u64>(8);
        tokio::spawn(async move {
            let mut last_sent = std::time::Instant::now()
                - std::time::Duration::from_millis(200);
            while let Some(bytes) = raw_progress_rx.recv().await {
                if last_sent.elapsed().as_millis() >= 100 {
                    let _ = progress_tx.try_send(bytes);
                    last_sent = std::time::Instant::now();
                }
            }
        });

        let mut progress_done = false;
        while !progress_done {
            tokio::select! {
                Some(bytes) = progress_rx.recv() => {
                    if let Some(ref cb) = progress_callback { cb(bytes); }
                }
                else => { progress_done = true; }
            }
        }

        match upload_res.await {
            Ok(val) => val.map_err(|e| AppError::Remote(e)),
            Err(e) => Err(AppError::Remote(e.to_string())),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use fm_core::rpc::FileSystemRpc;

    fn rpc() -> LocalWebDavRpc {
        LocalWebDavRpc {
            name: String::new(),
            url: "https://cloud.test/remote.php/dav".to_string(),
            user: Some("alice".to_string()),
            pass: Some("secret".to_string()),
            remote_path: None,
        }
    }

    #[test]
    fn iso_date_with_seconds_parses_to_that_instant() {
        let ts = parse_date_str("2024-01-15 10:30:00");
        assert!(ts > 1_705_000_000 && ts < 1_705_500_000, "ts={}", ts);
    }

    #[test]
    fn iso_date_without_seconds_parses_to_the_same_minute() {
        assert_eq!(
            parse_date_str("2024-01-15 10:30:00"),
            parse_date_str("2024-01-15 10:30")
        );
    }

    #[test]
    fn format_emitted_by_the_listing_round_trips() {
        use chrono::TimeZone;
        let dt = chrono::Local.timestamp_opt(1_718_452_800, 0).unwrap();
        let rendered = dt.format("%Y-%m-%d %H:%M:%S").to_string();
        assert_eq!(parse_date_str(&rendered), 1_718_452_800);
    }

    #[test]
    fn raw_rfc2822_header_returns_zero() {
        assert_eq!(parse_date_str("Mon, 01 Jan 2024 00:00:00 GMT"), 0);
    }

    #[test]
    fn empty_and_malformed_dates_return_zero() {
        assert_eq!(parse_date_str(""), 0);
        assert_eq!(parse_date_str("not a date"), 0);
        assert_eq!(parse_date_str("2024-06-31 00:00:00"), 0);
    }

    #[test]
    fn later_date_has_larger_timestamp() {
        assert!(parse_date_str("2024-01-01 00:00:00") > parse_date_str("2023-01-01 00:00:00"));
    }

    #[test]
    fn connection_id_carries_user_and_url() {
        assert_eq!(
            rpc().connection_id().unwrap(),
            "webdav://alice@https://cloud.test/remote.php/dav"
        );
    }

    #[test]
    fn connection_id_without_a_user_leaves_the_name_empty() {
        let mut r = rpc();
        r.user = None;
        assert_eq!(
            r.connection_id().unwrap(),
            "webdav://@https://cloud.test/remote.php/dav"
        );
    }

    #[test]
    fn two_users_on_one_server_get_different_connection_ids() {
        let mut other = rpc();
        other.user = Some("bob".to_string());
        assert_ne!(rpc().connection_id(), other.connection_id());
    }

    #[test]
    fn display_name_falls_back_to_the_url_when_unnamed() {
        assert_eq!(
            rpc().display_name().unwrap(),
            "https://cloud.test/remote.php/dav"
        );
    }

    #[test]
    fn display_name_prefers_the_configured_name() {
        let mut r = rpc();
        r.name = "Nextcloud".to_string();
        assert_eq!(r.display_name().unwrap(), "Nextcloud");
    }

    #[test]
    fn root_gets_the_netdrive_icon_and_subdirs_the_folder_icon() {
        let r = rpc();
        assert!(r.get_icon("/").ends_with("/netdrive.svg"));
        assert!(r.get_icon("").ends_with("/netdrive.svg"));
        assert!(r.get_icon("\\").ends_with("/netdrive.svg"));
        assert!(r.get_icon("/docs").ends_with("/folder.svg"));
    }
}
