use common::AppError;
use fm_core::rpc::FileSystemRpc;
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const PROGRESS_THROTTLE_MS: u64 = 100;

pub struct ThrottleTimer {
    last: std::time::Instant,
}

impl ThrottleTimer {
    pub fn new() -> Self {
        Self { last: std::time::Instant::now() }
    }

    pub fn ready(&mut self) -> bool {
        let now = std::time::Instant::now();
        if now.duration_since(self.last) >= std::time::Duration::from_millis(PROGRESS_THROTTLE_MS) {
            self.last = now;
            true
        } else {
            false
        }
    }
}

pub fn join_path(parent: &str, child: &str) -> String {
    if parent.ends_with('/') || parent.is_empty() {
        format!("{}{}", parent, child)
    } else {
        format!("{}/{}", parent, child)
    }
}

pub fn mode_of(meta: &std::fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(meta.mode())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

#[derive(Clone, Debug)]
pub struct TransferItem {
    pub src_path: String,
    pub relative_path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: Option<u32>,
}

pub fn retarget_top_level(
    items: &mut [TransferItem],
    renames: &std::collections::HashMap<String, String>,
) {
    for item in items.iter_mut() {
        let (top, rest) = match item.relative_path.split_once('/') {
            Some((t, r)) => (t.to_string(), Some(r.to_string())),
            None => (item.relative_path.clone(), None),
        };
        let Some(new_top) = renames.get(&top) else {
            continue;
        };
        item.relative_path = match &rest {
            Some(r) => format!("{new_top}/{r}"),
            None => new_top.clone(),
        };
        if rest.is_none() {
            item.name = new_top.clone();
        }
    }
}

#[derive(Clone, Debug)]
pub enum CopyMessage {
    FileStart { name: String, size: u64 },
    Progress { file_bytes: u64, total_bytes_copied: u64 },
    FileDone,
    FileSkipped,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ErrorAction {
    Retry,
    Skip,
    Abort,
}

pub struct ErrorRequest {
    pub file: String,
    pub message: String,
    pub reply: tokio::sync::oneshot::Sender<ErrorAction>,
}

pub fn request_error_decision_blocking(
    error_tx: &tokio::sync::mpsc::UnboundedSender<ErrorRequest>,
    file: &str,
    message: String,
) -> ErrorAction {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    if error_tx.send(ErrorRequest { file: file.to_string(), message, reply: reply_tx }).is_err() {
        return ErrorAction::Abort;
    }
    reply_rx.blocking_recv().unwrap_or(ErrorAction::Abort)
}

pub async fn request_error_decision_async(
    error_tx: &tokio::sync::mpsc::UnboundedSender<ErrorRequest>,
    file: &str,
    message: String,
) -> ErrorAction {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    if error_tx.send(ErrorRequest { file: file.to_string(), message, reply: reply_tx }).is_err() {
        return ErrorAction::Abort;
    }
    reply_rx.await.unwrap_or(ErrorAction::Abort)
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}


pub async fn read_file_async(
    provider: std::rc::Rc<dyn FileSystemRpc>,
    path: String,
    progress_callback: Rc<dyn Fn(u64)>,
) -> Result<Vec<u8>, AppError> {
    provider
        .read_file(path, Some(Box::new(move |bytes_read| progress_callback(bytes_read))))
        .await
        .map_err(AppError::from)
}

pub async fn write_file_async(
    provider: std::rc::Rc<dyn FileSystemRpc>,
    path: String,
    content: Vec<u8>,
    permissions: Option<u32>,
    progress_callback: Rc<dyn Fn(u64)>,
) -> Result<(), AppError> {
    provider
        .write_file(path, content, permissions, Some(Box::new(move |bytes_written| progress_callback(bytes_written))))
        .await
        .map_err(AppError::from)
}

pub async fn list_dir_async(
    provider: std::rc::Rc<dyn FileSystemRpc>,
    path: String,
) -> Result<Vec<fm_core::rpc::RemoteFileEntry>, AppError> {
    provider.list_dir(path).await.map_err(AppError::from)
}

pub async fn create_dir_async(
    provider: std::rc::Rc<dyn FileSystemRpc>,
    parent_path: String,
    dir_name: String,
    permissions: Option<u32>,
) -> Result<(), AppError> {
    provider.create_directory(parent_path, dir_name, permissions).await.map_err(AppError::from)
}

pub async fn scan_items(
    provider: std::rc::Rc<dyn FileSystemRpc>,
    selected_items: Vec<(String, bool, u64, Option<u32>)>,
    src_parent: String,
    on_update: impl Fn(usize, usize, u64) + 'static,
    cancellation_flag: Arc<AtomicBool>,
) -> Result<Vec<TransferItem>, AppError> {
    let provider_is_local = provider.is_local()
        && !src_parent.contains(".zip")
        && !src_parent.contains(".tar")
        && !src_parent.contains(".tgz")
        && !src_parent.contains(".tbz");


    if provider_is_local {
        let src_parent_c = src_parent.clone();
        let selected_items_c = selected_items.clone();
        let cancellation_flag_c = cancellation_flag.clone();

        #[derive(Debug)]
        enum ScanMessage {
            Progress { files: usize, dirs: usize, bytes: u64 },
            Done(Vec<TransferItem>),
        }

        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        tokio::task::spawn_blocking(move || {
            struct LocalScanner {
                cancellation_flag: Arc<AtomicBool>,
                tx: tokio::sync::mpsc::Sender<ScanMessage>,
                items: Vec<TransferItem>,
                total_files: usize,
                total_dirs: usize,
                total_bytes: u64,
                last_update: std::time::Instant,
            }

            impl LocalScanner {
                fn maybe_send_progress(&mut self) {
                    let now = std::time::Instant::now();
                    if now.duration_since(self.last_update) >= std::time::Duration::from_millis(PROGRESS_THROTTLE_MS) {
                        let _ = self.tx.blocking_send(ScanMessage::Progress {
                            files: self.total_files,
                            dirs: self.total_dirs,
                            bytes: self.total_bytes,
                        });
                        self.last_update = now;
                    }
                }

                fn scan(
                    &mut self,
                    src_path: std::path::PathBuf,
                    relative_path: std::path::PathBuf,
                    name: String,
                ) -> Result<(), std::io::Error> {
                    if self.cancellation_flag.load(Ordering::Relaxed) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "Cancelled",
                        ));
                    }
                    let permissions = std::fs::metadata(&src_path).ok().as_ref().and_then(mode_of);

                    self.items.push(TransferItem {
                        src_path: src_path.to_string_lossy().to_string(),
                        relative_path: relative_path.to_string_lossy().to_string(),
                        name,
                        is_dir: true,
                        size: 0,
                        permissions,
                    });
                    self.total_dirs += 1;
                    self.maybe_send_progress();

                    for entry in std::fs::read_dir(&src_path)? {
                        if self.cancellation_flag.load(Ordering::Relaxed) {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Interrupted,
                                "Cancelled",
                            ));
                        }
                        let entry = entry?;
                        let file_name = entry.file_name().to_string_lossy().to_string();
                        let file_type = entry.file_type()?;
                        if file_type.is_dir() {
                            self.scan(
                                entry.path(),
                                relative_path.join(&file_name),
                                file_name,
                            )?;
                        } else {
                            let metadata = entry.metadata()?;
                            let permissions = mode_of(&metadata);

                            self.items.push(TransferItem {
                                src_path: entry.path().to_string_lossy().to_string(),
                                relative_path: relative_path
                                    .join(&file_name)
                                    .to_string_lossy()
                                    .to_string(),
                                name: file_name,
                                is_dir: false,
                                size: metadata.len(),
                                permissions,
                            });
                            self.total_files += 1;
                            self.total_bytes += metadata.len();
                            self.maybe_send_progress();
                        }
                    }
                    Ok(())
                }
            }

            let mut scanner = LocalScanner {
                cancellation_flag: cancellation_flag_c,
                tx,
                items: Vec::new(),
                total_files: 0,
                total_dirs: 0,
                total_bytes: 0,
                last_update: std::time::Instant::now(),
            };

            let mut err = None;
            for (name, is_dir, size, permissions) in selected_items_c {
                let src_path = std::path::PathBuf::from(&src_parent_c).join(&name);
                let relative_path = std::path::PathBuf::from(&name);
                if is_dir {
                    if let Err(e) = scanner.scan(src_path, relative_path, name) {
                        err = Some(e);
                        break;
                    }
                } else {
                    scanner.items.push(TransferItem {
                        src_path: src_path.to_string_lossy().to_string(),
                        relative_path: relative_path.to_string_lossy().to_string(),
                        name,
                        is_dir: false,
                        size,
                        permissions,
                    });
                    scanner.total_files += 1;
                    scanner.total_bytes += size;
                    scanner.maybe_send_progress();
                }
            }

            if err.is_none() && !scanner.cancellation_flag.load(Ordering::Relaxed) {
                let _ = scanner.tx.blocking_send(ScanMessage::Done(scanner.items));
            }
        });

        let mut items = Vec::new();
        while let Some(msg) = rx.recv().await {
            if cancellation_flag.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            match msg {
                ScanMessage::Progress { files, dirs, bytes } => {
                    on_update(files, dirs, bytes);
                }
                ScanMessage::Done(scanned_items) => {
                    items = scanned_items;
                }
            }
        }
        if cancellation_flag.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        Ok(items)
    } else {
        let mut items = Vec::new();
        let mut total_files = 0;
        let mut total_dirs = 0;
        let mut total_bytes = 0;
        let mut throttle = ThrottleTimer::new();

        let mut queue = Vec::new();
        for (name, is_dir, size, permissions) in selected_items {
            let src_path = join_path(&src_parent, &name);
            let relative_path = name.clone();
            if is_dir {
                queue.push((src_path, relative_path, name, permissions));
            } else {
                total_files += 1;
                total_bytes += size;
                items.push(TransferItem {
                    src_path,
                    relative_path,
                    name,
                    is_dir: false,
                    size,
                    permissions,
                });
            }
        }
        on_update(total_files, total_dirs, total_bytes);

        while let Some((src_dir, rel_dir, name, permissions)) = queue.pop() {
            if cancellation_flag.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            total_dirs += 1;

            if throttle.ready() {
                on_update(total_files, total_dirs, total_bytes);
            }

            items.push(TransferItem {
                src_path: src_dir.clone(),
                relative_path: rel_dir.clone(),
                name,
                is_dir: true,
                size: 0,
                permissions,
            });

            let entries = list_dir_async(provider.clone(), src_dir.clone()).await?;
            for entry in entries {
                let sub_src = join_path(&src_dir, &entry.name);
                let sub_rel = format!("{}/{}", rel_dir, entry.name);
                if entry.is_dir {
                    queue.push((sub_src, sub_rel, entry.name.clone(), entry.permissions));
                } else {
                    total_files += 1;
                    total_bytes += entry.size;

                    if throttle.ready() {
                        on_update(total_files, total_dirs, total_bytes);
                    }

                    items.push(TransferItem {
                        src_path: sub_src,
                        relative_path: sub_rel,
                        name: entry.name,
                        is_dir: false,
                        size: entry.size,
                        permissions: entry.permissions,
                    });
                }
            }
        }
        on_update(total_files, total_dirs, total_bytes);
        Ok(items)
    }
}

