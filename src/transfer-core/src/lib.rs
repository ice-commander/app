use common::AppError;
use fm_core::rpc::FileSystemRpc;
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const PROGRESS_THROTTLE_MS: u64 = 40;

pub struct ThrottleTimer {
    last: Option<std::time::Instant>,
}

impl ThrottleTimer {
    pub fn new() -> Self {
        Self { last: None }
    }

    pub fn ready(&mut self) -> bool {
        let now = std::time::Instant::now();
        let due = match self.last {
            None => true,
            Some(last) => {
                now.duration_since(last) >= std::time::Duration::from_millis(PROGRESS_THROTTLE_MS)
            }
        };
        if due {
            self.last = Some(now);
        }
        due
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
    FileStart {
        name: String,
        size: u64,
    },
    Progress {
        file_bytes: u64,
        total_bytes_copied: u64,
    },
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
    if error_tx
        .send(ErrorRequest {
            file: file.to_string(),
            message,
            reply: reply_tx,
        })
        .is_err()
    {
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
    if error_tx
        .send(ErrorRequest {
            file: file.to_string(),
            message,
            reply: reply_tx,
        })
        .is_err()
    {
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
        .read_file(
            path,
            Some(Box::new(move |bytes_read| progress_callback(bytes_read))),
        )
        .await
        .map_err(AppError::from)
}

async fn on_this_disk(path: &str) -> Option<Box<dyn fm_core::rpc::ReadStream>> {
    struct FromDisk(std::fs::File);

    #[async_trait::async_trait(?Send)]
    impl fm_core::rpc::ReadStream for FromDisk {
        async fn fill(&mut self, into: &mut [u8]) -> Result<usize, AppError> {
            self.0.read(into).map_err(AppError::from)
        }
    }

    std::fs::File::open(path).ok().map(|file| {
        let boxed: Box<dyn fm_core::rpc::ReadStream> = Box::new(FromDisk(file));
        boxed
    })
}

pub const COPY_BUFFER_MAX: usize = 256 * 1024;

pub fn buffer_for(size: u64) -> usize {
    if size == 0 {
        return COPY_BUFFER_MAX;
    }
    (size as usize).min(COPY_BUFFER_MAX).max(1)
}

pub async fn pour(
    from: &Rc<dyn FileSystemRpc>,
    src_path: &str,
    into: &Rc<dyn FileSystemRpc>,
    dest_path: &str,
    size: u64,
    permissions: Option<u32>,
    report: Option<&dyn Fn(u64)>,
    cancellation_flag: &Arc<AtomicBool>,
) -> Option<Result<u64, AppError>> {
    let mut source = match from.on_disk(src_path.to_string()).await {
        Some(real) => on_this_disk(&real).await?,
        None => from.open_read(src_path.to_string()).await?,
    };
    let Some(mut sink) = into.open_write(dest_path.to_string(), permissions).await else {
        return None;
    };

    let mut carrying = vec![0u8; buffer_for(size)];
    let mut moved = 0u64;
    let mut throttle = ThrottleTimer::new();
    loop {
        if cancellation_flag.load(Ordering::Relaxed) {
            return Some(Err(AppError::Cancelled));
        }
        let got = match source.fill(&mut carrying).await {
            Ok(got) => got,
            Err(e) => return Some(Err(e)),
        };
        if got == 0 {
            break;
        }
        if let Err(e) = sink.put(&carrying[..got]).await {
            return Some(Err(e));
        }
        moved += got as u64;
        if let (Some(report), true) = (report, throttle.ready()) {
            report(moved);
        }
    }
    if let Err(e) = sink.finish().await {
        return Some(Err(e));
    }
    if let Some(report) = report {
        report(moved);
    }
    Some(Ok(moved))
}

pub async fn write_file_async(
    provider: std::rc::Rc<dyn FileSystemRpc>,
    path: String,
    content: Vec<u8>,
    permissions: Option<u32>,
    progress_callback: Rc<dyn Fn(u64)>,
) -> Result<(), AppError> {
    provider
        .write_file(
            path,
            content,
            permissions,
            Some(Box::new(move |bytes_written| {
                progress_callback(bytes_written)
            })),
        )
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
    provider
        .create_directory(parent_path, dir_name, permissions)
        .await
        .map_err(AppError::from)
}

pub async fn scan_items(
    provider: std::rc::Rc<dyn FileSystemRpc>,
    selected_items: Vec<(String, bool, u64, Option<u32>)>,
    src_parent: String,
    on_update: impl Fn(usize, usize, u64) + 'static,
    cancellation_flag: Arc<AtomicBool>,
) -> Result<Vec<TransferItem>, AppError> {
    let provider_is_local =
        provider.is_local() && !fm_core::plugin_fs::path_crosses_plugin_fs(&src_parent);

    if provider_is_local {
        let src_parent_c = src_parent.clone();
        let selected_items_c = selected_items.clone();
        let cancellation_flag_c = cancellation_flag.clone();

        #[derive(Debug)]
        enum ScanMessage {
            Progress {
                files: usize,
                dirs: usize,
                bytes: u64,
            },
            Done(Vec<TransferItem>),
            Failed(String),
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
                    if now.duration_since(self.last_update)
                        >= std::time::Duration::from_millis(PROGRESS_THROTTLE_MS)
                    {
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
                            self.scan(entry.path(), relative_path.join(&file_name), file_name)?;
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

            if scanner.cancellation_flag.load(Ordering::Relaxed) {
                return;
            }
            match err {
                Some(why) => {
                    let _ = scanner
                        .tx
                        .blocking_send(ScanMessage::Failed(why.to_string()));
                }
                None => {
                    let _ = scanner.tx.blocking_send(ScanMessage::Done(scanner.items));
                }
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
                ScanMessage::Failed(why) => return Err(AppError::Other(why)),
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
    let same_filesystem = !src_is_local
        && !dest_is_local
        && src_provider.connection_id().is_some()
        && src_provider.connection_id() == dest_provider.connection_id();

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
            let mut buffer = vec![0u8; COPY_BUFFER_MAX];

            let mut copy_one_file = |item: &TransferItem,
                                     dest_path: &str,
                                     base: u64,
                                     buffer: &mut [u8]|
             -> std::io::Result<u64> {
                let mut src_file = std::fs::File::open(&item.src_path)?;
                if let Some(parent) = std::path::Path::new(dest_path).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let mut dest_file = std::fs::File::create(dest_path)?;
                let mut file_copied = 0u64;
                loop {
                    if cancellation_flag_c.load(Ordering::Relaxed) {
                        drop(dest_file);
                        let _ = std::fs::remove_file(dest_path);
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "Cancelled",
                        ));
                    }
                    let bytes_read = src_file.read(buffer)?;
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
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "Cancelled",
                        ));
                    }

                    let attempt: std::io::Result<()> = if item.is_dir {
                        let path = std::path::Path::new(&dest_path);
                        std::fs::create_dir_all(path).map(|()| {
                            #[cfg(unix)]
                            if let Some(mode) = item.permissions {
                                use std::os::unix::fs::PermissionsExt;
                                let _ = std::fs::set_permissions(
                                    path,
                                    std::fs::Permissions::from_mode(mode),
                                );
                            }
                        })
                    } else {
                        let _ = progress_tx_c.send(CopyMessage::FileStart {
                            name: item.name.clone(),
                            size: item.size,
                        });
                        copy_one_file(item, &dest_path, local_overall_copied_bytes, &mut buffer)
                            .map(|copied| {
                                #[cfg(unix)]
                                if let Some(mode) = item.permissions {
                                    use std::os::unix::fs::PermissionsExt;
                                    let _ = std::fs::set_permissions(
                                        &dest_path,
                                        std::fs::Permissions::from_mode(mode),
                                    );
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
                        Err(e) => match request_error_decision_blocking(
                            &error_tx_c,
                            &item.name,
                            e.to_string(),
                        ) {
                            ErrorAction::Retry => continue,
                            ErrorAction::Skip => {
                                skipped_local.push(item.relative_path.clone());
                                if !item.is_dir {
                                    let _ = progress_tx_c.send(CopyMessage::FileSkipped);
                                }
                                break;
                            }
                            ErrorAction::Abort => {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::Interrupted,
                                    "Cancelled",
                                ));
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
                    create_dir_async(
                        dest_provider.clone(),
                        parent,
                        item.name.clone(),
                        item.permissions,
                    )
                    .await
                } else {
                    let _ = progress_tx.send(CopyMessage::FileStart {
                        name: item.name.clone(),
                        size: item.size,
                    });

                    if same_filesystem
                        && src_provider
                            .copy_within(item.src_path.clone(), dest_path.clone())
                            .await
                    {
                        let _ = progress_tx.send(CopyMessage::Progress {
                            file_bytes: item.size,
                            total_bytes_copied: overall_copied_bytes + item.size,
                        });
                        overall_copied_bytes += item.size;
                        let _ = progress_tx.send(CopyMessage::FileDone);
                        break;
                    }

                    let poured = {
                        let progress_tx_c = progress_tx.clone();
                        let base = overall_copied_bytes;
                        let report = move |moved: u64| {
                            let _ = progress_tx_c.send(CopyMessage::Progress {
                                file_bytes: moved,
                                total_bytes_copied: base + moved,
                            });
                        };
                        pour(
                            &src_provider,
                            &item.src_path,
                            &dest_provider,
                            &dest_path,
                            item.size,
                            item.permissions,
                            Some(&report),
                            &cancellation_flag,
                        )
                        .await
                    };
                    if let Some(outcome) = poured {
                        match outcome {
                            Ok(moved) => {
                                overall_copied_bytes += moved;
                                let _ = progress_tx.send(CopyMessage::FileDone);
                                break;
                            }
                            Err(AppError::Cancelled) => return Err(AppError::Cancelled),
                            Err(e) => {
                                match request_error_decision_async(
                                    &error_tx,
                                    &item.name,
                                    e.to_string(),
                                )
                                .await
                                {
                                    ErrorAction::Retry => continue,
                                    ErrorAction::Skip => {
                                        skipped.push(item.relative_path.clone());
                                        dest_provider
                                            .delete_entries(vec![dest_path.clone()])
                                            .await
                                            .ok();
                                        let _ = progress_tx.send(CopyMessage::FileSkipped);
                                        break;
                                    }
                                    ErrorAction::Abort => {
                                        dest_provider
                                            .delete_entries(vec![dest_path.clone()])
                                            .await
                                            .ok();
                                        return Err(AppError::Cancelled);
                                    }
                                }
                            }
                        }
                    }

                    let progress_tx_c = progress_tx.clone();
                    let overall_copied_bytes_c = overall_copied_bytes;
                    let read_throttle = Rc::new(std::cell::RefCell::new(ThrottleTimer::new()));
                    let read_throttle_c = read_throttle.clone();
                    let read_res = read_file_async(
                        src_provider.clone(),
                        item.src_path.clone(),
                        Rc::new(move |bytes_read| {
                            if read_throttle_c.borrow_mut().ready() {
                                let fetched = bytes_read / 2;
                                let _ = progress_tx_c.send(CopyMessage::Progress {
                                    file_bytes: fetched,
                                    total_bytes_copied: overall_copied_bytes_c + fetched,
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
                            let half = content.len() as u64 / 2;
                            let write_throttle =
                                Rc::new(std::cell::RefCell::new(ThrottleTimer::new()));
                            let write_throttle_c = write_throttle.clone();
                            write_file_async(
                                dest_provider.clone(),
                                dest_path.clone(),
                                content,
                                item.permissions,
                                Rc::new(move |bytes_written| {
                                    if write_throttle_c.borrow_mut().ready() {
                                        let stored = half + bytes_written / 2;
                                        let _ = progress_tx_c2.send(CopyMessage::Progress {
                                            file_bytes: stored,
                                            total_bytes_copied: overall_copied_bytes_c2 + stored,
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
                    Err(e) => {
                        match request_error_decision_async(&error_tx, &item.name, e.to_string())
                            .await
                        {
                            ErrorAction::Retry => continue,
                            ErrorAction::Skip => {
                                skipped.push(item.relative_path.clone());
                                if !dest_is_local && !item.is_dir {
                                    dest_provider
                                        .delete_entries(vec![dest_path.clone()])
                                        .await
                                        .ok();
                                }
                                if !item.is_dir {
                                    let _ = progress_tx.send(CopyMessage::FileSkipped);
                                }
                                break;
                            }
                            ErrorAction::Abort => {
                                if !dest_is_local && !item.is_dir {
                                    dest_provider
                                        .delete_entries(vec![dest_path.clone()])
                                        .await
                                        .ok();
                                }
                                return Err(AppError::Cancelled);
                            }
                        }
                    }
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
            src_provider
                .delete_entries(paths_to_delete)
                .await
                .map_err(AppError::from)?;
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
    use std::cell::{Cell, RefCell};

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

    struct Outcome {
        result: Result<(), AppError>,
        said: Vec<CopyMessage>,
    }

    impl Outcome {
        fn files_done(&self) -> usize {
            self.said
                .iter()
                .filter(|m| matches!(m, CopyMessage::FileDone))
                .count()
        }

        fn files_skipped(&self) -> usize {
            self.said
                .iter()
                .filter(|m| matches!(m, CopyMessage::FileSkipped))
                .count()
        }

        fn started(&self) -> Vec<String> {
            self.said
                .iter()
                .filter_map(|m| match m {
                    CopyMessage::FileStart { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect()
        }
    }

    fn file(name: &str, size: u64) -> TransferItem {
        TransferItem {
            src_path: format!("/from/{name}"),
            relative_path: name.to_string(),
            name: name.to_string(),
            is_dir: false,
            size,
            permissions: None,
        }
    }

    fn folder(name: &str) -> TransferItem {
        TransferItem {
            src_path: format!("/from/{name}"),
            relative_path: name.to_string(),
            name: name.to_string(),
            is_dir: true,
            size: 0,
            permissions: None,
        }
    }

    fn copying(
        from: Rc<dyn FileSystemRpc>,
        into: Rc<dyn FileSystemRpc>,
        items: Vec<TransferItem>,
        is_move: bool,
        answer: ErrorAction,
        cancellation_flag: Arc<AtomicBool>,
    ) -> Outcome {
        let (progress_tx, progress_rx) = std::sync::mpsc::channel();
        let (error_tx, mut error_rx) = tokio::sync::mpsc::unbounded_channel::<ErrorRequest>();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        let result = local.block_on(&rt, async move {
            let answering = tokio::task::spawn_local(async move {
                while let Some(asked) = error_rx.recv().await {
                    let _ = asked.reply.send(answer);
                }
            });
            let outcome = execute_transfer(
                from,
                into,
                items,
                "/to".to_string(),
                is_move,
                progress_tx,
                error_tx,
                cancellation_flag,
            )
            .await;
            answering.abort();
            outcome
        });
        Outcome {
            result,
            said: progress_rx.into_iter().collect(),
        }
    }

    #[derive(Default)]
    struct Told {
        files: RefCell<std::collections::HashMap<String, Vec<u8>>>,
        read_fails: RefCell<std::collections::HashMap<String, usize>>,
        write_fails: RefCell<std::collections::HashMap<String, usize>>,
        deleted: RefCell<Vec<String>>,
        delete_fails: Cell<bool>,
        made: RefCell<Vec<String>>,
    }

    impl Told {
        fn holding(files: &[(&str, &[u8])]) -> Rc<Self> {
            let told = Self::default();
            for (path, body) in files {
                told.files
                    .borrow_mut()
                    .insert((*path).to_string(), body.to_vec());
            }
            Rc::new(told)
        }

        fn empty() -> Rc<Self> {
            Rc::new(Self::default())
        }

        fn fail_reading(self: &Rc<Self>, path: &str, times: usize) {
            self.read_fails.borrow_mut().insert(path.to_string(), times);
        }

        fn fail_writing(self: &Rc<Self>, path: &str, times: usize) {
            self.write_fails
                .borrow_mut()
                .insert(path.to_string(), times);
        }

        fn owes(slot: &RefCell<std::collections::HashMap<String, usize>>, path: &str) -> bool {
            let mut owed = slot.borrow_mut();
            match owed.get_mut(path) {
                Some(0) | None => false,
                Some(left) => {
                    *left -= 1;
                    true
                }
            }
        }

        fn landed(&self, path: &str) -> Option<Vec<u8>> {
            self.files.borrow().get(path).cloned()
        }
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Told {
        async fn read_file(
            &self,
            path: String,
            progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<Vec<u8>, AppError> {
            if Self::owes(&self.read_fails, &path) {
                return Err(AppError::Other(format!("told to fail reading {path}")));
            }
            let body = self
                .files
                .borrow()
                .get(&path)
                .cloned()
                .ok_or_else(|| AppError::Other(format!("no such file {path}")))?;
            if let Some(cb) = progress_callback {
                cb(body.len() as u64);
            }
            Ok(body)
        }

        async fn write_file(
            &self,
            path: String,
            content: Vec<u8>,
            _permissions: Option<u32>,
            progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<(), AppError> {
            if Self::owes(&self.write_fails, &path) {
                return Err(AppError::Other(format!("told to fail writing {path}")));
            }
            if let Some(cb) = progress_callback {
                cb(content.len() as u64);
            }
            self.files.borrow_mut().insert(path, content);
            Ok(())
        }

        async fn create_directory(
            &self,
            parent_path: String,
            dir_name: String,
            _permissions: Option<u32>,
        ) -> Result<(), AppError> {
            self.made
                .borrow_mut()
                .push(join_path(&parent_path, &dir_name));
            Ok(())
        }

        async fn delete_entries(&self, paths: Vec<String>) -> Result<(), AppError> {
            if self.delete_fails.get() {
                return Err(AppError::Other("told to fail deleting".to_string()));
            }
            for path in paths {
                self.files.borrow_mut().remove(&path);
                self.deleted.borrow_mut().push(path);
            }
            Ok(())
        }
    }

    struct Remote {
        size: u64,
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Remote {
        async fn read_file(
            &self,
            _path: String,
            progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<Vec<u8>, AppError> {
            let body = vec![0u8; self.size as usize];
            if let Some(cb) = progress_callback {
                cb(body.len() as u64);
            }
            Ok(body)
        }

        async fn write_file(
            &self,
            _path: String,
            content: Vec<u8>,
            _permissions: Option<u32>,
            progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<(), AppError> {
            if let Some(cb) = progress_callback {
                cb(content.len() as u64);
            }
            Ok(())
        }
    }

    fn file_of(size: u64) -> TransferItem {
        TransferItem {
            src_path: "/remote/big.bin".to_string(),
            relative_path: "big.bin".to_string(),
            name: "big.bin".to_string(),
            is_dir: false,
            size,
            permissions: None,
        }
    }

    struct Chunked {
        size: u64,
        chunks: u64,
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Chunked {
        async fn read_file(
            &self,
            _path: String,
            progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<Vec<u8>, AppError> {
            let step = self.size / self.chunks;
            if let Some(cb) = progress_callback {
                for n in 1..=self.chunks {
                    std::thread::sleep(std::time::Duration::from_millis(PROGRESS_THROTTLE_MS + 20));
                    cb(step * n);
                }
            }
            Ok(vec![0u8; self.size as usize])
        }

        async fn write_file(
            &self,
            _path: String,
            content: Vec<u8>,
            _permissions: Option<u32>,
            progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<(), AppError> {
            let step = content.len() as u64 / self.chunks;
            if let Some(cb) = progress_callback {
                for n in 1..=self.chunks {
                    std::thread::sleep(std::time::Duration::from_millis(PROGRESS_THROTTLE_MS + 20));
                    cb(step * n);
                }
            }
            Ok(())
        }
    }

    fn transfer_between(
        src: Rc<dyn FileSystemRpc>,
        dest: Rc<dyn FileSystemRpc>,
        size: u64,
    ) -> Vec<CopyMessage> {
        let (progress_tx, progress_rx) = std::sync::mpsc::channel();
        let (error_tx, _error_rx) = tokio::sync::mpsc::unbounded_channel();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async {
            execute_transfer(
                src,
                dest,
                vec![file_of(size)],
                "/dest".to_string(),
                false,
                progress_tx,
                error_tx,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        });
        progress_rx.into_iter().collect()
    }

    #[test]
    fn a_file_that_is_read_then_written_never_goes_backwards() {
        let size = 1024 * 1024;
        let said = transfer_between(
            Rc::new(Chunked { size, chunks: 4 }),
            Rc::new(Chunked { size, chunks: 4 }),
            size,
        );

        let seen: Vec<u64> = said
            .iter()
            .filter_map(|m| match m {
                CopyMessage::Progress { file_bytes, .. } => Some(*file_bytes),
                _ => None,
            })
            .collect();

        assert!(seen.len() > 2, "a chunked transfer should report often");
        for pair in seen.windows(2) {
            assert!(
                pair[1] >= pair[0],
                "the bar jumped back from {} to {}: {seen:?}",
                pair[0],
                pair[1]
            );
        }
        assert_eq!(
            seen.last().copied(),
            Some(size),
            "the file ends at its full size"
        );
    }

    fn transfer_one(size: u64) -> Vec<CopyMessage> {
        let (progress_tx, progress_rx) = std::sync::mpsc::channel();
        let (error_tx, _error_rx) = tokio::sync::mpsc::unbounded_channel();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async {
            execute_transfer(
                Rc::new(Remote { size }),
                Rc::new(Remote { size }),
                vec![file_of(size)],
                "/dest".to_string(),
                false,
                progress_tx,
                error_tx,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        });
        progress_rx.into_iter().collect()
    }

    #[test]
    fn a_filesystem_that_reports_only_at_the_end_moves_the_bar_only_at_the_halves() {
        let size = 8 * 1024 * 1024;
        let said = transfer_one(size);

        let seen: Vec<u64> = said
            .iter()
            .filter_map(|m| match m {
                CopyMessage::Progress { file_bytes, .. } => Some(*file_bytes),
                _ => None,
            })
            .collect();

        assert!(
            seen.iter().all(|b| *b == size / 2 || *b == size),
            "nothing finer than the two halves can be known here; got {seen:?}"
        );
        assert_eq!(seen.last().copied(), Some(size), "and it ends at full");
    }

    struct OnlyAView {
        real: std::path::PathBuf,
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for OnlyAView {
        async fn on_disk(&self, _path: String) -> Option<String> {
            Some(self.real.to_string_lossy().to_string())
        }
    }

    struct Landing;

    struct LandedFile(std::fs::File);

    #[async_trait::async_trait(?Send)]
    impl fm_core::rpc::WriteStream for LandedFile {
        async fn put(&mut self, bytes: &[u8]) -> Result<(), AppError> {
            self.0.write_all(bytes).map_err(AppError::from)
        }
        async fn finish(self: Box<Self>) -> Result<(), AppError> {
            Ok(())
        }
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Landing {
        async fn open_write(
            &self,
            path: String,
            _permissions: Option<u32>,
        ) -> Option<Box<dyn fm_core::rpc::WriteStream>> {
            std::fs::File::create(path).ok().map(|f| {
                let boxed: Box<dyn fm_core::rpc::WriteStream> = Box::new(LandedFile(f));
                boxed
            })
        }
    }

    #[test]
    fn a_view_of_the_disk_is_copied_from_the_disk() {
        let here = std::env::temp_dir().join("ic-view-of-disk-probe");
        let _ = std::fs::create_dir_all(&here);
        let real = here.join("downloaded.bin");
        let size = 4 * 1024 * 1024usize;
        std::fs::write(&real, vec![3u8; size]).unwrap();
        let landing = here.join("landed");
        let _ = std::fs::create_dir_all(&landing);

        let (progress_tx, progress_rx) = std::sync::mpsc::channel();
        let (error_tx, _error_rx) = tokio::sync::mpsc::unbounded_channel();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async {
            execute_transfer(
                Rc::new(OnlyAView { real: real.clone() }),
                Rc::new(Landing),
                vec![TransferItem {
                    src_path: "downloaded.bin".to_string(),
                    relative_path: "downloaded.bin".to_string(),
                    name: "downloaded.bin".to_string(),
                    is_dir: false,
                    size: size as u64,
                    permissions: None,
                }],
                landing.to_string_lossy().to_string(),
                false,
                progress_tx,
                error_tx,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .expect("the copy goes through");
        });

        let moved: Vec<u64> = progress_rx
            .into_iter()
            .filter_map(|m| match m {
                CopyMessage::Progress { file_bytes, .. } => Some(file_bytes),
                _ => None,
            })
            .collect();
        assert_eq!(
            moved.last().copied(),
            Some(size as u64),
            "all of it crossed: {moved:?}"
        );
        let _ = std::fs::remove_dir_all(&here);
    }

    #[test]
    fn a_retried_file_is_copied_on_the_second_attempt() {
        let from = Told::holding(&[("/from/a.txt", b"hello")]);
        let into = Told::empty();
        into.fail_writing("/to/a.txt", 1);

        let done = copying(
            from,
            into.clone(),
            vec![file("a.txt", 5)],
            false,
            ErrorAction::Retry,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        assert_eq!(into.landed("/to/a.txt").as_deref(), Some(&b"hello"[..]));
        assert_eq!(done.files_done(), 1);
    }

    #[test]
    fn a_skipped_file_is_left_out_and_the_others_still_cross() {
        let from = Told::holding(&[
            ("/from/first.txt", b"one"),
            ("/from/bad.txt", b"two"),
            ("/from/last.txt", b"three"),
        ]);
        let into = Told::empty();
        into.fail_writing("/to/bad.txt", 99);

        let done = copying(
            from,
            into.clone(),
            vec![
                file("first.txt", 3),
                file("bad.txt", 3),
                file("last.txt", 5),
            ],
            false,
            ErrorAction::Skip,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        assert!(into.landed("/to/first.txt").is_some());
        assert!(
            into.landed("/to/bad.txt").is_none(),
            "the bad one is left out"
        );
        assert!(into.landed("/to/last.txt").is_some(), "and the rest follow");
        assert_eq!(done.files_skipped(), 1);
        assert_eq!(done.files_done(), 2);
    }

    #[test]
    fn aborting_stops_before_the_rest() {
        let from = Told::holding(&[("/from/bad.txt", b"one"), ("/from/after.txt", b"two")]);
        let into = Told::empty();
        into.fail_writing("/to/bad.txt", 99);

        let done = copying(
            from,
            into.clone(),
            vec![file("bad.txt", 3), file("after.txt", 3)],
            false,
            ErrorAction::Abort,
            Arc::new(AtomicBool::new(false)),
        );

        assert_eq!(done.result, Err(AppError::Cancelled));
        assert!(into.landed("/to/after.txt").is_none());
        assert_eq!(
            done.started(),
            vec!["bad.txt"],
            "the one after it is never begun"
        );
    }

    #[test]
    fn a_move_deletes_the_source_but_keeps_what_was_skipped() {
        let from = Told::holding(&[("/from/moved.txt", b"one"), ("/from/kept.txt", b"two")]);
        let into = Told::empty();
        into.fail_writing("/to/kept.txt", 99);

        let done = copying(
            from.clone(),
            into,
            vec![file("moved.txt", 3), file("kept.txt", 3)],
            true,
            ErrorAction::Skip,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        let gone = from.deleted.borrow().clone();
        assert!(gone.contains(&"/from/moved.txt".to_string()), "{gone:?}");
        assert!(
            !gone.contains(&"/from/kept.txt".to_string()),
            "a file that never arrived must not be deleted: {gone:?}"
        );
    }

    #[test]
    fn a_cancelled_copy_says_it_was_cancelled() {
        let from = Told::holding(&[("/from/a.txt", b"hello")]);
        let stop = Arc::new(AtomicBool::new(true));

        let done = copying(
            from,
            Told::empty(),
            vec![file("a.txt", 5)],
            false,
            ErrorAction::Abort,
            stop,
        );

        assert_eq!(done.result, Err(AppError::Cancelled));
        assert_eq!(done.files_done(), 0);
    }

    #[test]
    fn a_folder_is_made_before_what_it_holds() {
        let from = Told::holding(&[("/from/docs/inner.txt", b"x")]);
        let into = Told::empty();

        let mut inner = file("docs/inner.txt", 1);
        inner.src_path = "/from/docs/inner.txt".to_string();
        let done = copying(
            from,
            into.clone(),
            vec![folder("docs"), inner],
            false,
            ErrorAction::Abort,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        assert_eq!(into.made.borrow().clone(), vec!["/to/docs".to_string()]);
        assert!(into.landed("/to/docs/inner.txt").is_some());
    }

    #[test]
    fn a_source_that_will_not_be_read_can_be_skipped() {
        let from = Told::holding(&[("/from/a.txt", b"one"), ("/from/b.txt", b"two")]);
        from.fail_reading("/from/a.txt", 99);
        let into = Told::empty();

        let done = copying(
            from,
            into.clone(),
            vec![file("a.txt", 3), file("b.txt", 3)],
            false,
            ErrorAction::Skip,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        assert!(into.landed("/to/a.txt").is_none());
        assert!(into.landed("/to/b.txt").is_some());
    }

    #[test]
    fn copying_nothing_is_not_a_failure() {
        let done = copying(
            Told::empty(),
            Told::empty(),
            Vec::new(),
            false,
            ErrorAction::Abort,
            Arc::new(AtomicBool::new(false)),
        );
        assert!(done.result.is_ok());
        assert_eq!(done.files_done(), 0);
    }

    #[test]
    fn a_stream_with_nowhere_to_stream_to_still_copies() {
        struct Streams(Rc<Told>);

        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for Streams {
            async fn read_file(
                &self,
                path: String,
                progress: Option<Box<dyn Fn(u64) + 'static>>,
            ) -> Result<Vec<u8>, AppError> {
                self.0.read_file(path, progress).await
            }
            async fn open_read(&self, path: String) -> Option<Box<dyn fm_core::rpc::ReadStream>> {
                struct Held(Vec<u8>, usize);
                #[async_trait::async_trait(?Send)]
                impl fm_core::rpc::ReadStream for Held {
                    async fn fill(&mut self, into: &mut [u8]) -> Result<usize, AppError> {
                        let giving = (self.0.len() - self.1).min(into.len());
                        into[..giving].copy_from_slice(&self.0[self.1..self.1 + giving]);
                        self.1 += giving;
                        Ok(giving)
                    }
                }
                self.0.landed(&path).map(|body| {
                    let boxed: Box<dyn fm_core::rpc::ReadStream> = Box::new(Held(body, 0));
                    boxed
                })
            }
        }

        let held = Told::holding(&[("/from/a.txt", b"hello")]);
        let into = Told::empty();
        let done = copying(
            Rc::new(Streams(held)),
            into.clone(),
            vec![file("a.txt", 5)],
            false,
            ErrorAction::Abort,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        assert_eq!(
            into.landed("/to/a.txt").as_deref(),
            Some(&b"hello"[..]),
            "the whole-file path carried it instead"
        );
    }

    #[test]
    fn a_stream_that_breaks_part_way_fails_the_copy() {
        struct Breaks;

        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for Breaks {
            async fn open_read(&self, _path: String) -> Option<Box<dyn fm_core::rpc::ReadStream>> {
                struct Stopping(usize);
                #[async_trait::async_trait(?Send)]
                impl fm_core::rpc::ReadStream for Stopping {
                    async fn fill(&mut self, into: &mut [u8]) -> Result<usize, AppError> {
                        self.0 += 1;
                        if self.0 > 1 {
                            return Err(AppError::Other("the connection went".to_string()));
                        }
                        into[..4].copy_from_slice(b"part");
                        Ok(4)
                    }
                }
                Some(Box::new(Stopping(0)))
            }
        }

        struct Takes;
        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for Takes {
            async fn open_write(
                &self,
                _path: String,
                _permissions: Option<u32>,
            ) -> Option<Box<dyn fm_core::rpc::WriteStream>> {
                struct Sink;
                #[async_trait::async_trait(?Send)]
                impl fm_core::rpc::WriteStream for Sink {
                    async fn put(&mut self, _bytes: &[u8]) -> Result<(), AppError> {
                        Ok(())
                    }
                    async fn finish(self: Box<Self>) -> Result<(), AppError> {
                        Ok(())
                    }
                }
                Some(Box::new(Sink))
            }
        }

        let done = copying(
            Rc::new(Breaks),
            Rc::new(Takes),
            vec![file("a.txt", 1000)],
            false,
            ErrorAction::Abort,
            Arc::new(AtomicBool::new(false)),
        );

        assert_eq!(
            done.result,
            Err(AppError::Cancelled),
            "a broken transfer must not be reported as done"
        );
        assert_eq!(done.files_done(), 0);
    }

    #[test]
    fn a_server_that_can_copy_its_own_file_is_asked_first() {
        struct Itself {
            asked: RefCell<Vec<(String, String)>>,
            read_asked: Cell<bool>,
        }

        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for Itself {
            fn connection_id(&self) -> Option<String> {
                Some("one-and-the-same".to_string())
            }
            async fn copy_within(&self, from: String, to: String) -> bool {
                self.asked.borrow_mut().push((from, to));
                true
            }
            async fn read_file(
                &self,
                _path: String,
                _progress: Option<Box<dyn Fn(u64) + 'static>>,
            ) -> Result<Vec<u8>, AppError> {
                self.read_asked.set(true);
                Ok(Vec::new())
            }
        }

        let server = Rc::new(Itself {
            asked: RefCell::new(Vec::new()),
            read_asked: Cell::new(false),
        });
        let done = copying(
            server.clone(),
            server.clone(),
            vec![file("a.txt", 9)],
            false,
            ErrorAction::Abort,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        assert_eq!(
            server.asked.borrow().clone(),
            vec![("/from/a.txt".to_string(), "/to/a.txt".to_string())]
        );
        assert!(
            !server.read_asked.get(),
            "the bytes must not cross when the server copied it itself"
        );
        assert_eq!(done.files_done(), 1);
    }

    #[test]
    fn two_different_servers_are_not_asked_to_copy_within() {
        struct Named {
            id: &'static str,
            asked: Cell<bool>,
            files: Rc<Told>,
        }

        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for Named {
            fn connection_id(&self) -> Option<String> {
                Some(self.id.to_string())
            }
            async fn copy_within(&self, _from: String, _to: String) -> bool {
                self.asked.set(true);
                false
            }
            async fn read_file(
                &self,
                path: String,
                progress: Option<Box<dyn Fn(u64) + 'static>>,
            ) -> Result<Vec<u8>, AppError> {
                self.files.read_file(path, progress).await
            }
            async fn write_file(
                &self,
                path: String,
                content: Vec<u8>,
                permissions: Option<u32>,
                progress: Option<Box<dyn Fn(u64) + 'static>>,
            ) -> Result<(), AppError> {
                self.files
                    .write_file(path, content, permissions, progress)
                    .await
            }
        }

        let from = Rc::new(Named {
            id: "one",
            asked: Cell::new(false),
            files: Told::holding(&[("/from/a.txt", b"hello")]),
        });
        let landing = Told::empty();
        let into = Rc::new(Named {
            id: "another",
            asked: Cell::new(false),
            files: landing.clone(),
        });

        let done = copying(
            from.clone(),
            into,
            vec![file("a.txt", 5)],
            false,
            ErrorAction::Abort,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        assert!(
            !from.asked.get(),
            "a copy between two servers is not one server's own"
        );
        assert_eq!(landing.landed("/to/a.txt").as_deref(), Some(&b"hello"[..]));
    }

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn named(name: &str) -> Self {
            let at = std::env::temp_dir().join(format!("ic-scan-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&at);
            std::fs::create_dir_all(&at).expect("a folder to walk");
            Self(at)
        }

        fn file(&self, path: &str, body: &[u8]) -> &Self {
            let at = self.0.join(path);
            if let Some(parent) = at.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(at, body).unwrap();
            self
        }

        fn folder(&self, path: &str) -> &Self {
            std::fs::create_dir_all(self.0.join(path)).unwrap();
            self
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct OnThisDisk;

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for OnThisDisk {
        fn is_local(&self) -> bool {
            true
        }
    }

    fn scanning(at: &Scratch, chosen: &[(&str, bool)]) -> Result<Vec<TransferItem>, AppError> {
        let picked: Vec<(String, bool, u64, Option<u32>)> = chosen
            .iter()
            .map(|(name, is_dir)| ((*name).to_string(), *is_dir, 0, None))
            .collect();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async move {
            scan_items(
                Rc::new(OnThisDisk),
                picked,
                at.0.to_string_lossy().to_string(),
                |_, _, _| {},
                Arc::new(AtomicBool::new(false)),
            )
            .await
        })
    }

    #[test]
    fn a_walk_finds_the_whole_tree() {
        let at = Scratch::named("tree");
        at.file("docs/one.txt", b"first")
            .file("docs/inner/two.txt", b"second")
            .folder("docs/empty");

        let found = scanning(&at, &[("docs", true)]).expect("the walk goes through");

        let named: Vec<&str> = found.iter().map(|i| i.relative_path.as_str()).collect();
        assert!(named.contains(&"docs"), "{named:?}");
        assert!(named.contains(&"docs/one.txt"), "{named:?}");
        assert!(named.contains(&"docs/inner/two.txt"), "{named:?}");
        assert!(
            named.contains(&"docs/empty"),
            "an empty folder is still carried: {named:?}"
        );

        let one = found
            .iter()
            .find(|i| i.relative_path == "docs/one.txt")
            .unwrap();
        assert_eq!(one.size, 5);
        assert!(!one.is_dir);
        assert!(
            found
                .iter()
                .find(|i| i.relative_path == "docs")
                .unwrap()
                .is_dir
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_folder_that_cannot_be_read_is_not_an_empty_one() {
        use std::os::unix::fs::PermissionsExt;

        if nix_is_root() {
            eprintln!("run as root: permissions do not refuse anything");
            return;
        }
        let at = Scratch::named("refused");
        at.file("docs/readable.txt", b"x").folder("docs/shut");
        let shut = at.0.join("docs/shut");
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o000)).unwrap();

        let outcome = scanning(&at, &[("docs", true)]);

        let _ = std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o755));
        assert!(
            outcome.is_err(),
            "a refused folder must not read as an empty one: {:?}",
            outcome.map(|found| found.len())
        );
    }

    #[cfg(unix)]
    fn nix_is_root() -> bool {
        std::fs::metadata("/proc/self")
            .map(|_| unsafe { libc_getuid() } == 0)
            .unwrap_or(false)
    }

    #[cfg(unix)]
    unsafe fn libc_getuid() -> u32 {
        extern "C" {
            fn getuid() -> u32;
        }
        getuid()
    }

    #[test]
    #[cfg(unix)]
    fn a_tree_copied_on_this_disk_arrives_whole() {
        use std::os::unix::fs::PermissionsExt;

        let from = Scratch::named("local-from");
        let into = Scratch::named("local-into");
        from.file("docs/one.txt", b"first")
            .file("docs/inner/two.txt", b"second")
            .folder("docs/empty");
        std::fs::set_permissions(
            from.0.join("docs/one.txt"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();

        let found = scanning(&from, &[("docs", true)]).expect("the walk goes through");
        let done = {
            let (progress_tx, progress_rx) = std::sync::mpsc::channel();
            let (error_tx, _error_rx) = tokio::sync::mpsc::unbounded_channel();
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();
            let local = tokio::task::LocalSet::new();
            let result = local.block_on(&rt, async {
                execute_transfer(
                    Rc::new(OnThisDisk),
                    Rc::new(OnThisDisk),
                    found,
                    into.0.to_string_lossy().to_string(),
                    false,
                    progress_tx,
                    error_tx,
                    Arc::new(AtomicBool::new(false)),
                )
                .await
            });
            Outcome {
                result,
                said: progress_rx.into_iter().collect(),
            }
        };

        assert!(done.result.is_ok(), "{:?}", done.result);
        assert_eq!(
            std::fs::read(into.0.join("docs/one.txt")).unwrap(),
            b"first"
        );
        assert_eq!(
            std::fs::read(into.0.join("docs/inner/two.txt")).unwrap(),
            b"second"
        );
        assert!(
            into.0.join("docs/empty").is_dir(),
            "an empty folder is carried over too"
        );
        assert_eq!(
            std::fs::metadata(into.0.join("docs/one.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600,
            "the mode comes with the file"
        );
        assert_eq!(done.files_done(), 2);
    }

    #[test]
    fn cancelling_before_the_delete_takes_nothing() {
        struct StopsOnDelete {
            held: Rc<Told>,
            stop: Arc<AtomicBool>,
        }

        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for StopsOnDelete {
            async fn read_file(
                &self,
                path: String,
                progress: Option<Box<dyn Fn(u64) + 'static>>,
            ) -> Result<Vec<u8>, AppError> {
                let body = self.held.read_file(path, progress).await;
                self.stop.store(true, Ordering::Relaxed);
                body
            }
            async fn delete_entries(&self, paths: Vec<String>) -> Result<(), AppError> {
                self.held.delete_entries(paths).await
            }
        }

        let held = Told::holding(&[("/from/a.txt", b"one")]);
        let stop = Arc::new(AtomicBool::new(false));
        let from = Rc::new(StopsOnDelete {
            held: held.clone(),
            stop: stop.clone(),
        });

        let done = copying(
            from,
            Told::empty(),
            vec![file("a.txt", 3)],
            true,
            ErrorAction::Abort,
            stop,
        );

        assert_eq!(done.result, Err(AppError::Cancelled));
        assert!(
            held.deleted.borrow().is_empty(),
            "a cancelled move deletes nothing: {:?}",
            held.deleted.borrow()
        );
    }

    #[test]
    fn a_move_deletes_what_is_inside_before_the_folder() {
        let from = Told::holding(&[
            ("/from/docs/inner/deep.txt", b"x"),
            ("/from/docs/one.txt", b"y"),
        ]);
        let into = Told::empty();

        let mut deep = file("docs/inner/deep.txt", 1);
        deep.src_path = "/from/docs/inner/deep.txt".to_string();
        let mut one = file("docs/one.txt", 1);
        one.src_path = "/from/docs/one.txt".to_string();

        let done = copying(
            from.clone(),
            into,
            vec![folder("docs"), folder("docs/inner"), deep, one],
            true,
            ErrorAction::Abort,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        let order = from.deleted.borrow().clone();
        let place = |what: &str| order.iter().position(|p| p == what).unwrap_or(usize::MAX);
        assert!(
            place("/from/docs/inner/deep.txt") < place("/from/docs/inner"),
            "a file goes before the folder holding it: {order:?}"
        );
        assert!(
            place("/from/docs/inner") < place("/from/docs"),
            "and an inner folder before its parent: {order:?}"
        );
    }

    #[test]
    fn a_folder_holding_something_skipped_is_kept() {
        let from = Told::holding(&[("/from/docs/kept.txt", b"x"), ("/from/other.txt", b"y")]);
        let into = Told::empty();
        into.fail_writing("/to/docs/kept.txt", 99);

        let mut kept = file("docs/kept.txt", 1);
        kept.src_path = "/from/docs/kept.txt".to_string();

        let done = copying(
            from.clone(),
            into,
            vec![folder("docs"), kept, file("other.txt", 1)],
            true,
            ErrorAction::Skip,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        let gone = from.deleted.borrow().clone();
        assert!(
            !gone.contains(&"/from/docs".to_string()),
            "the folder still holds a file that never arrived: {gone:?}"
        );
        assert!(
            gone.contains(&"/from/other.txt".to_string()),
            "what did arrive is still moved: {gone:?}"
        );
    }

    #[test]
    fn a_folder_is_not_mistaken_for_one_whose_name_starts_the_same() {
        let from = Told::holding(&[("/from/docs/bad.txt", b"x"), ("/from/doc/fine.txt", b"y")]);
        let into = Told::empty();
        into.fail_writing("/to/docs/bad.txt", 99);

        let mut bad = file("docs/bad.txt", 1);
        bad.src_path = "/from/docs/bad.txt".to_string();
        let mut fine = file("doc/fine.txt", 1);
        fine.src_path = "/from/doc/fine.txt".to_string();

        let done = copying(
            from.clone(),
            into,
            vec![folder("doc"), folder("docs"), fine, bad],
            true,
            ErrorAction::Skip,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(done.result.is_ok(), "{:?}", done.result);
        let gone = from.deleted.borrow().clone();
        assert!(
            gone.contains(&"/from/doc".to_string()),
            "`doc` holds nothing that was skipped and goes: {gone:?}"
        );
        assert!(
            !gone.contains(&"/from/docs".to_string()),
            "`docs` holds the skipped file and stays: {gone:?}"
        );
    }

    #[test]
    fn a_write_that_fails_on_the_flush_is_not_a_copy() {
        struct Reads;

        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for Reads {
            async fn open_read(&self, _path: String) -> Option<Box<dyn fm_core::rpc::ReadStream>> {
                struct Once(bool);
                #[async_trait::async_trait(?Send)]
                impl fm_core::rpc::ReadStream for Once {
                    async fn fill(&mut self, into: &mut [u8]) -> Result<usize, AppError> {
                        if self.0 {
                            return Ok(0);
                        }
                        self.0 = true;
                        into[..5].copy_from_slice(b"hello");
                        Ok(5)
                    }
                }
                Some(Box::new(Once(false)))
            }
        }

        struct FailsAtTheEnd {
            removed: Rc<RefCell<Vec<String>>>,
        }

        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for FailsAtTheEnd {
            async fn open_write(
                &self,
                _path: String,
                _permissions: Option<u32>,
            ) -> Option<Box<dyn fm_core::rpc::WriteStream>> {
                struct Sink;
                #[async_trait::async_trait(?Send)]
                impl fm_core::rpc::WriteStream for Sink {
                    async fn put(&mut self, _bytes: &[u8]) -> Result<(), AppError> {
                        Ok(())
                    }
                    async fn finish(self: Box<Self>) -> Result<(), AppError> {
                        Err(AppError::Other("the disk filled up".to_string()))
                    }
                }
                Some(Box::new(Sink))
            }
            async fn delete_entries(&self, paths: Vec<String>) -> Result<(), AppError> {
                self.removed.borrow_mut().extend(paths);
                Ok(())
            }
        }

        let removed = Rc::new(RefCell::new(Vec::new()));
        let done = copying(
            Rc::new(Reads),
            Rc::new(FailsAtTheEnd {
                removed: removed.clone(),
            }),
            vec![file("a.txt", 5)],
            false,
            ErrorAction::Skip,
            Arc::new(AtomicBool::new(false)),
        );

        assert!(
            done.result.is_ok(),
            "skipping it carries on: {:?}",
            done.result
        );
        assert_eq!(done.files_done(), 0, "nothing was copied");
        assert_eq!(done.files_skipped(), 1);
        assert_eq!(
            removed.borrow().clone(),
            vec!["/to/a.txt".to_string()],
            "what the failed write left is taken away"
        );
    }

    #[test]
    fn stopping_part_way_through_a_file_cancels_it() {
        struct StopsHalfway {
            stop: Arc<AtomicBool>,
        }

        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for StopsHalfway {
            async fn open_read(&self, _path: String) -> Option<Box<dyn fm_core::rpc::ReadStream>> {
                struct Feeding {
                    stop: Arc<AtomicBool>,
                    given: usize,
                }
                #[async_trait::async_trait(?Send)]
                impl fm_core::rpc::ReadStream for Feeding {
                    async fn fill(&mut self, into: &mut [u8]) -> Result<usize, AppError> {
                        self.given += 1;
                        if self.given > 1 {
                            self.stop.store(true, Ordering::Relaxed);
                        }
                        into[..10].fill(b'x');
                        Ok(10)
                    }
                }
                Some(Box::new(Feeding {
                    stop: self.stop.clone(),
                    given: 0,
                }))
            }
        }

        struct Takes;
        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for Takes {
            async fn open_write(
                &self,
                _path: String,
                _permissions: Option<u32>,
            ) -> Option<Box<dyn fm_core::rpc::WriteStream>> {
                struct Sink;
                #[async_trait::async_trait(?Send)]
                impl fm_core::rpc::WriteStream for Sink {
                    async fn put(&mut self, _bytes: &[u8]) -> Result<(), AppError> {
                        Ok(())
                    }
                    async fn finish(self: Box<Self>) -> Result<(), AppError> {
                        Ok(())
                    }
                }
                Some(Box::new(Sink))
            }
        }

        let stop = Arc::new(AtomicBool::new(false));
        let done = copying(
            Rc::new(StopsHalfway { stop: stop.clone() }),
            Rc::new(Takes),
            vec![file("big.bin", 10_000)],
            false,
            ErrorAction::Abort,
            stop,
        );

        assert_eq!(done.result, Err(AppError::Cancelled));
        assert_eq!(done.files_done(), 0, "a stopped file is not a copied one");
    }

    #[test]
    fn paths_are_joined_with_exactly_one_slash() {
        assert_eq!(join_path("/docs", "one.txt"), "/docs/one.txt");
        assert_eq!(join_path("/docs/", "one.txt"), "/docs/one.txt");
        assert_eq!(join_path("", "one.txt"), "one.txt");
        assert_eq!(join_path("/", "one.txt"), "/one.txt");
    }

    #[test]
    fn sizes_are_spelled_at_the_boundaries() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
    }

    #[test]
    #[cfg(unix)]
    fn a_files_mode_is_read_back() {
        use std::os::unix::fs::PermissionsExt;

        let at = Scratch::named("mode");
        at.file("one.txt", b"x");
        let path = at.0.join("one.txt");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        let told = mode_of(&std::fs::metadata(&path).unwrap()).expect("unix says the mode");
        assert_eq!(told & 0o777, 0o640);
    }

    #[test]
    fn the_first_report_is_never_held_back() {
        let mut throttle = ThrottleTimer::new();
        assert!(throttle.ready(), "the first report must go out at once");
        assert!(
            !throttle.ready(),
            "the one straight after it is still too early"
        );
    }

    #[test]
    fn the_buffer_fits_the_file_and_stops_at_the_measured_best() {
        assert_eq!(buffer_for(100), 100, "a tiny file gets a tiny buffer");
        assert_eq!(buffer_for(1), 1);
        assert_eq!(buffer_for(COPY_BUFFER_MAX as u64), COPY_BUFFER_MAX);
        assert_eq!(
            buffer_for(4 * 1024 * 1024 * 1024),
            COPY_BUFFER_MAX,
            "a four gigabyte file is carried in the same buffer as a one megabyte one"
        );
        assert!(
            COPY_BUFFER_MAX >= 64 * 1024,
            "below 64KB the measured throughput falls off a cliff"
        );
        assert_eq!(
            buffer_for(0),
            COPY_BUFFER_MAX,
            "an unknown length is given the full buffer rather than none"
        );
    }

    fn renames(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
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
        assert_eq!(
            items[0].relative_path, "a/notes.txt",
            "a nested namesake must not be hit"
        );
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
