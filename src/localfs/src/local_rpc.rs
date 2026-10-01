#[cfg(feature = "gtk")]
use adw::prelude::*;
use common::AppError;
use ic_platform::fs_local;
#[cfg(feature = "gtk")]
use std::cell::RefCell;
#[cfg(feature = "gtk")]
use std::collections::HashMap;
use std::path::PathBuf;
#[cfg(feature = "gtk")]
use std::sync::OnceLock;
#[cfg(feature = "gtk")]
use tokio::sync::Semaphore;

#[cfg(feature = "gtk")]
thread_local! {
    static PENDING_PICS: RefCell<HashMap<String, Vec<gtk::glib::WeakRef<gtk::Picture>>>> =
        RefCell::new(HashMap::new());
}

#[cfg(feature = "gtk")]
static BG_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();

#[cfg(feature = "gtk")]
fn get_bg_semaphore() -> &'static Semaphore {
    BG_SEMAPHORE.get_or_init(|| Semaphore::new(4))
}

#[cfg(feature = "gtk")]
fn extract_raw_thumbnail(path: &str) -> Option<Vec<u8>> {
    use std::fs::File;
    use std::io::{BufReader, Read, Seek, SeekFrom};

    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);

    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    let fields: Vec<_> = exif.fields().collect();

    let mut candidates = Vec::new();

    for field in &fields {
        let tag_num = field.tag.number();
        if tag_num == 0x0201 {
            // JPEGInterchangeFormat
            if let Some(offset) = field.value.get_uint(0) {
                let length = fields
                    .iter()
                    .find(|f| f.tag.number() == 0x0202 && f.ifd_num == field.ifd_num)
                    .and_then(|f| f.value.get_uint(0));
                candidates.push((offset, length));
            }
        }
        if tag_num == 0x0111 {
            // StripOffsets
            if let Some(offset) = field.value.get_uint(0) {
                let length = fields
                    .iter()
                    .find(|f| f.tag.number() == 0x0117 && f.ifd_num == field.ifd_num)
                    .and_then(|f| f.value.get_uint(0));
                candidates.push((offset, length));
            }
        }
        if tag_num == 0x0144 {
            // TileOffsets
            if let Some(offset) = field.value.get_uint(0) {
                let length = fields
                    .iter()
                    .find(|f| f.tag.number() == 0x0145 && f.ifd_num == field.ifd_num)
                    .and_then(|f| f.value.get_uint(0));
                candidates.push((offset, length));
            }
        }
    }

    let mut file = reader.into_inner();
    for (offset, length_opt) in candidates {
        if file.seek(SeekFrom::Start(offset as u64)).is_ok() {
            let mut magic = [0u8; 3];
            if file.read_exact(&mut magic).is_ok() {
                if magic[0] == 0xFF && magic[1] == 0xD8 && magic[2] == 0xFF {
                    if let Some(length) = length_opt {
                        file.seek(SeekFrom::Start(offset as u64)).ok()?;
                        let mut buf = vec![0u8; length as usize];
                        if file.read_exact(&mut buf).is_ok() {
                            return Some(buf);
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(feature = "gtk")]
pub fn get_local_thumbnail(path: &str) -> Option<gtk::gio::File> {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let is_media = matches!(
        ext.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "svg"
            | "nef"
            | "cr2"
            | "cr3"
            | "arw"
            | "dng"
            | "raf"
            | "orf"
            | "rw2"
            | "pef"
    );

    if is_media {
        Some(gtk::gio::File::for_path(path))
    } else {
        None
    }
}

fn parse_date_str(date_str: &str) -> u64 {
    use chrono::TimeZone;
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
        if let chrono::LocalResult::Single(dt) = chrono::Local.from_local_datetime(&naive) {
            return dt.timestamp() as u64;
        }
    }
    0
}

pub struct LocalFileSystemRpc {
    pub config: client_config::AppConfig,
}

impl LocalFileSystemRpc {
    pub fn new(config: client_config::AppConfig) -> Self {
        Self { config }
    }
}

#[async_trait::async_trait(?Send)]
impl fm_core::rpc::FileSystemRpc for LocalFileSystemRpc {
    fn supports_terminal(&self) -> bool {
        true
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    async fn read_file_opt(
        &self,
        path: String,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        blocking: bool,
    ) -> Result<Vec<u8>, AppError> {
        if !blocking {
            return self.read_file(path, progress_callback).await;
        }
        let res = std::fs::read(PathBuf::from(path)).map_err(AppError::from);
        if let (Ok(bytes), Some(cb)) = (&res, &progress_callback) {
            cb(bytes.len() as u64);
        }
        res
    }
    async fn list_dir(&self, path: String) -> Result<Vec<fm_core::rpc::RemoteFileEntry>, AppError> {
        let (dirs, files) = fs_local::list_local_directory(std::path::Path::new(&path));
        let mut entries = Vec::new();
        for d in dirs {
            let mut permissions = None;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let full_path = std::path::Path::new(&path).join(&d.0);
                if let Ok(metadata) = std::fs::metadata(&full_path) {
                    permissions = Some(metadata.mode());
                }
            }
            entries.push(fm_core::rpc::RemoteFileEntry {
                name: d.0,
                is_dir: true,
                size: 0,
                modified: parse_date_str(&d.1),
                permissions,
                extra: Vec::new(),
            });
        }
        for f in files {
            let mut permissions = None;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let full_path = std::path::Path::new(&path).join(&f.0);
                if let Ok(metadata) = std::fs::metadata(&full_path) {
                    permissions = Some(metadata.mode());
                }
            }
            entries.push(fm_core::rpc::RemoteFileEntry {
                name: f.0,
                is_dir: false,
                size: f.1,
                modified: parse_date_str(&f.2),
                permissions,
                extra: Vec::new(),
            });
        }
        Ok(entries)
    }

    async fn create_directory(
        &self,
        parent_path: String,
        dir_name: String,
        permissions: Option<u32>,
    ) -> Result<(), AppError> {
        let mut path = PathBuf::from(&parent_path);
        path.push(dir_name);
        std::fs::create_dir(&path).map_err(AppError::from)?;
        #[cfg(unix)]
        if let Some(mode) = permissions {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode));
        }
        Ok(())
    }

    async fn delete_entries(&self, paths: Vec<String>) -> Result<(), AppError> {
        tokio::task::spawn_blocking(move || {
            for path in paths {
                let safe = sanitize_windows_path(&PathBuf::from(&path));
                let res = if safe.is_dir() {
                    std::fs::remove_dir_all(&safe)
                } else {
                    std::fs::remove_file(&safe)
                };
                res.map_err(|e| AppError::Other(format!("{}: {}", path, e)))?;
            }
            Ok(())
        })
        .await
        .map_err(|e| AppError::Other(format!("delete task panicked: {e}")))?
    }

    async fn rename_entry(&self, path: String, new_path: String) -> Result<(), AppError> {
        let path_buf = PathBuf::from(&path);
        let new_path_buf = PathBuf::from(&new_path);
        fs_local::rename_local_entry(&path_buf, &new_path_buf)
            .await
            .map_err(AppError::from)
    }

    async fn duplicate_entry(&self, src: String, dst: String) -> Result<(), AppError> {
        let src_buf = PathBuf::from(&src);
        let dst_buf = PathBuf::from(&dst);
        fs_local::duplicate_local_entry(&src_buf, &dst_buf)
            .await
            .map_err(AppError::from)
    }

    fn request_file_download(&self, file_path: String, _transfer_id: uuid::Uuid) {
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open")
            .arg(&file_path)
            .spawn();

        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&file_path).spawn();

        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd")
            .args(&["/c", "start", "", &file_path])
            .spawn();
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

        crate::spawn_local(async move {
            let _ = tokio::fs::copy(&local_file_path, &dest_path).await;
        });
    }

    async fn read_file(
        &self,
        path: String,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<Vec<u8>, AppError> {
        let path_buf = PathBuf::from(path);
        let res = tokio::fs::read(&path_buf).await.map_err(AppError::from);
        if let Ok(ref bytes) = res {
            if let Some(ref cb) = progress_callback {
                cb(bytes.len() as u64);
            }
        }
        res
    }

    async fn write_file(
        &self,
        path: String,
        content: Vec<u8>,
        permissions: Option<u32>,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<(), AppError> {
        let path_buf = PathBuf::from(path);
        let len = content.len() as u64;
        const SYNC_WRITE_MAX: usize = 8 * 1024 * 1024;
        let res = if content.len() <= SYNC_WRITE_MAX {
            std::fs::write(&path_buf, &content).map_err(AppError::from)
        } else {
            tokio::fs::write(&path_buf, content)
                .await
                .map_err(AppError::from)
        };
        if res.is_ok() {
            #[cfg(unix)]
            if let Some(mode) = permissions {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path_buf, std::fs::Permissions::from_mode(mode));
            }
            if let Some(ref cb) = progress_callback {
                cb(len);
            }
        }
        res
    }

    fn is_local(&self) -> bool {
        true
    }

    /// One disk, however many panels and tabs are looking at it.
    fn fs_id(&self) -> String {
        "local".to_string()
    }

    fn display_name(&self) -> Option<String> {
        Some(crate::hostname())
    }

    fn get_icon(&self, path: &str) -> String {
        let path_norm = path.replace('\\', "/");
        let home_path = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default()
            .replace('\\', "/");

        if path == "~" || path_norm == home_path {
            "/com/icecommander/gtk/at-home.svg".to_string()
        } else if path_norm == "/" || path_norm.is_empty() {
            "/com/icecommander/gtk/home.svg".to_string()
        } else {
            #[cfg(feature = "gtk")]
            let matched_mount = {
                let monitor = gtk::gio::VolumeMonitor::get();
                let mut matched = false;
                for mount in monitor.mounts() {
                    if let Some(m_path) = mount.root().path() {
                        if path_norm == m_path.to_string_lossy().to_string() {
                            matched = true;
                            break;
                        }
                    }
                }
                matched
            };
            #[cfg(not(feature = "gtk"))]
            let matched_mount = false;

            if matched_mount {
                "/com/icecommander/gtk/ssd.svg".to_string()
            } else {
                #[cfg(target_os = "windows")]
                let is_win_drive = {
                    let trimmed = path_norm.trim_end_matches('/');
                    trimmed.len() == 2 && trimmed.as_bytes()[1] == b':'
                };
                #[cfg(not(target_os = "windows"))]
                let is_win_drive = false;
                if is_win_drive {
                    "/com/icecommander/gtk/ssd.svg".to_string()
                } else {
                    "/com/icecommander/gtk/folder.svg".to_string()
                }
            }
        }
    }

    async fn read_at(&self, path: String, offset: u64, len: usize) -> Result<Vec<u8>, AppError> {
        fs_local::read_local_file_chunk(&PathBuf::from(path), offset, len)
            .await
            .map_err(AppError::Other)
    }

    async fn write_at(&self, path: String, offset: u64, data: Vec<u8>) -> Result<(), AppError> {
        fs_local::write_local_file_chunk(&PathBuf::from(path), offset, &data)
            .await
            .map_err(AppError::Other)
    }

    async fn set_file_length(&self, path: String, len: u64) -> Result<(), AppError> {
        fs_local::set_local_file_length(&PathBuf::from(path), len)
            .await
            .map_err(AppError::Other)
    }

    fn supports_offset_io(&self) -> bool {
        true
    }

    async fn get_permissions(&self, path: String) -> Result<u32, AppError> {
        let path = sanitize_windows_path(std::path::Path::new(&path));
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(&path).map_err(AppError::from)?;
            Ok(metadata.mode())
        }
        #[cfg(not(unix))]
        {
            let metadata = std::fs::metadata(&path).map_err(AppError::from)?;
            if metadata.is_dir() {
                Ok(0o777)
            } else {
                Ok(0o666)
            }
        }
    }

    fn supports_permissions(&self) -> bool {
        true
    }

    async fn set_permissions(&self, path: String, permissions: u32) -> Result<(), AppError> {
        let path = sanitize_windows_path(std::path::Path::new(&path));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(permissions))
                .map_err(AppError::from)?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = permissions;
            Ok(())
        }
    }
}