pub async fn execute_transfer(
    src_provider: std::rc::Rc<dyn FileSystemRpc>,
    dest_provider: std::rc::Rc<dyn FileSystemRpc>,
    items: Vec<TransferItem>,
    dest_parent: String,
    is_move: bool,
    progress_tx: std::sync::mpsc::Sender<CopyMessage>,
    error_tx: tokio::sync::mpsc::UnboundedSender<ErrorRequest>,
    cancellation_flag: Arc<AtomicBool>,
) -> Result<(), AppError> {
    let src_is_local = src_provider.is_local() && !items.is_empty();
    let dest_is_local = dest_provider.is_local();

    let mut skipped: Vec<String> = Vec::new();

    if src_is_local && dest_is_local {
        let items_c = items.clone();
        let cancellation_flag_c = cancellation_flag.clone();
        let progress_tx_c = progress_tx.clone();
        let dest_parent_c = dest_parent.clone();
        let error_tx_c = error_tx.clone();

        let join_handle = tokio::task::spawn_blocking(move || {
            let mut local_overall_copied_bytes = 0u64;
            let mut throttle = ThrottleTimer::new();
            let mut skipped_local: Vec<String> = Vec::new();

            let mut copy_one_file = |item: &TransferItem, dest_path: &str, base: u64| -> std::io::Result<u64> {
                let mut src_file = std::fs::File::open(&item.src_path)?;
                if let Some(parent) = std::path::Path::new(dest_path).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let mut dest_file = std::fs::File::create(dest_path)?;
                let mut file_copied = 0u64;
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    if cancellation_flag_c.load(Ordering::Relaxed) {
                        drop(dest_file);
                        let _ = std::fs::remove_file(dest_path);
                        return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Cancelled"));
                    }
                    let bytes_read = src_file.read(&mut buffer)?;
                    if bytes_read == 0 {
                        break;
                    }
                    dest_file.write_all(&buffer[..bytes_read])?;
                    file_copied += bytes_read as u64;
                    if throttle.ready() {
                        let _ = progress_tx_c.send(CopyMessage::Progress {
                            file_bytes: file_copied,
                            total_bytes_copied: base + file_copied,
                        });
                    }
                }
                drop(dest_file);
                Ok(file_copied)
            };

            for item in items_c.iter() {
                let dest_path = join_path(&dest_parent_c, &item.relative_path);
                loop {
                    if cancellation_flag_c.load(Ordering::Relaxed) {
                        return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Cancelled"));
                    }

                    let attempt: std::io::Result<()> = if item.is_dir {
                        let path = std::path::Path::new(&dest_path);
                        std::fs::create_dir_all(path).map(|()| {
                            #[cfg(unix)]
                            if let Some(mode) = item.permissions {
                                use std::os::unix::fs::PermissionsExt;
                                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
                            }
                        })
                    } else {
                        let _ = progress_tx_c.send(CopyMessage::FileStart {
                            name: item.name.clone(),
                            size: item.size,
                        });
                        copy_one_file(item, &dest_path, local_overall_copied_bytes).map(|copied| {
                            #[cfg(unix)]
                            if let Some(mode) = item.permissions {
                                use std::os::unix::fs::PermissionsExt;
                                let _ = std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(mode));
                            }
                            let _ = progress_tx_c.send(CopyMessage::Progress {
                                file_bytes: copied,
                                total_bytes_copied: local_overall_copied_bytes + copied,
                            });
                            local_overall_copied_bytes += copied;
                            let _ = progress_tx_c.send(CopyMessage::FileDone);
                        })
                    };

                    match attempt {
                        Ok(()) => break,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Err(e),
                        Err(e) => match request_error_decision_blocking(&error_tx_c, &item.name, e.to_string()) {
                            ErrorAction::Retry => continue,
                            ErrorAction::Skip => {
                                skipped_local.push(item.relative_path.clone());
                                if !item.is_dir {
                                    let _ = progress_tx_c.send(CopyMessage::FileSkipped);
                                }
                                break;
                            }
                            ErrorAction::Abort => {
                                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Cancelled"));
                            }
                        },
                    }
                }
            }
            Ok::<Vec<String>, std::io::Error>(skipped_local)
        });

        skipped = join_handle
            .await
            .map_err(|e| AppError::Other(format!("Blocking task panicked: {}", e)))?
            .map_err(AppError::from)?;
    } else {
        let mut overall_copied_bytes = 0u64;
        for item in items.iter() {
            let dest_path = join_path(&dest_parent, &item.relative_path);
            loop {
                if cancellation_flag.load(Ordering::Relaxed) {
                    return Err(AppError::Cancelled);
                }

                let attempt: Result<(), AppError> = if item.is_dir {
                    let parent = if let Some(idx) = dest_path.rfind('/') {
                        dest_path[..idx].to_string()
                    } else {
                        String::new()
                    };
                    create_dir_async(dest_provider.clone(), parent, item.name.clone(), item.permissions).await
                } else {
                    let _ = progress_tx.send(CopyMessage::FileStart {
                        name: item.name.clone(),
                        size: item.size,
                    });

                    let progress_tx_c = progress_tx.clone();
                    let overall_copied_bytes_c = overall_copied_bytes;
                    let read_throttle = Rc::new(std::cell::RefCell::new(ThrottleTimer::new()));
                    let read_throttle_c = read_throttle.clone();
                    let read_res = read_file_async(
                        src_provider.clone(),
                        item.src_path.clone(),
                        Rc::new(move |bytes_read| {
                            if read_throttle_c.borrow_mut().ready() {
                                let _ = progress_tx_c.send(CopyMessage::Progress {
                                    file_bytes: bytes_read,
                                    total_bytes_copied: overall_copied_bytes_c + bytes_read,
                                });
                            }
                        }),
                    )
                    .await;

                    match read_res {
                        Err(e) => Err(e),
                        Ok(content) => {
                            if cancellation_flag.load(Ordering::Relaxed) {
                                return Err(AppError::Cancelled);
                            }
                            let progress_tx_c2 = progress_tx.clone();
                            let overall_copied_bytes_c2 = overall_copied_bytes;
                            let write_throttle = Rc::new(std::cell::RefCell::new(ThrottleTimer::new()));
                            let write_throttle_c = write_throttle.clone();
                            write_file_async(
                                dest_provider.clone(),
                                dest_path.clone(),
                                content,
                                item.permissions,
                                Rc::new(move |bytes_written| {
                                    if write_throttle_c.borrow_mut().ready() {
                                        let _ = progress_tx_c2.send(CopyMessage::Progress {
                                            file_bytes: bytes_written,
                                            total_bytes_copied: overall_copied_bytes_c2 + bytes_written,
                                        });
                                    }
                                }),
                            )
                            .await
                            .map(|()| {
                                let _ = progress_tx.send(CopyMessage::Progress {
                                    file_bytes: item.size,
                                    total_bytes_copied: overall_copied_bytes + item.size,
                                });
                                overall_copied_bytes += item.size;
                                let _ = progress_tx.send(CopyMessage::FileDone);
                            })
                        }
                    }
                };

                match attempt {
                    Ok(()) => break,
                    Err(AppError::Cancelled) => return Err(AppError::Cancelled),
                    Err(e) => match request_error_decision_async(&error_tx, &item.name, e.to_string()).await {
                        ErrorAction::Retry => continue,
                        ErrorAction::Skip => {
                            skipped.push(item.relative_path.clone());
                            if !dest_is_local && !item.is_dir {
                                dest_provider.delete_entries(vec![dest_path.clone()]).await.ok();
                            }
                            if !item.is_dir {
                                let _ = progress_tx.send(CopyMessage::FileSkipped);
                            }
                            break;
                        }
                        ErrorAction::Abort => {
                            if !dest_is_local && !item.is_dir {
                                dest_provider.delete_entries(vec![dest_path.clone()]).await.ok();
                            }
                            return Err(AppError::Cancelled);
                        }
                    },
                }
            }
        }
    }

    if is_move {
        let mut paths_to_delete = Vec::new();
        for item in items.iter().rev() {
            if cancellation_flag.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            let rel = &item.relative_path;
            let is_skipped = skipped.iter().any(|s| s == rel);
            let holds_skipped =
                item.is_dir && skipped.iter().any(|s| s.starts_with(&format!("{}/", rel)));
            if is_skipped || holds_skipped {
                continue;
            }
            paths_to_delete.push(item.src_path.clone());
        }

        if !paths_to_delete.is_empty() {
            src_provider.delete_entries(paths_to_delete).await.map_err(AppError::from)?;
        }
    }

    Ok(())
}


