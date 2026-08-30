use common::AppError;
use fm_core::rpc::FileSystemRpc;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use chrono::TimeZone;

#[derive(Clone, Debug)]
pub struct ArchiveEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: Option<u32>,
    pub modified: u64,
}

pub struct ArchiveFileSystemRpc {
    pub relative_path_in_parent: String,
    pub parent_rpc: Rc<dyn FileSystemRpc>,
    pub entries: Rc<RefCell<Option<Vec<ArchiveEntry>>>>,
    pub archive_data: Rc<RefCell<Option<Vec<u8>>>>,
}

impl ArchiveFileSystemRpc {
    pub fn new(relative_path_in_parent: String, parent_rpc: Rc<dyn FileSystemRpc>) -> Self {
        Self {
            relative_path_in_parent,
            parent_rpc,
            entries: Rc::new(RefCell::new(None)),
            archive_data: Rc::new(RefCell::new(None)),
        }
    }
}

fn is_tar_format_from_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".tar.gz")
        || n.ends_with(".tgz")
        || n.ends_with(".tar.bz2")
        || n.ends_with(".tbz2")
        || n.ends_with(".tbz")
        || n.ends_with(".tar")
}

fn is_gzip_data(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b
}

fn is_bzip2_data(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..3] == b"BZh" && data[3].is_ascii_digit() && data[3] != b'0'
}

fn tar_reader_mem(data: &[u8]) -> Box<dyn std::io::Read + '_> {
    if is_gzip_data(data) {
        Box::new(flate2::read::GzDecoder::new(data))
    } else if is_bzip2_data(data) {
        Box::new(bzip2::read::MultiBzDecoder::new(data))
    } else {
        Box::new(data)
    }
}

fn scan_archive_from_memory(
    data: &[u8],
    _archive_path: &str,
    is_tar: bool,
) -> Result<Vec<ArchiveEntry>, AppError> {
    if is_tar {
        scan_tar_memory(data)
    } else {
        scan_zip_memory(data)
    }
}

fn scan_zip_memory(data: &[u8]) -> Result<Vec<ArchiveEntry>, AppError> {
    let reader = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| AppError::Other(e.to_string()))?;
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let name = entry.name().replace('\\', "/");
            let permissions = entry.unix_mode();
            
            let modified = {
                let zip_dt = entry.last_modified();
                let year = zip_dt.year() as i32;
                let month = zip_dt.month() as u32;
                let day = zip_dt.day() as u32;
                let hour = zip_dt.hour() as u32;
                let minute = zip_dt.minute() as u32;
                let second = zip_dt.second() as u32;
                
                chrono::NaiveDate::from_ymd_opt(year, month, day)
                    .and_then(|d| d.and_hms_opt(hour, minute, second))
                    .and_then(|ndt| chrono::Local.from_local_datetime(&ndt).single())
                    .map(|dt| dt.timestamp() as u64)
                    .unwrap_or(0)
            };

            entries.push(ArchiveEntry {
                name,
                is_dir: entry.is_dir() || entry.name().ends_with('/'),
                size: entry.size(),
                permissions,
                modified,
            });
        }
    }
    Ok(entries)
}

fn scan_tar_memory(data: &[u8]) -> Result<Vec<ArchiveEntry>, AppError> {
    let mut entries = Vec::new();
    let mut archive = tar::Archive::new(tar_reader_mem(data));
    if let Ok(tar_entries) = archive.entries() {
        for entry_res in tar_entries {
            if let Ok(entry) = entry_res {
                if let Ok(p) = entry.path() {
                    let name = p.to_string_lossy().replace('\\', "/");
                    let is_dir = entry.header().entry_type().is_dir();
                    let permissions = entry.header().mode().ok();
                    let modified = entry.header().mtime().unwrap_or(0);
                    entries.push(ArchiveEntry {
                        name,
                        is_dir,
                        size: entry.size(),
                        permissions,
                        modified,
                    });
                }
            }
        }
    }
    Ok(entries)
}