#[cfg(feature = "gtk")]
impl LocalFileSystemRpc {
    pub fn thumbnail_into(&self, display_path: &str, pic: &gtk::Picture) {
        use gtk::prelude::*;
        let show_thumbs = self
            .config
            .get::<bool>("ui.show_thumbnails")
            .unwrap_or(true);
        if !show_thumbs {
            return;
        }
        let Some(file) = get_local_thumbnail(display_path) else {
            return;
        };
        let Some(path_buf) = file.path() else {
            return;
        };
        let path_str = path_buf.to_string_lossy().to_string();

        pic.set_widget_name(&path_str);
        let mut already_pending = false;
        PENDING_PICS.with(|p| {
            let mut map = p.borrow_mut();
            let waiters = map.entry(path_str.clone()).or_insert_with(|| {
                already_pending = false;
                Vec::new()
            });
            already_pending = !waiters.is_empty();
            waiters.push(pic.downgrade());
        });
        if already_pending {
            return;
        }

        tokio::spawn(async move {
            let _permit = match get_bg_semaphore().acquire().await {
                Ok(p) => p,
                Err(_) => return,
            };
            let path_decode = path_str.clone();
            let res = tokio::task::spawn_blocking(move || {
                let ext = path_decode.rsplit('.').next().unwrap_or("").to_lowercase();
                let is_raw = matches!(
                    ext.as_str(),
                    "nef" | "cr2" | "cr3" | "arw" | "dng" | "raf" | "orf" | "rw2" | "pef"
                );
                if is_raw {
                    extract_raw_thumbnail(&path_decode)
                        .and_then(|jpg| {
                            let gb = gtk::glib::Bytes::from(&jpg);
                            let stream = gtk::gio::MemoryInputStream::from_bytes(&gb);
                            gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
                                &stream,
                                256,
                                256,
                                true,
                                gtk::gio::Cancellable::NONE,
                            )
                            .ok()
                        })
                        .and_then(|pb| pb.save_to_bufferv("png", &[]).ok())
                        .unwrap_or_default()
                } else {
                    gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&path_decode, 256, 256, true)
                        .ok()
                        .and_then(|pb| pb.save_to_bufferv("png", &[]).ok())
                        .unwrap_or_default()
                }
            })
            .await;

            if let Ok(bytes) = res {
                let path = path_str;
                gtk::glib::idle_add(move || {
                    let waiters = PENDING_PICS
                        .with(|p| p.borrow_mut().remove(&path))
                        .unwrap_or_default();
                    if !bytes.is_empty() {
                        for weak_pic in waiters {
                            if let Some(pic) = weak_pic.upgrade() {
                                if pic.widget_name() == path {
                                    let gb = gtk::glib::Bytes::from(&bytes);
                                    if let Ok(tex) = gtk::gdk::Texture::from_bytes(&gb) {
                                        pic.set_paintable(Some(&tex));
                                    }
                                }
                            }
                        }
                    }
                    gtk::glib::ControlFlow::Break
                });
            }
        });
    }
}

