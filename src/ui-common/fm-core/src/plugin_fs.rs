use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::sync::{Arc, Mutex};

fn cstr(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string()
}

type Spawner = std::rc::Rc<dyn Fn(std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>)>;

fn filesystems() -> &'static std::sync::Mutex<Vec<FsPlugin>> {
    static FILESYSTEMS: std::sync::OnceLock<std::sync::Mutex<Vec<FsPlugin>>> =
        std::sync::OnceLock::new();
    FILESYSTEMS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn registry() -> std::sync::MutexGuard<'static, Vec<FsPlugin>> {
    filesystems().lock().unwrap_or_else(|e| e.into_inner())
}

thread_local! {
    static SPAWNER: RefCell<Option<Spawner>> = const { RefCell::new(None) };
    static ICON_PROVIDER: RefCell<Option<IconProvider>> = const { RefCell::new(None) };
}

type IconProvider = std::rc::Rc<dyn Fn(&str) -> Option<String>>;

pub fn set_icon_provider(provider: IconProvider) {
    ICON_PROVIDER.with(|i| *i.borrow_mut() = Some(provider));
}

pub fn set_spawner(spawner: Spawner) {
    SPAWNER.with(|s| *s.borrow_mut() = Some(spawner));
}

fn spawn(future: impl std::future::Future<Output = ()> + 'static) {
    let spawner = SPAWNER.with(|s| s.borrow().clone());
    if let Some(run) = spawner {
        run(Box::pin(future));
    }
}

pub fn open_externally(path: &std::path::Path) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", ""])
        .arg(path)
        .spawn();
}

#[derive(Clone)]
pub struct FsPlugin {
    pub extensions: Vec<String>,
    table: ic_plugin_api::BoundedVTable,
    user_data: usize,
}

macro_rules! fs_call {
    ($plugin:expr, $field:ident, $kind:ty) => {
        $plugin
            .table
            .field::<Option<$kind>>(std::mem::offset_of!(ic_plugin_api::IcFsVTable, $field))
            .flatten()
    };
}

impl FsPlugin {
    fn open_in_fn(&self) -> Option<ic_plugin_api::IcFsOpenInFn> {
        fs_call!(self, open_in, ic_plugin_api::IcFsOpenInFn)
    }
    fn close_fn(&self) -> Option<ic_plugin_api::IcFsCloseFn> {
        fs_call!(self, close, ic_plugin_api::IcFsCloseFn)
    }
    fn list_fn(&self) -> Option<ic_plugin_api::IcFsListFn> {
        fs_call!(self, list, ic_plugin_api::IcFsListFn)
    }
    fn read_fn(&self) -> Option<ic_plugin_api::IcFsReadFn> {
        fs_call!(self, read, ic_plugin_api::IcFsReadFn)
    }
    fn read_only_fn(&self) -> Option<ic_plugin_api::IcFsFlagFn> {
        fs_call!(self, is_read_only, ic_plugin_api::IcFsFlagFn)
    }
    fn last_error_fn(&self) -> Option<ic_plugin_api::IcFsErrorFn> {
        fs_call!(self, last_error, ic_plugin_api::IcFsErrorFn)
    }
    fn write_fn(&self) -> Option<ic_plugin_api::IcFsWriteFn> {
        fs_call!(self, write, ic_plugin_api::IcFsWriteFn)
    }
    fn create_dir_fn(&self) -> Option<ic_plugin_api::IcFsPathFn> {
        fs_call!(self, create_dir, ic_plugin_api::IcFsPathFn)
    }
    fn remove_fn(&self) -> Option<ic_plugin_api::IcFsPathFn> {
        fs_call!(self, remove, ic_plugin_api::IcFsPathFn)
    }
    fn rename_fn(&self) -> Option<ic_plugin_api::IcFsRenameFn> {
        fs_call!(self, rename, ic_plugin_api::IcFsRenameFn)
    }
    pub fn supports_write(&self) -> bool {
        self.write_fn().is_some()
    }
    fn shell_open_fn(&self) -> Option<ic_plugin_api::IcShellOpenFn> {
        fs_call!(self, shell_open, ic_plugin_api::IcShellOpenFn)
    }
    fn shell_read_fn(&self) -> Option<ic_plugin_api::IcShellReadFn> {
        fs_call!(self, shell_read, ic_plugin_api::IcShellReadFn)
    }
    fn shell_write_fn(&self) -> Option<ic_plugin_api::IcShellWriteFn> {
        fs_call!(self, shell_write, ic_plugin_api::IcShellWriteFn)
    }
    fn shell_resize_fn(&self) -> Option<ic_plugin_api::IcShellResizeFn> {
        fs_call!(self, shell_resize, ic_plugin_api::IcShellResizeFn)
    }
    fn shell_close_fn(&self) -> Option<ic_plugin_api::IcShellCloseFn> {
        fs_call!(self, shell_close, ic_plugin_api::IcShellCloseFn)
    }
    fn shell_available_fn(&self) -> Option<ic_plugin_api::IcShellAvailableFn> {
        fs_call!(self, shell_available, ic_plugin_api::IcShellAvailableFn)
    }
    fn columns_fn(&self) -> Option<ic_plugin_api::IcFsColumnsFn> {
        fs_call!(self, columns, ic_plugin_api::IcFsColumnsFn)
    }
    fn rows_fn(&self) -> Option<ic_plugin_api::IcFsRowsFn> {
        fs_call!(self, list_rows, ic_plugin_api::IcFsRowsFn)
    }
    fn action_state_fn(&self) -> Option<ic_plugin_api::IcFsActionStateFn> {
        fs_call!(self, action_state, ic_plugin_api::IcFsActionStateFn)
    }
    fn cell_clicked_fn(&self) -> Option<ic_plugin_api::IcFsCellClickedFn> {
        fs_call!(self, cell_clicked, ic_plugin_api::IcFsCellClickedFn)
    }
    pub fn carries_a_shell(&self) -> bool {
        self.shell_open_fn().is_some()
            && self.shell_read_fn().is_some()
            && self.shell_write_fn().is_some()
    }
}

impl FsPlugin {
    pub fn adopt(
        vtable: *const ic_plugin_api::IcFsVTable,
        user_data: *mut c_void,
    ) -> Option<FsPlugin> {
        let table = unsafe {
            ic_plugin_api::BoundedVTable::adopt(
                vtable as *const u8,
                std::mem::size_of::<ic_plugin_api::IcFsVTable>(),
            )
        }?;
        let plugin = FsPlugin {
            extensions: Vec::new(),
            table,
            user_data: user_data as usize,
        };
        plugin.is_complete().then_some(plugin)
    }

    fn is_complete(&self) -> bool {
        self.open_in_fn().is_some()
            && self.close_fn().is_some()
            && self.list_fn().is_some()
            && self.read_fn().is_some()
            && self.read_only_fn().is_some()
            && self.last_error_fn().is_some()
    }
}

pub fn register(
    extensions: *const c_char,
    vtable: *const ic_plugin_api::IcFsVTable,
    user_data: *mut c_void,
) -> c_int {
    if vtable.is_null() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let list = crate::suffix::claimed(&cstr(extensions));
    if list.is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let Some(table) = (unsafe {
        ic_plugin_api::BoundedVTable::adopt(
            vtable as *const u8,
            std::mem::size_of::<ic_plugin_api::IcFsVTable>(),
        )
    }) else {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    };
    let plugin = FsPlugin {
        extensions: list,
        table,
        user_data: user_data as usize,
    };
    if !plugin.is_complete() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let mut held = registry();
    match held
        .iter()
        .position(|seen| seen.extensions == plugin.extensions)
    {
        Some(index) => held[index] = plugin,
        None => held.push(plugin),
    }
    ic_plugin_api::IC_OK
}

fn test_lease_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

pub struct RegistryLease(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

pub fn lease_registry_for_test() -> RegistryLease {
    let held = test_lease_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry().clear();
    RegistryLease(held)
}

impl FsPlugin {
    /// Which of this plugin's extensions the name matched.
    pub fn extension_matching(&self, name: &str) -> Option<String> {
        crate::suffix::longest_match(name, &self.extensions).cloned()
    }
}

/// Whose mount a file of this name opens. The longest claim wins, and between
/// equal claims the one registered first, so the answer does not change from
/// one question to the next.
pub fn filesystem_for(name: &str) -> Option<FsPlugin> {
    let mut best: Option<(usize, &FsPlugin)> = None;
    let held = registry();
    for plugin in held.iter() {
        let Some(claim) = crate::suffix::longest_match(name, &plugin.extensions) else {
            continue;
        };
        if best.is_none_or(|(len, _)| claim.len() > len) {
            best = Some((claim.len(), plugin));
        }
    }
    best.map(|(_, plugin)| plugin.clone())
}

pub fn path_crosses_plugin_fs(path: &str) -> bool {
    registry().iter().any(|plugin| {
        path.split(['/', '\\'])
            .any(|segment| crate::suffix::matches_any(segment, &plugin.extensions))
    })
}

pub fn mount_label(name: &str) -> Option<String> {
    let plugin = filesystem_for(name)?;
    let claimed = crate::suffix::longest_match(name, &plugin.extensions)?;
    let head = claimed.trim_start_matches('.');
    let head = head.split('.').next().unwrap_or(head);
    Some(head.to_uppercase())
}

pub fn split_at_plugin_fs(path: &str) -> Option<(String, String)> {
    let segments: Vec<&str> = path.split(['/', '\\']).collect();
    let mount = segments
        .iter()
        .rposition(|segment| filesystem_for(segment).is_some())?;
    let outer = segments[..=mount].join("/");
    let inner = segments[mount + 1..].join("/");
    Some((outer, inner))
}

pub fn handles_extension(name: &str) -> bool {
    filesystem_for(name).is_some()
}

#[derive(Clone)]
enum Source {
    Nested {
        parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc>,
        relative_path: String,
    },
    Connection {
        kind: String,
        settings: Vec<u8>,
        open: ic_plugin_api::IcConnectionOpenFn,
    },
}

/// Opens the mount on that file, through the filesystem rather than on its
/// bytes. The source belongs to the caller and is good for as long as the
/// handle it opened.
fn open_standing(
    plugin: &FsPlugin,
    standing: &crate::host_fs::Staged,
    fs_id: &str,
) -> (ic_plugin_api::IcFsHandle, ic_plugin_api::IcFsSource) {
    let Ok(name) = CString::new(standing.name.clone()) else {
        return (std::ptr::null_mut(), std::ptr::null_mut());
    };
    let source = crate::host_fs::HostSource::rooted_at(&standing.root, fs_id, "").open();
    let handle = (plugin.open_in_fn().expect("checked at registration"))(
        source,
        name.as_ptr(),
        plugin.user_data as *mut c_void,
    );
    if handle.is_null() {
        crate::host_fs::HostSource::close(source);
        return (std::ptr::null_mut(), std::ptr::null_mut());
    }
    (handle, source)
}

struct Carried(ic_plugin_api::IcFsHandle);
unsafe impl Send for Carried {}

/// How a mount shows itself at the top of a path: the name the user gave the
/// connection, and the picture its kind brought. Absent for a mount nobody
/// named, which is then drawn the way any folder is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub name: String,
    pub icon_svg: Option<String>,
}