fn read_file_from_archive_memory(
    data: &[u8],
    internal_file: &str,
    is_tar: bool,
) -> Result<Vec<u8>, AppError> {
    let internal_file_norm = internal_file.replace('\\', "/");
    let internal_file_norm = internal_file_norm.trim_start_matches('/');

    if is_tar {
        let mut tar = tar::Archive::new(tar_reader_mem(data));
        for entry_res in tar.entries().map_err(AppError::from)? {
            let mut entry = entry_res.map_err(AppError::from)?;
            let path = entry.path().map_err(AppError::from)?;
            let entry_name = path.to_string_lossy().replace('\\', "/");
            if entry_name.trim_start_matches('/') == internal_file_norm {
                if entry.header().entry_type().is_dir() {
                    return Err(AppError::Other("Is a directory".to_string()));
                }
                let mut content = Vec::with_capacity(entry.size() as usize);
                std::io::copy(&mut entry, &mut content).map_err(AppError::from)?;
                return Ok(content);
            }
        }
    } else {
        let reader = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(reader).map_err(|e| AppError::Other(e.to_string()))?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| AppError::Other(e.to_string()))?;
            let entry_name = entry.name().replace('\\', "/");
            if entry_name.trim_start_matches('/') == internal_file_norm {
                if entry.is_dir() {
                    return Err(AppError::Other("Is a directory".to_string()));
                }
                let mut content = Vec::with_capacity(entry.size() as usize);
                std::io::copy(&mut entry, &mut content).map_err(AppError::from)?;
                return Ok(content);
            }
        }
    }
    Err(AppError::Other(format!("File not found in archive: {}", internal_file)))
}

impl ArchiveFileSystemRpc {
    async fn ensure_loaded(
        entries_cell: Rc<RefCell<Option<Vec<ArchiveEntry>>>>,
        archive_data_cell: Rc<RefCell<Option<Vec<u8>>>>,
        parent_rpc: Rc<dyn FileSystemRpc>,
        rel_path: String,
    ) -> Result<(Vec<ArchiveEntry>, Vec<u8>), AppError> {
        crate::utils::log_debug(&format!(
            "DEBUG_ARCHIVE: ensure_loaded starting for rel_path={}",
            rel_path
        ));

        let cached = {
            let entries_borrow = entries_cell.borrow();
            let data_borrow = archive_data_cell.borrow();
            if let (Some(entries), Some(data)) = (&*entries_borrow, &*data_borrow) {
                Some((entries.clone(), data.clone()))
            } else {
                None
            }
        };

        if let Some(res) = cached {
            crate::utils::log_debug("DEBUG_ARCHIVE: returning cached entries & data");
            return Ok(res);
        }

        crate::utils::log_debug("DEBUG_ARCHIVE: cache empty. Calling parent_rpc.read_file");
        let data = match parent_rpc.read_file(rel_path.clone(), None).await {
            Ok(d) => {
                crate::utils::log_debug(&format!(
                    "DEBUG_ARCHIVE: successfully read archive file from parent provider, size={}",
                    d.len()
                ));
                d
            }
            Err(e) => {
                crate::utils::log_debug(&format!("DEBUG_ARCHIVE: ERROR: parent_rpc.read_file failed: {}", e));
                return Err(e);
            }
        };

        let is_tar = is_tar_format_from_name(&rel_path);

        crate::utils::log_debug("DEBUG_ARCHIVE: spawning blocking scan task");
        let (scanned, data) = tokio::task::spawn_blocking(move || {
            let effective = if is_tar && (is_gzip_data(&data) || is_bzip2_data(&data)) {
                let mut dec = tar_reader_mem(&data);
                let mut out = Vec::new();
                std::io::Read::read_to_end(&mut dec, &mut out).map_err(AppError::from)?;
                out
            } else {
                data
            };
            let entries = scan_archive_from_memory(&effective, &rel_path, is_tar)?;
            Ok::<_, AppError>((entries, effective))
        })
        .await
        .map_err(|e| AppError::Other(format!("blocking task error: {:?}", e)))??;

        crate::utils::log_debug(&format!(
            "DEBUG_ARCHIVE: scan succeeded. Found {} entries",
            scanned.len()
        ));

        *entries_cell.borrow_mut() = Some(scanned.clone());
        *archive_data_cell.borrow_mut() = Some(data.clone());

        Ok((scanned, data))
    }
}

#[async_trait::async_trait(?Send)]
impl fm_core::rpc::FileSystemRpc for ArchiveFileSystemRpc {
    fn get_icon(&self, path: &str) -> String {
        let path_norm = path.replace('\\', "/");
        if !path_norm.is_empty() && path_norm != "/" {
            return "/com/icecommander/gtk/folder.svg".to_string();
        }
        "/com/icecommander/gtk/file.svg".to_string()
    }

