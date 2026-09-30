pub mod catalog;
pub mod loader;

use ic_plugin_api::{IcClickFn, IcHost, IcPluginInit, IC_ABI_VERSION, IC_HOST_MAGIC, IC_OK};
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

#[derive(Clone)]
pub struct ToolbarEntry {
    pub id: String,
    pub svg: Vec<u8>,
    pub tooltip: String,
    pub side: u32,
    pub priority: i32,
    pub enable_flags: u32,
    on_click: IcClickFn,
    user_data: usize,
}

impl ToolbarEntry {
    pub fn fire(&self, parent: *mut c_void) {
        (self.on_click)(self.user_data as *mut c_void, parent);
    }
}

#[derive(Clone)]
pub struct SelectedItem {
    pub path: String,
    pub key: String,
    pub is_dir: bool,
}

type SelectionFn = std::rc::Rc<dyn Fn() -> Vec<SelectedItem>>;

thread_local! {
    static TOOLBAR: RefCell<Vec<ToolbarEntry>> = const { RefCell::new(Vec::new()) };
    static SELECTION: RefCell<Option<SelectionFn>> = const { RefCell::new(None) };
    static SNAPSHOT: RefCell<Vec<(CString, CString, bool)>> = const { RefCell::new(Vec::new()) };
    static VIEW: RefCell<Vec<ic_plugin_api::IcSelectionItem>> = const { RefCell::new(Vec::new()) };
    static SOURCES: RefCell<Vec<PanelSource>> = const { RefCell::new(Vec::new()) };
    static ACTIONS: RefCell<Vec<PanelAction>> = const { RefCell::new(Vec::new()) };
    static FS_ACTIONS: RefCell<Vec<FsAction>> = const { RefCell::new(Vec::new()) };
    /// Extensions whose panels draw their own toolbar instead of the usual one.
    static BARE_TOOLBARS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static HEADER: RefCell<Vec<HeaderEntry>> = const { RefCell::new(Vec::new()) };
    static DRIVE_SOURCES: RefCell<Vec<DriveSource>> = const { RefCell::new(Vec::new()) };
    static PINNED_SOURCES: RefCell<Vec<PinnedSource>> = const { RefCell::new(Vec::new()) };
    static CLOSE_REQUEST: RefCell<Option<std::rc::Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

pub fn set_selection_provider(provider: SelectionFn) {
    SELECTION.with(|s| *s.borrow_mut() = Some(provider));
}

extern "C" fn host_selection() -> ic_plugin_api::IcSelection {
    let items = SELECTION.with(|s| s.borrow().as_ref().map(|f| f()).unwrap_or_default());
    let owned: Vec<(CString, CString, bool)> = items
        .into_iter()
        .filter_map(|i| {
            let path = CString::new(i.path).ok()?;
            let key = CString::new(i.key).ok()?;
            Some((path, key, i.is_dir))
        })
        .collect();
    let view: Vec<ic_plugin_api::IcSelectionItem> = owned
        .iter()
        .map(|(p, k, d)| ic_plugin_api::IcSelectionItem {
            path: p.as_ptr(),
            key: k.as_ptr(),
            is_dir: if *d { 1 } else { 0 },
        })
        .collect();
    SNAPSHOT.with(|s| *s.borrow_mut() = owned);
    let count = view.len() as u32;
    VIEW.with(|v| {
        *v.borrow_mut() = view;
        let borrowed = v.borrow();
        ic_plugin_api::IcSelection {
            items: borrowed.as_ptr(),
            count,
        }
    })
}

fn cstr(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string()
}

extern "C" fn host_version() -> *const c_char {
    static VERSION: std::sync::OnceLock<CString> = std::sync::OnceLock::new();
    VERSION
        .get_or_init(|| CString::new(env!("CARGO_PKG_VERSION")).unwrap_or_default())
        .as_ptr()
}

extern "C" fn host_log_warn(message: *const c_char) {
    let text = cstr(message);
    ic_logging::warn!("plugin: {text}");
}

extern "C" fn host_add_toolbar_button(
    id: *const c_char,
    svg: *const c_char,
    tooltip: *const c_char,
    side: u32,
    priority: i32,
    enable_flags: u32,
    on_click: IcClickFn,
    user_data: *mut c_void,
) -> c_int {
    let entry = ToolbarEntry {
        id: cstr(id),
        svg: cstr(svg).into_bytes(),
        tooltip: cstr(tooltip),
        side,
        priority,
        enable_flags,
        on_click,
        user_data: user_data as usize,
    };
    if entry.id.is_empty() || entry.svg.is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    TOOLBAR.with(|t| {
        let mut list = t.borrow_mut();
        if let Some(seen) = list.iter().position(|e| e.id == entry.id) {
            ic_logging::warn!("plugin: toolbar id {} registered twice", entry.id);
            list[seen] = entry;
        } else {
            list.push(entry);
        }
    });
    IC_OK
}

pub fn host_table() -> IcHost {
    IcHost {
        magic: IC_HOST_MAGIC,
        struct_size: std::mem::size_of::<IcHost>() as u32,
        abi_version: IC_ABI_VERSION,
        host_version,
        log_warn: host_log_warn,
        add_toolbar_button: host_add_toolbar_button,
        selection: host_selection,
        register_panel_source: host_register_panel_source,
        open_panel_source: host_open_panel_source,
        close_panel_source: host_close_panel_source,
        add_header_button: host_add_header_button,
        register_filesystem: host_register_filesystem,
        register_panel_action: host_register_panel_action,
        register_connection_kind: host_register_connection_kind,
        register_view: host_register_view,
        open_view: host_open_view,
        view_invalidate: host_view_invalidate,
        canvas_invalidate: host_canvas_invalidate,
        register_locales: host_register_locales,
        register_asset: host_register_asset,
        register_plugin_asset: host_register_plugin_asset,
        fs_caps: fm_core::host_fs::fs_caps,
        fs_open: fm_core::host_fs::fs_open,
        fs_read: fm_core::host_fs::fs_read,
        fs_seek: fm_core::host_fs::fs_seek,
        fs_write: fm_core::host_fs::fs_write,
        fs_truncate: fm_core::host_fs::fs_truncate,
        fs_close: fm_core::host_fs::fs_close,
        fs_list: fm_core::host_fs::fs_list,
        fs_local_path: fm_core::host_fs::fs_local_path,
        fs_changed: host_fs_changed,
        register_viewer: host_register_viewer,
        settings_read: host_settings_read,
        settings_write: host_settings_write,
        register_panel_tree: host_register_panel_tree,
        set_header_label: host_set_header_label,
        set_header_visible: host_set_header_visible,
        set_default_toolbar_visible: host_set_default_toolbar_visible,
        register_drive_source: host_register_drive_source,
        drives_changed: host_drives_changed,
        register_fs_action: host_register_fs_action,
        fs_invalidate: host_fs_invalidate,
        language: host_language,
        register_pinned_connections: host_register_pinned_connections,
        pinned_connections_changed: host_pinned_connections_changed,
        set_header_icon: host_set_header_icon,
    }
}

/// The code `ic_i18n` is set to, kept alive for the process: a plugin holds the
/// pointer for as long as it likes, and switching language in the application
/// leaves the old string where it was rather than freeing it under anyone.
extern "C" fn host_language() -> *const c_char {
    fn held() -> &'static std::sync::Mutex<Vec<&'static CStr>> {
        static HELD: std::sync::OnceLock<std::sync::Mutex<Vec<&'static CStr>>> =
            std::sync::OnceLock::new();
        HELD.get_or_init(|| std::sync::Mutex::new(Vec::new()))
    }
    let now = ic_i18n::current_lang();
    let mut seen = held().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = seen.iter().find(|text| text.to_bytes() == now.as_bytes()) {
        return found.as_ptr();
    }
    let Ok(owned) = CString::new(now) else {
        return c"en".as_ptr();
    };
    let leaked: &'static CStr = Box::leak(owned.into_boxed_c_str());
    seen.push(leaked);
    leaked.as_ptr()
}

extern "C" fn host_register_panel_source(
    id: *const c_char,
    title: *const c_char,
    svg: *const c_char,
    rows: ic_plugin_api::IcTableFn,
    user_data: *mut c_void,
) -> c_int {
    remember(PanelSource {
        id: cstr(id),
        title: cstr(title),
        svg: cstr(svg).into_bytes(),
        rows,
        tree: None,
        user_data: user_data as usize,
    })
}

extern "C" fn host_register_panel_tree(
    id: *const c_char,
    title: *const c_char,
    svg: *const c_char,
    rows: ic_plugin_api::IcTreeFn,
    user_data: *mut c_void,
) -> c_int {
    remember(PanelSource {
        id: cstr(id),
        title: cstr(title),
        svg: cstr(svg).into_bytes(),
        rows: empty_table,
        tree: Some(rows),
        user_data: user_data as usize,
    })
}

extern "C" fn empty_table(_user_data: *mut c_void) -> ic_plugin_api::IcTable {
    ic_plugin_api::IcTable::EMPTY
}

fn remember(source: PanelSource) -> c_int {
    if source.id.is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    SOURCES.with(|s| {
        let mut list = s.borrow_mut();
        if let Some(seen) = list.iter().position(|e| e.id == source.id) {
            list[seen] = source;
        } else {
            list.push(source);
        }
    });
    IC_OK
}

#[derive(Clone)]
pub struct PanelAction {
    pub source_id: String,
    pub action_id: String,
    pub svg: Vec<u8>,
    pub tooltip: String,
    pub enable_flags: u32,
    on_click: IcClickFn,
    user_data: usize,
}

impl PanelAction {
    pub fn fire(&self, parent: *mut c_void) {
        (self.on_click)(self.user_data as *mut c_void, parent);
    }
}

/// A toolbar button that belongs to a plugin's filesystem: shown only while
/// the panel stands inside a mount of one of `extensions`.
#[derive(Clone)]
pub struct FsAction {
    pub extensions: Vec<String>,
    pub action_id: String,
    pub svg: Vec<u8>,
    pub tooltip: String,
    pub enable_flags: u32,
    /// `IC_ACTION_BUTTON` or `IC_ACTION_TOGGLE`: what widget to draw. Fixed at
    /// registration, because the toolbar is built before any mount is open.
    pub kind: u32,
    on_click: ic_plugin_api::IcFsClickFn,
    user_data: usize,
}

impl FsAction {
    pub fn is_toggle(&self) -> bool {
        self.kind == ic_plugin_api::IC_ACTION_TOGGLE
    }

    /// `mount` is the handle of the filesystem the panel is standing in, which
    /// is what the button acts on.
    pub fn fire(&self, mount: ic_plugin_api::IcFsHandle, parent: *mut c_void) {
        (self.on_click)(mount, self.user_data as *mut c_void, parent);
    }

    /// Whether this button belongs on the toolbar of the mount the panel is
    /// standing in. An empty scope means the panel is not inside a plugin
    /// filesystem at all, and then no plugin button belongs there.
    pub fn belongs_to(&self, scope: &str) -> bool {
        !scope.is_empty() && self.extensions.iter().any(|e| e == scope)
    }
}

/// The extensions a plugin claimed, read by the one rule the application has
/// for them, so a button and its filesystem always agree on spelling and on
/// which of `.gz` and `.tar.gz` a file belongs to.
fn extensions_of(list: *const c_char) -> Vec<String> {
    fm_core::suffix::claimed(&cstr(list))
}

/// What a plugin may name when it says its filesystem moved on.
///
/// An extension, spelled as `register_filesystem` spells it — `.zip` — or the
/// id of a connection kind, which has no extension because it is not reached
/// through a file at all. Without the second, a server or a peer-to-peer share
/// had no way of saying that anything had changed.
fn stale_names_of(list: *const c_char) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for name in cstr(list).split(',') {
        let lowered = name.trim().to_lowercase();
        let usable = if lowered.starts_with('.') {
            lowered.len() >= 2
        } else {
            !lowered.is_empty()
        };
        if !usable || found.contains(&lowered) {
            continue;
        }
        found.push(lowered);
    }
    found
}

extern "C" fn host_register_fs_action(
    extensions: *const c_char,
    action_id: *const c_char,
    svg: *const c_char,
    tooltip: *const c_char,
    enable_flags: u32,
    kind: u32,
    on_click: ic_plugin_api::IcFsClickFn,
    user_data: *mut c_void,
) -> c_int {
    let action = FsAction {
        extensions: extensions_of(extensions),
        action_id: cstr(action_id),
        svg: cstr(svg).into_bytes(),
        tooltip: cstr(tooltip),
        enable_flags,
        kind,
        on_click,
        user_data: user_data as usize,
    };
    if action.extensions.is_empty() || action.action_id.is_empty() || action.svg.is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    FS_ACTIONS.with(|a| {
        let mut list = a.borrow_mut();
        match list
            .iter()
            .position(|e| e.extensions == action.extensions && e.action_id == action.action_id)
        {
            Some(seen) => list[seen] = action,
            None => list.push(action),
        }
    });
    IC_OK
}

pub fn fs_actions() -> Vec<FsAction> {
    FS_ACTIONS.with(|a| a.borrow().clone())
}

extern "C" fn host_set_default_toolbar_visible(extensions: *const c_char, shown: c_int) -> c_int {
    let wanted = extensions_of(extensions);
    if wanted.is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    BARE_TOOLBARS.with(|held| {
        let mut list = held.borrow_mut();
        for extension in wanted {
            let already = list.iter().position(|seen| *seen == extension);
            match (shown == 0, already) {
                (true, None) => list.push(extension),
                (false, Some(at)) => {
                    list.remove(at);
                }
                _ => {}
            }
        }
    });
    IC_OK
}

/// Whether the panel standing in this filesystem shows its own toolbar
/// buttons. An empty scope is not a plugin filesystem, and keeps them.
pub fn default_toolbar_shown(scope: &str) -> bool {
    if scope.is_empty() {
        return true;
    }
    BARE_TOOLBARS.with(|held| !held.borrow().iter().any(|seen| seen == scope))
}

extern "C" fn host_fs_invalidate(extensions: *const c_char) -> c_int {
    let wanted = stale_names_of(extensions);
    if wanted.is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let installed = FS_INVALIDATE.with(|slot| slot.borrow().is_some());
    here_or_hand_over(Wanted::FsInvalidate { extensions: wanted }, installed)
}

extern "C" fn host_register_panel_action(
    source_id: *const c_char,
    action_id: *const c_char,
    svg: *const c_char,
    tooltip: *const c_char,
    enable_flags: u32,
    on_click: IcClickFn,
    user_data: *mut c_void,
) -> c_int {
    let action = PanelAction {
        source_id: cstr(source_id),
        action_id: cstr(action_id),
        svg: cstr(svg).into_bytes(),
        tooltip: cstr(tooltip),
        enable_flags,
        on_click,
        user_data: user_data as usize,
    };
    if action.source_id.is_empty() || action.action_id.is_empty() || action.svg.is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    ACTIONS.with(|a| {
        let mut list = a.borrow_mut();
        if let Some(seen) = list
            .iter()
            .position(|e| e.source_id == action.source_id && e.action_id == action.action_id)
        {
            list[seen] = action;
        } else {
            list.push(action);
        }
    });
    IC_OK
}

#[derive(Clone)]
pub struct HeaderEntry {
    pub id: String,
    pub svg: Vec<u8>,
    pub label: String,
    pub tooltip: String,
    pub side: u32,
    pub priority: i32,
    /// Whether the button is on the header at all. A plugin that never says
    /// otherwise gets one that is always there.
    pub shown: bool,
    on_click: IcClickFn,
    user_data: usize,
}

impl HeaderEntry {
    pub fn fire(&self, parent: *mut c_void) {
        (self.on_click)(self.user_data as *mut c_void, parent);
    }
}

#[allow(clippy::too_many_arguments)]
extern "C" fn host_add_header_button(
    id: *const c_char,
    svg: *const c_char,
    label: *const c_char,
    tooltip: *const c_char,
    side: u32,
    priority: i32,
    on_click: IcClickFn,
    user_data: *mut c_void,
) -> c_int {
    let entry = HeaderEntry {
        id: cstr(id),
        svg: cstr(svg).into_bytes(),
        label: cstr(label),
        tooltip: cstr(tooltip),
        side,
        priority,
        // A plugin that says nothing gets the always-there button it was written for.
        shown: true,
        on_click,
        user_data: user_data as usize,
    };
    if entry.id.is_empty() || (entry.svg.is_empty() && entry.label.is_empty()) {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    HEADER.with(|h| {
        let mut list = h.borrow_mut();
        if let Some(seen) = list.iter().position(|e| e.id == entry.id) {
            list[seen] = entry;
        } else {
            list.push(entry);
        }
    });
    IC_OK
}

struct DriveSource {
    kind: String,
    svg: Vec<u8>,
    rows: ic_plugin_api::IcDrivesFn,
    user_data: usize,
}

/// One entry a plugin offers in the drives list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginDrive {
    /// The connection kind that mounts it.
    pub kind: String,
    pub key: String,
    pub name: String,
    pub subtitle: String,
    pub svg: Vec<u8>,
    /// What to hand that kind's `open`.
    pub settings: std::collections::BTreeMap<String, String>,
    pub online: bool,
}

extern "C" fn host_drives_changed() -> c_int {
    let installed = DRIVES_CHANGED.with(|slot| slot.borrow().is_some());
    here_or_hand_over(Wanted::DrivesChanged, installed)
}