/// Where a copy whose destination names a file some plugin claims actually
/// goes: inside that file, not into a folder of that name. The file is made
/// first if it is not there yet — which is how a new archive comes to exist —
/// and what comes back is the mount to copy into. `None` when nothing claims
/// the name, and the destination is then a folder like any other.
pub async fn destination_inside(
    parent: &std::rc::Rc<dyn crate::rpc::FileSystemRpc>,
    path: &str,
) -> Option<Result<std::rc::Rc<dyn crate::rpc::FileSystemRpc>, common::AppError>> {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let leaf = trimmed.rsplit(['/', '\\']).next().unwrap_or_default();
    let plugin = filesystem_for(leaf)?;
    let holds = trimmed.len() - leaf.len();
    let held_in = trimmed[..holds].trim_end_matches(['/', '\\']).to_string();
    let held = match parent.list_dir(held_in).await {
        Ok(listed) => listed.into_iter().find(|entry| entry.name == leaf),
        // Let the copy itself report why a folder cannot be listed.
        Err(why) => return Some(Err(why)),
    };
    // A folder that happens to be named like an archive is still a folder.
    if held.as_ref().map(|entry| entry.is_dir).unwrap_or(false) {
        return None;
    }
    if held.is_none() {
        // Nothing but the file itself: the plugin is opened on an empty one
        // and writes the container when the first entry is copied in. The
        // host has no business knowing what a zip looks like.
        if let Err(why) = parent
            .write_file(trimmed.to_string(), Vec::new(), None, None)
            .await
        {
            return Some(Err(why));
        }
    }
    Some(Ok(std::rc::Rc::new(PluginFsRpc::new(
        plugin,
        trimmed.to_string(),
        parent.clone(),
    ))))
}

pub struct PluginFsRpc {
    plugin: FsPlugin,
    source: Source,
    shown: Option<Shown>,
    handle: std::cell::Cell<ic_plugin_api::IcFsHandle>,
    /// The filesystem the plugin was let into, and where its file stands on
    /// this machine. Both live exactly as long as the open mount.
    let_into: std::cell::Cell<ic_plugin_api::IcFsSource>,
    standing: RefCell<Option<crate::host_fs::Staged>>,
    /// What the last listing said this mount's own columns are. Kept because
    /// the panel asks for them after the listing, from the frontend's thread.
    columns: RefCell<Vec<crate::rpc::ColumnSpec>>,
    /// Held for as long as the plugin is inside one of its own calls.
    ///
    /// A connection's work runs on a worker thread while the interface keeps
    /// asking this mount cheap questions — which button is pressed, has it a
    /// terminal — from the thread that draws. Those questions go into the same
    /// plugin handle, and a plugin holds it as one `&mut` of its own; two at
    /// once is not a race the plugin can defend itself against.
    busy: Arc<Mutex<()>>,
    /// The last answer to each of those questions, for when the plugin is busy.
    /// Being one listing out of date is better than waiting for a server.
    said: RefCell<Asked>,
}

/// What the cheap questions answered last time they could be asked.
#[derive(Default)]
struct Asked {
    actions: std::collections::HashMap<String, u32>,
    terminal: Option<bool>,
}

/// The columns a mount declared, in the order it declared them. Ones with no
/// key are dropped: the panel has nothing to address them by.
fn specs_of(columns: ic_plugin_api::IcColumns) -> Vec<crate::rpc::ColumnSpec> {
    columns
        .as_slice()
        .iter()
        .map(|column| crate::rpc::ColumnSpec {
            key: column.key_string(),
            title: column.title_string(),
            width: (column.width > 0).then_some(column.width),
            kind: if column.is_check() {
                crate::rpc::ColumnKind::Check
            } else {
                crate::rpc::ColumnKind::Text
            },
        })
        .filter(|spec| !spec.key.is_empty())
        .collect()
}

impl PluginFsRpc {
    pub fn new(
        plugin: FsPlugin,
        relative_path_in_parent: String,
        parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc>,
    ) -> Self {
        Self {
            plugin,
            source: Source::Nested {
                parent,
                relative_path: relative_path_in_parent,
            },
            shown: None,
            handle: std::cell::Cell::new(std::ptr::null_mut()),
            let_into: std::cell::Cell::new(std::ptr::null_mut()),
            standing: RefCell::new(None),
            columns: RefCell::new(Vec::new()),
            busy: Arc::new(Mutex::new(())),
            said: RefCell::new(Asked::default()),
        }
    }

    pub fn for_connection(
        plugin: FsPlugin,
        open: ic_plugin_api::IcConnectionOpenFn,
        kind: String,
        settings_json: String,
    ) -> Self {
        Self {
            plugin,
            source: Source::Connection {
                kind,
                settings: settings_json.into_bytes(),
                open,
            },
            shown: None,
            handle: std::cell::Cell::new(std::ptr::null_mut()),
            let_into: std::cell::Cell::new(std::ptr::null_mut()),
            standing: RefCell::new(None),
            columns: RefCell::new(Vec::new()),
            busy: Arc::new(Mutex::new(())),
            said: RefCell::new(Asked::default()),
        }
    }

    /// Says how this mount is to be named and drawn where a path begins.
    pub fn shown_as(mut self, shown: Shown) -> Self {
        self.shown = Some(shown);
        self
    }

    pub fn connection_origin(&self) -> Option<(String, String)> {
        match &self.source {
            Source::Connection { kind, settings, .. } => {
                Some((kind.clone(), String::from_utf8_lossy(settings).to_string()))
            }
            Source::Nested { .. } => None,
        }
    }

    /// Whether this mount's work waits on something outside the machine, and
    /// so belongs off the thread that draws. A mount inside a file is as far
    /// away as whatever holds that file.
    fn reaches_outside(&self) -> bool {
        match &self.source {
            Source::Connection { .. } => true,
            Source::Nested { parent, .. } => parent.runs_off_thread(),
        }
    }

    async fn call<T>(
        &self,
        work: impl FnOnce(ic_plugin_api::IcFsHandle) -> T + Send + 'static,
    ) -> Result<T, common::AppError>
    where
        T: Send + 'static,
    {
        let handle = self.ensure_open().await?;
        if !self.reaches_outside() {
            let _inside = self.busy.lock();
            return Ok(work(handle));
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            let _inside = self.busy.lock();
            return Ok(work(handle));
        };
        let carried = Carried(handle);
        let busy = self.busy.clone();
        runtime
            .spawn_blocking(move || {
                let carried = carried;
                let _inside = busy.lock();
                work(carried.0)
            })
            .await
            .map_err(|e| common::AppError::Other(e.to_string()))
    }

    /// Asks the plugin something small, or answers from what it said last time
    /// if it is inside a call of its own. Never waits: this is the path the
    /// thread that draws takes, and a listing on a slow server must not stop a
    /// toolbar from being painted.
    fn ask_if_free<T>(&self, ask: impl FnOnce(ic_plugin_api::IcFsHandle) -> T) -> Option<T> {
        let handle = self.handle.get();
        if handle.is_null() {
            return None;
        }
        let _inside = self.busy.try_lock().ok()?;
        Some(ask(handle))
    }

    async fn ensure_open(&self) -> Result<ic_plugin_api::IcFsHandle, common::AppError> {
        let existing = self.handle.get();
        if !existing.is_null() {
            return Ok(existing);
        }
        let mut opened_on = std::ptr::null_mut();
        let mut stood_on = None;
        let handle = match &self.source {
            Source::Nested {
                parent,
                relative_path,
            } => {
                // The plugin is handed the filesystem the file lives on and
                // the path it lives at, never its bytes: an archive on a disk
                // is read where it lies, however large it is.
                let standing = crate::host_fs::stage(parent, relative_path).await?;
                let (handle, source) = open_standing(&self.plugin, &standing, &parent.fs_id());
                opened_on = source;
                stood_on = Some(standing);
                handle
            }
            Source::Connection { settings, open, .. } => {
                let carried = Carried(self.plugin.user_data as *mut c_void);
                let settings = settings.clone();
                let open = *open;
                match tokio::runtime::Handle::try_current() {
                    Ok(runtime) => {
                        runtime
                            .spawn_blocking(move || {
                                let carried = carried;
                                Carried(open(settings.as_ptr(), settings.len() as u64, carried.0))
                            })
                            .await
                            .map_err(|e| common::AppError::Other(e.to_string()))?
                            .0
                    }
                    Err(_) => open(
                        settings.as_ptr(),
                        settings.len() as u64,
                        self.plugin.user_data as *mut c_void,
                    ),
                }
            }
        };
        if handle.is_null() {
            return Err(common::AppError::Other(
                "the plugin could not open this source".to_string(),
            ));
        }
        let winner = self.handle.get();
        if !winner.is_null() {
            (self.plugin.close_fn().expect("checked at registration"))(handle);
            crate::host_fs::HostSource::close(opened_on);
            return Ok(winner);
        }
        self.handle.set(handle);
        self.let_into.set(opened_on);
        *self.standing.borrow_mut() = stood_on;
        if self.reaches_outside() {
            // A new connection to a server is a new look at it. Its name is
            // the same as last time on purpose — two tabs on one server share
            // what they read — so opening it again is the moment to say that
            // whatever was remembered from a previous session must be read
            // once more. What is remembered stays drawable until it is.
            crate::listing::forget_below(&crate::rpc::FileSystemRpc::fs_id(self));
        }
        Ok(handle)
    }

    fn read_only_error() -> common::AppError {
        common::AppError::Other("this filesystem is read-only".to_string())
    }

    fn joined(parent: &str, name: &str) -> String {
        let trimmed = parent.trim_end_matches('/');
        format!("{trimmed}/{name}")
    }

    /// Carries a nested mount back to where it came from.
    ///
    /// A mount standing on a disk has nowhere to be carried: the plugin wrote
    /// into the real file through the host, and there it already is. A mount
    /// standing on a copy — because its file is on a server — is written back
    /// whole, which is the one fetch and the one upload the application has
    /// always done for those.
    async fn put_back(&self) -> Result<(), common::AppError> {
        let Source::Nested {
            parent,
            relative_path,
        } = &self.source
        else {
            return Ok(());
        };
        let copied = {
            let standing = self.standing.borrow();
            let Some(standing) = standing.as_ref() else {
                return Ok(());
            };
            // A mount standing on the real file has already written where it
            // stands, and one that wrote nothing has nothing to carry: putting
            // a file back anyway would overwrite one that is already right.
            if !standing.was_written() {
                return Ok(());
            }
            standing.at()
        };
        let whole = std::fs::read(&copied).map_err(|e| common::AppError::Other(e.to_string()))?;
        parent
            .write_file(relative_path.clone(), whole, None, None)
            .await
    }

    fn error_of(&self, handle: ic_plugin_api::IcFsHandle) -> common::AppError {
        let ptr = (self
            .plugin
            .last_error_fn()
            .expect("checked at registration"))(handle);
        let text = if ptr.is_null() {
            "the plugin reported no reason".to_string()
        } else {
            cstr(ptr)
        };
        common::AppError::Other(text)
    }
}