fn sanitize_windows_path(path: &std::path::Path) -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        let p_str = path.to_string_lossy();
        if p_str.starts_with('/') && p_str.len() >= 3 && p_str.chars().nth(2) == Some(':') {
            return std::path::PathBuf::from(&p_str[1..]);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_date_returns_nonzero_timestamp() {
        let ts = parse_date_str("2024-01-15 10:30:00");
        assert!(ts > 0, "expected nonzero timestamp, got {}", ts);
    }

    #[test]
    fn known_date_has_expected_approximate_range() {
        let ts = parse_date_str("2024-01-01 00:00:00");
        assert!(ts > 1_700_000_000 && ts < 1_800_000_000, "ts={}", ts);
    }

    #[test]
    fn empty_string_returns_zero() {
        assert_eq!(parse_date_str(""), 0);
    }

    #[test]
    fn invalid_format_returns_zero() {
        assert_eq!(parse_date_str("not a date"), 0);
    }

    #[test]
    fn invalid_month_returns_zero() {
        assert_eq!(parse_date_str("2024-13-01 00:00:00"), 0);
    }

    #[test]
    fn invalid_day_returns_zero() {
        assert_eq!(parse_date_str("2024-01-32 00:00:00"), 0);
    }

    #[test]
    fn later_date_has_larger_timestamp() {
        let earlier = parse_date_str("2023-01-01 00:00:00");
        let later = parse_date_str("2024-01-01 00:00:00");
        assert!(later > earlier);
    }

    #[test]
    fn iso_format_emitted_by_the_listing_round_trips() {
        use chrono::TimeZone;
        let dt = chrono::Local.timestamp_opt(1_718_452_800, 0).unwrap();
        let rendered = dt.format("%Y-%m-%d %H:%M:%S").to_string();
        assert_eq!(parse_date_str(&rendered), 1_718_452_800);
    }

    #[test]
    fn iso_date_without_seconds_returns_zero() {
        assert_eq!(parse_date_str("2024-01-15 10:30"), 0);
    }
}