    #[cfg(feature = "gtk")]
    fn get_icon_svg(&self, path: &str) -> Option<String> {
        let path_norm = path.replace('\\', "/");
        if !path_norm.is_empty() && path_norm != "/" {
            return None;
        }
        let name = self.relative_path_in_parent.rsplit('/').next().unwrap_or("");
        let lower = name.to_lowercase();
        let label = if is_tar_format_from_name(&lower) {
            "TAR"
        } else if lower.ends_with(".zip") {
            "ZIP"
        } else {
            return None;
        };
        Some(gtk_fm_ui::icon_generator::generate_svg_icon(
            label,
            gtk_fm_ui::icon_generator::FileType::Archive,
            80,
        ))
    }

    async fn create_directory(
        &self,
        _parent_path: String,
        _dir_name: String,
        _permissions: Option<u32>,
    ) -> Result<(), AppError> {
        Err(AppError::Other("Archive filesystem is read-only".to_string()))
    }

    async fn rename_entry(&self, _path: String, _new_path: String) -> Result<(), AppError> {
        Err(AppError::Other("Archive filesystem is read-only".to_string()))
    }

    fn request_file_download(&self, file_path: String, _transfer_id: uuid::Uuid) {
        let entries_cell = self.entries.clone();
        let archive_data_cell = self.archive_data.clone();
        let parent_rpc = self.parent_rpc.clone();
        let rel_path = self.relative_path_in_parent.clone();

        crate::spawn_local(async move {
            let is_tar = is_tar_format_from_name(&rel_path);
            let load_res = Self::ensure_loaded(
                entries_cell,
                archive_data_cell,
                parent_rpc,
                rel_path,
            )
            .await;

            if let Ok((_, data)) = load_res {
                let file_path_clone = file_path.clone();
                let res = tokio::task::spawn_blocking(move || {
                    read_file_from_archive_memory(&data, &file_path_clone, is_tar)
                })
                .await;

                if let Ok(Ok(content)) = res {
                    let mut temp_dir = std::env::temp_dir();
                    let file_name = Path::new(&file_path).file_name().unwrap_or_default();
                    temp_dir.push(file_name);

                    if tokio::fs::write(&temp_dir, content).await.is_ok() {
                        let temp_path_str = temp_dir.to_string_lossy().to_string();
                        #[cfg(target_os = "linux")]
                        let _ = std::process::Command::new("xdg-open")
                            .arg(&temp_path_str)
                            .spawn();
                        #[cfg(target_os = "macos")]
                        let _ = std::process::Command::new("open")
                            .arg(&temp_path_str)
                            .spawn();
                        #[cfg(target_os = "windows")]
                        let _ = std::process::Command::new("cmd")
                            .args(&["/c", "start", "", &temp_path_str])
                            .spawn();
                    }
                }
            }
        });
    }