impl Drop for PluginFsRpc {
    fn drop(&mut self) {
        let handle = self.handle.get();
        if !handle.is_null() {
            (self.plugin.close_fn().expect("checked at registration"))(handle);
            self.handle.set(std::ptr::null_mut());
        }
        // After the mount, never before: the plugin may read through the
        // source while it is closing.
        crate::host_fs::HostSource::close(self.let_into.get());
        self.let_into.set(std::ptr::null_mut());
    }
}

#[async_trait::async_trait(?Send)]
impl crate::rpc::FileSystemRpc for PluginFsRpc {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn plugin_mount(&self) -> Option<usize> {
        let handle = self.handle.get();
        (!handle.is_null()).then_some(handle as usize)
    }

    /// A tick changes what the plugin holds, so this one waits its turn rather
    /// than answering from memory — but it is a click, not a listing, and the
    /// mount it waits for is one the user is already looking at.
    fn toggle_cell(&self, dir: &str, name: &str, column: &str, ticked: bool) -> bool {
        let handle = self.handle.get();
        let Some(clicked) = self.plugin.cell_clicked_fn() else {
            return false;
        };
        if handle.is_null() {
            return false;
        }
        let (Ok(dir), Ok(name), Ok(column)) =
            (CString::new(dir), CString::new(name), CString::new(column))
        else {
            return false;
        };
        let _inside = self.busy.lock();
        clicked(
            handle,
            dir.as_ptr(),
            name.as_ptr(),
            column.as_ptr(),
            c_int::from(ticked),
        ) == ic_plugin_api::IC_OK
    }

    fn plugin_action_state(&self, action_id: &str) -> u32 {
        let Some(ask) = self.plugin.action_state_fn() else {
            return ic_plugin_api::IC_ACTION_DEFAULT;
        };
        let Ok(wanted) = CString::new(action_id) else {
            return ic_plugin_api::IC_ACTION_DEFAULT;
        };
        match self.ask_if_free(|handle| ask(handle, wanted.as_ptr())) {
            Some(state) => {
                self.said
                    .borrow_mut()
                    .actions
                    .insert(action_id.to_string(), state);
                state
            }
            None => self
                .said
                .borrow()
                .actions
                .get(action_id)
                .copied()
                .unwrap_or(ic_plugin_api::IC_ACTION_DEFAULT),
        }
    }

    /// The extension of the mount the panel is standing in, so a plugin's own
    /// toolbar buttons can be shown here and nowhere else. A connection is not
    /// reached through a file, so it has no extension to be scoped by.
    fn plugin_scope(&self) -> Option<String> {
        match &self.source {
            Source::Nested { relative_path, .. } => self.plugin.extension_matching(relative_path),
            Source::Connection { .. } => None,
        }
    }

    /// Added to the panel's own columns, never in place of them, so name, size
    /// and date stay where the user expects.
    fn extra_columns(&self) -> Vec<crate::rpc::ColumnSpec> {
        let cached = self.columns.borrow().clone();
        if !cached.is_empty() {
            return cached;
        }
        let Some(ask) = self.plugin.columns_fn() else {
            return Vec::new();
        };
        let Some(specs) = self.ask_if_free(|handle| specs_of(ask(handle))) else {
            return Vec::new();
        };
        *self.columns.borrow_mut() = specs.clone();
        specs
    }

    async fn list_dir(
        &self,
        path: String,
    ) -> Result<Vec<crate::rpc::RemoteFileEntry>, common::AppError> {
        let wanted = CString::new(path).map_err(|e| common::AppError::Other(e.to_string()))?;
        if let Some(rows) = self.plugin.rows_fn() {
            let columns = self.plugin.columns_fn();
            let (specs, entries) = self
                .call(move |handle| {
                    let specs = columns.map(|ask| specs_of(ask(handle))).unwrap_or_default();
                    let declared = specs.len();
                    let entries = rows(handle, wanted.as_ptr())
                        .as_slice()
                        .iter()
                        .map(|row| crate::rpc::RemoteFileEntry {
                            name: row.entry.name_string(),
                            is_dir: row.entry.is_directory(),
                            size: row.entry.size,
                            modified: row.entry.modified,
                            permissions: row.entry.permissions_opt(),
                            extra: (0..declared).map(|at| row.extra_at(at)).collect(),
                        })
                        .filter(|e| !e.name.is_empty())
                        .collect::<Vec<_>>();
                    (specs, entries)
                })
                .await?;
            *self.columns.borrow_mut() = specs;
            return Ok(entries);
        }
        let list = self.plugin.list_fn().expect("checked at registration");
        self.call(move |handle| {
            list(handle, wanted.as_ptr())
                .as_slice()
                .iter()
                .map(|e| crate::rpc::RemoteFileEntry {
                    name: e.name_string(),
                    is_dir: e.is_directory(),
                    size: e.size,
                    modified: e.modified,
                    permissions: e.permissions_opt(),
                    extra: Vec::new(),
                })
                .filter(|e| !e.name.is_empty())
                .collect()
        })
        .await
    }

