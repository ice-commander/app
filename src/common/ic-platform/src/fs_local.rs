use ic_model::EntryInfo;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc as tokio_mpsc;

pub fn sanitize_windows_path(path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let p_str = path.to_string_lossy();
        if p_str.starts_with('/') && p_str.len() >= 3 && p_str.chars().nth(2) == Some(':') {
            return PathBuf::from(&p_str[1..]);
        }
    }
    path.to_path_buf()
}

pub fn resolve_and_sandbox_path(base_path: &Path, user_path: &str) -> Option<PathBuf> {
    let rel_path = user_path.trim_start_matches('/');
    for component in Path::new(rel_path).components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    Some(base_path.join(rel_path))
}

pub type DirWithDate = (String, String);
pub type FileWithDate = (String, u64, String);
pub type DirectoryListing = (Vec<DirWithDate>, Vec<FileWithDate>);

pub fn list_local_directory(target_dir: &Path) -> DirectoryListing {
    let mut directories = Vec::new();
    let mut files = Vec::new();

    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn GetLogicalDrives() -> u32;
        }

        let p_str = target_dir.to_string_lossy();
        if p_str == "/" {
            let mask = unsafe { GetLogicalDrives() };
            for b in 0..26 {
                if (mask & (1 << b)) != 0 {
                    let drive = format!("{}:\\", (b'A' + b) as char);
                    directories.push((drive, "".to_string()));
                }
            }
            return (directories, files);
        }
    }

    let safe_target = sanitize_windows_path(target_dir);

    if let Ok(read_dir) = std::fs::read_dir(&safe_target) {
        for entry_result in read_dir.flatten() {
            if let Ok(file_type) = entry_result.file_type() {
                let name = entry_result.file_name().to_string_lossy().to_string();
                let date_str = entry_result
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|t| {
                        let datetime: chrono::DateTime<chrono::Local> = t.into();
                        datetime.format("%Y-%m-%d %H:%M:%S").to_string()
                    })
                    .unwrap_or_default();
                if file_type.is_dir() {
                    directories.push((name, date_str));
                } else {
                    let size = entry_result.metadata().map(|m| m.len()).unwrap_or(0);
                    files.push((name, size, date_str));
                }
            }
        }
    }
    (directories, files)
}

pub fn get_local_metadata(target_entry: &Path) -> Option<EntryInfo> {
    let safe_target = sanitize_windows_path(target_entry);
    if let Ok(meta) = std::fs::metadata(&safe_target) {
        let name = safe_target
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let entry_type = if meta.is_dir() { "Directory" } else { "File" };
        Some(EntryInfo {
            name,
            entry_type: entry_type.to_string(),
            size: meta.len(),
        })
    } else {
        None
    }
}

pub async fn create_local_directory(target_dir: &Path) -> Result<(), String> {
    let safe_target = sanitize_windows_path(target_dir);
    tokio::fs::create_dir(&safe_target)
        .await
        .map_err(|e| e.to_string())
}

pub async fn delete_local_entry(target_entry: &Path) -> Result<(), String> {
    let safe_target = sanitize_windows_path(target_entry);
    if safe_target.is_dir() {
        tokio::fs::remove_dir_all(&safe_target).await
    } else {
        tokio::fs::remove_file(&safe_target).await
    }
    .map_err(|e| e.to_string())
}

pub async fn rename_local_entry(target_entry: &Path, new_entry: &Path) -> Result<(), String> {
    let safe_target = sanitize_windows_path(target_entry);
    let safe_new = sanitize_windows_path(new_entry);

    if let Some(parent) = safe_new.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tokio::fs::rename(&safe_target, &safe_new)
        .await
        .map_err(|e| e.to_string())
}