extern "C" fn host_register_drive_source(
    kind: *const c_char,
    svg: *const c_char,
    rows: ic_plugin_api::IcDrivesFn,
    user_data: *mut c_void,
) -> c_int {
    let kind = cstr(kind);
    if kind.trim().is_empty() {
        ic_logging::warn!("plugin: a drive source was registered without a kind");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let entry = DriveSource {
        kind,
        svg: cstr(svg).into_bytes(),
        rows,
        user_data: user_data as usize,
    };
    DRIVE_SOURCES.with(|held| {
        let mut list = held.borrow_mut();
        match list.iter().position(|seen| seen.kind == entry.kind) {
            Some(at) => list[at] = entry,
            None => list.push(entry),
        }
    });
    IC_OK
}

/// What the plugins currently offer, asked afresh: a peer that just went
/// offline should leave the list, and one that appeared should join it.
pub fn plugin_drives() -> Vec<PluginDrive> {
    let asked: Vec<(String, Vec<u8>, ic_plugin_api::IcDrivesFn, usize)> =
        DRIVE_SOURCES.with(|held| {
            held.borrow()
                .iter()
                .map(|source| {
                    (
                        source.kind.clone(),
                        source.svg.clone(),
                        source.rows,
                        source.user_data,
                    )
                })
                .collect()
        });
    ic_logging::debug!("drives: asking {} plugin source(s)", asked.len());
    let mut found = Vec::new();
    for (kind, svg, rows, user_data) in asked {
        let answered = rows(user_data as *mut c_void);
        if answered.magic != ic_plugin_api::IC_DRIVES_MAGIC {
            ic_logging::warn!("drives: {kind} answered with an unstamped table, ignoring it");
            continue;
        }
        ic_logging::debug!("drives: {kind} offered {} row(s)", answered.count);
        if answered.rows.is_null() {
            continue;
        }
        let held = unsafe { std::slice::from_raw_parts(answered.rows, answered.count as usize) };
        for row in held {
            let key = cstr(row.key);
            if key.trim().is_empty() {
                continue;
            }
            let own = cstr(row.svg);
            found.push(PluginDrive {
                kind: kind.clone(),
                key,
                name: cstr(row.name),
                subtitle: cstr(row.subtitle),
                svg: if own.is_empty() {
                    svg.clone()
                } else {
                    own.into_bytes()
                },
                settings: serde_json::from_str(&cstr(row.settings)).unwrap_or_default(),
                online: row.online != 0,
            });
        }
    }
    found
}

struct PinnedSource {
    id: String,
    rows: ic_plugin_api::IcPinnedConnectionsFn,
    user_data: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedConnection {
    pub id: String,
    pub title: String,
    pub svg: Vec<u8>,
    pub view: String,
}

extern "C" fn host_register_pinned_connections(
    id: *const c_char,
    rows: ic_plugin_api::IcPinnedConnectionsFn,
    user_data: *mut c_void,
) -> c_int {
    let id = cstr(id);
    if id.trim().is_empty() {
        ic_logging::warn!("plugin: pinned connections were registered without an id");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let entry = PinnedSource {
        id,
        rows,
        user_data: user_data as usize,
    };
    PINNED_SOURCES.with(|held| {
        let mut list = held.borrow_mut();
        match list.iter().position(|seen| seen.id == entry.id) {
            Some(at) => list[at] = entry,
            None => list.push(entry),
        }
    });
    IC_OK
}

extern "C" fn host_pinned_connections_changed() -> c_int {
    let installed = PINNED_CHANGED.with(|slot| slot.borrow().is_some());
    here_or_hand_over(Wanted::PinnedConnectionsChanged, installed)
}

pub fn pinned_connections() -> Vec<PinnedConnection> {
    let asked: Vec<(String, ic_plugin_api::IcPinnedConnectionsFn, usize)> =
        PINNED_SOURCES.with(|held| {
            held.borrow()
                .iter()
                .map(|source| (source.id.clone(), source.rows, source.user_data))
                .collect()
        });
    let mut found: Vec<PinnedConnection> = Vec::new();
    for (source, rows, user_data) in asked {
        let answered = rows(user_data as *mut c_void);
        if answered.magic != ic_plugin_api::IC_PINNED_MAGIC {
            ic_logging::warn!("pinned: {source} answered with an unstamped table, ignoring it");
            continue;
        }
        if answered.rows.is_null() {
            continue;
        }
        let held = unsafe { std::slice::from_raw_parts(answered.rows, answered.count as usize) };
        for row in held {
            let entry = PinnedConnection {
                id: cstr(row.id),
                title: cstr(row.title),
                svg: cstr(row.svg).into_bytes(),
                view: cstr(row.view),
            };
            if entry.id.trim().is_empty() || view_title(&entry.view).is_none() {
                ic_logging::warn!(
                    "pinned: {source} offered {:?} with no view behind it, skipping it",
                    entry.id
                );
                continue;
            }
            if found.iter().any(|seen| seen.id == entry.id) {
                continue;
            }
            found.push(entry);
        }
    }
    found
}

pub fn header_entries(side: u32) -> Vec<HeaderEntry> {
    HEADER.with(|h| {
        let mut list: Vec<HeaderEntry> = h
            .borrow()
            .iter()
            .filter(|e| e.side == side)
            .cloned()
            .collect();
        list.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));
        list
    })
}

pub fn panel_actions() -> Vec<PanelAction> {
    ACTIONS.with(|a| a.borrow().clone())
}

extern "C" fn host_close_panel_source() -> c_int {
    let handler = CLOSE_REQUEST.with(|h| h.borrow().clone());
    match handler {
        Some(f) => {
            f();
            IC_OK
        }
        None => ic_plugin_api::IC_ERR_INIT_FAILED,
    }
}

pub fn set_close_handler(handler: std::rc::Rc<dyn Fn()>) {
    CLOSE_REQUEST.with(|h| *h.borrow_mut() = Some(handler));
}

pub fn panel_source(id: &str) -> Option<PanelSource> {
    SOURCES.with(|s| s.borrow().iter().find(|e| e.id == id).cloned())
}

/// Every panel a plugin offers, in registration order. A frontend shows these
/// rather than asking for a source it knows the name of.
pub fn panel_sources() -> Vec<PanelSource> {
    SOURCES.with(|s| s.borrow().clone())
}

thread_local! {
    static OPEN_REQUEST: RefCell<Option<std::rc::Rc<dyn Fn(&str)>>> = const { RefCell::new(None) };
}

pub fn set_open_handler(handler: std::rc::Rc<dyn Fn(&str)>) {
    OPEN_REQUEST.with(|h| *h.borrow_mut() = Some(handler));
}

extern "C" fn host_open_panel_source(id: *const c_char) -> c_int {
    let wanted = cstr(id);
    if panel_source(&wanted).is_none() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let handler = OPEN_REQUEST.with(|h| h.borrow().clone());
    match handler {
        Some(f) => {
            f(&wanted);
            IC_OK
        }
        None => ic_plugin_api::IC_ERR_INIT_FAILED,
    }
}

/// Which application the plugins are being loaded into; said before any is loaded.
fn host_kind_slot() -> &'static std::sync::Mutex<String> {
    static HELD: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
    HELD.get_or_init(|| std::sync::Mutex::new(ic_plugin_api::IC_HOST_GTK.to_string()))
}

pub fn set_host_kind(kind: &str) {
    match host_kind_slot().lock() {
        Ok(mut held) => *held = kind.to_string(),
        Err(poisoned) => *poisoned.into_inner() = kind.to_string(),
    }
}

pub fn host_kind() -> String {
    match host_kind_slot().lock() {
        Ok(held) => held.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

pub fn host_ref() -> &'static IcHost {
    static HOST: std::sync::OnceLock<IcHost> = std::sync::OnceLock::new();
    HOST.get_or_init(host_table)
}

pub fn load_plugins(plugins: &[(&str, IcPluginInit)]) {
    let host = host_ref();
    let kind = std::ffi::CString::new(host_kind()).unwrap_or_default();
    for (name, init) in plugins {
        let code = init(host as *const IcHost, kind.as_ptr());
        if code != IC_OK {
            ic_logging::warn!("plugin {name} refused to initialise, code {code}");
        }
    }
}

pub fn toolbar_entries(side: u32) -> Vec<ToolbarEntry> {
    TOOLBAR.with(|t| {
        let mut list: Vec<ToolbarEntry> = t
            .borrow()
            .iter()
            .filter(|e| e.side == side)
            .cloned()
            .collect();
        list.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
        list
    })
}

#[derive(Clone)]
pub struct PanelSource {
    pub id: String,
    pub title: String,
    pub svg: Vec<u8>,
    rows: ic_plugin_api::IcTableFn,
    /// Present when the plugin registered a panel you can walk into.
    tree: Option<ic_plugin_api::IcTreeFn>,
    user_data: usize,
}

impl PanelSource {
    pub fn is_a_tree(&self) -> bool {
        self.tree.is_some()
    }

    pub fn snapshot(
        &self,
    ) -> (
        Vec<fm_core::rpc::ColumnSpec>,
        Vec<fm_core::rpc::RemoteFileEntry>,
        u32,
    ) {
        self.snapshot_at("")
    }

    /// The rows at `path`. A flat source ignores it, as it always has.
    pub fn snapshot_at(
        &self,
        path: &str,
    ) -> (
        Vec<fm_core::rpc::ColumnSpec>,
        Vec<fm_core::rpc::RemoteFileEntry>,
        u32,
    ) {
        if let Some(walk) = self.tree {
            return self.walked(walk, path);
        }
        let table = (self.rows)(self.user_data as *mut c_void);
        let columns = table.columns_slice();
        if columns.is_empty() {
            return (Vec::new(), Vec::new(), 0);
        }
        let specs: Vec<fm_core::rpc::ColumnSpec> = columns
            .iter()
            .skip(1)
            .map(|c| fm_core::rpc::ColumnSpec {
                key: c.key_string(),
                title: c.title_string(),
                width: if c.width > 0 { Some(c.width) } else { None },
                // A panel source has no ticks: its rows are read-only.
                kind: fm_core::rpc::ColumnKind::Text,
            })
            .collect();
        let mut entries = Vec::with_capacity(table.row_count as usize);
        for row in 0..table.row_count {
            let name = table.cell(row, 0).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let extra: Vec<String> = (1..table.column_count)
                .map(|c| table.cell(row, c).unwrap_or_default())
                .collect();
            entries.push(fm_core::rpc::RemoteFileEntry {
                name,
                is_dir: false,
                size: 0,
                modified: 0,
                permissions: None,
                extra,
            });
        }
        (specs, entries, table.key_column)
    }

    fn walked(
        &self,
        walk: ic_plugin_api::IcTreeFn,
        path: &str,
    ) -> (
        Vec<fm_core::rpc::ColumnSpec>,
        Vec<fm_core::rpc::RemoteFileEntry>,
        u32,
    ) {
        let Ok(asked) = std::ffi::CString::new(path) else {
            return (Vec::new(), Vec::new(), 0);
        };
        let table = walk(asked.as_ptr(), self.user_data as *mut c_void);
        if !table.is_sound() {
            ic_logging::warn!("plugin: {} answered with a table it did not stamp", self.id);
            return (Vec::new(), Vec::new(), 0);
        }
        let columns = table.columns_slice();
        if columns.is_empty() {
            return (Vec::new(), Vec::new(), 0);
        }
        let specs: Vec<fm_core::rpc::ColumnSpec> = columns
            .iter()
            .skip(1)
            .map(|c| fm_core::rpc::ColumnSpec {
                key: c.key_string(),
                title: c.title_string(),
                width: if c.width > 0 { Some(c.width) } else { None },
                // A panel source has no ticks: its rows are read-only.
                kind: fm_core::rpc::ColumnKind::Text,
            })
            .collect();
        let mut entries = Vec::with_capacity(table.row_count as usize);
        for row in 0..table.row_count {
            let name = table.cell(row, 0).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let extra: Vec<String> = (1..table.column_count)
                .map(|c| table.cell(row, c).unwrap_or_default())
                .collect();
            entries.push(fm_core::rpc::RemoteFileEntry {
                name,
                is_dir: table.opens(row),
                size: 0,
                modified: 0,
                permissions: None,
                extra,
            });
        }
        (specs, entries, table.key_column)
    }
}

pub struct PluginPanelRpc {
    pub source: PanelSource,
    columns: RefCell<Vec<fm_core::rpc::ColumnSpec>>,
    key_column: std::cell::Cell<u32>,
}

impl PluginPanelRpc {
    pub fn key_column(&self) -> u32 {
        self.key_column.get()
    }
}

impl PluginPanelRpc {
    pub fn new(source: PanelSource) -> Self {
        Self {
            source,
            columns: RefCell::new(Vec::new()),
            key_column: std::cell::Cell::new(0),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl fm_core::rpc::FileSystemRpc for PluginPanelRpc {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    async fn list_dir(
        &self,
        path: String,
    ) -> Result<Vec<fm_core::rpc::RemoteFileEntry>, common::AppError> {
        let (specs, entries, key_column) = self.source.snapshot_at(path.trim_matches('/'));
        *self.columns.borrow_mut() = specs;
        self.key_column.set(key_column);
        Ok(entries)
    }

    fn extra_columns(&self) -> Vec<fm_core::rpc::ColumnSpec> {
        let cached = self.columns.borrow().clone();
        if !cached.is_empty() {
            return cached;
        }
        let (specs, _, key_column) = self.source.snapshot();
        *self.columns.borrow_mut() = specs.clone();
        self.key_column.set(key_column);
        specs
    }

    fn columns_replace_defaults(&self) -> bool {
        true
    }

    fn wants_quick_filter(&self) -> bool {
        true
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_local(&self) -> bool {
        false
    }

    fn display_name(&self) -> Option<String> {
        Some(self.source.title.clone())
    }
}

pub type IcConnectionOpenFn = extern "C" fn(
    settings: *const u8,
    settings_len: u64,
    user_data: *mut c_void,
) -> ic_plugin_api::IcFsHandle;

pub struct ConnectionKind {
    pub id: String,
    pub document: Option<ic_view::Document>,
    pub user_data: usize,
    table: Option<ic_plugin_api::BoundedVTable>,
    filesystem: Option<fm_core::plugin_fs::FsPlugin>,
}

impl ConnectionKind {
    pub fn open_fn(&self) -> Option<IcConnectionOpenFn> {
        let table = self.table.as_ref()?;
        table.field::<Option<IcConnectionOpenFn>>(std::mem::offset_of!(
            ic_plugin_api::IcConnectionVTable,
            open
        ))?
    }

    pub fn can_open(&self) -> bool {
        self.open_fn().is_some() && self.filesystem.is_some()
    }

    fn describe_fn(&self) -> Option<ic_plugin_api::IcViewDescribeFn> {
        let table = self.table.as_ref()?;
        table.field::<Option<ic_plugin_api::IcViewDescribeFn>>(std::mem::offset_of!(
            ic_plugin_api::IcConnectionVTable,
            describe
        ))?
    }

    fn event_fn(&self) -> Option<ic_plugin_api::IcViewEventFn> {
        let table = self.table.as_ref()?;
        table.field::<Option<ic_plugin_api::IcViewEventFn>>(std::mem::offset_of!(
            ic_plugin_api::IcConnectionVTable,
            on_event
        ))?
    }

    pub fn mount(
        &self,
        settings: &std::collections::BTreeMap<String, String>,
    ) -> Option<std::rc::Rc<dyn fm_core::rpc::FileSystemRpc>> {
        self.mount_shown(settings, None)
    }

    /// The same mount, told how it is to be named and drawn where a path
    /// begins: the entry the user picked, not the plugin behind it.
    pub fn mount_shown(
        &self,
        settings: &std::collections::BTreeMap<String, String>,
        shown: Option<fm_core::plugin_fs::Shown>,
    ) -> Option<std::rc::Rc<dyn fm_core::rpc::FileSystemRpc>> {
        let open = self.open_fn()?;
        let filesystem = self.filesystem.clone()?;
        let encoded = serde_json::to_string(settings).ok()?;
        let mounted = fm_core::plugin_fs::PluginFsRpc::for_connection(
            filesystem,
            open,
            self.id.clone(),
            encoded,
        );
        Some(std::rc::Rc::new(match shown {
            Some(shown) => mounted.shown_as(shown),
            None => mounted,
        }))
    }
}

fn connection_kinds_registry() -> &'static std::sync::Mutex<Vec<ConnectionKind>> {
    static KINDS: std::sync::OnceLock<std::sync::Mutex<Vec<ConnectionKind>>> =
        std::sync::OnceLock::new();
    KINDS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

pub fn with_connection_kinds<R>(reader: impl FnOnce(&[ConnectionKind]) -> R) -> R {
    match connection_kinds_registry().lock() {
        Ok(list) => reader(&list),
        Err(poisoned) => reader(&poisoned.into_inner()),
    }
}

pub fn connection_kind_ids() -> Vec<String> {
    with_connection_kinds(|list| list.iter().map(|kind| kind.id.clone()).collect())
}

pub fn mount_connection(
    id: &str,
    settings: &std::collections::BTreeMap<String, String>,
) -> Option<std::rc::Rc<dyn fm_core::rpc::FileSystemRpc>> {
    mount_connection_shown(id, settings, None)
}

pub fn mount_connection_shown(
    id: &str,
    settings: &std::collections::BTreeMap<String, String>,
    shown: Option<fm_core::plugin_fs::Shown>,
) -> Option<std::rc::Rc<dyn fm_core::rpc::FileSystemRpc>> {
    with_connection_kinds(|list| {
        list.iter()
            .find(|kind| kind.id.eq_ignore_ascii_case(id))
            .and_then(|kind| kind.mount_shown(settings, shown.clone()))
    })
}

fn connection_describe_of(id: &str) -> Option<(ic_plugin_api::IcViewDescribeFn, usize)> {
    with_connection_kinds(|list| {
        let kind = list.iter().find(|kind| kind.id.eq_ignore_ascii_case(id))?;
        Some((kind.describe_fn()?, kind.user_data))
    })
}

fn connection_event_of(id: &str) -> Option<(String, ic_plugin_api::IcViewEventFn, usize)> {
    with_connection_kinds(|list| {
        let kind = list.iter().find(|kind| kind.id.eq_ignore_ascii_case(id))?;
        Some((kind.id.clone(), kind.event_fn()?, kind.user_data))
    })
}

/// Hands a form event to the plugin behind a connection kind. `None` means the
/// kind does not take events, which is the ordinary case: most forms are filled
/// in and committed without the plugin hearing anything.
pub fn connection_event(id: &str, event: &serde_json::Value) -> Option<serde_json::Value> {
    let (registered, on_event, user_data) = connection_event_of(id)?;
    let mut event = event.clone();
    // Whatever a frontend calls the kind, the plugin is told the id it registered.
    if let Some(object) = event.as_object_mut() {
        object.insert("view".to_string(), serde_json::json!(registered));
    }
    let encoded = serde_json::to_vec(&event).ok()?;
    let answer = answer_of(on_event(
        encoded.as_ptr(),
        encoded.len() as u64,
        user_data as *mut c_void,
    ))?;
    serde_json::from_str(&answer).ok()
}

/// A connection kind seen as something that can describe itself and answer
/// events, so a frontend can drive its form with the same session machinery it
/// uses for a plugin window.
pub struct Kinds;

impl ic_view_session::ViewHost for Kinds {
    fn describe(&self, id: &str, _context: &serde_json::Value) -> Option<ic_view::Document> {
        connection_document(id)
    }

    fn event(&self, id: &str, event: &serde_json::Value) -> Option<serde_json::Value> {
        connection_event(id, event)
    }
}

/// Whether a kind's form can act at all. A frontend uses this to decide if it
/// has to wire its buttons to the plugin.
pub fn connection_takes_events(id: &str) -> bool {
    connection_event_of(id).is_some()
}

/// The form for a kind. A plugin that offers `describe` is asked here, outside
/// the registry lock, and what it says replaces the document it registered;
/// an answer the host cannot use falls back to that document rather than
/// leaving the user with no form at all.
pub fn connection_document(id: &str) -> Option<ic_view::Document> {
    let registered = with_connection_kinds(|list| {
        list.iter()
            .find(|kind| kind.id.eq_ignore_ascii_case(id))
            .map(|kind| kind.document.clone())
    })??;
    let Some((describe, user_data)) = connection_describe_of(id) else {
        return Some(registered);
    };
    let answered = answer_of(describe(std::ptr::null(), 0, user_data as *mut c_void))
        .and_then(|source| accepted_document(id, &source));
    Some(answered.unwrap_or(registered))
}

pub struct ViewPlugin {
    pub id: String,
    pub title: String,
    pub user_data: usize,
    table: ic_plugin_api::BoundedVTable,
}

impl ViewPlugin {
    fn describe_fn(&self) -> Option<ic_plugin_api::IcViewDescribeFn> {
        self.table
            .field::<Option<ic_plugin_api::IcViewDescribeFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewVTable,
                describe
            ))
            .flatten()
    }

    fn event_fn(&self) -> Option<ic_plugin_api::IcViewEventFn> {
        self.table
            .field::<Option<ic_plugin_api::IcViewEventFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewVTable,
                on_event
            ))
            .flatten()
    }

    fn closed_fn(&self) -> Option<ic_plugin_api::IcViewClosedFn> {
        self.table
            .field::<Option<ic_plugin_api::IcViewClosedFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewVTable,
                closed
            ))
            .flatten()
    }
}

