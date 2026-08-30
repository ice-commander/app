use crate::dialogs::{open_downloaded_file, show_error_dialog, show_info_dialog};
use common::AppError;
use fm_core::rpc::RemoteFileEntry;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

struct ProgressReader<R, F> {
    inner: R,
    progress_callback: F,
    total_read: u64,
}

impl<R: std::io::Read, F: Fn(u64)> std::io::Read for ProgressReader<R, F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.total_read += n as u64;
            (self.progress_callback)(self.total_read);
        }
        Ok(n)
    }
}

fn parse_date_str(date_str: &str) -> u64 {
    use chrono::{Datelike, TimeZone};
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
    let assumed = format!("{} {}", chrono::Local::now().year(), date_str);
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&assumed, "%Y %b %e %H:%M") {
        if let chrono::LocalResult::Single(dt) = chrono::Local.from_local_datetime(&naive) {
            return dt.timestamp() as u64;
        }
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%b %e %Y") {
        if let Some(naive) = date.and_hms_opt(0, 0, 0) {
            if let chrono::LocalResult::Single(dt) = chrono::Local.from_local_datetime(&naive) {
                return dt.timestamp() as u64;
            }
        }
    }
    0
}

pub struct LocalFtpRpc {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub ftp_session: std::sync::Arc<std::sync::Mutex<Option<crate::fs_ftp::FtpSession>>>,
}

#[async_trait::async_trait(?Send)]
impl fm_core::rpc::FileSystemRpc for LocalFtpRpc {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn content_wait(&self) -> fm_core::rpc::ContentWait {
        crate::net_content_wait()
    }
    fn connection_id(&self) -> Option<String> {
        Some(format!("ftp://{}@{}:{}", self.user, self.host, self.port))
    }

    fn display_name(&self) -> Option<String> {
        Some(if self.name.is_empty() { self.host.clone() } else { self.name.clone() })
    }

    fn get_icon(&self, path: &str) -> String {
        let path_norm = path.replace('\\', "/");
        if path_norm == "/" || path_norm.is_empty() {
            "/com/icecommander/gtk/ftp.svg".to_string()
        } else {
            "/com/icecommander/gtk/folder.svg".to_string()
        }
    }

    async fn list_dir(&self, path: String) -> Result<Vec<RemoteFileEntry>, AppError> {
        let host = self.host.clone();
        let port = self.port;
        let user = self.user.clone();
        let pass = self.pass.clone();
        let ftp_session = self.ftp_session.clone();

        let res = tokio::task::spawn_blocking(move || {
            crate::fs_ftp::list_ftp_directory_cached(
                ftp_session, &host, port, &user, &pass, &path,
            )
        })
        .await;

        match res {
            Ok(Ok((dirs, files))) => {
                let mut entries = Vec::new();
                for d in dirs {
                    entries.push(RemoteFileEntry { name: d.0, is_dir: true, size: 0, modified: parse_date_str(&d.1), permissions: None });
                }
                for f in files {
                    entries.push(RemoteFileEntry { name: f.0, is_dir: false, size: f.1, modified: parse_date_str(&f.2), permissions: None });
                }
                Ok(entries)
            }
            Ok(Err(e)) => Err(AppError::Remote(e)),
            Err(e) => Err(AppError::Remote(e.to_string())),
        }
    }

    async fn create_directory(&self, parent_path: String, dir_name: String, _permissions: Option<u32>) -> Result<(), AppError> {
        let host = self.host.clone();
        let port = self.port;
        let user = self.user.clone();
        let pass = self.pass.clone();
        let mut path = PathBuf::from(&parent_path);
        path.push(&dir_name);
        let path_str = path.to_string_lossy().to_string();
        let ftp_session = self.ftp_session.clone();

        tokio::task::spawn_blocking(move || {
            crate::fs_ftp::create_ftp_directory_cached(
                ftp_session, &host, port, &user, &pass, &path_str,
            )
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))
    }