pub async fn duplicate_local_entry(source: &Path, dest: &Path) -> Result<(), String> {
    let safe_source = sanitize_windows_path(source);
    let safe_dest = sanitize_windows_path(dest);

    if !safe_source.is_dir() {
        return tokio::fs::copy(&safe_source, &safe_dest)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string());
    }

    tokio::task::spawn_blocking(move || {
        fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
            std::fs::create_dir_all(dst)?;
            for entry in std::fs::read_dir(src)? {
                let entry = entry?;
                let target = dst.join(entry.file_name());
                if entry.file_type()?.is_dir() {
                    copy_tree(&entry.path(), &target)?;
                } else {
                    std::fs::copy(entry.path(), &target)?;
                }
            }
            Ok(())
        }
        copy_tree(&safe_source, &safe_dest)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

pub async fn start_local_download(
    target_file: PathBuf,
    tx: tokio_mpsc::Sender<Result<Vec<u8>, String>>,
) -> Result<u64, String> {
    let safe_target = sanitize_windows_path(&target_file);
    match tokio::fs::File::open(&safe_target).await {
        Ok(mut file) => {
            let total_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 16 * 1024];
                loop {
                    match file.read(&mut buffer).await {
                        Ok(0) => {
                            let _ = tx.send(Ok(Vec::new())).await; // EOF signal
                            break;
                        }
                        Ok(n) => {
                            if tx.send(Ok(buffer[..n].to_vec())).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string())).await;
                            break;
                        }
                    }
                }
            });
            Ok(total_size)
        }
        Err(e) => Err(e.to_string()),
    }
}