pub type ProviderFactory = Box<dyn FnOnce() -> Rc<dyn FileSystemRpc> + Send>;

#[allow(clippy::too_many_arguments)]
pub async fn execute_transfer_offthread(
    src_factory: ProviderFactory,
    dest_factory: ProviderFactory,
    items: Vec<TransferItem>,
    dest_parent: String,
    is_move: bool,
    progress_tx: std::sync::mpsc::Sender<CopyMessage>,
    error_tx: tokio::sync::mpsc::UnboundedSender<ErrorRequest>,
    cancellation_flag: Arc<AtomicBool>,
) -> Result<(), AppError> {
    let handle = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| AppError::Other(format!("transfer runtime: {e}")))?;
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async move {
            let src_provider = src_factory();
            let dest_provider = dest_factory();
            execute_transfer(
                src_provider,
                dest_provider,
                items,
                dest_parent,
                is_move,
                progress_tx,
                error_tx,
                cancellation_flag,
            )
            .await
        })
    });
    handle
        .await
        .map_err(|e| AppError::Other(format!("transfer task: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(src: &str, rel: &str, name: &str, is_dir: bool) -> TransferItem {
        TransferItem {
            src_path: src.to_string(),
            relative_path: rel.to_string(),
            name: name.to_string(),
            is_dir,
            size: 0,
            permissions: None,
        }
    }

    fn renames(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn retargeting_a_file_never_touches_what_it_reads() {
        let mut items = [item("/docs/notes.txt", "notes.txt", "notes.txt", false)];
        retarget_top_level(&mut items, &renames(&[("notes.txt", "notes (1).txt")]));
        assert_eq!(items[0].src_path, "/docs/notes.txt");
        assert_eq!(items[0].relative_path, "notes (1).txt");
        assert_eq!(items[0].name, "notes (1).txt");
    }

    #[test]
    fn a_folder_is_renamed_and_its_children_follow_without_being_renamed() {
        let mut items = [
            item("/docs/proj", "proj", "proj", true),
            item("/docs/proj/a.txt", "proj/a.txt", "a.txt", false),
            item("/docs/proj/sub", "proj/sub", "sub", true),
            item("/docs/proj/sub/b.txt", "proj/sub/b.txt", "b.txt", false),
        ];
        retarget_top_level(&mut items, &renames(&[("proj", "proj (1)")]));
        assert_eq!(items[0].relative_path, "proj (1)");
        assert_eq!(items[0].name, "proj (1)");
        assert_eq!(items[1].relative_path, "proj (1)/a.txt");
        assert_eq!(items[1].name, "a.txt");
        assert_eq!(items[2].relative_path, "proj (1)/sub");
        assert_eq!(items[2].name, "sub");
        assert_eq!(items[3].relative_path, "proj (1)/sub/b.txt");
        assert_eq!(items[3].name, "b.txt");
        for it in &items {
            assert!(it.src_path == "/docs/proj" || it.src_path.starts_with("/docs/proj/"));
        }
    }

    #[test]
    fn an_item_outside_the_map_is_left_alone() {
        let mut items = [item("/docs/other.txt", "other.txt", "other.txt", false)];
        retarget_top_level(&mut items, &renames(&[("notes.txt", "notes (1).txt")]));
        assert_eq!(items[0].relative_path, "other.txt");
        assert_eq!(items[0].name, "other.txt");
    }

    #[test]
    fn only_the_first_segment_is_matched() {
        let mut items = [item("/docs/a/notes.txt", "a/notes.txt", "notes.txt", false)];
        retarget_top_level(&mut items, &renames(&[("notes.txt", "notes (1).txt")]));
        assert_eq!(items[0].relative_path, "a/notes.txt", "a nested namesake must not be hit");
        assert_eq!(items[0].name, "notes.txt");
    }

    #[test]
    fn an_empty_map_changes_nothing() {
        let mut items = [item("/docs/notes.txt", "notes.txt", "notes.txt", false)];
        retarget_top_level(&mut items, &std::collections::HashMap::new());
        assert_eq!(items[0].relative_path, "notes.txt");
        assert_eq!(items[0].name, "notes.txt");
    }
}