/// A plugin that offers to show files of some kind when the user asks to look
/// at one — F3 in a panel.
///
/// It is a window with a file behind it: the same declarative document as any
/// other, so whichever frontend is running draws it. The application's own
/// text and hex viewer is what shows a file nobody claimed, and it is never
/// replaced — only stood in front of.
pub struct ViewerPlugin {
    pub id: String,
    /// The extensions it claims, longest first, read by the one rule the
    /// application has for them.
    pub extensions: Vec<String>,
    /// What settles it when two plugins claim the same file; the larger wins.
    pub priority: i32,
    pub user_data: usize,
    table: ic_plugin_api::BoundedVTable,
    /// The window behind it, copied at registration like every other table:
    /// the plugin built both on the stack of `init`.
    window: ic_plugin_api::BoundedVTable,
}

impl ViewerPlugin {
    pub fn open_fn(&self) -> Option<ic_plugin_api::IcViewerOpenFn> {
        self.table
            .field::<ic_plugin_api::IcViewerOpenFn>(std::mem::offset_of!(
                ic_plugin_api::IcViewerVTable,
                open
            ))
    }

    pub fn content_fn(&self) -> Option<ic_plugin_api::IcViewerContentFn> {
        self.table
            .field::<Option<ic_plugin_api::IcViewerContentFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewerVTable,
                content
            ))
            .flatten()
    }

    pub fn closed_fn(&self) -> Option<ic_plugin_api::IcViewClosedFn> {
        self.table
            .field::<Option<ic_plugin_api::IcViewClosedFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewerVTable,
                closed
            ))
            .flatten()
    }

    pub fn closing_fn(&self) -> Option<ic_plugin_api::IcViewerClosingFn> {
        self.table
            .field::<Option<ic_plugin_api::IcViewerClosingFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewerVTable,
                closing
            ))
            .flatten()
    }

    pub fn canvas_ready_fn(&self) -> Option<ic_plugin_api::IcCanvasReadyFn> {
        self.table
            .field::<Option<ic_plugin_api::IcCanvasReadyFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewerVTable,
                canvas_ready
            ))
            .flatten()
    }

    pub fn canvas_draw_fn(&self) -> Option<ic_plugin_api::IcCanvasDrawFn> {
        self.table
            .field::<Option<ic_plugin_api::IcCanvasDrawFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewerVTable,
                canvas_draw
            ))
            .flatten()
    }

    pub fn canvas_gone_fn(&self) -> Option<ic_plugin_api::IcCanvasGoneFn> {
        self.table
            .field::<Option<ic_plugin_api::IcCanvasGoneFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewerVTable,
                canvas_gone
            ))
            .flatten()
    }

    pub fn describe_fn(&self) -> Option<ic_plugin_api::IcViewDescribeFn> {
        self.window
            .field::<Option<ic_plugin_api::IcViewDescribeFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewVTable,
                describe
            ))
            .flatten()
    }

    pub fn event_fn(&self) -> Option<ic_plugin_api::IcViewEventFn> {
        self.window
            .field::<Option<ic_plugin_api::IcViewEventFn>>(std::mem::offset_of!(
                ic_plugin_api::IcViewVTable,
                on_event
            ))
            .flatten()
    }
}

fn viewers_registry() -> &'static std::sync::Mutex<Vec<ViewerPlugin>> {
    static VIEWERS: std::sync::OnceLock<std::sync::Mutex<Vec<ViewerPlugin>>> =
        std::sync::OnceLock::new();
    VIEWERS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

pub fn with_viewers<R>(reader: impl FnOnce(&[ViewerPlugin]) -> R) -> R {
    match viewers_registry().lock() {
        Ok(list) => reader(&list),
        Err(poisoned) => reader(&poisoned.into_inner()),
    }
}

/// Which plugin shows a file of this name, if any.
///
/// By extension and nothing else: the application does not read a file to find
/// out what it is. The longest claim wins — `.tar.gz` before `.gz` — then the
/// larger priority, then whoever registered first, so the same file opens the
/// same way every time.
pub fn viewer_for(name: &str) -> Option<String> {
    with_viewers(|viewers| {
        let mut best: Option<(usize, i32, &ViewerPlugin)> = None;
        for viewer in viewers {
            let Some(claim) = fm_core::suffix::longest_match(name, &viewer.extensions) else {
                continue;
            };
            let better = match best {
                None => true,
                Some((len, priority, _)) => {
                    claim.len() > len || (claim.len() == len && viewer.priority > priority)
                }
            };
            if better {
                best = Some((claim.len(), viewer.priority, viewer));
            }
        }
        best.map(|(_, _, viewer)| viewer.id.clone())
    })
}

/// What each registered viewer claims, for a report and for the tests.
pub fn viewers_offered() -> Vec<(String, Vec<String>, i32)> {
    with_viewers(|viewers| {
        viewers
            .iter()
            .map(|viewer| {
                (
                    viewer.id.clone(),
                    viewer.extensions.clone(),
                    viewer.priority,
                )
            })
            .collect()
    })
}