    async fn read_file(
        &self,
        path: String,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<Vec<u8>, AppError> {
        let entries_cell = self.entries.clone();
        let archive_data_cell = self.archive_data.clone();
        let parent_rpc = self.parent_rpc.clone();
        let rel_path = self.relative_path_in_parent.clone();
        let is_tar = is_tar_format_from_name(&rel_path);

        let (_, data) = Self::ensure_loaded(
            entries_cell,
            archive_data_cell,
            parent_rpc,
            rel_path,
        )
        .await?;
        let res = read_file_from_archive_memory(&data, &path, is_tar)?;

        if let Some(ref cb) = progress_callback {
            cb(res.len() as u64);
        }
        Ok(res)
    }

    async fn write_file(
        &self,
        _path: String,
        _content: Vec<u8>,
        _permissions: Option<u32>,
        _progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<(), AppError> {
        Err(AppError::Other("Archive filesystem is read-only".to_string()))
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn trigger_file_upload(
        &self,
        _target_path: String,
        _file_name: String,
        _local_file_path: std::path::PathBuf,
        _transfer_id: uuid::Uuid,
    ) {
    }

    async fn list_dir(&self, path: String) -> Result<Vec<fm_core::rpc::RemoteFileEntry>, AppError> {
        let internal_dir = path.replace('\\', "/");
        let internal_dir = internal_dir
            .trim_start_matches('/')
            .trim_end_matches('/')
            .to_string();

        let entries_cell = self.entries.clone();
        let archive_data_cell = self.archive_data.clone();
        let parent_rpc = self.parent_rpc.clone();
        let rel_path = self.relative_path_in_parent.clone();

        let (entries_list, _) = Self::ensure_loaded(
            entries_cell,
            archive_data_cell,
            parent_rpc,
            rel_path,
        )
        .await?;

        {
                    let mut dirs = std::collections::HashSet::new();
                    let mut files = std::collections::HashMap::new();

                    for entry in &entries_list {
                        let name = entry.name.trim_start_matches('/').trim_end_matches('/');
                        if name.is_empty() {
                            continue;
                        }

                        if internal_dir.is_empty() {
                            let parts: Vec<&str> = name.split('/').collect();
                            if parts.len() > 1 && !(parts.len() == 2 && parts[1].is_empty()) {
                                dirs.insert(parts[0].to_string());
                            } else {
                                if entry.is_dir {
                                    dirs.insert(parts[0].to_string());
                                } else {
                                    files.insert(parts[0].to_string(), entry.size);
                                }
                            }
                        } else {
                            let prefix = format!("{}/", internal_dir);
                            if name.starts_with(&prefix) {
                                let subpath = &name[prefix.len()..];
                                if subpath.is_empty() {
                                    continue;
                                }
                                let parts: Vec<&str> = subpath.split('/').collect();
                                if parts.len() > 1 && !(parts.len() == 2 && parts[1].is_empty()) {
                                    dirs.insert(parts[0].to_string());
                                } else {
                                    if entry.is_dir {
                                        dirs.insert(parts[0].to_string());
                                    } else {
                                        files.insert(parts[0].to_string(), entry.size);
                                    }
                                }
                            }
                        }
                    }

                    for d in &dirs {
                        files.remove(d);
                    }

                    let mut final_entries = Vec::new();
                    for d in dirs {
                        let full_entry_path = if internal_dir.is_empty() {
                            d.clone()
                        } else {
                            format!("{}/{}", internal_dir, d)
                        };
                        let full_entry_path_slash = format!("{}/", full_entry_path);
                        
                        let (permissions, modified) = if let Some(entry) = entries_list.iter().find(|e| {
                            let normalized = e.name.trim_start_matches('/').trim_end_matches('/');
                            normalized == full_entry_path || normalized == &full_entry_path_slash
                        }) {
                            (entry.permissions, entry.modified)
                        } else {
                            (Some(0o755), 0)
                        };

                        final_entries.push(fm_core::rpc::RemoteFileEntry {
                            name: d,
                            is_dir: true,
                            size: 0,
                            modified,
                            permissions,
                        });
                    }
                    for (f, size) in files {
                        let full_entry_path = if internal_dir.is_empty() {
                            f.clone()
                        } else {
                            format!("{}/{}", internal_dir, f)
                        };
                        
                        let (permissions, modified) = if let Some(entry) = entries_list.iter().find(|e| {
                            let normalized = e.name.trim_start_matches('/').trim_end_matches('/');
                            normalized == full_entry_path
                        }) {
                            (entry.permissions, entry.modified)
                        } else {
                            (None, 0)
                        };

                        final_entries.push(fm_core::rpc::RemoteFileEntry {
                            name: f,
                            is_dir: false,
                            size,
                            modified,
                            permissions,
                        });
                    }

                    final_entries.sort_by(|a, b| a.name.cmp(&b.name));
                    Ok(final_entries)
                }
    }

    async fn delete_entries(&self, _paths: Vec<String>) -> Result<(), AppError> {
        Err(AppError::Other("Archive filesystem is read-only".to_string()))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_and_read_zip_from_memory() {
        let mut buffer = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buffer);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::FileOptions::default();

            zip.start_file("foo.txt", options).unwrap();
            use std::io::Write;
            zip.write_all(b"Hello in-memory zip!").unwrap();

            zip.add_directory("bar/", options).unwrap();

            zip.start_file("bar/baz.txt", options).unwrap();
            zip.write_all(b"Nested file content").unwrap();

            zip.finish().unwrap();
        }

        let entries = scan_archive_from_memory(&buffer, "test.zip", false).unwrap();
        assert_eq!(entries.len(), 3);

        let foo_entry = entries.iter().find(|e| e.name == "foo.txt").unwrap();
        assert!(!foo_entry.is_dir);
        assert_eq!(foo_entry.size, 20);

        let bar_entry = entries.iter().find(|e| e.name == "bar/").unwrap();
        assert!(bar_entry.is_dir);

        let baz_entry = entries.iter().find(|e| e.name == "bar/baz.txt").unwrap();
        assert!(!baz_entry.is_dir);
        assert_eq!(baz_entry.size, 19);

        let foo_content = read_file_from_archive_memory(&buffer, "foo.txt", false).unwrap();
        assert_eq!(foo_content, b"Hello in-memory zip!");

        let baz_content = read_file_from_archive_memory(&buffer, "/bar/baz.txt", false).unwrap();
        assert_eq!(baz_content, b"Nested file content");
    }

    #[test]
    fn test_scan_and_read_tar_from_memory() {
        let mut buffer = Vec::new();
        {
            let mut tar_builder = tar::Builder::new(&mut buffer);

            let mut header = tar::Header::new_gnu();
            header.set_size(14);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(0o644);
            tar_builder
                .append_data(&mut header, "hello.txt", &b"Hello from TAR"[..])
                .unwrap();

            tar_builder.append_dir("my_folder/", ".").unwrap();
            tar_builder.finish().unwrap();
        }

        let entries = scan_archive_from_memory(&buffer, "test.tar", true).unwrap();
        assert_eq!(entries.len(), 2);

        let hello_entry = entries.iter().find(|e| e.name == "hello.txt").unwrap();
        assert!(!hello_entry.is_dir);
        assert_eq!(hello_entry.size, 14);
        assert_eq!(hello_entry.permissions, Some(0o644));

        let folder_entry = entries.iter().find(|e| e.name == "my_folder/").unwrap();
        assert!(folder_entry.is_dir);

        let hello_content = read_file_from_archive_memory(&buffer, "hello.txt", true).unwrap();
        assert_eq!(hello_content, b"Hello from TAR");
    }

    #[test]
    fn tar_gz_is_tar_format() {
        assert!(is_tar_format_from_name("archive.tar.bz2"));
        assert!(is_tar_format_from_name("archive.tbz2"));
        assert!(is_tar_format_from_name("archive.tbz"));
        assert!(is_tar_format_from_name("archive.tar.gz"));
    }

    #[test]
    fn tgz_is_tar_format() {
        assert!(is_tar_format_from_name("backup.tgz"));
    }

    #[test]
    fn tar_is_tar_format() {
        assert!(is_tar_format_from_name("files.tar"));
    }

    #[test]
    fn zip_is_not_tar_format() {
        assert!(!is_tar_format_from_name("archive.zip"));
    }

    #[test]
    fn tar_format_check_is_case_insensitive() {
        assert!(is_tar_format_from_name("ARCHIVE.TAR.GZ"));
        assert!(is_tar_format_from_name("Backup.TGZ"));
    }

    #[test]
    fn unknown_extension_is_not_tar() {
        assert!(!is_tar_format_from_name("file.7z"));
        assert!(!is_tar_format_from_name("file.rar"));
    }

    #[test]
    fn read_directory_entry_from_zip_returns_error() {
        let mut buffer = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buffer);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = zip::write::FileOptions::default();
            zip.add_directory("mydir/", opts).unwrap();
            zip.finish().unwrap();
        }
        let result = read_file_from_archive_memory(&buffer, "mydir/", false);
        assert!(result.is_err());
    }