    async fn delete_entries(&self, paths: Vec<String>) -> Result<(), AppError> {
        let host = self.host.clone();
        let port = self.port;
        let user = self.user.clone();
        let pass = self.pass.clone();
        let ftp_session = self.ftp_session.clone();

        tokio::task::spawn_blocking(move || {
            crate::fs_ftp::delete_ftp_entries_cached(
                ftp_session, &host, port, &user, &pass, &paths,
            )
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))
    }

    async fn rename_entry(&self, path: String, new_path: String) -> Result<(), AppError> {
        let host = self.host.clone();
        let port = self.port;
        let user = self.user.clone();
        let pass = self.pass.clone();
        let ftp_session = self.ftp_session.clone();

        tokio::task::spawn_blocking(move || {
            crate::fs_ftp::rename_ftp_entry_cached(
                ftp_session, &host, port, &user, &pass, &path, &new_path,
            )
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))
    }

    fn request_file_download(&self, file_path: String, _transfer_id: uuid::Uuid) {
        let host = self.host.clone();
        let port = self.port;
        let user = self.user.clone();
        let pass = self.pass.clone();

        let filename = Path::new(&file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("downloaded_file")
            .to_string();

        let mut dest_path = dirs::download_dir().unwrap_or_else(|| std::env::temp_dir());
        dest_path.push(&filename);

        crate::spawn_local(async move {
            let (tx, mut rx) = mpsc::channel::<Result<Vec<u8>, String>>(100);

            let dest_path_clone = dest_path.clone();
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

            let download_res = tokio::task::spawn_blocking(move || {
                crate::fs_ftp::start_ftp_download(host, port, user, pass, file_path, tx)
            })
            .await;

            let result = match download_res {
                Ok(Ok(_)) => writer_handle
                    .await
                    .unwrap_or(Err("Writer task panicked".to_string())),
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
        let host = self.host.clone();
        let port = self.port;
        let user = self.user.clone();
        let pass = self.pass.clone();

        let mut dest_path = PathBuf::from(&target_path);
        dest_path.push(&file_name);
        let dest_path_str = dest_path.to_string_lossy().to_string();

        crate::spawn_local(async move {
            let host_c = host.clone();
            let user_c = user.clone();
            let pass_c = pass.clone();
            let dest_path_str_c = dest_path_str.clone();
            let local_file_path_c = local_file_path.clone();
            let res = tokio::task::spawn_blocking(move || {
                crate::fs_ftp::upload_ftp_file(
                    &host_c,
                    port,
                    &user_c,
                    &pass_c,
                    &dest_path_str_c,
                    &local_file_path_c,
                )
            })
            .await;

            match res {
                Ok(Ok(())) => {
                }
                Ok(Err(e)) => {
                    show_error_dialog(&*crate::i18n::tr("connections.upload_error"), &e);
                }
                Err(e) => {
                    show_error_dialog(&*crate::i18n::tr("connections.upload_error"), &e.to_string());
                }
            }
        });
    }

    async fn read_file(
        &self,
        path: String,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<Vec<u8>, AppError> {
        let host = self.host.clone();
        let port = self.port;
        let user = self.user.clone();
        let pass = self.pass.clone();

        let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>(100);

        let download_res = tokio::task::spawn_blocking(move || {
            crate::fs_ftp::start_ftp_download(host, port, user, pass, path, tx)
        });

        let (prog_tx, mut prog_rx) = mpsc::channel::<u64>(8);
        let mut assemble = tokio::spawn(async move {
            let mut rx = rx;
            let mut all = Vec::new();
            let mut last_sent = std::time::Instant::now();
            while let Some(res) = rx.recv().await {
                match res {
                    Ok(bytes) => {
                        if bytes.is_empty() {
                            break;
                        }
                        all.extend_from_slice(&bytes);
                        if last_sent.elapsed().as_millis() >= 100 {
                            let _ = prog_tx.try_send(all.len() as u64);
                            last_sent = std::time::Instant::now();
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
            Ok(all)
        });

        let assembled = if progress_callback.is_some() {
            loop {
                tokio::select! {
                    biased;
                    res = &mut assemble => break res,
                    Some(total) = prog_rx.recv() => {
                        if let Some(ref cb) = progress_callback {
                            cb(total);
                        }
                    }
                }
            }
        } else {
            drop(prog_rx); // assembler's try_send just fails silently
            assemble.await
        };
        let all_bytes = match assembled {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Err(AppError::Remote(e)),
            Err(e) => return Err(AppError::Remote(e.to_string())),
        };

        match download_res.await {
            Ok(Ok(_)) => Ok(all_bytes),
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
        let host = self.host.clone();
        let port = self.port;
        let user = self.user.clone();
        let pass = self.pass.clone();
        let ftp_session = self.ftp_session.clone();

        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<u64>(100);

        let upload_res = tokio::task::spawn_blocking(move || {
            let mut cursor = std::io::Cursor::new(content);
            let last_sent = std::cell::Cell::new(
                std::time::Instant::now() - std::time::Duration::from_millis(200),
            );
            let mut progress_reader = ProgressReader {
                inner: &mut cursor,
                progress_callback: move |bytes| {
                    if last_sent.get().elapsed().as_millis() >= 100 {
                        let _ = progress_tx.try_send(bytes);
                        last_sent.set(std::time::Instant::now());
                    }
                },
                total_read: 0,
            };

            let mut attempts = 0;
            loop {
                let mut lock = ftp_session.lock().unwrap();
                let needs_connect = match &mut *lock {
                    Some(sess) => !sess.is_alive(),
                    None => true,
                };

                if needs_connect {
                    *lock = None;
                    match crate::fs_ftp::FtpSession::connect(&host, port, &user, &pass) {
                        Ok(sess) => *lock = Some(sess),
                        Err(e) => return Err(e),
                    }
                }

                let session_ref = lock.as_mut().unwrap();
                progress_reader.inner.set_position(0);
                progress_reader.total_read = 0;

                let put_res = session_ref.ftp_stream.put_file(&path, &mut progress_reader).map_err(|e| e.to_string());
                match put_res {
                    Ok(_) => return Ok(()),
                    Err(e) => {
                        attempts += 1;
                        if attempts >= 2 {
                            return Err(e);
                        }
                        *lock = None;
                    }
                }
            }
        });

        let mut progress_done = false;
        while !progress_done {
            tokio::select! {
                Some(bytes) = progress_rx.recv() => {
                    if let Some(ref cb) = progress_callback {
                        cb(bytes);
                    }
                }
                else => { progress_done = true; }
            }
        }

        match upload_res.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(AppError::Remote(e)),
            Err(e) => Err(AppError::Remote(e.to_string())),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use fm_core::rpc::FileSystemRpc;
    use std::io::Read;

    fn rpc(name: &str) -> LocalFtpRpc {
        LocalFtpRpc {
            name: name.to_string(),
            host: "example.test".to_string(),
            port: 2121,
            user: "alice".to_string(),
            pass: "secret".to_string(),
            ftp_session: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    #[test]
    fn iso_date_with_seconds_parses_to_that_instant() {
        let ts = parse_date_str("2024-01-15 10:30:00");
        assert!(ts > 1_705_000_000 && ts < 1_705_500_000, "ts={}", ts);
    }

    #[test]
    fn iso_date_without_seconds_parses_to_the_same_minute() {
        let with = parse_date_str("2024-01-15 10:30:00");
        let without = parse_date_str("2024-01-15 10:30");
        assert_eq!(with, without);
    }

    #[test]
    fn iso_format_emitted_by_the_listing_round_trips() {
        use chrono::TimeZone;
        let dt = chrono::Local.timestamp_opt(1_718_452_800, 0).unwrap();
        let rendered = dt.format("%Y-%m-%d %H:%M:%S").to_string();
        assert_eq!(parse_date_str(&rendered), 1_718_452_800);
    }

    #[test]
    fn unix_ls_date_with_implied_year_parses() {
        assert!(parse_date_str("Jun 28 14:12") > 0);
        assert!(parse_date_str("Jun 1 14:12") > 0);
        assert!(parse_date_str("Jun  1 14:12") > 0);
    }

    #[test]
    fn implied_year_dates_all_land_in_one_year() {
        let first = parse_date_str("Jan 1 00:00");
        let last = parse_date_str("Dec 31 23:59");
        assert!(first > 0 && last > 0);
        let span = last - first;
        assert!(span > 360 * 86_400 && span < 366 * 86_400, "span={}", span);
    }

    #[test]
    fn implied_year_dates_are_ordered_within_the_year() {
        assert!(parse_date_str("Jun 28 14:12") > parse_date_str("Jan 1 00:00"));
        assert!(parse_date_str("Jun 28 14:12") < parse_date_str("Dec 31 23:59"));
    }

    #[test]
    fn unix_ls_date_with_explicit_year_parses_to_that_year() {
        let ts = parse_date_str("Jun 28 2021");
        assert!(ts > 1_624_700_000 && ts < 1_624_950_000, "ts={}", ts);
    }

    #[test]
    fn explicit_year_wins_over_the_implied_one() {
        assert!(parse_date_str("Jun 28 2021") < parse_date_str("Jun 28 14:12"));
    }

    #[test]
    fn empty_date_returns_zero() {
        assert_eq!(parse_date_str(""), 0);
    }

    #[test]
    fn unparseable_date_returns_zero() {
        assert_eq!(parse_date_str("not a date"), 0);
        assert_eq!(parse_date_str("Xyz 99 25:99"), 0);
        assert_eq!(parse_date_str("2024-13-01 00:00:00"), 0);
    }

    #[test]
    fn later_iso_date_has_larger_timestamp() {
        assert!(parse_date_str("2024-01-01 00:00:00") > parse_date_str("2023-01-01 00:00:00"));
    }

    #[test]
    fn progress_reader_reports_running_totals_not_chunk_sizes() {
        let seen = std::cell::RefCell::new(Vec::new());
        let mut reader = ProgressReader {
            inner: std::io::Cursor::new(b"0123456789".to_vec()),
            progress_callback: |n: u64| seen.borrow_mut().push(n),
            total_read: 0,
        };
        let mut buf = [0u8; 4];
        while reader.read(&mut buf).unwrap() > 0 {}
        assert_eq!(*seen.borrow(), vec![4, 8, 10]);
    }

    #[test]
    fn progress_reader_passes_the_bytes_through_unchanged() {
        let mut reader = ProgressReader {
            inner: std::io::Cursor::new(b"payload".to_vec()),
            progress_callback: |_n: u64| {},
            total_read: 0,
        };
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"payload");
        assert_eq!(reader.total_read, 7);
    }

    #[test]
    fn progress_reader_stays_silent_on_end_of_stream() {
        let calls = std::cell::Cell::new(0u32);
        let mut reader = ProgressReader {
            inner: std::io::Cursor::new(Vec::new()),
            progress_callback: |_n: u64| calls.set(calls.get() + 1),
            total_read: 0,
        };
        let mut buf = [0u8; 8];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn progress_reader_propagates_errors_without_reporting_progress() {
        struct Failing;
        impl std::io::Read for Failing {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "boom"))
            }
        }
        let calls = std::cell::Cell::new(0u32);
        let mut reader = ProgressReader {
            inner: Failing,
            progress_callback: |_n: u64| calls.set(calls.get() + 1),
            total_read: 0,
        };
        let mut buf = [0u8; 8];
        let err = reader.read(&mut buf).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn connection_id_carries_user_host_and_port() {
        assert_eq!(
            rpc("").connection_id().unwrap(),
            "ftp://alice@example.test:2121"
        );
    }

    #[test]
    fn display_name_falls_back_to_host_when_unnamed() {
        assert_eq!(rpc("").display_name().unwrap(), "example.test");
    }

    #[test]
    fn display_name_prefers_the_configured_name() {
        assert_eq!(rpc("Backup box").display_name().unwrap(), "Backup box");
    }

    #[test]
    fn root_gets_the_ftp_icon_and_subdirs_the_folder_icon() {
        let r = rpc("");
        assert!(r.get_icon("/").ends_with("/ftp.svg"));
        assert!(r.get_icon("").ends_with("/ftp.svg"));
        assert!(r.get_icon("/pub/incoming").ends_with("/folder.svg"));
    }

    #[test]
    fn backslash_root_is_normalised_to_the_ftp_icon() {
        assert!(rpc("").get_icon("\\").ends_with("/ftp.svg"));
    }
}