    async fn read_file(
        &self,
        path: String,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<Vec<u8>, common::AppError> {
        let wanted = CString::new(path).map_err(|e| common::AppError::Other(e.to_string()))?;
        let read = self.plugin.read_fn().expect("checked at registration");
        let owned = self
            .call(move |handle| {
                let bytes = read(handle, wanted.as_ptr());
                (!bytes.data.is_null()).then(|| bytes.as_slice().to_vec())
            })
            .await?;
        let Some(owned) = owned else {
            return Err(self.error_of(self.handle.get()));
        };
        if let Some(cb) = progress_callback {
            cb(owned.len() as u64);
        }
        Ok(owned)
    }

    async fn create_directory(
        &self,
        parent_path: String,
        dir_name: String,
        _permissions: Option<u32>,
    ) -> Result<(), common::AppError> {
        let Some(create) = self.plugin.create_dir_fn() else {
            return Err(Self::read_only_error());
        };
        let wanted = CString::new(Self::joined(&parent_path, &dir_name))
            .map_err(|e| common::AppError::Other(e.to_string()))?;
        let done = self
            .call(move |handle| create(handle, wanted.as_ptr()) == ic_plugin_api::IC_OK)
            .await?;
        if !done {
            return Err(self.error_of(self.handle.get()));
        }
        self.put_back().await
    }

    async fn delete_entries(&self, paths: Vec<String>) -> Result<(), common::AppError> {
        let Some(remove) = self.plugin.remove_fn() else {
            return Err(Self::read_only_error());
        };
        let wanted: Vec<CString> = paths
            .into_iter()
            .map(|path| CString::new(path).map_err(|e| common::AppError::Other(e.to_string())))
            .collect::<Result<_, _>>()?;
        let done = self
            .call(move |handle| {
                wanted
                    .iter()
                    .all(|path| remove(handle, path.as_ptr()) == ic_plugin_api::IC_OK)
            })
            .await?;
        if !done {
            return Err(self.error_of(self.handle.get()));
        }
        self.put_back().await
    }

    async fn rename_entry(&self, path: String, new: String) -> Result<(), common::AppError> {
        let Some(rename) = self.plugin.rename_fn() else {
            return Err(Self::read_only_error());
        };
        let from = CString::new(path).map_err(|e| common::AppError::Other(e.to_string()))?;
        let to = CString::new(new).map_err(|e| common::AppError::Other(e.to_string()))?;
        let done = self
            .call(move |handle| rename(handle, from.as_ptr(), to.as_ptr()) == ic_plugin_api::IC_OK)
            .await?;
        if !done {
            return Err(self.error_of(self.handle.get()));
        }
        self.put_back().await
    }

    async fn write_file(
        &self,
        path: String,
        content: Vec<u8>,
        _permissions: Option<u32>,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<(), common::AppError> {
        let Some(write) = self.plugin.write_fn() else {
            return Err(Self::read_only_error());
        };
        let wanted = CString::new(path).map_err(|e| common::AppError::Other(e.to_string()))?;
        let written = content.len() as u64;
        let done = self
            .call(move |handle| {
                write(handle, wanted.as_ptr(), content.as_ptr(), written) == ic_plugin_api::IC_OK
            })
            .await?;
        if !done {
            return Err(self.error_of(self.handle.get()));
        }
        self.put_back().await?;
        if let Some(report) = progress_callback {
            report(written);
        }
        Ok(())
    }

    fn is_read_only(&self) -> bool {
        if !self.plugin.supports_write() {
            return true;
        }
        let handle = self.handle.get();
        if handle.is_null() {
            return true;
        }
        (self.plugin.read_only_fn().expect("checked at registration"))(handle) != 0
    }

    fn display_name(&self) -> Option<String> {
        self.shown
            .as_ref()
            .map(|shown| shown.name.clone())
            .filter(|name| !name.trim().is_empty())
    }

    fn get_icon_svg(&self, path: &str) -> Option<String> {
        let inside = path.replace('\\', "/");
        let inside = inside.trim_matches('/');
        if !inside.is_empty() {
            return None;
        }
        if let Some(brought) = self.shown.as_ref().and_then(|shown| shown.icon_svg.clone()) {
            return Some(brought);
        }
        let Source::Nested { relative_path, .. } = &self.source else {
            return None;
        };
        let leaf = relative_path.rsplit(['/', '\\']).next().unwrap_or("");
        if leaf.is_empty() {
            return None;
        }
        let provider = ICON_PROVIDER.with(|i| i.borrow().clone())?;
        provider(leaf)
    }

    fn content_wait(&self) -> crate::rpc::ContentWait {
        match &self.source {
            Source::Nested { parent, .. } => parent.content_wait(),
            Source::Connection { .. } => match common::request_timeout() {
                None => crate::rpc::ContentWait::Infinite,
                Some(limit) => crate::rpc::ContentWait::Bounded(limit),
            },
        }
    }

    fn is_local(&self) -> bool {
        false
    }

    fn runs_off_thread(&self) -> bool {
        self.reaches_outside()
    }

    /// A connection is named by what opens it; a mount inside a file is named
    /// by what holds it and where in it. The settings are folded into a number
    /// rather than spelled out: they carry passwords, and a name is kept in a
    /// map for as long as the application runs.
    fn fs_id(&self) -> String {
        match &self.source {
            Source::Connection { kind, settings, .. } => {
                use std::hash::{Hash, Hasher};
                let mut folded = std::collections::hash_map::DefaultHasher::new();
                settings.hash(&mut folded);
                format!("conn:{kind}:{:016x}", folded.finish())
            }
            Source::Nested {
                parent,
                relative_path,
            } => {
                let holding = parent.fs_id();
                if holding.is_empty() {
                    String::new()
                } else {
                    format!("{holding}/{}", relative_path.trim_matches('/'))
                }
            }
        }
    }

    fn supports_terminal(&self) -> bool {
        if !self.plugin.carries_a_shell() {
            return false;
        }
        // A property of the mount, not of the plugin: one peer shares a shell, the next does not.
        let Some(asked) = self.plugin.shell_available_fn() else {
            return true;
        };
        match self.ask_if_free(|handle| asked(handle) != 0) {
            Some(has) => {
                self.said.borrow_mut().terminal = Some(has);
                has
            }
            // Busy, and nobody has asked before: a plugin that carries a shell
            // is taken at its word until the mount can answer for itself.
            None => self.said.borrow().terminal.unwrap_or(true),
        }
    }

    fn open_shell(
        &self,
        cwd: &str,
        rows: u16,
        cols: u16,
    ) -> Option<ic_platform::terminal::PtySession> {
        let open = self.plugin.shell_open_fn()?;
        let read = self.plugin.shell_read_fn()?;
        let write = self.plugin.shell_write_fn()?;
        let resize = self.plugin.shell_resize_fn();
        let close = self.plugin.shell_close_fn();

        let handle = self.handle.get();
        if handle.is_null() {
            return None;
        }
        let wanted = CString::new(cwd).ok()?;
        let shell = open(handle, wanted.as_ptr(), rows as u32, cols as u32);
        if shell.is_null() {
            return None;
        }

        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(256);
        let (output_tx, output_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(256);
        let (resize_tx, mut resize_rx) = tokio::sync::mpsc::channel::<(u16, u16)>(16);

        let carried = Carried(shell);
        std::thread::spawn(move || {
            let carried = carried;
            let shell = carried.0;
            loop {
                while let Ok(bytes) = input_rx.try_recv() {
                    if write(shell, bytes.as_ptr(), bytes.len() as u64) != ic_plugin_api::IC_OK {
                        break;
                    }
                }
                if let (Some(resize), Ok((rows, cols))) = (resize, resize_rx.try_recv()) {
                    resize(shell, rows as u32, cols as u32);
                }
                let produced = read(shell);
                if produced.data.is_null() {
                    break;
                }
                if produced.len == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                if output_tx
                    .blocking_send(produced.as_slice().to_vec())
                    .is_err()
                {
                    break;
                }
            }
            if let Some(close) = close {
                close(shell);
            }
        });

        Some(ic_platform::terminal::PtySession {
            input_tx,
            output_rx,
            resize_tx,
        })
    }

    fn request_file_download(&self, file_path: String, _transfer_id: uuid::Uuid) {
        let plugin = self.plugin.clone();
        let source = self.source.clone();
        // Standing inside the mount already: read through the handle it opened
        // rather than opening the whole thing a second time. Fetching one file
        // out of an archive used to read the archive again, and on a server
        // that is the whole of it over the network for the sake of one entry.
        let standing = self.handle.get();
        let busy = self.busy.clone();
        spawn(async move {
            let opened_here = standing.is_null();
            // Kept until the handle is closed: the plugin reads through it.
            let mut let_into = std::ptr::null_mut();
            let mut stood_on = None;
            let handle = if opened_here {
                match source {
                    Source::Nested {
                        parent,
                        relative_path,
                    } => {
                        let Ok(standing) = crate::host_fs::stage(&parent, &relative_path).await
                        else {
                            return;
                        };
                        let (handle, source) = open_standing(&plugin, &standing, &parent.fs_id());
                        let_into = source;
                        stood_on = Some(standing);
                        handle
                    }
                    Source::Connection { settings, open, .. } => open(
                        settings.as_ptr(),
                        settings.len() as u64,
                        plugin.user_data as *mut c_void,
                    ),
                }
            } else {
                standing
            };
            if handle.is_null() {
                crate::host_fs::HostSource::close(let_into);
                return;
            }
            let wanted = match CString::new(file_path.clone()) {
                Ok(v) => v,
                Err(_) => {
                    if opened_here {
                        (plugin.close_fn().expect("checked at registration"))(handle);
                        crate::host_fs::HostSource::close(let_into);
                    }
                    return;
                }
            };
            let owned = {
                // The mount may be answering the panel at the same time, and a
                // plugin holds its handle as one borrow of its own.
                let _inside = busy.lock();
                let content =
                    (plugin.read_fn().expect("checked at registration"))(handle, wanted.as_ptr());
                content.as_slice().to_vec()
            };
            if opened_here {
                (plugin.close_fn().expect("checked at registration"))(handle);
                crate::host_fs::HostSource::close(let_into);
            }
            drop(stood_on);
            if owned.is_empty() {
                return;
            }
            let leaf = std::path::Path::new(&file_path)
                .file_name()
                .map(|n| n.to_owned())
                .unwrap_or_default();
            let mut target = std::env::temp_dir();
            target.push(leaf);
            if std::fs::write(&target, owned).is_ok() {
                open_externally(&target);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_registered_filesystem_is_visible_from_another_thread() {
        let _lease = lease_registry_for_test();
        let exts = CString::new(".probe").unwrap();
        assert_eq!(
            register(exts.as_ptr(), &table(), std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        assert!(handles_extension("x.probe"));
        let seen = std::thread::spawn(|| handles_extension("x.probe"))
            .join()
            .expect("the probe thread finished");
        assert!(
            seen,
            "a blocking-pool thread must see the same registry as the main thread"
        );
    }

    #[test]
    fn only_the_declared_extensions_match() {
        let _lease = lease_registry_for_test();
        let exts = CString::new(".probe,.pr2").unwrap();
        register(exts.as_ptr(), &table(), std::ptr::null_mut());
        assert!(handles_extension("a.probe"));
        assert!(handles_extension("a.pr2"));
        assert!(!handles_extension("a.zip"));
    }

    #[test]
    fn a_path_crossing_a_plugin_filesystem_is_recognised_by_segment() {
        let _lease = lease_registry_for_test();
        let exts = CString::new(".probe").unwrap();
        register(exts.as_ptr(), &table(), std::ptr::null_mut());
        assert!(path_crosses_plugin_fs("/home/u/a.probe/inner"));
        assert!(
            !path_crosses_plugin_fs("/home/u/my.probeworks/inner"),
            "a longer name that merely contains the extension must not match"
        );
    }

    #[test]
    fn the_label_for_a_mount_point_comes_from_what_the_plugin_claims() {
        let _lease = lease_registry_for_test();
        let exts = std::ffi::CString::new(".zip,.tar.gz,.tbz2").unwrap();
        let table = table();
        assert_eq!(
            register(exts.as_ptr(), &table, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        assert_eq!(mount_label("holiday.zip").as_deref(), Some("ZIP"));
        assert_eq!(mount_label("HOLIDAY.ZIP").as_deref(), Some("ZIP"));
        assert_eq!(mount_label("backup.tar.gz").as_deref(), Some("TAR"));
        assert_eq!(mount_label("backup.tbz2").as_deref(), Some("TBZ2"));
        assert_eq!(
            mount_label("notes.txt"),
            None,
            "the app knows no formats of its own"
        );
    }

    #[test]
    fn a_path_that_crosses_a_plugin_filesystem_splits_at_the_mount() {
        let _lease = lease_registry_for_test();
        let exts = std::ffi::CString::new(".zip").unwrap();
        let table = table();
        assert_eq!(
            register(exts.as_ptr(), &table, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        assert_eq!(
            split_at_plugin_fs("/home/u/photos.zip/holiday/a.jpg"),
            Some((
                "/home/u/photos.zip".to_string(),
                "holiday/a.jpg".to_string()
            ))
        );
        assert_eq!(
            split_at_plugin_fs("/home/u/photos.zip"),
            Some(("/home/u/photos.zip".to_string(), String::new()))
        );
        assert_eq!(split_at_plugin_fs("/home/u/notes.txt"), None);
    }

    #[test]
    fn the_innermost_plugin_filesystem_wins_the_split() {
        let _lease = lease_registry_for_test();
        let exts = std::ffi::CString::new(".zip").unwrap();
        let table = table();
        assert_eq!(
            register(exts.as_ptr(), &table, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        assert_eq!(
            split_at_plugin_fs("/a/outer.zip/inner.zip/deep.txt"),
            Some(("/a/outer.zip/inner.zip".to_string(), "deep.txt".to_string()))
        );
    }

    fn where_list_ran() -> &'static std::sync::Mutex<Option<std::thread::ThreadId>> {
        static SEEN: std::sync::OnceLock<std::sync::Mutex<Option<std::thread::ThreadId>>> =
            std::sync::OnceLock::new();
        SEEN.get_or_init(|| std::sync::Mutex::new(None))
    }

    extern "C" fn open_connection(
        _: *const u8,
        _: u64,
        _: *mut c_void,
    ) -> ic_plugin_api::IcFsHandle {
        1 as ic_plugin_api::IcFsHandle
    }

    extern "C" fn list_recording_its_thread(
        _: ic_plugin_api::IcFsHandle,
        _: *const c_char,
    ) -> ic_plugin_api::IcListing {
        *where_list_ran().lock().unwrap() = Some(std::thread::current().id());
        ic_plugin_api::IcListing {
            items: std::ptr::null(),
            count: 0,
        }
    }

    fn recording_plugin() -> (FsPlugin, RegistryLease) {
        let lease = lease_registry_for_test();
        let mut table = table();
        table.list = list_recording_its_thread;
        let exts = CString::new(".probe").unwrap();
        assert_eq!(
            register(exts.as_ptr(), &table, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        (filesystem_for("x.probe").expect("registered"), lease)
    }

    #[tokio::test]
    async fn a_filesystem_that_reaches_outside_is_called_off_the_thread_that_asked() {
        let (plugin, _lease) = recording_plugin();
        *where_list_ran().lock().unwrap() = None;
        let fs = PluginFsRpc::for_connection(
            plugin,
            open_connection,
            "probe".to_string(),
            "{}".to_string(),
        );
        crate::rpc::FileSystemRpc::list_dir(&fs, String::new())
            .await
            .expect("the stub lists nothing without failing");
        let ran_on = where_list_ran()
            .lock()
            .unwrap()
            .expect("the plugin was called");
        assert_ne!(
            ran_on,
            std::thread::current().id(),
            "a connection filesystem must not block the thread that drives the interface"
        );
    }

    #[tokio::test]
    async fn a_filesystem_nested_over_a_parent_stays_on_the_calling_thread() {
        struct Parent;
        #[async_trait::async_trait(?Send)]
        impl crate::rpc::FileSystemRpc for Parent {
            async fn read_file(
                &self,
                _: String,
                _: Option<Box<dyn Fn(u64) + 'static>>,
            ) -> Result<Vec<u8>, common::AppError> {
                Ok(Vec::new())
            }
        }
        let (plugin, _lease) = recording_plugin();
        *where_list_ran().lock().unwrap() = None;
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(Parent);
        let fs = PluginFsRpc::new(plugin, "a.probe".to_string(), parent);
        crate::rpc::FileSystemRpc::list_dir(&fs, String::new())
            .await
            .expect("the stub lists nothing without failing");
        let ran_on = where_list_ran()
            .lock()
            .unwrap()
            .expect("the plugin was called");
        assert_eq!(
            ran_on,
            std::thread::current().id(),
            "work already in memory costs more to move to a thread than to do"
        );
    }

    /// A server keeps its name between connections on purpose, so two tabs on
    /// one server share what they read. The price is that a *new* connection
    /// would otherwise be handed the last session's listings — so opening one
    /// is the moment everything under that name has to be looked at again.
    #[tokio::test]
    async fn connecting_again_looks_at_the_server_afresh() {
        use crate::rpc::FileSystemRpc;
        let (plugin, _lease) = recording_plugin();
        crate::listing::forget_everything();

        let first = PluginFsRpc::for_connection(
            plugin.clone(),
            open_connection,
            "probe".to_string(),
            "{\"host\":\"example\"}".to_string(),
        );
        let as_trait: std::rc::Rc<dyn FileSystemRpc> = std::rc::Rc::new(first);
        crate::listing::list(&as_trait, "/docs", crate::listing::Freshness::Fresh)
            .await
            .expect("a listing");
        assert!(crate::listing::is_remembered(as_trait.as_ref(), "/docs"));

        // The same server, opened again: the same name, and a fresh look.
        let again = PluginFsRpc::for_connection(
            plugin,
            open_connection,
            "probe".to_string(),
            "{\"host\":\"example\"}".to_string(),
        );
        let again_trait: std::rc::Rc<dyn FileSystemRpc> = std::rc::Rc::new(again);
        assert_eq!(
            again_trait.fs_id(),
            as_trait.fs_id(),
            "two mounts of one server must share what they read"
        );
        crate::rpc::FileSystemRpc::list_dir(again_trait.as_ref(), String::new())
            .await
            .expect("opening the connection");

        assert!(
            !crate::listing::is_remembered(as_trait.as_ref(), "/docs"),
            "the new connection was handed the last session's listing"
        );
        assert!(
            crate::listing::remembered_of(as_trait.as_ref(), "/docs").is_some(),
            "and there should still be something to draw until it is read"
        );
    }

    /// An archive on a server is as far away as the server. Its work belongs
    /// on a thread of its own, or a listing inside it stops the application
    /// for as long as the network takes.
    #[tokio::test]
    async fn a_filesystem_nested_over_something_remote_is_called_off_the_thread_that_asked() {
        struct RemoteParent;
        #[async_trait::async_trait(?Send)]
        impl crate::rpc::FileSystemRpc for RemoteParent {
            async fn read_file(
                &self,
                _: String,
                _: Option<Box<dyn Fn(u64) + 'static>>,
            ) -> Result<Vec<u8>, common::AppError> {
                Ok(Vec::new())
            }
            fn runs_off_thread(&self) -> bool {
                true
            }
        }
        let (plugin, _lease) = recording_plugin();
        *where_list_ran().lock().unwrap() = None;
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(RemoteParent);
        let fs = PluginFsRpc::new(plugin, "a.probe".to_string(), parent);
        crate::rpc::FileSystemRpc::list_dir(&fs, String::new())
            .await
            .expect("the stub lists nothing without failing");
        let ran_on = where_list_ran()
            .lock()
            .unwrap()
            .expect("the plugin was called");
        assert_ne!(
            ran_on,
            std::thread::current().id(),
            "an archive on a server was listed on the thread that asked for it"
        );
    }

    use super::*;

    extern "C" fn open_in(
        _: ic_plugin_api::IcFsSource,
        _: *const c_char,
        _: *mut c_void,
    ) -> ic_plugin_api::IcFsHandle {
        1usize as ic_plugin_api::IcFsHandle
    }
    extern "C" fn close(_: ic_plugin_api::IcFsHandle) {}
    extern "C" fn list(_: ic_plugin_api::IcFsHandle, _: *const c_char) -> ic_plugin_api::IcListing {
        ic_plugin_api::IcListing::EMPTY
    }
    extern "C" fn read(_: ic_plugin_api::IcFsHandle, _: *const c_char) -> ic_plugin_api::IcBytes {
        ic_plugin_api::IcBytes::EMPTY
    }
    extern "C" fn read_only(_: ic_plugin_api::IcFsHandle) -> c_int {
        1
    }
    extern "C" fn last_error(_: ic_plugin_api::IcFsHandle) -> *const c_char {
        std::ptr::null()
    }

    fn table() -> ic_plugin_api::IcFsVTable {
        ic_plugin_api::IcFsVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcFsVTable>() as u32,
            open_in,
            close,
            list,
            read,
            is_read_only: read_only,
            last_error,
            write: None,
            create_dir: None,
            remove: None,
            rename: None,
            shell_open: None,
            shell_read: None,
            shell_write: None,
            shell_resize: None,
            shell_close: None,
            shell_available: None,
            columns: None,
            list_rows: None,
            action_state: None,
            cell_clicked: None,
        }
    }
    struct SlowParent {
        opened: std::rc::Rc<std::cell::Cell<usize>>,
    }

    #[async_trait::async_trait(?Send)]
    impl crate::rpc::FileSystemRpc for SlowParent {
        async fn read_file(
            &self,
            _path: String,
            _cb: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<Vec<u8>, common::AppError> {
            self.opened.set(self.opened.get() + 1);
            Ok(vec![1, 2, 3])
        }

        fn content_wait(&self) -> crate::rpc::ContentWait {
            crate::rpc::ContentWait::Bounded(std::time::Duration::from_secs(7))
        }
    }

    /// A parent filesystem that keeps what was written into it, standing for
    /// the disk the archive file lives on.
    struct HoldingParent {
        put_back: std::rc::Rc<RefCell<Vec<(String, Vec<u8>)>>>,
        holds: Vec<String>,
    }

    #[async_trait::async_trait(?Send)]
    impl crate::rpc::FileSystemRpc for HoldingParent {
        async fn read_file(
            &self,
            _path: String,
            _cb: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<Vec<u8>, common::AppError> {
            Ok(vec![1, 2, 3])
        }

        async fn list_dir(
            &self,
            _path: String,
        ) -> Result<Vec<crate::rpc::RemoteFileEntry>, common::AppError> {
            Ok(self
                .holds
                .iter()
                .map(|name| crate::rpc::RemoteFileEntry {
                    name: name.trim_end_matches('/').to_string(),
                    is_dir: name.ends_with('/'),
                    ..Default::default()
                })
                .collect())
        }

        async fn write_file(
            &self,
            path: String,
            content: Vec<u8>,
            _permissions: Option<u32>,
            _cb: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<(), common::AppError> {
            self.put_back.borrow_mut().push((path, content));
            Ok(())
        }
    }

    thread_local! {
        static WRITES: RefCell<Vec<(String, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
        static DIRS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static REMOVED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static RENAMED: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
        static REFUSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static REASON: RefCell<Option<CString>> = const { RefCell::new(None) };
    }

    fn forget_recorded_calls() {
        WRITES.with(|w| w.borrow_mut().clear());
        DIRS.with(|d| d.borrow_mut().clear());
        REMOVED.with(|r| r.borrow_mut().clear());
        RENAMED.with(|r| r.borrow_mut().clear());
        REFUSE.with(|r| r.set(false));
    }

    extern "C" fn refused() -> c_int {
        REASON.with(|slot| {
            *slot.borrow_mut() = Some(CString::new("the archive is sealed").unwrap());
        });
        -1
    }

    extern "C" fn stub_write(
        _: ic_plugin_api::IcFsHandle,
        path: *const c_char,
        bytes: *const u8,
        len: u64,
    ) -> c_int {
        if REFUSE.with(|r| r.get()) {
            return refused();
        }
        let content = if bytes.is_null() {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(bytes, len as usize) }.to_vec()
        };
        WRITES.with(|w| w.borrow_mut().push((cstr(path), content)));
        stub_writes_itself_out();
        ic_plugin_api::IC_OK
    }

    extern "C" fn stub_create_dir(_: ic_plugin_api::IcFsHandle, path: *const c_char) -> c_int {
        DIRS.with(|d| d.borrow_mut().push(cstr(path)));
        stub_writes_itself_out();
        ic_plugin_api::IC_OK
    }

    extern "C" fn stub_remove(_: ic_plugin_api::IcFsHandle, path: *const c_char) -> c_int {
        let name = cstr(path);
        if name.ends_with(".keep") {
            return refused();
        }
        REMOVED.with(|r| r.borrow_mut().push(name));
        stub_writes_itself_out();
        ic_plugin_api::IC_OK
    }

    extern "C" fn stub_rename(
        _: ic_plugin_api::IcFsHandle,
        from: *const c_char,
        to: *const c_char,
    ) -> c_int {
        RENAMED.with(|r| r.borrow_mut().push((cstr(from), cstr(to))));
        stub_writes_itself_out();
        ic_plugin_api::IC_OK
    }

    extern "C" fn stub_last_error(_: ic_plugin_api::IcFsHandle) -> *const c_char {
        REASON.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|text| text.as_ptr())
                .unwrap_or(std::ptr::null())
        })
    }

    extern "C" fn writable(_: ic_plugin_api::IcFsHandle) -> c_int {
        0
    }

    thread_local! {
        /// What the mount would hand back as the whole file, and `None` for a
        /// mount that cannot put itself together again.
        static WHOLE: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    }

    thread_local! {
        /// The source and the file name the stub mount was opened on, so it
        /// can write into that file the way a real plugin does: through the
        /// host, never by handing bytes back.
        static OPENED_ON: RefCell<(ic_plugin_api::IcFsSource, String)> =
            RefCell::new((std::ptr::null_mut(), String::new()));
    }

    extern "C" fn stub_open_in(
        source: ic_plugin_api::IcFsSource,
        path: *const c_char,
        _: *mut c_void,
    ) -> ic_plugin_api::IcFsHandle {
        OPENED_ON.with(|held| *held.borrow_mut() = (source, cstr(path)));
        1usize as ic_plugin_api::IcFsHandle
    }

    /// What a plugin does when it has changed what it holds: it writes the
    /// file it was opened on, through the host.
    fn stub_writes_itself_out() {
        let Some(bytes) = WHOLE.with(|slot| slot.borrow().clone()) else {
            return;
        };
        OPENED_ON.with(|held| {
            let (source, name) = &*held.borrow();
            let Ok(named) = CString::new(name.clone()) else {
                return;
            };
            let stream =
                crate::host_fs::fs_open(*source, named.as_ptr(), ic_plugin_api::IC_OPEN_WRITE);
            if stream.is_null() {
                return;
            }
            crate::host_fs::fs_write(stream, bytes.as_ptr(), bytes.len() as u64);
            crate::host_fs::fs_close(stream);
        });
    }

    fn writable_table() -> ic_plugin_api::IcFsVTable {
        ic_plugin_api::IcFsVTable {
            open_in: stub_open_in,
            is_read_only: writable,
            last_error: stub_last_error,
            write: Some(stub_write),
            create_dir: Some(stub_create_dir),
            remove: Some(stub_remove),
            rename: Some(stub_rename),
            ..table()
        }
    }

    thread_local! {
        static COLUMNS_DECLARED: std::cell::Cell<usize> = const { std::cell::Cell::new(2) };
        static CELLS_PER_ROW: std::cell::Cell<usize> = const { std::cell::Cell::new(2) };
    }

    extern "C" fn stub_columns(_: ic_plugin_api::IcFsHandle) -> ic_plugin_api::IcColumns {
        let declared = COLUMNS_DECLARED.with(|c| c.get());
        let all = [
            (c"status", c"Status", 120),
            (c"done", c"Done", 0),
            (c"peers", c"Peers", 60),
        ];
        let columns: Vec<ic_plugin_api::IcFsColumn> = all[..declared]
            .iter()
            .map(|(key, title, width)| ic_plugin_api::IcFsColumn {
                key: key.as_ptr(),
                title: title.as_ptr(),
                width: *width,
                kind: ic_plugin_api::IC_COLUMN_TEXT,
            })
            .collect();
        let held = Box::leak(columns.into_boxed_slice());
        ic_plugin_api::IcColumns {
            count: held.len() as u32,
            items: held.as_ptr(),
            ..ic_plugin_api::IcColumns::EMPTY
        }
    }

    extern "C" fn stub_rows(
        _: ic_plugin_api::IcFsHandle,
        _: *const c_char,
    ) -> ic_plugin_api::IcRows {
        let per_row = CELLS_PER_ROW.with(|c| c.get());
        let all = [c"seeding", c"41%", c"12"];
        let cells: Vec<*const c_char> = all[..per_row].iter().map(|s| s.as_ptr()).collect();
        let cells = Box::leak(cells.into_boxed_slice());
        let rows = vec![ic_plugin_api::IcRow {
            entry: ic_plugin_api::IcDirEntry {
                name: c"big.iso".as_ptr(),
                is_dir: 0,
                size: 4096,
                modified: 1700,
                permissions: 0,
                has_permissions: 0,
            },
            extra: cells.as_ptr(),
            extra_count: cells.len() as u32,
        }];
        let rows = Box::leak(rows.into_boxed_slice());
        ic_plugin_api::IcRows {
            count: rows.len() as u32,
            items: rows.as_ptr(),
            ..ic_plugin_api::IcRows::EMPTY
        }
    }

    fn filesystem_with_columns(extension: &str, declared: usize, per_row: usize) -> PluginFsRpc {
        COLUMNS_DECLARED.with(|c| c.set(declared));
        CELLS_PER_ROW.with(|c| c.set(per_row));
        let mut vtable = table();
        vtable.columns = Some(stub_columns);
        vtable.list_rows = Some(stub_rows);
        let exts = CString::new(extension).unwrap();
        assert_eq!(
            register(exts.as_ptr(), &vtable, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let plugin = filesystem_for(&format!("x{extension}")).expect("registered");
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(SlowParent {
            opened: std::rc::Rc::new(std::cell::Cell::new(0)),
        });
        PluginFsRpc::new(plugin, format!("x{extension}"), parent)
    }

    thread_local! {
        static ASKED_ABOUT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    extern "C" fn stub_action_state(_: ic_plugin_api::IcFsHandle, action_id: *const c_char) -> u32 {
        let asked = cstr(action_id);
        ASKED_ABOUT.with(|seen| seen.borrow_mut().push(asked.clone()));
        match asked.as_str() {
            "running" => ic_plugin_api::IC_ACTION_SHOWN | ic_plugin_api::IC_ACTION_ON,
            "gone" => 0,
            _ => ic_plugin_api::IC_ACTION_DEFAULT,
        }
    }

    fn filesystem_with_button_states(extension: &str) -> PluginFsRpc {
        ASKED_ABOUT.with(|seen| seen.borrow_mut().clear());
        let mut vtable = table();
        vtable.action_state = Some(stub_action_state);
        let exts = CString::new(extension).unwrap();
        assert_eq!(
            register(exts.as_ptr(), &vtable, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let plugin = filesystem_for(&format!("x{extension}")).expect("registered");
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(SlowParent {
            opened: std::rc::Rc::new(std::cell::Cell::new(0)),
        });
        PluginFsRpc::new(plugin, format!("x{extension}"), parent)
    }

    thread_local! {
        static CLICKS: RefCell<Vec<(String, String, String, bool)>> =
            const { RefCell::new(Vec::new()) };
        static REFUSE_CLICK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    extern "C" fn stub_columns_with_a_tick(
        _: ic_plugin_api::IcFsHandle,
    ) -> ic_plugin_api::IcColumns {
        let columns = vec![
            ic_plugin_api::IcFsColumn {
                key: c"fetch".as_ptr(),
                title: c"Fetch".as_ptr(),
                width: 60,
                kind: ic_plugin_api::IC_COLUMN_CHECK,
            },
            ic_plugin_api::IcFsColumn {
                key: c"status".as_ptr(),
                title: c"Status".as_ptr(),
                width: 110,
                kind: ic_plugin_api::IC_COLUMN_TEXT,
            },
        ];
        let held = Box::leak(columns.into_boxed_slice());
        ic_plugin_api::IcColumns {
            count: held.len() as u32,
            items: held.as_ptr(),
            ..ic_plugin_api::IcColumns::EMPTY
        }
    }

    extern "C" fn stub_cell_clicked(
        _: ic_plugin_api::IcFsHandle,
        dir: *const c_char,
        name: *const c_char,
        column: *const c_char,
        ticked: c_int,
    ) -> c_int {
        if REFUSE_CLICK.with(|r| r.get()) {
            return ic_plugin_api::IC_ERR_INIT_FAILED;
        }
        CLICKS.with(|seen| {
            seen.borrow_mut()
                .push((cstr(dir), cstr(name), cstr(column), ticked != 0))
        });
        ic_plugin_api::IC_OK
    }

    fn filesystem_with_a_tick_column(extension: &str) -> PluginFsRpc {
        CLICKS.with(|seen| seen.borrow_mut().clear());
        REFUSE_CLICK.with(|r| r.set(false));
        let mut vtable = table();
        vtable.columns = Some(stub_columns_with_a_tick);
        vtable.list_rows = Some(stub_rows);
        vtable.cell_clicked = Some(stub_cell_clicked);
        let exts = CString::new(extension).unwrap();
        assert_eq!(
            register(exts.as_ptr(), &vtable, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let plugin = filesystem_for(&format!("x{extension}")).expect("registered");
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(SlowParent {
            opened: std::rc::Rc::new(std::cell::Cell::new(0)),
        });
        PluginFsRpc::new(plugin, format!("x{extension}"), parent)
    }

    thread_local! {
        /// How many times the plugin was asked for a button's state.
        static STATE_ASKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    extern "C" fn counted_action_state(
        _handle: ic_plugin_api::IcFsHandle,
        _action_id: *const c_char,
    ) -> u32 {
        STATE_ASKS.with(|n| n.set(n.get() + 1));
        ic_plugin_api::IC_ACTION_SHOWN
    }

    fn filesystem_with_a_toolbar(extension: &str) -> PluginFsRpc {
        STATE_ASKS.with(|n| n.set(0));
        let mut vtable = table();
        vtable.action_state = Some(counted_action_state);
        let exts = CString::new(extension).unwrap();
        assert_eq!(
            register(exts.as_ptr(), &vtable, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let plugin = filesystem_for(&format!("x{extension}")).expect("registered");
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(SlowParent {
            opened: std::rc::Rc::new(std::cell::Cell::new(0)),
        });
        PluginFsRpc::new(plugin, format!("x{extension}"), parent)
    }

    /// Fetching one file out of an archive used to open the archive a second
    /// time — reading the whole container again for the sake of one entry, and
    /// over a network that is the whole of it. A mount that is already open
    /// answers from the handle it has.
    #[tokio::test]
    async fn downloading_from_an_open_mount_does_not_read_the_container_again() {
        use crate::rpc::FileSystemRpc;
        // `recording_plugin` takes the registry lease itself; taking a second
        // one here is a deadlock, not a precaution.
        let (plugin, _lease) = recording_plugin();
        let reads = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(SlowParent {
            opened: reads.clone(),
        });
        let fs = PluginFsRpc::new(plugin, "a.probe".to_string(), parent);

        // Standing in it: one read of the container, at opening.
        fs.list_dir(String::new()).await.expect("a listing");
        assert_eq!(reads.get(), 1, "opening reads the container once");

        // Whatever the download does afterwards, it must not read it again.
        let ran: std::rc::Rc<
            std::cell::RefCell<Vec<std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>>>,
        > = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let collecting = ran.clone();
        set_spawner(std::rc::Rc::new(move |future| {
            collecting.borrow_mut().push(future);
        }));
        fs.request_file_download("one".to_string(), uuid::Uuid::nil());
        let queued = std::mem::take(&mut *ran.borrow_mut());
        for future in queued {
            future.await;
        }
        assert_eq!(
            reads.get(),
            1,
            "the container was read again to fetch one file out of it"
        );
    }

    /// The thread that draws asks a mount small questions while a worker
    /// thread may be inside the same plugin handle for a listing. Two callers
    /// in one handle is what a plugin cannot defend itself against, so the
    /// drawing side never enters a mount that is busy — it answers with what
    /// it heard last time, and does not wait.
    #[test]
    fn a_busy_mount_is_not_entered_by_the_thread_that_draws() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_a_toolbar(".busyprobe");
        // A mount that was never opened answers nothing and asks nothing.
        assert_eq!(
            fs.plugin_action_state("go"),
            ic_plugin_api::IC_ACTION_DEFAULT
        );
        assert_eq!(STATE_ASKS.with(|n| n.get()), 0);

        // Standing in for an open mount: the handle is what the plugin gave.
        fs.handle.set(1usize as ic_plugin_api::IcFsHandle);
        assert_eq!(fs.plugin_action_state("go"), ic_plugin_api::IC_ACTION_SHOWN);
        assert_eq!(STATE_ASKS.with(|n| n.get()), 1, "it asked the plugin once");

        // Now the plugin is inside a call of its own.
        let held = fs.busy.clone();
        let inside = held.lock().expect("the lock");
        assert_eq!(
            fs.plugin_action_state("go"),
            ic_plugin_api::IC_ACTION_SHOWN,
            "it should answer with what it heard last"
        );
        assert_eq!(
            STATE_ASKS.with(|n| n.get()),
            1,
            "it entered a plugin that was already busy"
        );
        drop(inside);

        // Free again, and it asks for itself once more.
        assert_eq!(fs.plugin_action_state("go"), ic_plugin_api::IC_ACTION_SHOWN);
        assert_eq!(STATE_ASKS.with(|n| n.get()), 2);
    }

    /// Nothing was ever asked, and the plugin is busy: the answer is the
    /// application's own default rather than a wait.
    #[test]
    fn a_question_nobody_asked_before_falls_back_rather_than_waiting() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_a_toolbar(".busyprobe2");
        fs.handle.set(1usize as ic_plugin_api::IcFsHandle);
        let held = fs.busy.clone();
        let inside = held.lock().expect("the lock");
        assert_eq!(
            fs.plugin_action_state("go"),
            ic_plugin_api::IC_ACTION_DEFAULT
        );
        assert_eq!(STATE_ASKS.with(|n| n.get()), 0);
        drop(inside);
    }

    #[test]
    fn a_mount_can_declare_a_column_of_tick_boxes() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        CELLS_PER_ROW.with(|c| c.set(2));
        let fs = filesystem_with_a_tick_column(".tickprobe");
        futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        let columns = fs.extra_columns();
        assert_eq!(
            columns
                .iter()
                .map(|c| (c.key.as_str(), c.kind))
                .collect::<Vec<_>>(),
            vec![
                ("fetch", crate::rpc::ColumnKind::Check),
                ("status", crate::rpc::ColumnKind::Text)
            ]
        );
    }

    #[test]
    fn clicking_a_tick_reaches_the_plugin_with_where_and_what() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_a_tick_column(".clickprobe");
        futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        assert!(fs.toggle_cell("/disc", "one.flac", "fetch", true));
        assert_eq!(
            CLICKS.with(|seen| seen.borrow().clone()),
            vec![(
                "/disc".to_string(),
                "one.flac".to_string(),
                "fetch".to_string(),
                true
            )]
        );
        assert!(fs.toggle_cell("/disc", "one.flac", "fetch", false));
        assert_eq!(
            CLICKS.with(|seen| seen.borrow().last().cloned()),
            Some((
                "/disc".to_string(),
                "one.flac".to_string(),
                "fetch".to_string(),
                false
            ))
        );
    }

    #[test]
    fn a_plugin_that_refuses_a_click_says_so_rather_than_being_assumed() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_a_tick_column(".refuseprobe");
        futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        REFUSE_CLICK.with(|r| r.set(true));
        assert!(
            !fs.toggle_cell("/", "one.flac", "fetch", true),
            "the panel must not list again over a click that was not taken"
        );
    }

    #[test]
    fn a_mount_with_no_ticks_never_hears_about_a_click() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_columns(".notickprobe", 1, 1);
        futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        assert!(!fs.toggle_cell("/", "big.iso", "status", true));
    }

    #[test]
    fn a_click_before_the_mount_is_open_goes_nowhere() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_a_tick_column(".earlyprobe");
        // Nothing has been listed, so there is no handle to click through.
        assert!(!fs.toggle_cell("/", "one.flac", "fetch", true));
        assert!(CLICKS.with(|seen| seen.borrow().is_empty()));
    }

    #[test]
    fn what_the_application_provides_itself_takes_no_clicks() {
        use crate::rpc::FileSystemRpc;
        struct Plain;
        #[async_trait::async_trait(?Send)]
        impl crate::rpc::FileSystemRpc for Plain {}
        assert!(!Plain.toggle_cell("/", "a", "b", true));
    }

    #[test]
    fn a_mount_says_which_of_its_buttons_are_pressed_and_which_are_gone() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_button_states(".stateprobe");
        // Opening the mount is what gives the plugin something to answer about.
        futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        let running = fs.plugin_action_state("running");
        assert!(running & ic_plugin_api::IC_ACTION_SHOWN != 0);
        assert!(running & ic_plugin_api::IC_ACTION_ON != 0);
        assert!(
            running & ic_plugin_api::IC_ACTION_ENABLED == 0,
            "a view already being shown is not worth pressing again"
        );
        assert_eq!(fs.plugin_action_state("gone"), 0);
        assert_eq!(
            fs.plugin_action_state("ordinary"),
            ic_plugin_api::IC_ACTION_DEFAULT
        );
        assert_eq!(
            ASKED_ABOUT.with(|seen| seen.borrow().clone()),
            vec![
                "running".to_string(),
                "gone".to_string(),
                "ordinary".to_string()
            ],
            "each button is asked about by its own id"
        );
    }

    #[test]
    fn a_mount_that_is_not_open_yet_leaves_every_button_alone() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_button_states(".unopenedprobe");
        // Nothing has been listed, so there is no handle to ask through.
        assert_eq!(
            fs.plugin_action_state("gone"),
            ic_plugin_api::IC_ACTION_DEFAULT
        );
        assert!(ASKED_ABOUT.with(|seen| seen.borrow().is_empty()));
    }

    #[test]
    fn a_plugin_with_no_opinion_leaves_its_buttons_there_and_usable() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = writable_filesystem(".noopinionprobe");
        futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        assert_eq!(
            fs.plugin_action_state("anything"),
            ic_plugin_api::IC_ACTION_DEFAULT
        );
    }

    #[test]
    fn a_mount_names_the_extension_it_was_opened_through() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_columns(".scopeprobe", 1, 1);
        assert_eq!(fs.plugin_scope().as_deref(), Some(".scopeprobe"));
    }

    #[test]
    fn the_longest_extension_is_the_one_a_mount_is_scoped_by() {
        let _lease = lease_registry_for_test();
        let exts = CString::new(".gz,.tar.gz").unwrap();
        assert_eq!(
            register(exts.as_ptr(), &table(), std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let plugin = filesystem_for("holiday.tar.gz").expect("registered");
        assert_eq!(
            plugin.extension_matching("holiday.tar.gz").as_deref(),
            Some(".tar.gz")
        );
        assert_eq!(
            plugin.extension_matching("notes.gz").as_deref(),
            Some(".gz")
        );
        assert!(plugin.extension_matching("notes.txt").is_none());
    }

    #[test]
    fn what_the_application_provides_itself_is_in_no_plugins_scope() {
        use crate::rpc::FileSystemRpc;
        struct Plain;
        #[async_trait::async_trait(?Send)]
        impl crate::rpc::FileSystemRpc for Plain {}
        assert!(Plain.plugin_scope().is_none());
    }

    #[test]
    fn a_plugin_that_offers_rows_fills_the_columns_it_declared() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_columns(".colprobe", 2, 2);
        let listed = futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "big.iso");
        assert_eq!(listed[0].size, 4096);
        assert_eq!(
            listed[0].extra,
            vec!["seeding".to_string(), "41%".to_string()]
        );
        let columns = fs.extra_columns();
        assert_eq!(
            columns
                .iter()
                .map(|c| (c.key.as_str(), c.title.as_str(), c.width))
                .collect::<Vec<_>>(),
            vec![("status", "Status", Some(120)), ("done", "Done", None)]
        );
    }

    #[test]
    fn the_plugins_columns_are_added_to_the_panels_own_never_instead_of_them() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_columns(".addprobe", 2, 2);
        futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        assert!(
            !fs.columns_replace_defaults(),
            "name, size and date must stay where the user expects them"
        );
    }

    #[test]
    fn a_row_shorter_than_the_declared_columns_is_padded_rather_than_read_past() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_columns(".shortprobe", 3, 1);
        let listed = futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        assert_eq!(
            listed[0].extra,
            vec!["seeding".to_string(), String::new(), String::new()]
        );
    }

    #[test]
    fn cells_beyond_the_declared_columns_are_dropped() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = filesystem_with_columns(".longprobe", 1, 3);
        let listed = futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        assert_eq!(listed[0].extra, vec!["seeding".to_string()]);
        assert_eq!(fs.extra_columns().len(), 1);
    }

    #[test]
    fn a_plugin_from_before_the_columns_existed_lists_as_it_always_did() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = writable_filesystem(".oldprobe");
        let listed = futures::executor::block_on(fs.list_dir("/".to_string())).expect("listed");
        assert!(listed.is_empty());
        assert!(fs.extra_columns().is_empty());
    }

    fn writable_filesystem(extension: &str) -> PluginFsRpc {
        forget_recorded_calls();
        let exts = CString::new(extension).unwrap();
        assert_eq!(
            register(exts.as_ptr(), &writable_table(), std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let plugin = filesystem_for(&format!("x{extension}")).expect("registered");
        assert!(plugin.supports_write());
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(SlowParent {
            opened: std::rc::Rc::new(std::cell::Cell::new(0)),
        });
        PluginFsRpc::new(plugin, format!("x{extension}"), parent)
    }

    /// A mount kept in a file of the parent's, with somewhere to put it back.
    fn nested_filesystem(
        extension: &str,
        whole: Option<Vec<u8>>,
    ) -> (PluginFsRpc, std::rc::Rc<RefCell<Vec<(String, Vec<u8>)>>>) {
        forget_recorded_calls();
        WHOLE.with(|slot| *slot.borrow_mut() = whole);
        let vtable = writable_table();
        let exts = CString::new(extension).unwrap();
        assert_eq!(
            register(exts.as_ptr(), &vtable, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let plugin = filesystem_for(&format!("x{extension}")).expect("registered");
        let put_back = std::rc::Rc::new(RefCell::new(Vec::new()));
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(HoldingParent {
            put_back: put_back.clone(),
            holds: Vec::new(),
        });
        (
            PluginFsRpc::new(plugin, format!("x{extension}"), parent),
            put_back,
        )
    }

    fn holding(
        holds: &[&str],
    ) -> (
        std::rc::Rc<dyn crate::rpc::FileSystemRpc>,
        std::rc::Rc<RefCell<Vec<(String, Vec<u8>)>>>,
    ) {
        let put_back = std::rc::Rc::new(RefCell::new(Vec::new()));
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(HoldingParent {
            put_back: put_back.clone(),
            holds: holds.iter().map(|name| (*name).to_string()).collect(),
        });
        (parent, put_back)
    }

    #[test]
    fn a_copy_into_a_name_a_plugin_claims_makes_the_file_and_goes_inside_it() {
        let _lease = lease_registry_for_test();
        forget_recorded_calls();
        WHOLE.with(|slot| *slot.borrow_mut() = None);
        let vtable = writable_table();
        let exts = CString::new(".destprobe").unwrap();
        assert_eq!(
            register(exts.as_ptr(), &vtable, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );

        let (parent, put_back) = holding(&[]);
        let inside =
            futures::executor::block_on(destination_inside(&parent, "/home/u/work.destprobe"))
                .expect("the name is one a plugin claims")
                .expect("the file is made");
        assert_eq!(
            put_back.borrow().as_slice(),
            [("/home/u/work.destprobe".to_string(), Vec::new())],
            "a destination that is not there yet is made empty, and the plugin \
             writes the container itself when something is copied in"
        );
        assert!(
            inside.as_any().is_some(),
            "and what comes back is the mount, not the folder it sits in"
        );

        // One that is already there is opened, not written over.
        let (held, put_back) = holding(&["work.destprobe"]);
        futures::executor::block_on(destination_inside(&held, "/home/u/work.destprobe"))
            .expect("claimed")
            .expect("opened");
        assert!(
            put_back.borrow().is_empty(),
            "an archive that exists is not replaced by an empty one"
        );
    }

    #[test]
    fn a_folder_named_like_an_archive_is_still_a_folder() {
        let _lease = lease_registry_for_test();
        forget_recorded_calls();
        let vtable = writable_table();
        let exts = CString::new(".dirprobe").unwrap();
        assert_eq!(
            register(exts.as_ptr(), &vtable, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let (parent, _) = holding(&["work.dirprobe/"]);
        assert!(
            futures::executor::block_on(destination_inside(&parent, "/home/u/work.dirprobe"))
                .is_none(),
            "what is already a folder is copied into as a folder"
        );
    }

    #[test]
    fn a_copy_into_an_ordinary_folder_is_left_alone() {
        let _lease = lease_registry_for_test();
        let (parent, _) = holding(&[]);
        assert!(
            futures::executor::block_on(destination_inside(&parent, "/home/u/documents")).is_none(),
            "nothing claims that name, so it is a folder like any other"
        );
    }

    #[test]
    fn writing_into_a_nested_mount_puts_the_whole_file_back_where_it_came_from() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let (fs, put_back) =
            nested_filesystem(".packprobe", Some(b"the archive, rewritten".to_vec()));

        futures::executor::block_on(fs.write_file(
            "/added.txt".to_string(),
            b"fresh".to_vec(),
            None,
            None,
        ))
        .expect("the mount took the file");
        assert_eq!(
            put_back.borrow().as_slice(),
            [(
                "x.packprobe".to_string(),
                b"the archive, rewritten".to_vec()
            )],
            "the file the mount lives in is written back whole, once"
        );

        put_back.borrow_mut().clear();
        futures::executor::block_on(fs.delete_entries(vec!["/added.txt".to_string()]))
            .expect("the mount removed it");
        assert_eq!(
            put_back.borrow().len(),
            1,
            "and again after anything else that changes it"
        );
    }

    /// A mount that changed nothing has nothing to carry back. Writing anyway
    /// would put a file over one that is already right — and on a server that
    /// is the whole of it, uploaded for no reason.
    #[test]
    fn a_mount_that_changed_nothing_is_not_written_back_over_its_own_file() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let (fs, put_back) = nested_filesystem(".packprobe2", None);

        futures::executor::block_on(fs.write_file(
            "/added.txt".to_string(),
            b"fresh".to_vec(),
            None,
            None,
        ))
        .expect("the mount took the file");
        assert!(
            put_back.borrow().is_empty(),
            "the plugin wrote nothing, so nothing goes back over the file it came from"
        );
    }

    /// An archive on a disk is opened where it lies: the plugin writes into
    /// the real file through the host, and there is nothing to carry back at
    /// all. This is what the whole change is for — a file of any size, opened
    /// without a copy of it in memory or on the disk beside it.
    #[test]
    fn a_mount_on_a_local_file_is_written_in_place_and_never_carried_back() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        forget_recorded_calls();
        WHOLE.with(|slot| *slot.borrow_mut() = Some(b"the archive, rewritten".to_vec()));

        let root =
            std::env::temp_dir().join(format!("ic-plugin-fs-inplace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a directory of our own");
        let at = root.join("work.inplaceprobe");
        std::fs::write(&at, b"the archive, as it was").expect("the file the mount lives in");

        let exts = CString::new(".inplaceprobe").unwrap();
        assert_eq!(
            register(exts.as_ptr(), &writable_table(), std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let plugin = filesystem_for("work.inplaceprobe").expect("registered");
        let put_back = std::rc::Rc::new(RefCell::new(Vec::new()));
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(LocalParent {
            put_back: put_back.clone(),
        });
        let fs = PluginFsRpc::new(plugin, at.to_string_lossy().into_owned(), parent);

        futures::executor::block_on(fs.write_file(
            "/added.txt".to_string(),
            b"fresh".to_vec(),
            None,
            None,
        ))
        .expect("the mount took the file");

        assert_eq!(
            std::fs::read(&at).expect("the file is still there"),
            b"the archive, rewritten",
            "the plugin wrote into the real file, through the host"
        );
        assert!(
            put_back.borrow().is_empty(),
            "and nothing was read out of the mount to be written back"
        );
        drop(fs);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A parent that is a disk: paths on it are paths on this machine, so a
    /// mount stands on the real folder rather than on a copy.
    struct LocalParent {
        put_back: std::rc::Rc<RefCell<Vec<(String, Vec<u8>)>>>,
    }

    #[async_trait::async_trait(?Send)]
    impl crate::rpc::FileSystemRpc for LocalParent {
        fn is_local(&self) -> bool {
            true
        }
        async fn read_file(
            &self,
            path: String,
            _: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<Vec<u8>, common::AppError> {
            std::fs::read(path).map_err(|e| common::AppError::Other(e.to_string()))
        }
        async fn write_file(
            &self,
            path: String,
            content: Vec<u8>,
            _: Option<u32>,
            _: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<(), common::AppError> {
            self.put_back.borrow_mut().push((path, content));
            Ok(())
        }
    }

    #[test]
    fn a_mount_is_named_and_drawn_the_way_the_entry_that_opened_it_was() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let plain = writable_filesystem(".shownprobe");
        assert_eq!(plain.display_name(), None, "nobody named it");
        assert_eq!(
            plain.get_icon_svg(""),
            None,
            "and nothing was brought for the root"
        );

        let shown = writable_filesystem(".shownprobe2").shown_as(Shown {
            name: "work files".to_string(),
            icon_svg: Some("<svg id=\"brought\"/>".to_string()),
        });
        assert_eq!(shown.display_name().as_deref(), Some("work files"));
        assert_eq!(
            shown.get_icon_svg("").as_deref(),
            Some("<svg id=\"brought\"/>")
        );
        assert_eq!(
            shown.get_icon_svg("inside/a/folder"),
            None,
            "only where the path begins"
        );

        let nameless = writable_filesystem(".shownprobe3").shown_as(Shown {
            name: "   ".to_string(),
            icon_svg: None,
        });
        assert_eq!(nameless.display_name(), None);
    }

    #[test]
    fn a_write_reaches_the_plugin_and_reports_what_it_stored() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = writable_filesystem(".wprobe");
        let seen = std::rc::Rc::new(std::cell::Cell::new(0u64));
        let sink = seen.clone();
        futures::executor::block_on(fs.write_file(
            "/inner/note.txt".to_string(),
            b"hello".to_vec(),
            None,
            Some(Box::new(move |n| sink.set(n))),
        ))
        .expect("the write is accepted");
        WRITES.with(|w| {
            let calls = w.borrow();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, "/inner/note.txt");
            assert_eq!(calls[0].1, b"hello".to_vec());
        });
        assert_eq!(seen.get(), 5, "progress is reported once the bytes landed");
    }

    #[test]
    fn a_refused_write_carries_the_reason_the_plugin_gave() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = writable_filesystem(".rprobe");
        REFUSE.with(|r| r.set(true));
        let failure = futures::executor::block_on(fs.write_file(
            "/inner/note.txt".to_string(),
            b"hello".to_vec(),
            None,
            None,
        ))
        .expect_err("the write is refused");
        assert!(
            failure.to_string().contains("the archive is sealed"),
            "{failure}"
        );
        WRITES.with(|w| assert!(w.borrow().is_empty()));
    }

    #[test]
    fn creating_a_directory_joins_the_parent_and_the_name_once() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = writable_filesystem(".dprobe");
        futures::executor::block_on(fs.create_directory(
            "/docs/".to_string(),
            "new".to_string(),
            None,
        ))
        .expect("the directory is created");
        futures::executor::block_on(fs.create_directory(
            "/docs".to_string(),
            "other".to_string(),
            None,
        ))
        .expect("the directory is created");
        DIRS.with(|d| assert_eq!(*d.borrow(), vec!["/docs/new", "/docs/other"]));
    }

    #[test]
    fn deleting_stops_at_the_first_entry_the_plugin_refuses() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = writable_filesystem(".xprobe");
        let failure = futures::executor::block_on(fs.delete_entries(vec![
            "/a.txt".to_string(),
            "/b.keep".to_string(),
            "/c.txt".to_string(),
        ]))
        .expect_err("the second entry is refused");
        assert!(failure.to_string().contains("the archive is sealed"));
        REMOVED.with(|r| {
            assert_eq!(
                *r.borrow(),
                vec!["/a.txt"],
                "nothing after the refusal runs"
            );
        });
    }

    #[test]
    fn renaming_passes_both_paths_through() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let fs = writable_filesystem(".nprobe");
        futures::executor::block_on(
            fs.rename_entry("/old.txt".to_string(), "/new.txt".to_string()),
        )
        .expect("the rename is accepted");
        RENAMED.with(|r| {
            assert_eq!(
                *r.borrow(),
                vec![("/old.txt".to_string(), "/new.txt".to_string())]
            );
        });
    }

    #[test]
    fn a_plugin_that_offers_no_write_call_refuses_every_change() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let exts = CString::new(".roprobe").unwrap();
        assert_eq!(
            register(exts.as_ptr(), &table(), std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let plugin = filesystem_for("x.roprobe").expect("registered");
        assert!(!plugin.supports_write());
        let parent: std::rc::Rc<dyn crate::rpc::FileSystemRpc> = std::rc::Rc::new(SlowParent {
            opened: std::rc::Rc::new(std::cell::Cell::new(0)),
        });
        let fs = PluginFsRpc::new(plugin, "x.roprobe".to_string(), parent);
        assert!(fs.is_read_only());
        for outcome in [
            futures::executor::block_on(fs.write_file("/a".to_string(), Vec::new(), None, None)),
            futures::executor::block_on(fs.create_directory(
                "/".to_string(),
                "d".to_string(),
                None,
            )),
            futures::executor::block_on(fs.delete_entries(vec!["/a".to_string()])),
            futures::executor::block_on(fs.rename_entry("/a".to_string(), "/b".to_string())),
        ] {
            let failure = outcome.expect_err("a read-only plugin refuses");
            assert!(failure.to_string().contains("read-only"), "{failure}");
        }
    }

    #[test]
    fn registering_the_same_extensions_again_replaces_rather_than_doubles() {
        let _lease = lease_registry_for_test();
        let exts = CString::new(".twice").unwrap();
        assert_eq!(
            register(exts.as_ptr(), &table(), std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        assert_eq!(
            register(exts.as_ptr(), &writable_table(), std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        assert_eq!(
            registry()
                .iter()
                .filter(|p| p.extensions == vec![".twice"])
                .count(),
            1,
            "the static and the deployed copy of one plugin must not both be listed"
        );
        let plugin = filesystem_for("x.twice").expect("registered");
        assert!(
            plugin.supports_write(),
            "the registration that came last is the one in use"
        );
    }

    #[test]
    fn two_plugins_claiming_different_extensions_both_stay() {
        let _lease = lease_registry_for_test();
        let first = CString::new(".alpha").unwrap();
        let second = CString::new(".beta").unwrap();
        register(first.as_ptr(), &table(), std::ptr::null_mut());
        register(second.as_ptr(), &table(), std::ptr::null_mut());
        assert_eq!(registry().len(), 2);
        assert!(filesystem_for("x.alpha").is_some());
        assert!(filesystem_for("x.beta").is_some());
    }

    #[test]
    fn a_plugin_built_before_the_write_side_existed_still_registers() {
        let _lease = lease_registry_for_test();
        let mut older = writable_table();
        older.struct_size = std::mem::offset_of!(ic_plugin_api::IcFsVTable, write) as u32;
        let exts = CString::new(".oldprobe").unwrap();
        assert_eq!(
            register(exts.as_ptr(), &older, std::ptr::null_mut()),
            ic_plugin_api::IC_OK
        );
        let plugin = filesystem_for("x.oldprobe").expect("registered");
        assert!(
            !plugin.supports_write(),
            "a shorter table must not be read past its own end"
        );
    }

    #[test]
    fn a_table_too_short_for_a_call_the_host_needs_is_refused() {
        let _lease = lease_registry_for_test();
        let mut broken = writable_table();
        broken.struct_size = std::mem::offset_of!(ic_plugin_api::IcFsVTable, last_error) as u32;
        let exts = CString::new(".badprobe").unwrap();
        assert_eq!(
            register(exts.as_ptr(), &broken, std::ptr::null_mut()),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        assert!(filesystem_for("x.badprobe").is_none());
    }

    #[test]
    fn a_plugin_filesystem_inherits_its_parents_patience() {
        use crate::rpc::FileSystemRpc;
        let _lease = lease_registry_for_test();
        let exts = CString::new(".probe").unwrap();
        register(exts.as_ptr(), &table(), std::ptr::null_mut());
        let plugin = filesystem_for("x.probe").unwrap();
        let parent: std::rc::Rc<dyn FileSystemRpc> = std::rc::Rc::new(SlowParent {
            opened: std::rc::Rc::new(std::cell::Cell::new(0)),
        });
        let fs = PluginFsRpc::new(plugin, "x.probe".to_string(), parent);
        assert_eq!(
            fs.content_wait(),
            crate::rpc::ContentWait::Bounded(std::time::Duration::from_secs(7)),
            "a remote parent's timeout must reach the nested level"
        );
    }
}