    #[test]
    fn read_nonexistent_file_from_zip_returns_error() {
        let mut buffer = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buffer);
            let mut zip = zip::ZipWriter::new(cursor);
            use std::io::Write;
            zip.start_file("real.txt", zip::write::FileOptions::default()).unwrap();
            zip.write_all(b"data").unwrap();
            zip.finish().unwrap();
        }
        let result = read_file_from_archive_memory(&buffer, "ghost.txt", false);
        assert!(result.is_err());
    }

    #[test]
    fn scan_zip_empty_archive_returns_empty() {
        let mut buffer = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buffer);
            let mut zip = zip::ZipWriter::new(cursor);
            zip.finish().unwrap();
        }
        let entries = scan_zip_memory(&buffer).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn scan_invalid_bytes_returns_error() {
        let result = scan_zip_memory(b"this is not a zip file");
        assert!(result.is_err());
    }

    #[test]
    fn bzip2_magic_needs_the_level_digit() {
        assert!(is_bzip2_data(b"BZh9somecompressedbytes"));
        assert!(is_bzip2_data(b"BZh1somecompressedbytes"));
        assert!(!is_bzip2_data(b"BZhistory.txt"));
        assert!(!is_bzip2_data(b"BZh0nope"));
        assert!(!is_bzip2_data(b"BZh"));
        assert!(!is_bzip2_data(b""));
    }

    #[test]
    fn gzip_magic_is_recognised_and_not_confused_with_bzip2() {
        assert!(is_gzip_data(&[0x1f, 0x8b, 0x08, 0x00]));
        assert!(!is_gzip_data(b"BZh9"));
        assert!(!is_bzip2_data(&[0x1f, 0x8b, 0x08, 0x00]));
    }
}