extern "C" fn host_register_viewer(
    viewer_id: *const c_char,
    extensions: *const c_char,
    priority: i32,
    vtable: *const ic_plugin_api::IcViewerVTable,
    user_data: *mut c_void,
) -> c_int {
    let id = cstr(viewer_id);
    if id.trim().is_empty() {
        ic_logging::warn!("plugin: a viewer was registered without an id");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let claimed = extensions_of(extensions);
    if claimed.is_empty() {
        ic_logging::warn!("plugin: viewer {id} claimed no extensions");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let Some(table) = (unsafe {
        ic_plugin_api::BoundedVTable::adopt(
            vtable as *const u8,
            std::mem::size_of::<ic_plugin_api::IcViewerVTable>(),
        )
    }) else {
        ic_logging::warn!("plugin: viewer {id} was registered without a table");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    };
    // The window behind it is on the plugin's stack too, and it is what the
    // frontend asks to describe itself long after `init` has returned.
    let window = table
        .field::<*const ic_plugin_api::IcViewVTable>(std::mem::offset_of!(
            ic_plugin_api::IcViewerVTable,
            view
        ))
        .filter(|window| !window.is_null())
        .and_then(|window| unsafe {
            ic_plugin_api::BoundedVTable::adopt(
                window as *const u8,
                std::mem::size_of::<ic_plugin_api::IcViewVTable>(),
            )
        });
    let Some(window) = window else {
        ic_logging::warn!("plugin: viewer {id} brought no window to draw");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    };
    let entry = ViewerPlugin {
        id: id.clone(),
        extensions: claimed,
        priority,
        user_data: user_data as usize,
        table,
        window,
    };
    if entry.open_fn().is_none() || entry.describe_fn().is_none() {
        ic_logging::warn!("plugin: viewer {id} cannot open a file or cannot describe itself");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let mut list = match viewers_registry().lock() {
        Ok(list) => list,
        Err(poisoned) => poisoned.into_inner(),
    };
    match list.iter().position(|viewer| viewer.id == id) {
        Some(index) => list[index] = entry,
        None => list.push(entry),
    }
    IC_OK
}

/// A plugin has written to the filesystem it was let into, so what the
/// application remembers about it is wrong.
///
/// The memory belongs to the thread that draws, and this may arrive on any
/// thread, so it travels the same way everything else does.
extern "C" fn host_fs_changed(source: ic_plugin_api::IcFsSource, path: *const c_char) -> c_int {
    let Some((fs_id, within)) = fm_core::host_fs::where_it_stands(source) else {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    };
    if fs_id.is_empty() {
        return IC_OK;
    }
    let named = cstr(path);
    let named = named.trim_matches('/');
    let at = match (within.trim_matches('/'), named) {
        ("", "") => String::new(),
        ("", leaf) => leaf.to_string(),
        (root, "") => root.to_string(),
        (root, leaf) => format!("{root}/{leaf}"),
    };
    let installed = FS_INVALIDATE.with(|slot| slot.borrow().is_some());
    here_or_hand_over(Wanted::FsChanged { fs_id, at }, installed)
}

fn views_registry() -> &'static std::sync::Mutex<Vec<ViewPlugin>> {
    static VIEWS: std::sync::OnceLock<std::sync::Mutex<Vec<ViewPlugin>>> =
        std::sync::OnceLock::new();
    VIEWS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

pub fn with_views<R>(reader: impl FnOnce(&[ViewPlugin]) -> R) -> R {
    match views_registry().lock() {
        Ok(list) => reader(&list),
        Err(poisoned) => reader(&poisoned.into_inner()),
    }
}

pub fn view_ids() -> Vec<String> {
    with_views(|list| list.iter().map(|view| view.id.clone()).collect())
}

pub fn view_title(id: &str) -> Option<String> {
    with_views(|list| {
        list.iter()
            .find(|view| view.id == id)
            .map(|view| view.title.clone())
    })
}

fn answer_of(bytes: ic_plugin_api::IcBytes) -> Option<String> {
    if bytes.data.is_null() || bytes.len == 0 {
        return None;
    }
    let raw = unsafe { std::slice::from_raw_parts(bytes.data, bytes.len as usize) };
    std::str::from_utf8(raw).ok().map(|text| text.to_string())
}

fn accepted_document(id: &str, source: &str) -> Option<ic_view::Document> {
    let (document, issues) = match ic_view::validate_source(source) {
        Ok(parsed) => parsed,
        Err(reason) => {
            ic_logging::warn!("plugin: view {id} returned an unusable document: {reason}");
            return None;
        }
    };
    let mut refused = false;
    for issue in &issues {
        if issue.severity == ic_view::Severity::Error {
            refused = true;
        }
        ic_logging::warn!("plugin: view {id} at {}: {}", issue.at, issue.message);
    }
    (!refused).then_some(document)
}

fn describe_of(id: &str) -> Option<(ic_plugin_api::IcViewDescribeFn, usize)> {
    with_views(|list| {
        let view = list.iter().find(|view| view.id == id)?;
        Some((view.describe_fn()?, view.user_data))
    })
}

fn event_of(id: &str) -> Option<(ic_plugin_api::IcViewEventFn, usize)> {
    with_views(|list| {
        let view = list.iter().find(|view| view.id == id)?;
        Some((view.event_fn()?, view.user_data))
    })
}

fn closed_of(id: &str) -> Option<(ic_plugin_api::IcViewClosedFn, usize)> {
    with_views(|list| {
        let view = list.iter().find(|view| view.id == id)?;
        Some((view.closed_fn()?, view.user_data))
    })
}

pub fn describe_view(id: &str, context: &serde_json::Value) -> Option<ic_view::Document> {
    let encoded = serde_json::to_vec(context).ok()?;
    let (describe, user_data) = describe_of(id)?;
    let source = answer_of(describe(
        encoded.as_ptr(),
        encoded.len() as u64,
        user_data as *mut c_void,
    ))?;
    accepted_document(id, &source)
}

pub fn view_event(id: &str, event: &serde_json::Value) -> Option<serde_json::Value> {
    let encoded = serde_json::to_vec(event).ok()?;
    let (on_event, user_data) = event_of(id)?;
    let answer = answer_of(on_event(
        encoded.as_ptr(),
        encoded.len() as u64,
        user_data as *mut c_void,
    ))?;
    serde_json::from_str(&answer).ok()
}

/// Everything a viewer answers, by its id and the window it was opened as.
///
/// The instance travels in the context, so one plugin can show two files at
/// once and know which of them a question is about.
fn viewer_by<R>(id: &str, with: impl FnOnce(&ViewerPlugin) -> R) -> Option<R> {
    with_viewers(|viewers| viewers.iter().find(|held| held.id == id).map(with))
}

/// Lets the plugin into the filesystem the file lives on. The source belongs
/// to the caller and must outlive the window.
pub fn open_viewer(id: &str, instance: u64, source: ic_plugin_api::IcFsSource, path: &str) -> bool {
    let Ok(named) = std::ffi::CString::new(path) else {
        return false;
    };
    viewer_by(id, |viewer| {
        let Some(open) = viewer.open_fn() else {
            return false;
        };
        open(
            instance,
            source,
            named.as_ptr(),
            viewer.user_data as *mut c_void,
        ) == IC_OK
    })
    .unwrap_or(false)
}

pub fn describe_viewer(
    id: &str,
    instance: u64,
    context: &serde_json::Value,
) -> Option<ic_view::Document> {
    let mut context = context.clone();
    if let Some(held) = context.as_object_mut() {
        held.insert("instance".to_string(), serde_json::json!(instance));
    }
    let encoded = serde_json::to_vec(&context).ok()?;
    let source = viewer_by(id, |viewer| {
        let describe = viewer.describe_fn()?;
        answer_of(describe(
            encoded.as_ptr(),
            encoded.len() as u64,
            viewer.user_data as *mut c_void,
        ))
    })
    .flatten()?;
    accepted_document(id, &source)
}

pub fn viewer_event(
    id: &str,
    instance: u64,
    event: &serde_json::Value,
) -> Option<serde_json::Value> {
    let mut event = event.clone();
    if let Some(held) = event.as_object_mut() {
        held.insert("instance".to_string(), serde_json::json!(instance));
    }
    let encoded = serde_json::to_vec(&event).ok()?;
    let answer = viewer_by(id, |viewer| {
        let on_event = viewer.event_fn()?;
        answer_of(on_event(
            encoded.as_ptr(),
            encoded.len() as u64,
            viewer.user_data as *mut c_void,
        ))
    })
    .flatten()?;
    serde_json::from_str(&answer).ok()
}

/// The pixels behind a `part:` the document named. The plugin keeps them
/// alive until it is asked again, so they are copied here and now.
pub fn viewer_part(id: &str, instance: u64, name: &str) -> Option<Vec<u8>> {
    let named = std::ffi::CString::new(name).ok()?;
    viewer_by(id, |viewer| {
        let content = viewer.content_fn()?;
        let answered = content(instance, named.as_ptr(), viewer.user_data as *mut c_void);
        (!answered.data.is_null() && answered.len > 0).then(|| {
            unsafe { std::slice::from_raw_parts(answered.data, answered.len as usize) }.to_vec()
        })
    })
    .flatten()
}

/// Asks the plugin whether its window may close.
///
/// A plugin that says nothing — most of them — lets every window go. One that
/// refuses keeps it open, and the caller draws it again so that whatever it
/// wanted to say is on the screen.
pub fn viewer_may_close(id: &str, instance: u64) -> bool {
    viewer_by(id, |viewer| {
        let Some(closing) = viewer.closing_fn() else {
            return true;
        };
        closing(instance, viewer.user_data as *mut c_void) == IC_OK
    })
    .unwrap_or(true)
}

/// Asked before a place to draw is put up, so a plugin that paints nothing is not handed one.
pub fn viewer_draws(id: &str) -> bool {
    viewer_by(id, |viewer| viewer.canvas_ready_fn().is_some()).unwrap_or(false)
}

/// The place is made and its context current; `false` means the plugin refused it.
pub fn viewer_canvas_ready(id: &str, instance: u64, canvas: &ic_plugin_api::IcCanvas) -> bool {
    viewer_by(id, |viewer| {
        let Some(ready) = viewer.canvas_ready_fn() else {
            return false;
        };
        ready(instance, canvas, viewer.user_data as *mut c_void) == IC_OK
    })
    .unwrap_or(false)
}

pub fn viewer_canvas_draw(id: &str, instance: u64, frame: &ic_plugin_api::IcFrame) {
    viewer_by(id, |viewer| {
        if let Some(draw) = viewer.canvas_draw_fn() {
            draw(instance, frame, viewer.user_data as *mut c_void);
        }
    });
}

pub fn viewer_canvas_gone(id: &str, instance: u64) {
    viewer_by(id, |viewer| {
        if let Some(gone) = viewer.canvas_gone_fn() {
            gone(instance, viewer.user_data as *mut c_void);
        }
    });
}

pub fn viewer_closed(id: &str, instance: u64) {
    viewer_by(id, |viewer| {
        if let Some(closed) = viewer.closed_fn() {
            closed(instance, viewer.user_data as *mut c_void);
        }
    });
}

/// One viewer's window, as the session machinery asks about it.
pub struct Viewer {
    pub id: String,
    pub instance: u64,
}

impl ic_view_session::ViewHost for Viewer {
    fn describe(&self, _id: &str, context: &serde_json::Value) -> Option<ic_view::Document> {
        describe_viewer(&self.id, self.instance, context)
    }

    fn event(&self, _id: &str, event: &serde_json::Value) -> Option<serde_json::Value> {
        viewer_event(&self.id, self.instance, event)
    }
}

pub struct Views;

impl ic_view_session::ViewHost for Views {
    fn describe(&self, id: &str, context: &serde_json::Value) -> Option<ic_view::Document> {
        describe_view(id, context)
    }

    fn event(&self, id: &str, event: &serde_json::Value) -> Option<serde_json::Value> {
        view_event(id, event)
    }
}

pub fn view_closed(id: &str, instance: u64) {
    if let Some((closed, user_data)) = closed_of(id) {
        closed(instance, user_data as *mut c_void);
    }
}

fn assets_registry() -> &'static std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>> {
    static ASSETS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    > = std::sync::OnceLock::new();
    ASSETS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// A plugin's file by the name whatever refers to it wrote: `<plugin>/<name>`
/// for one registered under its owner, or a bare name for the older unowned
/// form that every plugin shared.
pub fn asset(name: &str) -> Option<Vec<u8>> {
    match assets_registry().lock() {
        Ok(held) => held.get(name).cloned(),
        Err(poisoned) => poisoned.into_inner().get(name).cloned(),
    }
}

pub fn asset_names() -> Vec<String> {
    match assets_registry().lock() {
        Ok(held) => held.keys().cloned().collect(),
        Err(poisoned) => poisoned.into_inner().keys().cloned().collect(),
    }
}

fn keep_asset(under: String, bytes: *const u8, bytes_len: u64) -> c_int {
    if bytes.is_null() || bytes_len == 0 {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let raw = unsafe { std::slice::from_raw_parts(bytes, bytes_len as usize) };
    let mut held = match assets_registry().lock() {
        Ok(held) => held,
        Err(poisoned) => poisoned.into_inner(),
    };
    held.insert(under, raw.to_vec());
    IC_OK
}

extern "C" fn host_register_asset(name: *const c_char, bytes: *const u8, bytes_len: u64) -> c_int {
    let name = cstr(name);
    if name.trim().is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    ic_logging::warn!(
        "plugin: {name} was registered without an owner, so another plugin can take the name"
    );
    keep_asset(name, bytes, bytes_len)
}

extern "C" fn host_register_plugin_asset(
    plugin_id: *const c_char,
    name: *const c_char,
    bytes: *const u8,
    bytes_len: u64,
) -> c_int {
    let plugin_id = cstr(plugin_id);
    let name = cstr(name);
    if plugin_id.trim().is_empty() || name.trim().is_empty() || name.contains('/') {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    keep_asset(format!("{plugin_id}/{name}"), bytes, bytes_len)
}

fn settings_config() -> &'static std::sync::Mutex<Option<client_config::AppConfig>> {
    static HELD: std::sync::OnceLock<std::sync::Mutex<Option<client_config::AppConfig>>> =
        std::sync::OnceLock::new();
    HELD.get_or_init(|| std::sync::Mutex::new(None))
}

/// Hands the host the configuration plugins keep their settings in. A frontend
/// calls this before loading plugins; until it does, every read is empty and
/// every write is refused.
pub fn set_settings_config(config: client_config::AppConfig) {
    match settings_config().lock() {
        Ok(mut held) => *held = Some(config),
        Err(poisoned) => *poisoned.into_inner() = Some(config),
    }
}

fn settings_key(plugin_id: &str, key: &str) -> Option<String> {
    let clean = |text: &str| !text.trim().is_empty() && !text.contains('.');
    (clean(plugin_id) && !key.trim().is_empty()).then(|| format!("plugins.{plugin_id}.{key}"))
}

thread_local! {
    static SETTING: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn host_settings_read(
    plugin_id: *const c_char,
    key: *const c_char,
) -> ic_plugin_api::IcBytes {
    let Some(full) = settings_key(&cstr(plugin_id), &cstr(key)) else {
        return ic_plugin_api::IcBytes::EMPTY;
    };
    let held = match settings_config().lock() {
        Ok(held) => held.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let Some(config) = held else {
        return ic_plugin_api::IcBytes::EMPTY;
    };
    let Some(stored) = config.get::<String>(&full) else {
        return ic_plugin_api::IcBytes::EMPTY;
    };
    let plain = if secret_store::is_encrypted(&stored) {
        match secret_store::decrypt_secret(&stored) {
            Some(plain) => plain,
            None => return ic_plugin_api::IcBytes::EMPTY,
        }
    } else {
        stored
    };
    SETTING.with(|slot| {
        *slot.borrow_mut() = plain.into_bytes();
        let held = slot.borrow();
        ic_plugin_api::IcBytes {
            data: held.as_ptr(),
            len: held.len() as u64,
        }
    })
}

extern "C" fn host_settings_write(
    plugin_id: *const c_char,
    key: *const c_char,
    bytes: *const u8,
    len: u64,
    flags: u32,
) -> c_int {
    let Some(full) = settings_key(&cstr(plugin_id), &cstr(key)) else {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    };
    let held = match settings_config().lock() {
        Ok(held) => held.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let Some(config) = held else {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    };
    if bytes.is_null() {
        config.forget(&full);
        config.save();
        return IC_OK;
    }
    let raw = unsafe { std::slice::from_raw_parts(bytes, len as usize) };
    let Ok(text) = std::str::from_utf8(raw) else {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    };
    let stored = if flags & ic_plugin_api::IC_SETTING_SECRET != 0 {
        match secret_store::try_encrypt_secret(text) {
            Some(sealed) => sealed,
            // Refuse rather than write a secret in the clear.
            None => return ic_plugin_api::IC_ERR_INIT_FAILED,
        }
    } else {
        text.to_string()
    };
    config.set(&full, stored);
    config.save();
    IC_OK
}

extern "C" fn host_register_locales(
    language: *const c_char,
    catalogue: *const u8,
    catalogue_len: u64,
) -> c_int {
    let language = cstr(language);
    if language.trim().is_empty() || catalogue.is_null() || catalogue_len == 0 {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let raw = unsafe { std::slice::from_raw_parts(catalogue, catalogue_len as usize) };
    let Ok(source) = std::str::from_utf8(raw) else {
        ic_logging::warn!("plugin: a {language} catalogue is not utf-8");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    };
    if serde_json::from_str::<std::collections::HashMap<String, String>>(source).is_err() {
        ic_logging::warn!("plugin: a {language} catalogue is not a flat table of strings");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    ic_i18n::register(&[(&language, source)]);
    IC_OK
}

extern "C" fn host_register_view(
    id: *const c_char,
    title: *const c_char,
    vtable: *const ic_plugin_api::IcViewVTable,
    user_data: *mut c_void,
) -> c_int {
    let id = cstr(id);
    if id.trim().is_empty() {
        ic_logging::warn!("plugin: a view was registered without an id");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let Some(table) = (unsafe {
        ic_plugin_api::BoundedVTable::adopt(
            vtable as *const u8,
            std::mem::size_of::<ic_plugin_api::IcViewVTable>(),
        )
    }) else {
        ic_logging::warn!("plugin: view {id} was registered without a table");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    };
    let entry = ViewPlugin {
        id: id.clone(),
        title: cstr(title),
        user_data: user_data as usize,
        table,
    };
    if entry.describe_fn().is_none() {
        ic_logging::warn!("plugin: view {id} cannot describe itself");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let mut list = match views_registry().lock() {
        Ok(list) => list,
        Err(poisoned) => poisoned.into_inner(),
    };
    match list.iter().position(|view| view.id == id) {
        Some(index) => list[index] = entry,
        None => list.push(entry),
    }
    IC_OK
}

thread_local! {
    static VIEW_OPEN_REQUEST: RefCell<Option<std::rc::Rc<dyn Fn(&str, &str)>>> =
        const { RefCell::new(None) };
    static CANVAS_REDRAW_REQUEST: RefCell<Option<std::rc::Rc<dyn Fn(u64)>>> =
        const { RefCell::new(None) };
    static VIEW_INVALIDATE_REQUEST: RefCell<Option<std::rc::Rc<dyn Fn(&str)>>> =
        const { RefCell::new(None) };
    static HEADER_CHANGED: RefCell<Option<std::rc::Rc<dyn Fn(&str, &str)>>> =
        const { RefCell::new(None) };
    static HEADER_SHOWN: RefCell<Option<std::rc::Rc<dyn Fn(&str, bool)>>> =
        const { RefCell::new(None) };
    static HEADER_REPAINTED: RefCell<Option<std::rc::Rc<dyn Fn(&str, &[u8])>>> =
        const { RefCell::new(None) };
    static FORM_CHANGED: RefCell<Option<std::rc::Rc<dyn Fn(&str)>>> = const { RefCell::new(None) };
    static DRIVES_CHANGED: RefCell<Option<std::rc::Rc<dyn Fn()>>> = const { RefCell::new(None) };
    static PINNED_CHANGED: RefCell<Option<std::rc::Rc<dyn Fn()>>> = const { RefCell::new(None) };
    static FS_INVALIDATE: RefCell<Option<std::rc::Rc<dyn Fn(&[String])>>> = const { RefCell::new(None) };
}

/// What a plugin asked the host to do, once it has crossed onto the thread
/// that may do it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wanted {
    Open {
        view: String,
        argument: String,
    },
    Invalidate {
        view: String,
    },
    Redraw {
        instance: u64,
    },
    FormChanged {
        kind: String,
    },
    DrivesChanged,
    PinnedConnectionsChanged,
    FsInvalidate {
        extensions: Vec<String>,
    },
    /// One path on one filesystem, named by the plugin that was let into it.
    FsChanged {
        fs_id: String,
        at: String,
    },
    HeaderLabel {
        id: String,
        label: String,
    },
    HeaderVisible {
        id: String,
        shown: bool,
    },
    HeaderIcon {
        id: String,
        svg: Vec<u8>,
    },
}

type Waker = std::sync::Arc<dyn Fn(Wanted) + Send + Sync>;

fn waker_slot() -> &'static std::sync::Mutex<Option<Waker>> {
    static HELD: std::sync::OnceLock<std::sync::Mutex<Option<Waker>>> = std::sync::OnceLock::new();
    HELD.get_or_init(|| std::sync::Mutex::new(None))
}

/// Installs the way back onto the frontend's own thread.
///
/// A plugin learns things on a thread of its own — a peer appeared, a transfer
/// finished — and the handlers above touch widgets, so they may only run where
/// the frontend lives. The frontend leaves a `Send + Sync` closure here that
/// hands the request to its loop; `deliver` then runs it in the right place.
pub fn set_waker(waker: Waker) {
    match waker_slot().lock() {
        Ok(mut held) => *held = Some(waker),
        Err(poisoned) => *poisoned.into_inner() = Some(waker),
    }
}

fn waker() -> Option<Waker> {
    match waker_slot().lock() {
        Ok(held) => held.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Runs what a plugin asked for. The frontend calls this from its own thread,
/// with whatever the waker handed it.
pub fn deliver(wanted: Wanted) {
    match wanted {
        Wanted::Open { view, argument } => {
            if let Some(open) = VIEW_OPEN_REQUEST.with(|slot| slot.borrow().clone()) {
                open(&view, &argument);
            }
        }
        Wanted::Redraw { instance } => {
            if let Some(again) = CANVAS_REDRAW_REQUEST.with(|slot| slot.borrow().clone()) {
                again(instance);
            }
        }
        Wanted::Invalidate { view } => {
            if let Some(refresh) = VIEW_INVALIDATE_REQUEST.with(|slot| slot.borrow().clone()) {
                refresh(&view);
            }
        }
        Wanted::FormChanged { kind } => {
            if let Some(again) = FORM_CHANGED.with(|slot| slot.borrow().clone()) {
                again(&kind);
            }
        }
        Wanted::DrivesChanged => {
            if let Some(again) = DRIVES_CHANGED.with(|slot| slot.borrow().clone()) {
                again();
            }
        }
        Wanted::PinnedConnectionsChanged => {
            if let Some(again) = PINNED_CHANGED.with(|slot| slot.borrow().clone()) {
                again();
            }
        }
        Wanted::FsInvalidate { extensions } => {
            // Whoever is drawing may or may not care; what the application
            // remembers about those folders is wrong either way, and every
            // frontend shares that memory.
            fm_core::listing::forget_mounts_of(&extensions);
            if let Some(again) = FS_INVALIDATE.with(|slot| slot.borrow().clone()) {
                again(&extensions);
            }
        }
        Wanted::FsChanged { fs_id, at } => {
            // What was written to is wrong, and so is everything under it: a
            // folder removed takes its children with it.
            fm_core::listing::forget_id_within(&fs_id, &at);
            if let Some(again) = FS_INVALIDATE.with(|slot| slot.borrow().clone()) {
                again(&[fs_id]);
            }
        }
        Wanted::HeaderLabel { id, label } => {
            let known = HEADER.with(|h| {
                let mut list = h.borrow_mut();
                match list.iter_mut().find(|entry| entry.id == id) {
                    Some(entry) => {
                        entry.label = label.clone();
                        true
                    }
                    None => false,
                }
            });
            if !known {
                return;
            }
            if let Some(relabel) = HEADER_CHANGED.with(|slot| slot.borrow().clone()) {
                relabel(&id, &label);
            }
        }
        Wanted::HeaderIcon { id, svg } => {
            let known = HEADER.with(|h| {
                let mut list = h.borrow_mut();
                match list.iter_mut().find(|entry| entry.id == id) {
                    Some(entry) => {
                        entry.svg = svg.clone();
                        true
                    }
                    None => false,
                }
            });
            if !known {
                return;
            }
            if let Some(repaint) = HEADER_REPAINTED.with(|slot| slot.borrow().clone()) {
                repaint(&id, &svg);
            }
        }
        Wanted::HeaderVisible { id, shown } => {
            let known = HEADER.with(|h| {
                let mut list = h.borrow_mut();
                match list.iter_mut().find(|entry| entry.id == id) {
                    Some(entry) => {
                        entry.shown = shown;
                        true
                    }
                    None => false,
                }
            });
            if !known {
                return;
            }
            if let Some(show) = HEADER_SHOWN.with(|slot| slot.borrow().clone()) {
                show(&id, shown);
            }
        }
    }
}

/// Runs it here if this is already the right thread, otherwise posts it.
fn here_or_hand_over(wanted: Wanted, installed: bool) -> c_int {
    if installed {
        deliver(wanted);
        return IC_OK;
    }
    match waker() {
        Some(waker) => {
            waker(wanted);
            IC_OK
        }
        None => ic_plugin_api::IC_ERR_INIT_FAILED,
    }
}

pub fn set_view_open_handler(handler: std::rc::Rc<dyn Fn(&str, &str)>) {
    VIEW_OPEN_REQUEST.with(|slot| *slot.borrow_mut() = Some(handler));
}

pub fn set_canvas_redraw_handler(handler: std::rc::Rc<dyn Fn(u64)>) {
    CANVAS_REDRAW_REQUEST.with(|slot| *slot.borrow_mut() = Some(handler));
}

pub fn set_view_invalidate_handler(handler: std::rc::Rc<dyn Fn(&str)>) {
    VIEW_INVALIDATE_REQUEST.with(|slot| *slot.borrow_mut() = Some(handler));
}

/// Installs what redraws a header button's text. The host has already stored
/// the new label by the time this runs, so a frontend that rebuilds its header
/// from `header_entries` needs no handler at all.
/// Installs what redescribes an open connection form. A frontend that has no
/// form on screen needs none.
pub fn set_form_changed_handler(handler: std::rc::Rc<dyn Fn(&str)>) {
    FORM_CHANGED.with(|slot| *slot.borrow_mut() = Some(handler));
}

/// Installs what redraws the lists of drives. A frontend that shows none needs
/// no handler.
pub fn set_drives_changed_handler(handler: std::rc::Rc<dyn Fn()>) {
    DRIVES_CHANGED.with(|slot| *slot.borrow_mut() = Some(handler));
}

pub fn set_pinned_changed_handler(handler: std::rc::Rc<dyn Fn()>) {
    PINNED_CHANGED.with(|slot| *slot.borrow_mut() = Some(handler));
}

/// Told which extensions went stale, so a panel standing in one of them lists
/// again and the rest are left alone.
pub fn set_fs_invalidate_handler(handler: std::rc::Rc<dyn Fn(&[String])>) {
    FS_INVALIDATE.with(|slot| *slot.borrow_mut() = Some(handler));
}

pub fn set_header_changed_handler(handler: std::rc::Rc<dyn Fn(&str, &str)>) {
    HEADER_CHANGED.with(|slot| *slot.borrow_mut() = Some(handler));
}

/// Told which header button to show or hide.
pub fn set_header_shown_handler(handler: std::rc::Rc<dyn Fn(&str, bool)>) {
    HEADER_SHOWN.with(|slot| *slot.borrow_mut() = Some(handler));
}

/// Told which header button to draw with a new picture.
pub fn set_header_repainted_handler(handler: std::rc::Rc<dyn Fn(&str, &[u8])>) {
    HEADER_REPAINTED.with(|slot| *slot.borrow_mut() = Some(handler));
}

extern "C" fn host_open_view(id: *const c_char, arg: *const u8, arg_len: u64) -> c_int {
    let view = cstr(id);
    if view_title(&view).is_none() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let argument = if arg.is_null() || arg_len == 0 {
        String::new()
    } else {
        let raw = unsafe { std::slice::from_raw_parts(arg, arg_len as usize) };
        String::from_utf8_lossy(raw).to_string()
    };
    let installed = VIEW_OPEN_REQUEST.with(|slot| slot.borrow().is_some());
    here_or_hand_over(Wanted::Open { view, argument }, installed)
}

/// Drawing happens on the frontend's own thread, so this only asks for it.
extern "C" fn host_canvas_invalidate(instance: u64) -> c_int {
    let installed = CANVAS_REDRAW_REQUEST.with(|slot| slot.borrow().is_some());
    here_or_hand_over(Wanted::Redraw { instance }, installed)
}

/// Says that what was registered under this id should be described again. The
/// id may name a view or a connection kind: a plugin that offers the same page
/// through both has one thing to say and should not have to say it twice.
extern "C" fn host_view_invalidate(id: *const c_char) -> c_int {
    let named = cstr(id);
    if view_title(&named).is_some() {
        let installed = VIEW_INVALIDATE_REQUEST.with(|slot| slot.borrow().is_some());
        return here_or_hand_over(Wanted::Invalidate { view: named }, installed);
    }
    if with_connection_kinds(|list| list.iter().any(|kind| kind.id.eq_ignore_ascii_case(&named))) {
        let installed = FORM_CHANGED.with(|slot| slot.borrow().is_some());
        return here_or_hand_over(Wanted::FormChanged { kind: named }, installed);
    }
    ic_plugin_api::IC_ERR_INIT_FAILED
}

extern "C" fn host_set_header_label(id: *const c_char, label: *const c_char) -> c_int {
    let id = cstr(id);
    if id.trim().is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    // Buttons live only in the frontend thread's registry, so finding one means we are on it.
    let here = HEADER.with(|h| h.borrow().iter().any(|entry| entry.id == id));
    here_or_hand_over(
        Wanted::HeaderLabel {
            id,
            label: cstr(label),
        },
        here,
    )
}

extern "C" fn host_set_header_icon(id: *const c_char, svg: *const c_char) -> c_int {
    let id = cstr(id);
    let svg = cstr(svg);
    if id.trim().is_empty() || svg.trim().is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let here = HEADER.with(|h| h.borrow().iter().any(|entry| entry.id == id));
    here_or_hand_over(
        Wanted::HeaderIcon {
            id,
            svg: svg.into_bytes(),
        },
        here,
    )
}

extern "C" fn host_set_header_visible(id: *const c_char, shown: c_int) -> c_int {
    let id = cstr(id);
    if id.trim().is_empty() {
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let here = HEADER.with(|h| h.borrow().iter().any(|entry| entry.id == id));
    here_or_hand_over(
        Wanted::HeaderVisible {
            id,
            shown: shown != 0,
        },
        here,
    )
}

fn kind_document_from(
    id: &str,
    schema: *const u8,
    schema_len: u64,
) -> Result<Option<ic_view::Document>, c_int> {
    if schema.is_null() || schema_len == 0 {
        return Ok(None);
    }
    let raw = unsafe { std::slice::from_raw_parts(schema, schema_len as usize) };
    let source = match std::str::from_utf8(raw) {
        Ok(text) => text,
        Err(_) => {
            ic_logging::warn!("plugin: connection kind {id} has a document that is not utf-8");
            return Err(ic_plugin_api::IC_ERR_INIT_FAILED);
        }
    };
    let (document, issues) = match ic_view::validate_source(source) {
        Ok(parsed) => parsed,
        Err(reason) => {
            ic_logging::warn!("plugin: connection kind {id} has an unusable document: {reason}");
            return Err(ic_plugin_api::IC_ERR_INIT_FAILED);
        }
    };
    let mut refused = false;
    for issue in &issues {
        match issue.severity {
            ic_view::Severity::Error => {
                refused = true;
                ic_logging::warn!(
                    "plugin: connection kind {id} at {}: {}",
                    issue.at,
                    issue.message
                );
            }
            ic_view::Severity::Warning => {
                ic_logging::warn!(
                    "plugin: connection kind {id} at {}: {}",
                    issue.at,
                    issue.message
                )
            }
        }
    }
    if refused {
        return Err(ic_plugin_api::IC_ERR_INIT_FAILED);
    }
    Ok(Some(document))
}

extern "C" fn host_register_connection_kind(
    id: *const c_char,
    schema: *const u8,
    schema_len: u64,
    vtable: *const ic_plugin_api::IcConnectionVTable,
    user_data: *mut c_void,
) -> c_int {
    let id = cstr(id);
    if id.trim().is_empty() {
        ic_logging::warn!("plugin: a connection kind was registered without an id");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let document = match kind_document_from(&id, schema, schema_len) {
        Ok(document) => document,
        Err(code) => return code,
    };
    let table = unsafe {
        ic_plugin_api::BoundedVTable::adopt(
            vtable as *const u8,
            std::mem::size_of::<ic_plugin_api::IcConnectionVTable>(),
        )
    };
    let filesystem = table
        .as_ref()
        .and_then(|found| {
            found.field::<*const ic_plugin_api::IcFsVTable>(std::mem::offset_of!(
                ic_plugin_api::IcConnectionVTable,
                fs
            ))
        })
        .and_then(|pointer| fm_core::plugin_fs::FsPlugin::adopt(pointer, user_data));
    let entry = ConnectionKind {
        id: id.clone(),
        document,
        user_data: user_data as usize,
        table,
        filesystem,
    };
    if entry.document.is_none() && !entry.can_open() {
        ic_logging::warn!("plugin: connection kind {id} has neither a document nor a way to mount");
        return ic_plugin_api::IC_ERR_INIT_FAILED;
    }
    let mut list = match connection_kinds_registry().lock() {
        Ok(list) => list,
        Err(poisoned) => poisoned.into_inner(),
    };
    match list.iter().position(|kind| kind.id == id) {
        Some(index) => list[index] = entry,
        None => list.push(entry),
    }
    IC_OK
}

extern "C" fn host_register_filesystem(
    extensions: *const c_char,
    vtable: *const ic_plugin_api::IcFsVTable,
    user_data: *mut c_void,
) -> c_int {
    fm_core::plugin_fs::register(extensions, vtable, user_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIND_DOCUMENT: &str = r#"{
        "schema": 1,
        "kind": "example",
        "identity": "name",
        "summary": { "fmt": "{url}" },
        "fields": [
            { "bind": "name", "type": "text", "scope": "record", "required": true },
            { "bind": "url", "type": "text", "required": true, "empty_as_absent": true }
        ],
        "form": { "t": "column", "children": [
            { "t": "input", "id": "name", "bind": "name" },
            { "t": "input", "id": "url", "bind": "url" } ] }
    }"#;

    extern "C" fn kind_open(_: *const u8, _: u64, _: *mut c_void) -> ic_plugin_api::IcFsHandle {
        7 as ic_plugin_api::IcFsHandle
    }

    const LATER_DOCUMENT: &str = r#"{
        "schema": 1,
        "kind": "example",
        "identity": "name",
        "fields": [ { "bind": "name", "type": "text", "scope": "record", "required": true } ],
        "form": { "t": "column", "children": [
            { "t": "text", "id": "already", "text": "one account is enough" } ] }
    }"#;

    /// Each test counts through its own tally, handed over as `user_data`: these
    /// run in one process and in parallel, so a shared counter would make every
    /// assertion about a delta depend on what else happened to be running.
    type Tally = std::sync::atomic::AtomicUsize;

    fn counted(user_data: *mut c_void) {
        if !user_data.is_null() {
            unsafe { &*(user_data as *const Tally) }
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn so_far(tally: &Tally) -> usize {
        tally.load(std::sync::atomic::Ordering::Relaxed)
    }

    extern "C" fn clicked(
        _mount: ic_plugin_api::IcFsHandle,
        user_data: *mut c_void,
        _parent: *mut c_void,
    ) {
        counted(user_data);
    }

    fn c(text: &str) -> CString {
        CString::new(text).expect("no nul")
    }

    /// These live in one thread-local list shared by every test in this
    /// process, so each test registers under a name of its own and looks only
    /// for that name rather than counting the list.
    fn fs_action_named(action_id: &str) -> Option<FsAction> {
        fs_actions().into_iter().find(|a| a.action_id == action_id)
    }

    /// The language is one setting for the whole process, so these hold a lock
    /// rather than race each other: tests in one binary run in parallel.
    fn while_setting_the_language() -> std::sync::MutexGuard<'static, ()> {
        static ONE_AT_A_TIME: std::sync::OnceLock<std::sync::Mutex<()>> =
            std::sync::OnceLock::new();
        ONE_AT_A_TIME
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn a_plugin_is_told_which_language_the_application_is_showing() {
        let _held = while_setting_the_language();
        let before = ic_i18n::current_lang();
        ic_i18n::set_lang("ru");
        let spoken = unsafe { CStr::from_ptr(host_language()) }
            .to_string_lossy()
            .to_string();
        assert_eq!(spoken, "ru");
        ic_i18n::set_lang(before);
    }

    #[test]
    fn the_string_a_plugin_was_given_survives_a_change_of_language() {
        let _held = while_setting_the_language();
        let before = ic_i18n::current_lang();
        ic_i18n::set_lang("ru");
        let held = host_language();
        ic_i18n::set_lang("de");
        // The plugin kept the pointer; it must still read as what it said.
        assert_eq!(unsafe { CStr::from_ptr(held) }.to_string_lossy(), "ru");
        assert_eq!(
            unsafe { CStr::from_ptr(host_language()) }.to_string_lossy(),
            "de"
        );
        ic_i18n::set_lang(before);
    }

    #[test]
    fn asking_twice_for_the_same_language_hands_back_the_same_string() {
        let _held = while_setting_the_language();
        let before = ic_i18n::current_lang();
        ic_i18n::set_lang("pl");
        assert_eq!(host_language(), host_language(), "nothing leaks per call");
        ic_i18n::set_lang(before);
    }

    extern "C" fn header_clicked(_user_data: *mut c_void, _parent: *mut c_void) {}

    fn a_header_button(id: &str) -> c_int {
        host_add_header_button(
            c(id).as_ptr(),
            c("<svg/>").as_ptr(),
            c("").as_ptr(),
            c("").as_ptr(),
            ic_plugin_api::IC_SIDE_RIGHT,
            0,
            header_clicked,
            std::ptr::null_mut(),
        )
    }

    fn header_named(id: &str) -> Option<HeaderEntry> {
        header_entries(ic_plugin_api::IC_SIDE_RIGHT)
            .into_iter()
            .find(|entry| entry.id == id)
    }

    extern "C" fn two_shelves(_user_data: *mut c_void) -> ic_plugin_api::IcDrives {
        // Per thread: the pointers handed out have to outlive the call.
        thread_local! {
            static ROWS: RefCell<Vec<ic_plugin_api::IcDrive>> = const { RefCell::new(Vec::new()) };
        }
        ROWS.with(|held| {
            *held.borrow_mut() = vec![
                ic_plugin_api::IcDrive {
                    key: c"probe.own".as_ptr(),
                    name: c"With a picture".as_ptr(),
                    subtitle: c"".as_ptr(),
                    settings: c"{}".as_ptr(),
                    svg: c"<svg id=\"row\"/>".as_ptr(),
                    online: 1,
                },
                ic_plugin_api::IcDrive {
                    key: c"probe.bare".as_ptr(),
                    name: c"Without one".as_ptr(),
                    subtitle: c"".as_ptr(),
                    settings: c"{}".as_ptr(),
                    svg: std::ptr::null(),
                    online: 0,
                },
            ];
            let borrowed = held.borrow();
            ic_plugin_api::IcDrives {
                count: borrowed.len() as u32,
                rows: borrowed.as_ptr(),
                ..ic_plugin_api::IcDrives::EMPTY
            }
        })
    }

    fn drives_of(kind: &str) -> Vec<PluginDrive> {
        plugin_drives()
            .into_iter()
            .filter(|drive| drive.kind == kind)
            .collect()
    }

    #[test]
    fn a_row_with_no_picture_of_its_own_falls_back_to_the_sources() {
        assert_eq!(
            host_register_drive_source(
                c("probe.withicon").as_ptr(),
                c("<svg id=\"source\"/>").as_ptr(),
                two_shelves,
                std::ptr::null_mut(),
            ),
            IC_OK
        );
        let found = drives_of("probe.withicon");
        assert_eq!(found.len(), 2);
        assert_eq!(
            String::from_utf8_lossy(&found[0].svg),
            "<svg id=\"row\"/>",
            "a row that brought one keeps it"
        );
        assert_eq!(
            String::from_utf8_lossy(&found[1].svg),
            "<svg id=\"source\"/>",
            "and one that did not gets what the source registered"
        );
    }

    #[test]
    fn a_source_with_no_picture_leaves_the_application_to_draw_its_own() {
        // For entries better shown by whether they are reachable than by whose they are.
        assert_eq!(
            host_register_drive_source(
                c("probe.noicon").as_ptr(),
                c("").as_ptr(),
                two_shelves,
                std::ptr::null_mut(),
            ),
            IC_OK
        );
        let found = drives_of("probe.noicon");
        assert_eq!(found.len(), 2);
        assert!(
            found[1].svg.is_empty(),
            "nothing was brought and nothing was registered, so the application draws it"
        );
        assert!(!found[0].svg.is_empty(), "the other row still has its own");
        assert!(found[0].online && !found[1].online);
    }

    #[test]
    fn a_panel_keeps_its_own_toolbar_unless_a_plugin_says_otherwise() {
        assert!(
            default_toolbar_shown(".neversaidanything"),
            "every plugin written before this expected the usual toolbar"
        );
        assert!(
            default_toolbar_shown(""),
            "standing on ordinary files is not standing in a plugin at all"
        );
    }

    #[test]
    fn a_plugin_can_take_the_panels_toolbar_away_and_give_it_back() {
        assert_eq!(
            host_set_default_toolbar_visible(c(".Bare, .alsobare").as_ptr(), 0),
            IC_OK
        );
        // Read as `register_filesystem` reads them, so spelling never decides.
        assert!(!default_toolbar_shown(".bare"));
        assert!(!default_toolbar_shown(".alsobare"));
        assert!(default_toolbar_shown(".somethingelse"));

        assert_eq!(
            host_set_default_toolbar_visible(c(".bare").as_ptr(), 1),
            IC_OK
        );
        assert!(default_toolbar_shown(".bare"));
        assert!(!default_toolbar_shown(".alsobare"), "only the one named");
    }

    #[test]
    fn saying_it_twice_does_not_stack_up() {
        for _ in 0..3 {
            assert_eq!(
                host_set_default_toolbar_visible(c(".twicebare").as_ptr(), 0),
                IC_OK
            );
        }
        assert!(!default_toolbar_shown(".twicebare"));
        // One putting-back is enough, however many times it was taken away.
        assert_eq!(
            host_set_default_toolbar_visible(c(".twicebare").as_ptr(), 1),
            IC_OK
        );
        assert!(default_toolbar_shown(".twicebare"));
    }

    #[test]
    fn a_toolbar_declared_for_nothing_is_refused() {
        assert_eq!(
            host_set_default_toolbar_visible(c("").as_ptr(), 0),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        assert_eq!(
            host_set_default_toolbar_visible(c("zip").as_ptr(), 0),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
    }

    #[test]
    fn a_header_button_is_there_unless_the_plugin_says_otherwise() {
        assert_eq!(a_header_button("probe.always"), IC_OK);
        assert!(
            header_named("probe.always").expect("registered").shown,
            "every plugin written before this expected a button that stays"
        );
    }

    #[test]
    fn a_plugin_can_repaint_its_header_button_to_say_what_state_it_is_in() {
        let seen = std::rc::Rc::new(RefCell::new(Vec::<String>::new()));
        let sink = seen.clone();
        set_header_repainted_handler(std::rc::Rc::new(move |id: &str, svg: &[u8]| {
            if id == "probe.repaint" {
                sink.borrow_mut()
                    .push(String::from_utf8_lossy(svg).to_string());
            }
        }));
        assert_eq!(a_header_button("probe.repaint"), IC_OK);
        assert_eq!(
            header_named("probe.repaint").expect("registered").svg,
            b"<svg/>".to_vec(),
            "it starts as whatever it was registered with"
        );

        assert_eq!(
            host_set_header_icon(c("probe.repaint").as_ptr(), c("<svg id=\"on\"/>").as_ptr()),
            IC_OK
        );
        assert_eq!(
            header_named("probe.repaint").expect("registered").svg,
            b"<svg id=\"on\"/>".to_vec(),
            "and the host remembers the new one, for a button built later"
        );
        assert_eq!(seen.borrow().as_slice(), ["<svg id=\"on\"/>".to_string()]);
        set_header_repainted_handler(std::rc::Rc::new(|_: &str, _: &[u8]| {}));
    }

    #[test]
    fn a_repaint_with_nothing_to_draw_is_refused() {
        assert_eq!(
            host_set_header_icon(c("probe.repaint").as_ptr(), c("").as_ptr()),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        assert_eq!(
            host_set_header_icon(c("").as_ptr(), c("<svg/>").as_ptr()),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
    }

    #[test]
    fn a_plugin_can_take_its_header_button_away_and_bring_it_back() {
        // Only this button: one handler per thread, and the tests run in parallel.
        let seen = std::rc::Rc::new(RefCell::new(Vec::<(String, bool)>::new()));
        let sink = seen.clone();
        set_header_shown_handler(std::rc::Rc::new(move |id: &str, shown: bool| {
            if id == "probe.indicator" {
                sink.borrow_mut().push((id.to_string(), shown));
            }
        }));
        assert_eq!(a_header_button("probe.indicator"), IC_OK);

        assert_eq!(
            host_set_header_visible(c("probe.indicator").as_ptr(), 0),
            IC_OK
        );
        assert!(!header_named("probe.indicator").expect("registered").shown);
        assert_eq!(
            host_set_header_visible(c("probe.indicator").as_ptr(), 1),
            IC_OK
        );
        assert!(header_named("probe.indicator").expect("registered").shown);
        assert_eq!(
            seen.borrow().as_slice(),
            [
                ("probe.indicator".to_string(), false),
                ("probe.indicator".to_string(), true)
            ]
        );
        set_header_shown_handler(std::rc::Rc::new(|_: &str, _: bool| {}));
    }

    #[test]
    fn hiding_a_button_nobody_registered_reaches_no_one() {
        let seen = std::rc::Rc::new(RefCell::new(0usize));
        let sink = seen.clone();
        set_header_shown_handler(std::rc::Rc::new(move |id: &str, _: bool| {
            if id == "probe.nobody" {
                *sink.borrow_mut() += 1;
            }
        }));
        // No such button was added on this thread, so the frontend is left alone.
        deliver(Wanted::HeaderVisible {
            id: "probe.nobody".to_string(),
            shown: false,
        });
        assert_eq!(*seen.borrow(), 0);
        set_header_shown_handler(std::rc::Rc::new(|_: &str, _: bool| {}));
    }

    #[test]
    fn a_plugin_on_a_thread_of_its_own_needs_a_way_back_to_the_frontend() {
        // No waker in a unit test, so there is nowhere to carry the request to.
        assert_eq!(
            host_set_header_visible(c("probe.unreachable").as_ptr(), 0),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
    }

    #[test]
    fn a_button_with_no_name_cannot_be_hidden() {
        assert_eq!(
            host_set_header_visible(c("   ").as_ptr(), 0),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
    }

    #[test]
    fn a_button_for_a_filesystem_is_kept_under_the_extensions_it_named() {
        let tally = Tally::new(0);
        assert_eq!(
            host_register_fs_action(
                c(".Torrent, .TORRENT ").as_ptr(),
                c("torrent.start").as_ptr(),
                c("<svg/>").as_ptr(),
                c("Start").as_ptr(),
                ic_plugin_api::IC_ENABLE_ALWAYS,
                ic_plugin_api::IC_ACTION_BUTTON,
                clicked,
                &tally as *const Tally as *mut c_void,
            ),
            IC_OK
        );
        let action = fs_action_named("torrent.start").expect("registered");
        // Read the way `register_filesystem` reads them, so spelling never
        // decides whether the button shows up.
        assert_eq!(action.extensions, vec![".torrent".to_string()]);
        assert_eq!(action.tooltip, "Start");
        action.fire(std::ptr::null_mut(), std::ptr::null_mut());
        assert_eq!(so_far(&tally), 1);
    }

    #[test]
    fn a_button_belongs_only_to_the_mount_it_was_registered_for() {
        assert_eq!(
            host_register_fs_action(
                c(".scoped,.alsoscoped").as_ptr(),
                c("scoped.probe").as_ptr(),
                c("<svg/>").as_ptr(),
                c("").as_ptr(),
                ic_plugin_api::IC_ENABLE_ALWAYS,
                ic_plugin_api::IC_ACTION_BUTTON,
                clicked,
                std::ptr::null_mut(),
            ),
            IC_OK
        );
        let action = fs_action_named("scoped.probe").expect("registered");
        assert!(action.belongs_to(".scoped"));
        assert!(action.belongs_to(".alsoscoped"));
        assert!(!action.belongs_to(".zip"));
        // Standing on plain local files is not standing in any plugin mount.
        assert!(!action.belongs_to(""));
    }

    #[test]
    fn registering_the_same_button_again_replaces_it_rather_than_doubling_it() {
        for tooltip in ["first", "second"] {
            assert_eq!(
                host_register_fs_action(
                    c(".twice").as_ptr(),
                    c("twice.probe").as_ptr(),
                    c("<svg/>").as_ptr(),
                    c(tooltip).as_ptr(),
                    ic_plugin_api::IC_ENABLE_ALWAYS,
                    ic_plugin_api::IC_ACTION_BUTTON,
                    clicked,
                    std::ptr::null_mut(),
                ),
                IC_OK
            );
        }
        let found: Vec<FsAction> = fs_actions()
            .into_iter()
            .filter(|a| a.action_id == "twice.probe")
            .collect();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tooltip, "second");
    }

    #[test]
    fn a_button_with_nothing_to_scope_or_draw_it_by_is_refused() {
        let empty = [
            (".none", "", "<svg/>"),
            (".none", "none.probe", ""),
            ("zip", "none.probe", "<svg/>"),
            ("", "none.probe", "<svg/>"),
        ];
        for (extensions, action_id, svg) in empty {
            assert_eq!(
                host_register_fs_action(
                    c(extensions).as_ptr(),
                    c(action_id).as_ptr(),
                    c(svg).as_ptr(),
                    c("").as_ptr(),
                    ic_plugin_api::IC_ENABLE_ALWAYS,
                    ic_plugin_api::IC_ACTION_BUTTON,
                    clicked,
                    std::ptr::null_mut(),
                ),
                ic_plugin_api::IC_ERR_INIT_FAILED,
                "{extensions}/{action_id}/{svg} should not have been taken"
            );
        }
        assert!(fs_action_named("none.probe").is_none());
    }

    #[test]
    fn a_stale_listing_reaches_the_handler_with_the_extensions_that_went_stale() {
        let seen = std::rc::Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
        let sink = seen.clone();
        set_fs_invalidate_handler(std::rc::Rc::new(move |extensions: &[String]| {
            sink.borrow_mut().push(extensions.to_vec());
        }));
        assert_eq!(host_fs_invalidate(c(".Torrent").as_ptr()), IC_OK);
        assert_eq!(seen.borrow().as_slice(), [vec![".torrent".to_string()]]);
        set_fs_invalidate_handler(std::rc::Rc::new(|_: &[String]| {}));
    }

    /// A connection is not reached through a file, so it has no extension to
    /// be named by — it names its kind instead. Until this was allowed across
    /// the boundary, a server or a peer-to-peer share could not say that
    /// anything of its own had changed.
    #[test]
    fn a_connection_kind_may_say_that_its_filesystem_moved_on() {
        let seen = std::rc::Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
        let sink = seen.clone();
        set_fs_invalidate_handler(std::rc::Rc::new(move |names: &[String]| {
            sink.borrow_mut().push(names.to_vec());
        }));
        assert_eq!(host_fs_invalidate(c("node-in-net").as_ptr()), IC_OK);
        assert_eq!(seen.borrow().as_slice(), [vec!["node-in-net".to_string()]]);
        set_fs_invalidate_handler(std::rc::Rc::new(|_: &[String]| {}));
    }

    /// And both kinds of name may arrive together, the way a plugin that
    /// brings a connection and a filesystem would send them.
    #[test]
    fn an_extension_and_a_kind_can_be_named_in_one_breath() {
        let seen = std::rc::Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
        let sink = seen.clone();
        set_fs_invalidate_handler(std::rc::Rc::new(move |names: &[String]| {
            sink.borrow_mut().push(names.to_vec());
        }));
        assert_eq!(host_fs_invalidate(c(".zip, node-in-net").as_ptr()), IC_OK);
        assert_eq!(
            seen.borrow().as_slice(),
            [vec![".zip".to_string(), "node-in-net".to_string()]]
        );
        set_fs_invalidate_handler(std::rc::Rc::new(|_: &[String]| {}));
    }

    #[test]
    fn an_invalidation_naming_nothing_is_refused_without_reaching_anyone() {
        assert_eq!(
            host_fs_invalidate(c("").as_ptr()),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
    }

    thread_local! {
        static KIND_ANSWER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    fn kind_answer(source: &str) -> ic_plugin_api::IcBytes {
        KIND_ANSWER.with(|slot| {
            *slot.borrow_mut() = source.as_bytes().to_vec();
            let held = slot.borrow();
            ic_plugin_api::IcBytes {
                data: held.as_ptr(),
                len: held.len() as u64,
            }
        })
    }

    extern "C" fn kind_describe(
        _: *const u8,
        _: u64,
        user_data: *mut c_void,
    ) -> ic_plugin_api::IcBytes {
        counted(user_data);
        kind_answer(LATER_DOCUMENT)
    }

    extern "C" fn kind_describe_as_event(
        _: *const u8,
        _: u64,
        user_data: *mut c_void,
    ) -> ic_plugin_api::IcBytes {
        counted(user_data);
        kind_answer(r#"{ "set": { "data.touched": true } }"#)
    }

    extern "C" fn kind_echoes_the_view_it_was_told(
        event: *const u8,
        len: u64,
        user_data: *mut c_void,
    ) -> ic_plugin_api::IcBytes {
        counted(user_data);
        let source = unsafe { std::slice::from_raw_parts(event, len as usize) };
        let heard: serde_json::Value = serde_json::from_slice(source).unwrap_or_default();
        kind_answer(&serde_json::json!({ "set": { "data.view": heard["view"] } }).to_string())
    }

    extern "C" fn kind_describes_rubbish(
        _: *const u8,
        _: u64,
        _: *mut c_void,
    ) -> ic_plugin_api::IcBytes {
        kind_answer("{ not a document")
    }

    fn register_kind(
        id: &str,
        source: &[u8],
        vtable: *const ic_plugin_api::IcConnectionVTable,
    ) -> c_int {
        register_kind_for(id, source, vtable, std::ptr::null_mut())
    }

    fn register_kind_for(
        id: &str,
        source: &[u8],
        vtable: *const ic_plugin_api::IcConnectionVTable,
        user_data: *mut c_void,
    ) -> c_int {
        let owned = CString::new(id).expect("an id without a nul");
        host_register_connection_kind(
            owned.as_ptr(),
            source.as_ptr(),
            source.len() as u64,
            vtable,
            user_data,
        )
    }

    #[test]
    fn a_connection_kind_keeps_the_document_it_registered() {
        assert_eq!(
            register_kind("kind.keeps", KIND_DOCUMENT.as_bytes(), std::ptr::null()),
            IC_OK
        );
        assert!(connection_kind_ids().contains(&"kind.keeps".to_string()));
        let document = connection_document("kind.keeps").expect("registered");
        assert_eq!(document.kind, "example");
        assert_eq!(document.fields.len(), 2);
        assert_eq!(document.identity.as_deref(), Some("name"));
    }

    #[test]
    fn a_document_the_host_cannot_accept_is_refused_whole() {
        let broken = KIND_DOCUMENT.replace(r#""bind": "url", "type""#, r#""bind": "uri", "type""#);
        assert_ne!(broken, KIND_DOCUMENT);
        assert_eq!(
            register_kind("kind.broken", broken.as_bytes(), std::ptr::null()),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        assert!(connection_document("kind.broken").is_none());
    }

    #[test]
    fn a_document_that_is_not_utf8_is_refused() {
        assert_eq!(
            register_kind("kind.binary", &[0xff, 0xfe, 0x00], std::ptr::null()),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        assert!(connection_document("kind.binary").is_none());
    }

    #[test]
    fn a_kind_without_an_id_or_a_document_is_refused() {
        assert_eq!(
            register_kind("", KIND_DOCUMENT.as_bytes(), std::ptr::null()),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        let owned = CString::new("kind.empty").expect("an id without a nul");
        assert_eq!(
            host_register_connection_kind(
                owned.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null_mut()
            ),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
    }

    #[test]
    fn registering_the_same_kind_again_replaces_it() {
        assert_eq!(
            register_kind("kind.twice", KIND_DOCUMENT.as_bytes(), std::ptr::null()),
            IC_OK
        );
        let second = KIND_DOCUMENT.replace(r#""kind": "example""#, r#""kind": "replaced""#);
        assert_eq!(
            register_kind("kind.twice", second.as_bytes(), std::ptr::null()),
            IC_OK
        );
        assert_eq!(
            connection_document("kind.twice").expect("registered").kind,
            "replaced"
        );
        assert_eq!(
            connection_kind_ids()
                .iter()
                .filter(|id| id.as_str() == "kind.twice")
                .count(),
            1
        );
    }

    #[test]
    fn a_kind_registered_without_a_vtable_describes_but_cannot_open() {
        assert_eq!(
            register_kind("kind.describe", KIND_DOCUMENT.as_bytes(), std::ptr::null()),
            IC_OK
        );
        with_connection_kinds(|list| {
            let kind = list
                .iter()
                .find(|kind| kind.id == "kind.describe")
                .expect("registered");
            assert!(!kind.can_open());
            assert!(kind.open_fn().is_none());
            assert!(!kind.can_open());
        });
    }

    #[test]
    fn a_kind_with_a_full_vtable_can_open() {
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: kind_open,
            fs: std::ptr::null(),
            describe: None,
            on_event: None,
        };
        assert_eq!(
            register_kind("kind.opens", KIND_DOCUMENT.as_bytes(), &table),
            IC_OK
        );
        with_connection_kinds(|list| {
            let kind = list
                .iter()
                .find(|kind| kind.id == "kind.opens")
                .expect("registered");
            let open = kind
                .open_fn()
                .expect("the open call is within the claimed size");
            assert_eq!(
                open(std::ptr::null(), 0, std::ptr::null_mut()),
                7 as ic_plugin_api::IcFsHandle
            );
        });
    }

    thread_local! {
        static OPENED_WITH: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    extern "C" fn conn_open(
        settings: *const u8,
        len: u64,
        _: *mut c_void,
    ) -> ic_plugin_api::IcFsHandle {
        let raw = unsafe { std::slice::from_raw_parts(settings, len as usize) };
        OPENED_WITH.with(|seen| {
            seen.borrow_mut()
                .push(String::from_utf8_lossy(raw).to_string())
        });
        9usize as ic_plugin_api::IcFsHandle
    }

    extern "C" fn fs_open_in(
        _: ic_plugin_api::IcFsSource,
        _: *const c_char,
        _: *mut c_void,
    ) -> ic_plugin_api::IcFsHandle {
        9usize as ic_plugin_api::IcFsHandle
    }
    extern "C" fn fs_close(_: ic_plugin_api::IcFsHandle) {}
    extern "C" fn fs_list(
        _: ic_plugin_api::IcFsHandle,
        _: *const c_char,
    ) -> ic_plugin_api::IcListing {
        ic_plugin_api::IcListing::EMPTY
    }
    const SERVED: &[u8] = b"served over the connection";

    extern "C" fn fs_read(
        _: ic_plugin_api::IcFsHandle,
        _: *const c_char,
    ) -> ic_plugin_api::IcBytes {
        ic_plugin_api::IcBytes {
            data: SERVED.as_ptr(),
            len: SERVED.len() as u64,
        }
    }
    extern "C" fn fs_flag(_: ic_plugin_api::IcFsHandle) -> c_int {
        1
    }
    extern "C" fn fs_error(_: ic_plugin_api::IcFsHandle) -> *const c_char {
        std::ptr::null()
    }

    fn served_filesystem() -> ic_plugin_api::IcFsVTable {
        ic_plugin_api::IcFsVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcFsVTable>() as u32,
            open_in: fs_open_in,
            close: fs_close,
            list: fs_list,
            read: fs_read,
            is_read_only: fs_flag,
            last_error: fs_error,
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

    fn stored_settings() -> std::collections::BTreeMap<String, String> {
        std::collections::BTreeMap::from([
            ("host".to_string(), "example.org".to_string()),
            ("user".to_string(), "ivan".to_string()),
        ])
    }

    const VIEW_DOCUMENT: &str = r#"{
        "schema": 1,
        "fields": [],
        "form": { "t": "view", "surface": "dialog", "children": [
            { "t": "text", "id": "memory", "text": "Memory: {data.mem}" } ] }
    }"#;

    fn registry_is_free() -> bool {
        views_registry().try_lock().is_ok()
    }

    thread_local! {
        static REGISTRY_WAS_FREE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static DESCRIBED_WITH: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static ANSWER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static CLOSED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    fn answer_with(source: &str) -> ic_plugin_api::IcBytes {
        ANSWER.with(|slot| {
            *slot.borrow_mut() = source.as_bytes().to_vec();
            let held = slot.borrow();
            ic_plugin_api::IcBytes {
                data: held.as_ptr(),
                len: held.len() as u64,
            }
        })
    }

    extern "C" fn view_describe(
        ctx: *const u8,
        len: u64,
        _: *mut c_void,
    ) -> ic_plugin_api::IcBytes {
        REGISTRY_WAS_FREE.with(|slot| slot.set(registry_is_free()));
        let raw = unsafe { std::slice::from_raw_parts(ctx, len as usize) };
        DESCRIBED_WITH.with(|seen| {
            seen.borrow_mut()
                .push(String::from_utf8_lossy(raw).to_string())
        });
        answer_with(VIEW_DOCUMENT)
    }

    extern "C" fn view_describe_broken(
        _: *const u8,
        _: u64,
        _: *mut c_void,
    ) -> ic_plugin_api::IcBytes {
        answer_with(
            r#"{"schema":1,"fields":[],"form":{"t":"column","children":[
            {"t":"input","id":"a","bind":"nothing"}]}}"#,
        )
    }

    extern "C" fn stub_event(event: *const u8, len: u64, _: *mut c_void) -> ic_plugin_api::IcBytes {
        let raw = unsafe { std::slice::from_raw_parts(event, len as usize) };
        let seen: serde_json::Value = serde_json::from_slice(raw).unwrap_or_default();
        let node = seen.get("node").and_then(|v| v.as_str()).unwrap_or("");
        answer_with(&format!(r#"{{"set":{{"view.last":"{node}"}}}}"#))
    }

    extern "C" fn stub_closed(instance: u64, _: *mut c_void) {
        CLOSED.with(|slot| slot.set(instance));
    }

    fn view_table(describe: ic_plugin_api::IcViewDescribeFn) -> ic_plugin_api::IcViewVTable {
        ic_plugin_api::IcViewVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcViewVTable>() as u32,
            describe,
            on_event: Some(stub_event),
            closed: Some(stub_closed),
        }
    }

    fn register_view_named(id: &str, table: &ic_plugin_api::IcViewVTable) -> c_int {
        let owned = CString::new(id).expect("an id without a nul");
        let title = CString::new("System information").expect("a title without a nul");
        host_register_view(owned.as_ptr(), title.as_ptr(), table, std::ptr::null_mut())
    }

    #[test]
    fn a_plugin_can_hand_the_host_a_picture_to_show() {
        let name = CString::new("os-test").expect("a name without a nul");
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#;
        assert_eq!(
            host_register_asset(name.as_ptr(), svg.as_ptr(), svg.len() as u64),
            IC_OK
        );
        assert_eq!(asset("os-test").as_deref(), Some(svg.as_slice()));
        assert!(asset_names().contains(&"os-test".to_string()));
        assert!(asset("os-missing").is_none());
        assert_eq!(
            host_register_asset(name.as_ptr(), std::ptr::null(), 0),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
    }

    #[test]
    fn a_plugin_can_teach_the_host_its_own_words() {
        let language = CString::new("en").expect("a language without a nul");
        let catalogue = br#"{"sysinfo.hostname":"Host name"}"#;
        assert_eq!(
            host_register_locales(
                language.as_ptr(),
                catalogue.as_ptr(),
                catalogue.len() as u64
            ),
            IC_OK
        );
        assert_eq!(ic_i18n::tr("sysinfo.hostname"), "Host name");
    }

    #[test]
    fn a_catalogue_the_host_cannot_read_is_refused_rather_than_half_applied() {
        let language = CString::new("en").expect("a language without a nul");
        let nested = br#"{"sysinfo.nested":{"deeper":"no"}}"#;
        assert_eq!(
            host_register_locales(language.as_ptr(), nested.as_ptr(), nested.len() as u64),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        assert_eq!(ic_i18n::tr("sysinfo.nested"), "sysinfo.nested");
        assert_eq!(
            host_register_locales(std::ptr::null(), nested.as_ptr(), nested.len() as u64),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
    }

    #[test]
    fn a_registered_view_describes_itself_and_the_host_accepts_the_document() {
        DESCRIBED_WITH.with(|seen| seen.borrow_mut().clear());
        assert_eq!(
            register_view_named("view.sysinfo", &view_table(view_describe)),
            IC_OK
        );
        assert!(view_ids().contains(&"view.sysinfo".to_string()));
        assert_eq!(
            view_title("view.sysinfo").as_deref(),
            Some("System information")
        );
        let context = serde_json::json!({ "host": { "kind": "gtk" } });
        let document = describe_view("view.sysinfo", &context).expect("described");
        assert_eq!(document.form.surface, ic_view::Surface::Dialog);
        DESCRIBED_WITH.with(|seen| {
            assert_eq!(
                *seen.borrow(),
                vec![r#"{"host":{"kind":"gtk"}}"#.to_string()],
                "the context reaches the plugin as one json object"
            );
        });
    }

    #[test]
    fn the_registry_is_not_locked_while_the_plugin_is_running() {
        assert_eq!(
            register_view_named("view.reenters", &view_table(view_describe)),
            IC_OK
        );
        REGISTRY_WAS_FREE.with(|slot| slot.set(false));
        assert!(describe_view("view.reenters", &serde_json::json!({})).is_some());
        assert!(
            REGISTRY_WAS_FREE.with(|slot| slot.get()),
            "a plugin that calls back into the host from describe would deadlock"
        );
    }

    #[test]
    fn a_view_that_answers_with_an_unusable_document_is_refused() {
        assert_eq!(
            register_view_named("view.broken", &view_table(view_describe_broken)),
            IC_OK
        );
        assert!(describe_view("view.broken", &serde_json::json!({})).is_none());
    }

    #[test]
    fn a_view_table_without_a_describe_call_is_refused_at_registration() {
        #[repr(C)]
        struct OnlyTheSize {
            struct_size: u32,
        }
        let stub = OnlyTheSize {
            struct_size: std::mem::size_of::<u32>() as u32,
        };
        let pointer = &stub as *const OnlyTheSize as *const ic_plugin_api::IcViewVTable;
        let owned = CString::new("view.mute").expect("an id without a nul");
        assert_eq!(
            host_register_view(
                owned.as_ptr(),
                std::ptr::null(),
                pointer,
                std::ptr::null_mut()
            ),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        assert!(view_title("view.mute").is_none());
    }

    #[test]
    fn opening_a_view_reaches_the_handler_the_app_installed() {
        assert_eq!(
            register_view_named("view.opens", &view_table(view_describe)),
            IC_OK
        );
        let seen: std::rc::Rc<RefCell<Vec<(String, String)>>> =
            std::rc::Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        set_view_open_handler(std::rc::Rc::new(move |id: &str, argument: &str| {
            sink.borrow_mut()
                .push((id.to_string(), argument.to_string()));
        }));
        let id = CString::new("view.opens").expect("an id without a nul");
        let argument = br#"{"page":"hash"}"#;
        assert_eq!(
            host_open_view(id.as_ptr(), argument.as_ptr(), argument.len() as u64),
            IC_OK
        );
        assert_eq!(
            *seen.borrow(),
            vec![("view.opens".to_string(), r#"{"page":"hash"}"#.to_string())]
        );
        let unknown = CString::new("view.nobody").expect("an id without a nul");
        assert_eq!(
            host_open_view(unknown.as_ptr(), std::ptr::null(), 0),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
    }

    #[test]
    fn an_event_round_trips_and_closing_reaches_the_plugin() {
        assert_eq!(
            register_view_named("view.events", &view_table(view_describe)),
            IC_OK
        );
        let answer = view_event(
            "view.events",
            &serde_json::json!({ "type": "activate", "node": "copy" }),
        )
        .expect("the plugin answered");
        assert_eq!(answer["set"]["view.last"], serde_json::json!("copy"));
        CLOSED.with(|slot| slot.set(0));
        view_closed("view.events", 7);
        CLOSED.with(|slot| assert_eq!(slot.get(), 7));
    }

    #[test]
    fn a_kind_that_serves_its_own_filesystem_mounts_and_receives_its_settings() {
        OPENED_WITH.with(|seen| seen.borrow_mut().clear());
        let filesystem = served_filesystem();
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: conn_open,
            fs: &filesystem,
            describe: None,
            on_event: None,
        };
        assert_eq!(
            register_kind("kind.mount", KIND_DOCUMENT.as_bytes(), &table),
            IC_OK
        );
        with_connection_kinds(|list| {
            let kind = list
                .iter()
                .find(|kind| kind.id == "kind.mount")
                .expect("registered");
            assert!(kind.can_open());
        });
        let mounted = mount_connection("kind.mount", &stored_settings()).expect("mounts");
        let content = futures::executor::block_on(mounted.read_file("/a.txt".to_string(), None))
            .expect("the plugin answered");
        assert_eq!(content, SERVED.to_vec());
        OPENED_WITH.with(|seen| {
            assert_eq!(
                *seen.borrow(),
                vec![r#"{"host":"example.org","user":"ivan"}"#.to_string()],
                "the settings reach the plugin as one json object"
            );
        });
    }

    #[test]
    fn a_kind_that_only_describes_a_form_cannot_be_mounted() {
        assert_eq!(
            register_kind("kind.formonly", KIND_DOCUMENT.as_bytes(), std::ptr::null()),
            IC_OK
        );
        assert!(mount_connection("kind.formonly", &stored_settings()).is_none());
    }

    #[test]
    fn a_kind_with_no_document_mounts_and_offers_no_form() {
        let filesystem = served_filesystem();
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: conn_open,
            fs: &filesystem,
            describe: None,
            on_event: None,
        };
        assert_eq!(register_kind("kind.formless", b"", &table), IC_OK);
        assert!(connection_kind_ids().contains(&"kind.formless".to_string()));
        assert!(connection_document("kind.formless").is_none());
        assert!(mount_connection("kind.formless", &stored_settings()).is_some());
    }

    #[test]
    fn a_kind_with_neither_a_document_nor_a_way_to_mount_is_refused() {
        assert_eq!(
            register_kind("kind.nothing", b"", std::ptr::null()),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        let opens_nothing = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: kind_open,
            fs: std::ptr::null(),
            describe: None,
            on_event: None,
        };
        assert_eq!(
            register_kind("kind.nothing", b"", &opens_nothing),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        assert!(!connection_kind_ids().contains(&"kind.nothing".to_string()));
    }

    fn pinned(rows: Vec<ic_plugin_api::IcPinnedConnection>) -> ic_plugin_api::IcPinnedConnections {
        thread_local! {
            static ROWS: RefCell<Vec<ic_plugin_api::IcPinnedConnection>> =
                const { RefCell::new(Vec::new()) };
        }
        ROWS.with(|held| {
            *held.borrow_mut() = rows;
            let borrowed = held.borrow();
            ic_plugin_api::IcPinnedConnections {
                count: borrowed.len() as u32,
                rows: borrowed.as_ptr(),
                ..ic_plugin_api::IcPinnedConnections::EMPTY
            }
        })
    }

    fn pin(id: &'static CStr, view: &'static CStr) -> ic_plugin_api::IcPinnedConnection {
        ic_plugin_api::IcPinnedConnection {
            id: id.as_ptr(),
            title: c"igor".as_ptr(),
            svg: c"<svg id=\"pin\"/>".as_ptr(),
            view: view.as_ptr(),
        }
    }

    extern "C" fn one_good_and_three_bad(_: *mut c_void) -> ic_plugin_api::IcPinnedConnections {
        pinned(vec![
            pin(c"account", c"view.pinned"),
            pin(c"nowhere", c"view.nobody.registered"),
            pin(c"", c"view.pinned"),
            pin(c"account", c"view.pinned"),
        ])
    }

    extern "C" fn none_now(_: *mut c_void) -> ic_plugin_api::IcPinnedConnections {
        pinned(Vec::new())
    }

    extern "C" fn unstamped(_: *mut c_void) -> ic_plugin_api::IcPinnedConnections {
        ic_plugin_api::IcPinnedConnections {
            magic: 0,
            ..one_good_and_three_bad(std::ptr::null_mut())
        }
    }

    #[test]
    fn two_plugins_may_bring_a_picture_under_the_same_name() {
        let name = c"icon.svg";
        let mine = b"<svg id=\"mine\"/>";
        let theirs = b"<svg id=\"theirs\"/>";
        assert_eq!(
            host_register_plugin_asset(
                c"ic-one".as_ptr(),
                name.as_ptr(),
                mine.as_ptr(),
                mine.len() as u64
            ),
            IC_OK
        );
        assert_eq!(
            host_register_plugin_asset(
                c"ic-two".as_ptr(),
                name.as_ptr(),
                theirs.as_ptr(),
                theirs.len() as u64
            ),
            IC_OK
        );
        assert_eq!(asset("ic-one/icon.svg").as_deref(), Some(&mine[..]));
        assert_eq!(asset("ic-two/icon.svg").as_deref(), Some(&theirs[..]));
        assert_eq!(
            asset("icon.svg"),
            None,
            "an owned picture is not reachable by the bare name"
        );
    }

    #[test]
    fn an_asset_without_an_owner_or_with_a_path_for_a_name_is_refused() {
        let bytes = b"<svg/>";
        for (owner, name) in [
            (c"".as_ptr(), c"x.svg".as_ptr()),
            (c"ic-one".as_ptr(), c"".as_ptr()),
        ] {
            assert_eq!(
                host_register_plugin_asset(owner, name, bytes.as_ptr(), bytes.len() as u64),
                ic_plugin_api::IC_ERR_INIT_FAILED
            );
        }
        assert_eq!(
            host_register_plugin_asset(
                c"ic-one".as_ptr(),
                c"nested/x.svg".as_ptr(),
                bytes.as_ptr(),
                bytes.len() as u64
            ),
            ic_plugin_api::IC_ERR_INIT_FAILED,
            "a name with a slash would read as another plugin's"
        );
    }

    #[test]
    fn a_pinned_connection_is_listed_once_and_only_with_a_view_behind_it() {
        assert_eq!(
            register_view_named("view.pinned", &view_table(view_describe)),
            IC_OK
        );
        assert_eq!(
            host_register_pinned_connections(
                c"probe.pins".as_ptr(),
                one_good_and_three_bad,
                std::ptr::null_mut()
            ),
            IC_OK
        );
        assert_eq!(
            pinned_connections(),
            vec![PinnedConnection {
                id: "account".to_string(),
                title: "igor".to_string(),
                svg: b"<svg id=\"pin\"/>".to_vec(),
                view: "view.pinned".to_string(),
            }]
        );

        assert_eq!(
            host_register_pinned_connections(
                c"probe.pins".as_ptr(),
                none_now,
                std::ptr::null_mut()
            ),
            IC_OK
        );
        assert!(
            pinned_connections().is_empty(),
            "registered again under the same id, so it replaced the first"
        );
    }

    #[test]
    fn pinned_connections_without_a_source_id_or_a_stamp_are_not_taken() {
        assert_eq!(
            host_register_pinned_connections(c"".as_ptr(), none_now, std::ptr::null_mut()),
            ic_plugin_api::IC_ERR_INIT_FAILED
        );
        assert_eq!(
            register_view_named("view.pinned", &view_table(view_describe)),
            IC_OK
        );
        assert_eq!(
            host_register_pinned_connections(
                c"probe.bad".as_ptr(),
                unstamped,
                std::ptr::null_mut()
            ),
            IC_OK
        );
        assert!(pinned_connections().is_empty());
    }

    #[test]
    fn saying_the_pinned_connections_changed_reaches_the_frontend() {
        let heard = std::rc::Rc::new(std::cell::Cell::new(0));
        let counting = heard.clone();
        set_pinned_changed_handler(std::rc::Rc::new(move || counting.set(counting.get() + 1)));
        assert_eq!(host_pinned_connections_changed(), IC_OK);
        assert_eq!(heard.get(), 1);
    }

    #[test]
    fn a_kind_whose_filesystem_is_missing_calls_the_host_needs_cannot_be_mounted() {
        let mut filesystem = served_filesystem();
        filesystem.struct_size = std::mem::offset_of!(ic_plugin_api::IcFsVTable, last_error) as u32;
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: conn_open,
            fs: &filesystem,
            describe: None,
            on_event: None,
        };
        assert_eq!(
            register_kind("kind.halffs", KIND_DOCUMENT.as_bytes(), &table),
            IC_OK
        );
        with_connection_kinds(|list| {
            let kind = list
                .iter()
                .find(|kind| kind.id == "kind.halffs")
                .expect("registered");
            assert!(!kind.can_open(), "an unusable filesystem is not offered");
        });
        assert!(mount_connection("kind.halffs", &stored_settings()).is_none());
    }

    #[test]
    fn a_short_vtable_is_read_only_as_far_as_it_claims() {
        #[repr(C)]
        struct OnlyTheSize {
            struct_size: u32,
        }
        let stub = OnlyTheSize {
            struct_size: std::mem::size_of::<u32>() as u32,
        };
        let pointer = &stub as *const OnlyTheSize as *const ic_plugin_api::IcConnectionVTable;
        assert_eq!(
            register_kind("kind.short", KIND_DOCUMENT.as_bytes(), pointer),
            IC_OK
        );
        with_connection_kinds(|list| {
            let kind = list
                .iter()
                .find(|kind| kind.id == "kind.short")
                .expect("registered");
            assert!(kind.open_fn().is_none());
            assert!(!kind.can_open());
        });
        assert!(connection_document("kind.short").is_some());
    }

    #[test]
    fn a_kind_is_found_however_the_record_spells_its_protocol() {
        assert_eq!(
            register_kind("kind.case", KIND_DOCUMENT.as_bytes(), std::ptr::null()),
            IC_OK
        );
        assert!(connection_document("kind.case").is_some());
        assert!(connection_document("KIND.CASE").is_some());
        assert!(connection_document("Kind.Case").is_some());
        assert!(connection_document("kind.cas").is_none());
    }

    #[test]
    fn a_registered_kind_is_visible_from_another_thread() {
        assert_eq!(
            register_kind("kind.shared", KIND_DOCUMENT.as_bytes(), std::ptr::null()),
            IC_OK
        );
        let seen = std::thread::spawn(|| connection_document("kind.shared").is_some())
            .join()
            .expect("the reader thread finished");
        assert!(seen);
    }

    extern "C" fn noop(_: *mut c_void, _: *mut c_void) {}

    fn register(id: &str, side: u32, priority: i32) {
        let id = CString::new(id).unwrap();
        let svg = CString::new("<svg/>").unwrap();
        let tip = CString::new("tip").unwrap();
        host_add_toolbar_button(
            id.as_ptr(),
            svg.as_ptr(),
            tip.as_ptr(),
            side,
            priority,
            ic_plugin_api::IC_ENABLE_ALWAYS,
            noop,
            std::ptr::null_mut(),
        );
    }

    thread_local! {
        static FAKE: RefCell<(Vec<ic_plugin_api::IcColumn>, Vec<*const c_char>)> =
            const { RefCell::new((Vec::new(), Vec::new())) };
    }

    extern "C" fn fake_rows(_: *mut c_void) -> ic_plugin_api::IcTable {
        FAKE.with(|f| {
            let mut slot = f.borrow_mut();
            slot.0 = vec![
                ic_plugin_api::IcColumn {
                    key: c"name".as_ptr(),
                    title: c"Name".as_ptr(),
                    width: 200,
                },
                ic_plugin_api::IcColumn {
                    key: c"pid".as_ptr(),
                    title: c"PID".as_ptr(),
                    width: 80,
                },
                ic_plugin_api::IcColumn {
                    key: c"mem".as_ptr(),
                    title: c"Memory".as_ptr(),
                    width: 90,
                },
            ];
            slot.1 = vec![
                c"init".as_ptr(),
                c"1".as_ptr(),
                c"2 MB".as_ptr(),
                c"bash".as_ptr(),
                c"42".as_ptr(),
                c"7 MB".as_ptr(),
            ];
            ic_plugin_api::IcTable {
                columns: slot.0.as_ptr(),
                column_count: 3,
                cells: slot.1.as_ptr(),
                row_count: 2,
                key_column: 1,
            }
        })
    }

    fn fake_source() -> PanelSource {
        PanelSource {
            id: "fake".to_string(),
            title: "Fake".to_string(),
            svg: b"<svg/>".to_vec(),
            rows: fake_rows,
            tree: None,
            user_data: 0,
        }
    }

    #[test]
    fn a_bigger_priority_sits_further_from_the_edge() {
        TOOLBAR.with(|t| t.borrow_mut().clear());
        register("near", ic_plugin_api::IC_SIDE_RIGHT, 10);
        register("far", ic_plugin_api::IC_SIDE_RIGHT, 90);
        let ids: Vec<String> = toolbar_entries(ic_plugin_api::IC_SIDE_RIGHT)
            .iter()
            .map(|e| e.id.clone())
            .collect();
        assert_eq!(ids, vec!["far".to_string(), "near".to_string()]);
    }

    #[test]
    fn the_two_sides_do_not_mix() {
        TOOLBAR.with(|t| t.borrow_mut().clear());
        register("l", ic_plugin_api::IC_SIDE_LEFT, 1);
        register("r", ic_plugin_api::IC_SIDE_RIGHT, 1);
        assert_eq!(toolbar_entries(ic_plugin_api::IC_SIDE_LEFT).len(), 1);
        assert_eq!(toolbar_entries(ic_plugin_api::IC_SIDE_RIGHT).len(), 1);
    }

    #[test]
    fn a_registration_without_an_icon_is_refused() {
        TOOLBAR.with(|t| t.borrow_mut().clear());
        let id = CString::new("x").unwrap();
        let empty = CString::new("").unwrap();
        let code = host_add_toolbar_button(
            id.as_ptr(),
            empty.as_ptr(),
            empty.as_ptr(),
            0,
            0,
            ic_plugin_api::IC_ENABLE_ALWAYS,
            noop,
            std::ptr::null_mut(),
        );
        assert_ne!(code, IC_OK);
        assert!(toolbar_entries(0).is_empty());
    }

    #[test]
    fn registering_the_same_id_twice_replaces_it_rather_than_duplicating() {
        TOOLBAR.with(|t| t.borrow_mut().clear());
        register("same", ic_plugin_api::IC_SIDE_RIGHT, 1);
        register("same", ic_plugin_api::IC_SIDE_RIGHT, 2);
        let list = toolbar_entries(ic_plugin_api::IC_SIDE_RIGHT);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].priority, 2);
    }

    #[test]
    fn a_selection_comes_back_as_one_slice() {
        set_selection_provider(std::rc::Rc::new(|| {
            vec![
                SelectedItem {
                    path: "/a/dir".to_string(),
                    key: "/a/dir".to_string(),
                    is_dir: true,
                },
                SelectedItem {
                    path: "/a/file.bin".to_string(),
                    key: "42".to_string(),
                    is_dir: false,
                },
            ]
        }));
        let sel = host_selection();
        let items = sel.as_slice();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_directory());
        assert_eq!(items[1].path_string().as_deref(), Some("/a/file.bin"));
    }

    #[test]
    fn no_selection_comes_back_empty_rather_than_dangling() {
        set_selection_provider(std::rc::Rc::new(Vec::new));
        let sel = host_selection();
        assert_eq!(sel.count, 0);
        assert!(sel.as_slice().is_empty());
    }

    #[test]
    fn the_host_table_outlives_the_call_that_hands_it_out() {
        let first = host_ref() as *const IcHost as usize;
        let second = host_ref() as *const IcHost as usize;
        assert_eq!(
            first, second,
            "a plugin keeps this pointer, so it must not live on the stack"
        );
    }

    #[test]
    fn header_buttons_are_ordered_from_the_edge_outwards() {
        HEADER.with(|h| h.borrow_mut().clear());
        let mk = |id: &str, prio: i32| {
            let id = CString::new(id).unwrap();
            let svg = CString::new("<svg/>").unwrap();
            let empty = CString::new("").unwrap();
            host_add_header_button(
                id.as_ptr(),
                svg.as_ptr(),
                empty.as_ptr(),
                empty.as_ptr(),
                ic_plugin_api::IC_SIDE_RIGHT,
                prio,
                noop,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(mk("far", 50), IC_OK);
        assert_eq!(mk("near", 10), IC_OK);
        let ids: Vec<String> = header_entries(ic_plugin_api::IC_SIDE_RIGHT)
            .iter()
            .map(|e| e.id.clone())
            .collect();
        assert_eq!(ids, vec!["near".to_string(), "far".to_string()]);
    }

    #[test]
    fn a_header_button_needs_an_icon_or_a_label() {
        HEADER.with(|h| h.borrow_mut().clear());
        let id = CString::new("x").unwrap();
        let empty = CString::new("").unwrap();
        assert_ne!(
            host_add_header_button(
                id.as_ptr(),
                empty.as_ptr(),
                empty.as_ptr(),
                empty.as_ptr(),
                0,
                0,
                noop,
                std::ptr::null_mut(),
            ),
            IC_OK
        );
        let label = CString::new("Text only").unwrap();
        assert_eq!(
            host_add_header_button(
                id.as_ptr(),
                empty.as_ptr(),
                label.as_ptr(),
                empty.as_ptr(),
                0,
                0,
                noop,
                std::ptr::null_mut(),
            ),
            IC_OK
        );
    }

    #[test]
    fn a_header_label_is_replaced_in_place_and_the_frontend_is_told() {
        HEADER.with(|h| h.borrow_mut().clear());
        let id = CString::new("peers").unwrap();
        let svg = CString::new("<svg/>").unwrap();
        let first = CString::new("Node In Net").unwrap();
        let empty = CString::new("").unwrap();
        assert_eq!(
            host_add_header_button(
                id.as_ptr(),
                svg.as_ptr(),
                first.as_ptr(),
                empty.as_ptr(),
                ic_plugin_api::IC_SIDE_RIGHT,
                0,
                noop,
                std::ptr::null_mut(),
            ),
            IC_OK
        );

        let heard: std::rc::Rc<RefCell<Vec<(String, String)>>> = Default::default();
        let told = heard.clone();
        set_header_changed_handler(std::rc::Rc::new(move |id: &str, label: &str| {
            told.borrow_mut().push((id.to_string(), label.to_string()));
        }));

        let next = CString::new("Node In Net \u{b7} 3").unwrap();
        assert_eq!(host_set_header_label(id.as_ptr(), next.as_ptr()), IC_OK);

        let shown = header_entries(ic_plugin_api::IC_SIDE_RIGHT);
        let entry = shown.iter().find(|e| e.id == "peers").expect("still there");
        assert_eq!(entry.label, "Node In Net \u{b7} 3");
        assert_eq!(entry.svg, b"<svg/>", "only the text changes");
        assert_eq!(heard.borrow().len(), 1);
        assert_eq!(heard.borrow()[0].1, "Node In Net \u{b7} 3");
        HEADER_CHANGED.with(|slot| *slot.borrow_mut() = None);
    }

    #[test]
    fn a_label_for_a_button_nobody_added_is_refused_rather_than_invented() {
        HEADER.with(|h| h.borrow_mut().clear());
        let id = CString::new("nosuchbutton").unwrap();
        let label = CString::new("3").unwrap();
        // No waker and no button of that name: there is nowhere for it to go.
        assert_ne!(host_set_header_label(id.as_ptr(), label.as_ptr()), IC_OK);
        assert!(header_entries(ic_plugin_api::IC_SIDE_RIGHT).is_empty());
    }

    #[test]
    fn an_empty_label_leaves_the_button_with_its_icon() {
        HEADER.with(|h| h.borrow_mut().clear());
        let id = CString::new("quiet").unwrap();
        let svg = CString::new("<svg/>").unwrap();
        let label = CString::new("counting").unwrap();
        let empty = CString::new("").unwrap();
        host_add_header_button(
            id.as_ptr(),
            svg.as_ptr(),
            label.as_ptr(),
            empty.as_ptr(),
            ic_plugin_api::IC_SIDE_LEFT,
            0,
            noop,
            std::ptr::null_mut(),
        );
        assert_eq!(host_set_header_label(id.as_ptr(), empty.as_ptr()), IC_OK);
        let shown = header_entries(ic_plugin_api::IC_SIDE_LEFT);
        assert_eq!(shown[0].label, "");
        assert_eq!(shown[0].svg, b"<svg/>");
    }

    #[test]
    fn a_kind_that_describes_itself_is_asked_every_time_the_form_opens() {
        static ASKED: Tally = Tally::new(0);
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: kind_open,
            fs: std::ptr::null(),
            describe: Some(kind_describe),
            on_event: None,
        };
        assert_eq!(
            register_kind_for(
                "kind.singleton",
                KIND_DOCUMENT.as_bytes(),
                &table,
                &ASKED as *const Tally as *mut c_void,
            ),
            IC_OK
        );

        let first = connection_document("kind.singleton").expect("a form");
        let second = connection_document("kind.singleton").expect("a form");
        assert_eq!(
            so_far(&ASKED),
            2,
            "the plugin answers afresh rather than the host caching it"
        );

        let named = |document: &ic_view::Document, id: &str| {
            let mut found = false;
            document
                .form
                .walk(&mut |node| found |= node.id.as_deref() == Some(id));
            found
        };
        assert!(
            named(&first, "already"),
            "what it said now, not at registration"
        );
        assert!(named(&second, "already"));
        assert!(!named(&first, "url"));
    }

    #[test]
    fn a_kind_that_answers_with_rubbish_falls_back_to_what_it_registered() {
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: kind_open,
            fs: std::ptr::null(),
            describe: Some(kind_describes_rubbish),
            on_event: None,
        };
        assert_eq!(
            register_kind("kind.rubbish", KIND_DOCUMENT.as_bytes(), &table),
            IC_OK
        );

        let document = connection_document("kind.rubbish").expect("a form all the same");
        let mut found = false;
        document
            .form
            .walk(&mut |node| found |= node.id.as_deref() == Some("url"));
        assert!(
            found,
            "the user is left with the registered form, not with none"
        );
    }

    #[test]
    fn a_kind_from_an_older_plugin_has_no_describe_and_keeps_its_document() {
        static ASKED: Tally = Tally::new(0);
        let table = ic_plugin_api::IcConnectionVTable {
            // Exactly the table a plugin built before `describe` existed, so this
            // stays honest however the struct grows afterwards.
            struct_size: std::mem::offset_of!(ic_plugin_api::IcConnectionVTable, describe) as u32,
            open: kind_open,
            fs: std::ptr::null(),
            describe: Some(kind_describe),
            on_event: Some(kind_describe_as_event),
        };
        assert_eq!(
            register_kind_for(
                "kind.older",
                KIND_DOCUMENT.as_bytes(),
                &table,
                &ASKED as *const Tally as *mut c_void,
            ),
            IC_OK
        );

        let document = connection_document("kind.older").expect("a form");
        let mut found = false;
        document
            .form
            .walk(&mut |node| found |= node.id.as_deref() == Some("url"));
        assert!(found);
        assert!(
            !connection_takes_events("kind.older"),
            "nor is the field after it"
        );
        assert!(connection_event("kind.older", &serde_json::json!({})).is_none());
        assert_eq!(
            so_far(&ASKED),
            0,
            "a table that does not claim the fields is never read past its end"
        );
    }

    #[test]
    fn a_kind_that_takes_events_gets_them_and_its_answer_comes_back() {
        static HEARD: Tally = Tally::new(0);
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: kind_open,
            fs: std::ptr::null(),
            describe: Some(kind_describe),
            on_event: Some(kind_describe_as_event),
        };
        assert_eq!(
            register_kind_for(
                "kind.listens",
                KIND_DOCUMENT.as_bytes(),
                &table,
                &HEARD as *const Tally as *mut c_void,
            ),
            IC_OK
        );
        assert!(connection_takes_events("kind.listens"));

        let answer = connection_event(
            "kind.listens",
            &serde_json::json!({ "type": "activate", "node": "go" }),
        )
        .expect("the plugin answered");
        assert_eq!(answer["set"]["data.touched"], serde_json::json!(true));
        assert_eq!(so_far(&HEARD), 1, "once, for the one event");
    }

    #[test]
    fn a_plugin_is_told_the_kind_id_it_registered_however_the_frontend_spelled_it() {
        static HEARD: Tally = Tally::new(0);
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: kind_open,
            fs: std::ptr::null(),
            describe: Some(kind_describe),
            on_event: Some(kind_echoes_the_view_it_was_told),
        };
        assert_eq!(
            register_kind_for(
                "Kind.Spelled",
                KIND_DOCUMENT.as_bytes(),
                &table,
                &HEARD as *const Tally as *mut c_void,
            ),
            IC_OK
        );

        for spelling in ["kind.spelled", "KIND.SPELLED", "Kind.Spelled"] {
            let answer = connection_event(
                spelling,
                &serde_json::json!({ "type": "opened", "view": spelling }),
            )
            .expect("the plugin answered");
            assert_eq!(
                answer["set"]["data.view"],
                serde_json::json!("Kind.Spelled"),
                "a frontend calling the kind {spelling} must not change what the plugin hears"
            );
        }
    }

    #[test]
    fn a_kind_that_takes_no_events_says_so_rather_than_pretending() {
        assert_eq!(
            register_kind("kind.deaf", KIND_DOCUMENT.as_bytes(), std::ptr::null()),
            IC_OK
        );
        assert!(!connection_takes_events("kind.deaf"));
        assert!(connection_event("kind.deaf", &serde_json::json!({})).is_none());
        assert!(!connection_takes_events("kind.no.such.thing"));
    }

    #[test]
    fn the_first_column_becomes_the_name_and_the_rest_become_extra_columns() {
        let (specs, entries, key_column) = fake_source().snapshot();
        assert_eq!(key_column, 1);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].title, "PID");
        assert_eq!(specs[1].width, Some(90));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "init");
        assert_eq!(entries[0].extra, vec!["1".to_string(), "2 MB".to_string()]);
        assert_eq!(entries[1].name, "bash");
    }

    #[test]
    fn a_plugin_panel_is_read_only_and_names_itself() {
        use fm_core::rpc::FileSystemRpc;
        let rpc = PluginPanelRpc::new(fake_source());
        assert!(rpc.is_read_only());
        assert_eq!(rpc.display_name().as_deref(), Some("Fake"));
        assert_eq!(rpc.extra_columns().len(), 2);
    }

    #[test]
    fn a_source_must_have_an_id() {
        let empty = CString::new("").unwrap();
        let ok = CString::new("good").unwrap();
        assert_ne!(
            host_register_panel_source(
                empty.as_ptr(),
                ok.as_ptr(),
                ok.as_ptr(),
                fake_rows,
                std::ptr::null_mut()
            ),
            IC_OK
        );
    }

    #[test]
    fn opening_a_source_nobody_registered_is_refused() {
        let missing = CString::new("nope").unwrap();
        assert_ne!(host_open_panel_source(missing.as_ptr()), IC_OK);
    }
}

#[cfg(test)]
mod settings_tests {
    use std::ffi::CString;

    /// One configuration for the whole module: the host holds it globally, and
    /// these tests run in parallel threads, so a per-test one would be swapped
    /// out from under its neighbour. Each test uses its own plugin id instead.
    fn shared() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            super::set_settings_config(client_config::AppConfig::new(
                "ice-commander-plugin-settings",
            ));
        });
    }

    fn write(plugin: &str, key: &str, value: &str, flags: u32) -> std::os::raw::c_int {
        let host = super::host_table();
        let (id, key) = (CString::new(plugin).unwrap(), CString::new(key).unwrap());
        (host.settings_write)(
            id.as_ptr(),
            key.as_ptr(),
            value.as_ptr(),
            value.len() as u64,
            flags,
        )
    }

    fn forget(plugin: &str, key: &str) -> std::os::raw::c_int {
        let host = super::host_table();
        let (id, key) = (CString::new(plugin).unwrap(), CString::new(key).unwrap());
        (host.settings_write)(id.as_ptr(), key.as_ptr(), std::ptr::null(), 0, 0)
    }

    fn read(plugin: &str, key: &str) -> Option<String> {
        let host = super::host_table();
        let (id, key) = (CString::new(plugin).unwrap(), CString::new(key).unwrap());
        let got = (host.settings_read)(id.as_ptr(), key.as_ptr());
        if got.data.is_null() || got.len == 0 {
            return None;
        }
        let raw = unsafe { std::slice::from_raw_parts(got.data, got.len as usize) };
        Some(String::from_utf8_lossy(raw).to_string())
    }

    #[test]
    fn a_plugin_keeps_its_own_settings_across_a_restart() {
        shared();
        assert_eq!(
            write(
                "keeps",
                "endpoint",
                "https://example.org",
                ic_plugin_api::IC_SETTING_PLAIN
            ),
            super::IC_OK
        );
        assert_eq!(
            read("keeps", "endpoint").as_deref(),
            Some("https://example.org")
        );

        // what a second process would see: the same file, read again
        let again = client_config::AppConfig::new("ice-commander-plugin-settings");
        assert_eq!(
            again.get::<String>("plugins.keeps.endpoint").as_deref(),
            Some("https://example.org")
        );
    }

    #[test]
    fn a_secret_is_not_stored_as_it_was_given() {
        shared();
        assert_eq!(
            write(
                "sealed",
                "token",
                "t-0007",
                ic_plugin_api::IC_SETTING_SECRET
            ),
            super::IC_OK
        );
        let config = client_config::AppConfig::new("ice-commander-plugin-settings");
        let raw = config
            .get::<String>("plugins.sealed.token")
            .expect("stored");
        assert_ne!(raw, "t-0007", "a secret must not reach the file as typed");
        assert!(secret_store::is_encrypted(&raw));
        assert_eq!(read("sealed", "token").as_deref(), Some("t-0007"));
    }

    #[test]
    fn one_plugin_cannot_read_another_under_the_same_key() {
        shared();
        write("alpha", "shared", "mine", ic_plugin_api::IC_SETTING_PLAIN);
        write("omega", "shared", "theirs", ic_plugin_api::IC_SETTING_PLAIN);
        assert_eq!(read("alpha", "shared").as_deref(), Some("mine"));
        assert_eq!(read("omega", "shared").as_deref(), Some("theirs"));
    }

    #[test]
    fn a_null_value_forgets_the_key() {
        shared();
        write("forgets", "gone", "here", ic_plugin_api::IC_SETTING_PLAIN);
        assert!(read("forgets", "gone").is_some());
        assert_eq!(forget("forgets", "gone"), super::IC_OK);
        assert_eq!(read("forgets", "gone"), None);
    }

    #[test]
    fn a_plugin_can_never_address_a_key_outside_its_own_subtree() {
        // The id is one segment, so no spelling reaches ui.* or another plugin.
        assert_eq!(
            super::settings_key("ui", "language").as_deref(),
            Some("plugins.ui.language")
        );
        assert_eq!(super::settings_key("alpha.beta", "x"), None);
        assert_eq!(super::settings_key("", "x"), None);
        assert_eq!(super::settings_key("alpha", " "), None);

        shared();
        assert_ne!(write("alpha.beta", "x", "y", 0), super::IC_OK);
        assert_ne!(write("", "x", "y", 0), super::IC_OK);
        let config = client_config::AppConfig::new("ice-commander-plugin-settings");
        assert_eq!(
            config.get::<String>("ui.language"),
            None,
            "nothing a plugin wrote landed outside the plugins subtree"
        );
    }
}

#[cfg(test)]
mod panel_tree_tests {
    use std::ffi::CString;

    /// A panel of two devices; one is online and holds two resources. Exactly
    /// the shape node-in-net needs, with nothing behind it.
    extern "C" fn walk(
        path: *const std::os::raw::c_char,
        _user_data: *mut std::os::raw::c_void,
    ) -> ic_plugin_api::IcTree {
        thread_local! {
            static HELD: std::cell::RefCell<(Vec<CString>, Vec<*const std::os::raw::c_char>,
                                             Vec<ic_plugin_api::IcColumn>, Vec<std::os::raw::c_int>)> =
                std::cell::RefCell::new((Vec::new(), Vec::new(), Vec::new(), Vec::new()));
        }
        let asked = super::cstr(path);
        let rows: Vec<(&str, &str, bool)> = match asked.as_str() {
            "" => vec![("laniakea", "online", true), ("fozzy", "offline", false)],
            "laniakea" => vec![("documents", "shared", true), ("music", "shared", true)],
            "laniakea/documents" => vec![("notes.txt", "12 KB", false)],
            _ => Vec::new(),
        };
        HELD.with(|slot| {
            let mut held = slot.borrow_mut();
            held.0 = vec![
                CString::new("name").unwrap(),
                CString::new("state").unwrap(),
            ];
            held.2 = vec![
                ic_plugin_api::IcColumn {
                    key: held.0[0].as_ptr(),
                    title: held.0[0].as_ptr(),
                    width: 0,
                },
                ic_plugin_api::IcColumn {
                    key: held.0[1].as_ptr(),
                    title: held.0[1].as_ptr(),
                    width: 0,
                },
            ];
            let mut cells = Vec::new();
            let mut opens = Vec::new();
            let mut owned = Vec::new();
            for (name, state, enterable) in &rows {
                owned.push(CString::new(*name).unwrap());
                owned.push(CString::new(*state).unwrap());
                opens.push(std::os::raw::c_int::from(*enterable));
            }
            for held_text in &owned {
                cells.push(held_text.as_ptr());
            }
            held.0.extend(owned);
            held.1 = cells;
            held.3 = opens;
            ic_plugin_api::IcTree {
                magic: ic_plugin_api::IC_TREE_MAGIC,
                struct_size: std::mem::size_of::<ic_plugin_api::IcTree>() as u32,
                column_count: 2,
                columns: held.2.as_ptr(),
                cells: held.1.as_ptr(),
                row_count: rows.len() as u32,
                key_column: 0,
                enterable: held.3.as_ptr(),
            }
        })
    }

    fn registered() -> super::PanelSource {
        let host = super::host_table();
        let (id, title, svg) = (
            CString::new("probe.tree").unwrap(),
            CString::new("Probe").unwrap(),
            CString::new("<svg/>").unwrap(),
        );
        assert_eq!(
            (host.register_panel_tree)(
                id.as_ptr(),
                title.as_ptr(),
                svg.as_ptr(),
                walk,
                std::ptr::null_mut()
            ),
            super::IC_OK
        );
        super::panel_source("probe.tree").expect("registered")
    }

    #[test]
    fn a_tree_answers_a_different_list_at_each_level() {
        let source = registered();
        assert!(source.is_a_tree());

        let (columns, rows, key) = source.snapshot_at("");
        assert_eq!(key, 0);
        assert_eq!(
            columns.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
            vec!["state"],
            "the first column is the name; the rest are extra"
        );
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["laniakea", "fozzy"]
        );
        assert_eq!(rows[0].extra, vec!["online".to_string()]);

        let (_, inside, _) = source.snapshot_at("laniakea");
        assert_eq!(
            inside.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["documents", "music"]
        );

        let (_, deeper, _) = source.snapshot_at("laniakea/documents");
        assert_eq!(deeper.len(), 1);
        assert_eq!(deeper[0].name, "notes.txt");
    }

    #[test]
    fn only_the_rows_the_plugin_marked_can_be_walked_into() {
        let source = registered();
        let (_, rows, _) = source.snapshot_at("");
        assert!(rows[0].is_dir, "an online device opens");
        assert!(!rows[1].is_dir, "an offline one does not");

        let (_, files, _) = source.snapshot_at("laniakea/documents");
        assert!(!files[0].is_dir, "a file is not a folder");
    }

    #[test]
    fn a_level_the_plugin_knows_nothing_about_is_empty_rather_than_an_error() {
        let source = registered();
        let (_, rows, _) = source.snapshot_at("nowhere/at/all");
        assert!(rows.is_empty());
    }

    #[test]
    fn a_table_without_the_stamp_is_refused() {
        extern "C" fn unstamped(
            _path: *const std::os::raw::c_char,
            _user: *mut std::os::raw::c_void,
        ) -> ic_plugin_api::IcTree {
            let mut table = ic_plugin_api::IcTree::EMPTY;
            table.magic = 0;
            table
        }
        let host = super::host_table();
        let (id, title, svg) = (
            CString::new("probe.unstamped").unwrap(),
            CString::new("Probe").unwrap(),
            CString::new("<svg/>").unwrap(),
        );
        (host.register_panel_tree)(
            id.as_ptr(),
            title.as_ptr(),
            svg.as_ptr(),
            unstamped,
            std::ptr::null_mut(),
        );
        let source = super::panel_source("probe.unstamped").expect("registered");
        assert!(source.snapshot_at("").1.is_empty());
    }

    #[test]
    fn a_flat_source_still_ignores_the_path_as_it_always_did() {
        extern "C" fn flat(_user: *mut std::os::raw::c_void) -> ic_plugin_api::IcTable {
            ic_plugin_api::IcTable::EMPTY
        }
        let host = super::host_table();
        let (id, title, svg) = (
            CString::new("probe.flat").unwrap(),
            CString::new("Probe").unwrap(),
            CString::new("<svg/>").unwrap(),
        );
        (host.register_panel_source)(
            id.as_ptr(),
            title.as_ptr(),
            svg.as_ptr(),
            flat,
            std::ptr::null_mut(),
        );
        let source = super::panel_source("probe.flat").expect("registered");
        assert!(!source.is_a_tree());
        assert_eq!(source.snapshot_at("anything").1.len(), 0);
    }
}