pub async fn start_local_upload(
    target_file: PathBuf,
    mut rx: tokio_mpsc::Receiver<(u64, Vec<u8>)>,
) -> Result<(), String> {
    let safe_target = sanitize_windows_path(&target_file);
    let file_res = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&safe_target)
        .await;

    match file_res {
        Ok(mut file) => {
            tokio::spawn(async move {
                use tokio::io::AsyncSeekExt;
                while let Some((offset, data)) = rx.recv().await {
                    let _ = file.seek(std::io::SeekFrom::Start(offset)).await;
                    let _ = file.write_all(&data).await;
                }
            });
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

pub async fn create_local_file(target_path: &Path) -> Result<(), String> {
    let safe_target = sanitize_windows_path(target_path);
    tokio::fs::File::create(&safe_target)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub async fn write_local_file_chunk(
    target_path: &Path,
    offset: u64,
    data: &[u8],
) -> Result<(), String> {
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};
    let safe_target = sanitize_windows_path(target_path);
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&safe_target)
        .await
        .map_err(|e| e.to_string())?;

    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|e| e.to_string())?;

    file.write_all(data).await.map_err(|e| e.to_string())?;
    file.flush().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;


    #[test]
    fn sanitize_non_windows_path_is_identity() {
        #[cfg(not(target_os = "windows"))]
        {
            let p = Path::new("/home/user/docs");
            assert_eq!(sanitize_windows_path(p), PathBuf::from("/home/user/docs"));
        }
    }

    #[cfg(target_os = "windows")]
    mod sanitize_windows {
        use super::*;

        #[test]
        fn strips_leading_slash_before_drive() {
            let p = Path::new("/C:\\Users\\foo");
            assert_eq!(sanitize_windows_path(p), PathBuf::from("C:\\Users\\foo"));
        }

        #[test]
        fn normal_windows_path_unchanged() {
            let p = Path::new("C:\\Users\\foo");
            assert_eq!(sanitize_windows_path(p), PathBuf::from("C:\\Users\\foo"));
        }
    }


    #[test]
    fn sandbox_normal_path_resolves() {
        let base = Path::new("/base");
        let result = resolve_and_sandbox_path(base, "sub/dir/file.txt");
        assert_eq!(result, Some(PathBuf::from("/base/sub/dir/file.txt")));
    }

    #[test]
    fn sandbox_strips_leading_slash_from_user_path() {
        let base = Path::new("/base");
        let result = resolve_and_sandbox_path(base, "/sub/file.txt");
        assert_eq!(result, Some(PathBuf::from("/base/sub/file.txt")));
    }

    #[test]
    fn sandbox_blocks_parent_dir_traversal() {
        let base = Path::new("/base");
        assert!(resolve_and_sandbox_path(base, "../etc/passwd").is_none());
    }

    #[test]
    fn sandbox_blocks_nested_traversal() {
        let base = Path::new("/base");
        assert!(resolve_and_sandbox_path(base, "sub/../../etc/passwd").is_none());
    }

    #[test]
    fn sandbox_empty_path_returns_base() {
        let base = Path::new("/base");
        let result = resolve_and_sandbox_path(base, "");
        assert_eq!(result, Some(PathBuf::from("/base")));
    }

    #[test]
    fn sandbox_allows_explicit_current_dir() {
        let base = Path::new("/base");
        let result = resolve_and_sandbox_path(base, "./sub/file.txt");
        assert_eq!(result, Some(PathBuf::from("/base/./sub/file.txt")));
    }

    #[cfg(windows)]
    #[test]
    fn sandbox_blocks_drive_prefix_and_root() {
        let base = Path::new("C:\\base");
        assert!(resolve_and_sandbox_path(base, "C:\\Windows\\System32").is_none());
        assert!(resolve_and_sandbox_path(base, "\\\\server\\share").is_none());
        assert!(resolve_and_sandbox_path(base, "\\Windows").is_none());
    }


    #[test]
    fn list_empty_dir_returns_empty() {
        let dir = tempdir().unwrap();
        let (dirs, files) = list_local_directory(dir.path());
        assert!(dirs.is_empty());
        assert!(files.is_empty());
    }

    #[test]
    fn list_dir_separates_dirs_and_files() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        fs::write(dir.path().join("file.txt"), b"hello").unwrap();

        let (dirs, files) = list_local_directory(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].0, "subdir");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "file.txt");
        assert_eq!(files[0].1, 5);
    }

    #[test]
    fn list_nonexistent_dir_returns_empty() {
        let (dirs, files) = list_local_directory(Path::new("/this/does/not/exist/xyz123"));
        assert!(dirs.is_empty());
        assert!(files.is_empty());
    }


    #[test]
    fn metadata_of_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, b"hello").unwrap();

        let info = get_local_metadata(&file_path).unwrap();
        assert_eq!(info.name, "test.txt");
        assert_eq!(info.entry_type, "File");
        assert_eq!(info.size, 5);
    }

    #[test]
    fn metadata_of_directory() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("mydir");
        fs::create_dir(&sub).unwrap();

        let info = get_local_metadata(&sub).unwrap();
        assert_eq!(info.name, "mydir");
        assert_eq!(info.entry_type, "Directory");
    }

    #[test]
    fn metadata_of_missing_entry_is_none() {
        let info = get_local_metadata(Path::new("/no/such/file.txt"));
        assert!(info.is_none());
    }


    #[tokio::test]
    async fn create_directory_succeeds() {
        let dir = tempdir().unwrap();
        let new_dir = dir.path().join("new_folder");
        assert!(create_local_directory(&new_dir).await.is_ok());
        assert!(new_dir.is_dir());
    }

    #[tokio::test]
    async fn create_directory_fails_if_parent_missing() {
        let result = create_local_directory(Path::new("/tmp/nonexistent_xyz/sub")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_file_succeeds() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("to_delete.txt");
        fs::write(&file, b"bye").unwrap();

        assert!(delete_local_entry(&file).await.is_ok());
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn delete_directory_recursively() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("subdir");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("inner.txt"), b"x").unwrap();

        assert!(delete_local_entry(&sub).await.is_ok());
        assert!(!sub.exists());
    }

    #[tokio::test]
    async fn delete_nonexistent_entry_returns_error() {
        let result = delete_local_entry(Path::new("/no/such/file_xyz.txt")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rename_file_succeeds() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, b"data").unwrap();

        assert!(rename_local_entry(&src, &dst).await.is_ok());
        assert!(!src.exists());
        assert!(dst.exists());
    }

    #[tokio::test]
    async fn rename_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("file.txt");
        let dst = dir.path().join("nested/deep/file.txt");
        fs::write(&src, b"data").unwrap();

        assert!(rename_local_entry(&src, &dst).await.is_ok());
        assert!(dst.exists());
    }

    #[tokio::test]
    async fn create_local_file_creates_empty_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("empty.txt");

        assert!(create_local_file(&file).await.is_ok());
        assert!(file.exists());
        assert_eq!(fs::read(&file).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn write_chunk_at_offset_zero() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chunk.bin");

        assert!(write_local_file_chunk(&path, 0, b"hello").await.is_ok());
        let content = tokio::fs::read(&path).await.unwrap();
        assert_eq!(content, b"hello");
    }

    #[tokio::test]
    async fn write_chunk_at_nonzero_offset() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chunk.bin");
        tokio::fs::write(&path, b"0000000000").await.unwrap();

        assert!(write_local_file_chunk(&path, 5, b"world").await.is_ok());
        let content = tokio::fs::read(&path).await.unwrap();
        assert_eq!(&content[5..10], b"world");
    }

    #[tokio::test]
    async fn start_local_download_streams_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("data.bin");
        fs::write(&file, b"hello world").unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let size = start_local_download(file, tx).await.unwrap();
        assert_eq!(size, 11);

        let mut received = Vec::new();
        while let Some(chunk) = rx.recv().await {
            let data = chunk.unwrap();
            if data.is_empty() {
                break;
            } // EOF sentinel
            received.extend_from_slice(&data);
        }
        assert_eq!(received, b"hello world");
    }
}
