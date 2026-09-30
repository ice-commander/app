use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

pub const IC_HOST_MAGIC: u64 = 0x4943_5f48_4f53_5400;
pub const IC_ABI_VERSION: u32 = 3;

pub const IC_OK: c_int = 0;
pub const IC_ERR_HOST_TOO_OLD: c_int = 1;
pub const IC_ERR_HOST_UNKNOWN: c_int = 2;
pub const IC_ERR_INIT_FAILED: c_int = 3;
/// A filesystem call that could not be carried out. The host only ever asks
/// whether the answer was `IC_OK`, and reads `last_error` for the reason; this
/// exists so a plugin need not answer a failed write with an init error.
pub const IC_ERR_IO: c_int = 4;
/// `init` answering "not in this application". Not a failure: a viewer of pictures in a
/// terminal has nothing to draw.
pub const IC_ERR_NOT_THIS_HOST: c_int = 5;

pub type IcClickFn = extern "C" fn(user_data: *mut c_void, parent_window: *mut c_void);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcSelectionItem {
    pub path: *const c_char,
    pub key: *const c_char,
    pub is_dir: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcSelection {
    pub items: *const IcSelectionItem,
    pub count: u32,
}

impl IcSelection {
    pub const EMPTY: IcSelection = IcSelection {
        items: std::ptr::null(),
        count: 0,
    };

    pub fn as_slice(&self) -> &[IcSelectionItem] {
        if self.items.is_null() || self.count == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.items, self.count as usize) }
    }
}

impl IcSelectionItem {
    pub fn path_string(&self) -> Option<String> {
        if self.path.is_null() {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(self.path) }
                .to_string_lossy()
                .to_string(),
        )
    }

    pub fn key_string(&self) -> Option<String> {
        if self.key.is_null() {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(self.key) }
                .to_string_lossy()
                .to_string(),
        )
    }

    pub fn is_directory(&self) -> bool {
        self.is_dir != 0
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcColumn {
    pub key: *const c_char,
    pub title: *const c_char,
    pub width: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcTable {
    pub columns: *const IcColumn,
    pub column_count: u32,
    pub cells: *const *const c_char,
    pub row_count: u32,
    pub key_column: u32,
}

impl IcTable {
    pub const EMPTY: IcTable = IcTable {
        columns: std::ptr::null(),
        column_count: 0,
        cells: std::ptr::null(),
        row_count: 0,
        key_column: 0,
    };

    pub fn columns_slice(&self) -> &[IcColumn] {
        if self.columns.is_null() || self.column_count == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.columns, self.column_count as usize) }
    }

    pub fn cell(&self, row: u32, column: u32) -> Option<String> {
        if self.cells.is_null() || row >= self.row_count || column >= self.column_count {
            return None;
        }
        let index = row as usize * self.column_count as usize + column as usize;
        let ptr = unsafe { *self.cells.add(index) };
        if ptr.is_null() {
            return None;
        }
        Some(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string())
    }
}

impl IcColumn {
    pub fn key_string(&self) -> String {
        if self.key.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(self.key) }
            .to_string_lossy()
            .to_string()
    }

    pub fn title_string(&self) -> String {
        if self.title.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(self.title) }
            .to_string_lossy()
            .to_string()
    }
}

pub type IcTableFn = extern "C" fn(user_data: *mut c_void) -> IcTable;

pub const IC_TREE_MAGIC: u64 = 0x4943_5f54_5245_4500;

/// Rows of a panel you can walk into. Same shape as `IcTable` plus a flag per
/// row saying whether it opens, and a `struct_size` so it can grow: `IcTable`
/// has no header and so is frozen.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcTree {
    pub magic: u64,
    pub struct_size: u32,
    pub column_count: u32,
    pub columns: *const IcColumn,
    pub cells: *const *const c_char,
    pub row_count: u32,
    pub key_column: u32,
    /// One entry per row, non-zero where the row opens. Null means none do.
    pub enterable: *const c_int,
}

impl IcTree {
    pub const EMPTY: IcTree = IcTree {
        magic: IC_TREE_MAGIC,
        struct_size: std::mem::size_of::<IcTree>() as u32,
        column_count: 0,
        columns: std::ptr::null(),
        cells: std::ptr::null(),
        row_count: 0,
        key_column: 0,
        enterable: std::ptr::null(),
    };

    pub fn is_sound(&self) -> bool {
        self.magic == IC_TREE_MAGIC
            && self.struct_size as usize
                >= std::mem::size_of::<IcTree>() - std::mem::size_of::<*const c_int>()
    }

    pub fn columns_slice(&self) -> &[IcColumn] {
        if self.columns.is_null() || self.column_count == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.columns, self.column_count as usize) }
    }

    pub fn cell(&self, row: u32, column: u32) -> Option<String> {
        if self.cells.is_null() || row >= self.row_count || column >= self.column_count {
            return None;
        }
        let at = (row * self.column_count + column) as usize;
        let pointer = unsafe { *self.cells.add(at) };
        if pointer.is_null() {
            return Some(String::new());
        }
        Some(
            unsafe { CStr::from_ptr(pointer) }
                .to_string_lossy()
                .to_string(),
        )
    }

    /// Whether this row opens. Absent from an older plugin's struct, and then
    /// nothing opens — which is what a flat table means.
    pub fn opens(&self, row: u32) -> bool {
        let known = std::mem::size_of::<IcTree>();
        if (self.struct_size as usize) < known || self.enterable.is_null() || row >= self.row_count
        {
            return false;
        }
        unsafe { *self.enterable.add(row as usize) != 0 }
    }
}

/// `path` is empty at the root, and otherwise the row keys joined by `/`.
pub const IC_DRIVES_MAGIC: u64 = 0x4943_5f44_5249_5645;

/// One entry in the drives list, in the shape the application already shows:
/// a name, a line under it, a key and whether it is reachable now.
#[repr(C)]
pub struct IcDrive {
    /// What favourites and the active-drive highlight hang off, so it has to be
    /// the same string across restarts.
    pub key: *const c_char,
    pub name: *const c_char,
    pub subtitle: *const c_char,
    /// Handed to the connection kind's `open` when the user picks this entry.
    pub settings: *const c_char,
    /// This row's own icon. Null or empty falls back to the one the source was
    /// registered with, so a plugin whose entries all look alike says it once.
    pub svg: *const c_char,
    pub online: c_int,
}

#[repr(C)]
pub struct IcDrives {
    pub magic: u64,
    pub struct_size: u32,
    pub count: u32,
    pub rows: *const IcDrive,
}

impl IcDrives {
    pub const EMPTY: IcDrives = IcDrives {
        magic: IC_DRIVES_MAGIC,
        struct_size: std::mem::size_of::<IcDrives>() as u32,
        count: 0,
        rows: std::ptr::null(),
    };
}

/// Asked every time the list is drawn, on the frontend's own thread, so it must
/// answer from something the plugin already holds rather than go to a network.
pub type IcDrivesFn = extern "C" fn(user_data: *mut c_void) -> IcDrives;

pub const IC_PINNED_MAGIC: u64 = 0x4943_5f50_494e_4e00;

/// An entry the plugin keeps in the connections list; the user can neither rename nor delete it.
#[repr(C)]
pub struct IcPinnedConnection {
    pub id: *const c_char,
    pub title: *const c_char,
    /// Empty leaves the application to draw its own.
    pub svg: *const c_char,
    /// A view from `register_view`; its `describe` is handed this entry's `id`.
    pub view: *const c_char,
}

#[repr(C)]
pub struct IcPinnedConnections {
    pub magic: u64,
    pub struct_size: u32,
    pub count: u32,
    pub rows: *const IcPinnedConnection,
}

impl IcPinnedConnections {
    pub const EMPTY: IcPinnedConnections = IcPinnedConnections {
        magic: IC_PINNED_MAGIC,
        struct_size: std::mem::size_of::<IcPinnedConnections>() as u32,
        count: 0,
        rows: std::ptr::null(),
    };
}

/// Asked every time the connections list is drawn, on the frontend's thread.
pub type IcPinnedConnectionsFn = extern "C" fn(user_data: *mut c_void) -> IcPinnedConnections;

pub type IcTreeFn = extern "C" fn(path: *const c_char, user_data: *mut c_void) -> IcTree;

pub type IcFsHandle = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcDirEntry {
    pub name: *const c_char,
    pub is_dir: c_int,
    pub size: u64,
    pub modified: u64,
    pub permissions: u32,
    pub has_permissions: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcListing {
    pub items: *const IcDirEntry,
    pub count: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcBytes {
    pub data: *const u8,
    pub len: u64,
}

impl IcListing {
    pub const EMPTY: IcListing = IcListing {
        items: std::ptr::null(),
        count: 0,
    };

    pub fn as_slice(&self) -> &[IcDirEntry] {
        if self.items.is_null() || self.count == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.items, self.count as usize) }
    }
}

impl IcBytes {
    pub const EMPTY: IcBytes = IcBytes {
        data: std::ptr::null(),
        len: 0,
    };

    pub fn as_slice(&self) -> &[u8] {
        if self.data.is_null() || self.len == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.data, self.len as usize) }
    }
}

impl IcDirEntry {
    pub fn name_string(&self) -> String {
        if self.name.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(self.name) }
            .to_string_lossy()
            .to_string()
    }

    pub fn is_directory(&self) -> bool {
        self.is_dir != 0
    }

    pub fn permissions_opt(&self) -> Option<u32> {
        if self.has_permissions != 0 {
            Some(self.permissions)
        } else {
            None
        }
    }
}

pub const IC_COLUMNS_MAGIC: u64 = 0x4943_5f43_4f4c_5300;
pub const IC_ROWS_MAGIC: u64 = 0x4943_5f52_4f57_5300;

/// What a mount's own column shows. Text is drawn and left alone; a tick is
/// drawn as a box the user can click, and the click goes back to the plugin.
pub const IC_COLUMN_TEXT: u32 = 0;
pub const IC_COLUMN_CHECK: u32 = 1;

/// A column of a mount's own.
///
/// `IcColumn` is what a panel source declares and is frozen — it sits in
/// `IcTable`, which has no header to grow by — so a filesystem's columns, which
/// need to say more, are their own shape.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcFsColumn {
    pub key: *const c_char,
    pub title: *const c_char,
    pub width: i32,
    /// `IC_COLUMN_TEXT` or `IC_COLUMN_CHECK`.
    pub kind: u32,
}

/// The columns a mount adds to the ones the panel draws by itself. They are
/// added, never a replacement: name, size and date stay where they were.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcColumns {
    pub magic: u64,
    pub struct_size: u32,
    pub count: u32,
    pub items: *const IcFsColumn,
}

impl IcFsColumn {
    pub fn key_string(&self) -> String {
        text_at(self.key)
    }

    pub fn title_string(&self) -> String {
        text_at(self.title)
    }

    pub fn is_check(&self) -> bool {
        self.kind == IC_COLUMN_CHECK
    }
}

fn text_at(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string()
}

/// What a tick in a cell reads as. Anything that is not exactly this is not
/// ticked, so a plugin that answers text into a tick column reads as unticked
/// rather than as something undefined.
pub const IC_CELL_TICKED: &str = "1";

pub fn cell_is_ticked(cell: &str) -> bool {
    cell == IC_CELL_TICKED
}

/// A directory entry plus the cells for those columns.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcRow {
    pub entry: IcDirEntry,
    /// One string per column the mount declared, in that order. A null array,
    /// or a null cell, reads as empty.
    pub extra: *const *const c_char,
    /// How many strings `extra` really holds. A mount that declares more
    /// columns than it fills leaves the rest empty rather than being read past.
    pub extra_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcRows {
    pub magic: u64,
    pub struct_size: u32,
    pub count: u32,
    pub items: *const IcRow,
}

impl IcColumns {
    pub const EMPTY: IcColumns = IcColumns {
        magic: IC_COLUMNS_MAGIC,
        struct_size: std::mem::size_of::<IcColumns>() as u32,
        count: 0,
        items: std::ptr::null(),
    };

    pub fn is_sound(&self) -> bool {
        self.magic == IC_COLUMNS_MAGIC
            && self.struct_size as usize >= std::mem::size_of::<IcColumns>()
    }

    pub fn as_slice(&self) -> &[IcFsColumn] {
        if !self.is_sound() || self.items.is_null() || self.count == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.items, self.count as usize) }
    }
}

impl IcRows {
    pub const EMPTY: IcRows = IcRows {
        magic: IC_ROWS_MAGIC,
        struct_size: std::mem::size_of::<IcRows>() as u32,
        count: 0,
        items: std::ptr::null(),
    };

    pub fn is_sound(&self) -> bool {
        self.magic == IC_ROWS_MAGIC && self.struct_size as usize >= std::mem::size_of::<IcRows>()
    }

    pub fn as_slice(&self) -> &[IcRow] {
        if !self.is_sound() || self.items.is_null() || self.count == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.items, self.count as usize) }
    }
}

impl IcRow {
    /// The cell for column `index`, empty where the row does not reach that
    /// far. The row's own count is what bounds the read, never the number of
    /// columns the mount declared.
    pub fn extra_at(&self, index: usize) -> String {
        if self.extra.is_null() || index >= self.extra_count as usize {
            return String::new();
        }
        let ptr = unsafe { *self.extra.add(index) };
        if ptr.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string()
    }
}

/// A button that belongs to a mount is told which mount it was pressed in:
/// two panels can stand in two different torrents at once, so the plugin
/// cannot work it out from what it opened last. Null where the panel has left
/// the mount between the press and the call.
pub type IcFsClickFn =
    extern "C" fn(mount: IcFsHandle, user_data: *mut c_void, parent: *mut c_void);

/// Says a tick in one of this mount's own columns was clicked.
///
/// `dir` is the listing it happened in and `name` the row, spelled the way
/// `list` answered them. The plugin decides what that means and what the cell
/// reads as afterwards; the host lists again rather than assuming.
pub type IcFsCellClickedFn = extern "C" fn(
    handle: IcFsHandle,
    dir: *const c_char,
    name: *const c_char,
    column_key: *const c_char,
    ticked: c_int,
) -> c_int;

/// What one of this mount's toolbar buttons looks like now: a mask of
/// `IC_ACTION_SHOWN`, `IC_ACTION_ENABLED` and `IC_ACTION_ON`.
///
/// Asked on the frontend's own thread every time the toolbar is refreshed, so
/// it must answer from what the mount already holds rather than go anywhere.
pub type IcFsActionStateFn = extern "C" fn(handle: IcFsHandle, action_id: *const c_char) -> u32;

pub type IcFsColumnsFn = extern "C" fn(handle: IcFsHandle) -> IcColumns;
pub type IcFsRowsFn = extern "C" fn(handle: IcFsHandle, path: *const c_char) -> IcRows;

pub type IcFsCloseFn = extern "C" fn(handle: IcFsHandle);
pub type IcFsListFn = extern "C" fn(handle: IcFsHandle, path: *const c_char) -> IcListing;
pub type IcFsReadFn = extern "C" fn(handle: IcFsHandle, path: *const c_char) -> IcBytes;
pub type IcFsFlagFn = extern "C" fn(handle: IcFsHandle) -> c_int;
pub type IcFsErrorFn = extern "C" fn(handle: IcFsHandle) -> *const c_char;
pub type IcFsWriteFn =
    extern "C" fn(handle: IcFsHandle, path: *const c_char, bytes: *const u8, len: u64) -> c_int;
pub type IcFsPathFn = extern "C" fn(handle: IcFsHandle, path: *const c_char) -> c_int;
pub type IcFsRenameFn =
    extern "C" fn(handle: IcFsHandle, from: *const c_char, to: *const c_char) -> c_int;

pub type IcShellHandle = *mut c_void;

// One host thread drives all five in turn. `shell_read` returns promptly: empty slice when nothing is ready, null `data` once the shell has ended.
pub type IcShellOpenFn =
    extern "C" fn(handle: IcFsHandle, cwd: *const c_char, rows: u32, cols: u32) -> IcShellHandle;
pub type IcShellReadFn = extern "C" fn(shell: IcShellHandle) -> IcBytes;
pub type IcShellWriteFn = extern "C" fn(shell: IcShellHandle, bytes: *const u8, len: u64) -> c_int;
pub type IcShellResizeFn = extern "C" fn(shell: IcShellHandle, rows: u32, cols: u32) -> c_int;
pub type IcShellCloseFn = extern "C" fn(shell: IcShellHandle);

/// Whether *this* mount offers a shell. Absent means the answer is the same for
/// every mount of the plugin: it has one if `shell_open` is filled in.
pub type IcShellAvailableFn = extern "C" fn(handle: IcFsHandle) -> c_int;

/// The filesystem a file lives on, as a plugin sees it. Opaque: it belongs to
/// the host, which alone knows whether the bytes behind it are on a disk, on a
/// server or inside another archive.
pub type IcFsSource = *mut c_void;

/// An open read or write. The host holds whatever is underneath — a file
/// descriptor, a connection, a decompressor — for as long as it is open.
pub type IcStream = *mut c_void;

pub type IcFsOpenInFn =
    extern "C" fn(source: IcFsSource, path: *const c_char, user_data: *mut c_void) -> IcFsHandle;

/// How a stream is to be used. A plugin asks for no more than it needs: a
/// source that cannot write refuses `IC_OPEN_WRITE` rather than pretending.
pub const IC_OPEN_READ: u32 = 0;
/// Truncates what is there, or makes the file if it is not.
pub const IC_OPEN_WRITE: u32 = 1;
/// Reads and writes without truncating.
pub const IC_OPEN_UPDATE: u32 = 2;

pub const IC_SEEK_SET: u32 = 0;
pub const IC_SEEK_CURRENT: u32 = 1;
pub const IC_SEEK_END: u32 = 2;

/// What a source can do cheaply. These say what a thing COSTS, not what it
/// permits: a seek on a source without `IC_FS_SEEK` still works, and is paid
/// for by the host fetching what it must. A plugin reads them to choose an
/// algorithm — forwards through a tar, or jumping about a zip — not to decide
/// whether it is allowed.
pub const IC_FS_SEEK: u32 = 1 << 0;
/// Reading forwards is cheap; going back is not.
pub const IC_FS_SEQUENTIAL: u32 = 1 << 1;
pub const IC_FS_WRITE: u32 = 1 << 2;
pub const IC_FS_LIST: u32 = 1 << 3;
/// `fs_local_path` can answer with a real path on this machine.
pub const IC_FS_LOCAL_PATH: u32 = 1 << 4;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcFsVTable {
    pub struct_size: u32,
    /// Opened inside a file of another filesystem: an archive, a torrent.
    ///
    /// What is handed over is the filesystem that file lives on and the path
    /// it lives at — never its bytes. The plugin reads what it needs through
    /// `fs_open` and the calls beside it, which is how an archive on a server
    /// opens without the server sending the whole of it.
    pub open_in: IcFsOpenInFn,
    pub close: extern "C" fn(handle: IcFsHandle),
    pub list: extern "C" fn(handle: IcFsHandle, path: *const c_char) -> IcListing,
    pub read: extern "C" fn(handle: IcFsHandle, path: *const c_char) -> IcBytes,
    pub is_read_only: extern "C" fn(handle: IcFsHandle) -> c_int,
    pub last_error: extern "C" fn(handle: IcFsHandle) -> *const c_char,
    pub write: Option<IcFsWriteFn>,
    pub create_dir: Option<IcFsPathFn>,
    pub remove: Option<IcFsPathFn>,
    pub rename: Option<IcFsRenameFn>,
    pub shell_open: Option<IcShellOpenFn>,
    pub shell_read: Option<IcShellReadFn>,
    pub shell_write: Option<IcShellWriteFn>,
    pub shell_resize: Option<IcShellResizeFn>,
    pub shell_close: Option<IcShellCloseFn>,
    pub shell_available: Option<IcShellAvailableFn>,
    /// The columns this mount adds to the panel's own. Asked once per
    /// listing, before the rows.
    pub columns: Option<IcFsColumnsFn>,
    /// The same listing as `list`, carrying the cells for those columns.
    /// The host prefers it wherever both are filled in.
    pub list_rows: Option<IcFsRowsFn>,
    /// Whether each of this mount's toolbar buttons is there, usable and
    /// pressed. A mount that leaves this out gets `IC_ACTION_DEFAULT` for all
    /// of them.
    pub action_state: Option<IcFsActionStateFn>,
    /// A tick in one of this mount's own columns was clicked. A mount with no
    /// tick columns never hears from this.
    pub cell_clicked: Option<IcFsCellClickedFn>,
}

pub type IcViewDescribeFn =
    extern "C" fn(ctx: *const u8, ctx_len: u64, user_data: *mut c_void) -> IcBytes;
pub type IcViewEventFn =
    extern "C" fn(event: *const u8, event_len: u64, user_data: *mut c_void) -> IcBytes;
pub type IcViewClosedFn = extern "C" fn(instance: u64, user_data: *mut c_void);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcViewVTable {
    pub struct_size: u32,
    pub describe: IcViewDescribeFn,
    pub on_event: Option<IcViewEventFn>,
    pub closed: Option<IcViewClosedFn>,
}

/// A viewer opened on one file: which window it is, the filesystem the file
/// lives on, and where on it. Never the bytes — the plugin reads what it needs
/// through `fs_open` and the calls beside it, which is how a picture inside an
/// archive on a server is shown without fetching the archive.
pub type IcViewerOpenFn = extern "C" fn(
    instance: u64,
    source: IcFsSource,
    path: *const c_char,
    user_data: *mut c_void,
) -> c_int;

/// Pixels the plugin made rather than read: a page of a document, the picture
/// hidden inside a camera file. The host asks for them when it draws, and
/// keeps only as many as it wants — so a plugin holding a thousand-page
/// document holds no pages.
///
/// `name` is what the document called it after `part:`. The answer is the
/// plugin's to keep alive until it is asked again.
pub type IcViewerContentFn =
    extern "C" fn(instance: u64, name: *const c_char, user_data: *mut c_void) -> IcBytes;

/// What a plugin shows when the user asks to look at a file.
///
/// It is a view with a file behind it: the same `describe`, `on_event` and
/// `closed` as any window the plugin puts up, and the document it answers with
/// is drawn by whichever frontend is running. The context those are called
/// with carries `"instance"`, so a plugin showing two files at once knows
/// which of them is being asked about. A picture is a node naming where
/// the picture is, not a buffer: `file:<path>` for what is on the
/// filesystem the viewer was opened on, `part:<name>` for what the plugin made.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcViewerVTable {
    pub struct_size: u32,
    /// How the window describes itself and answers what is done in it.
    pub view: *const IcViewVTable,
    pub open: IcViewerOpenFn,
    /// The window was closed; the source it was opened on is not to be used
    /// again. Absent for a plugin that keeps nothing.
    pub closed: Option<IcViewClosedFn>,
    /// Answers `part:` references. Absent for a plugin that only ever names
    /// files it did not make.
    pub content: Option<IcViewerContentFn>,
    /// The window is about to close, and the plugin may say no.
    ///
    /// `IC_OK` lets it close; anything else keeps it open, and the host then
    /// asks the window to describe itself again — so a plugin that refuses can
    /// put up what it wanted to say ("this is not saved") in the same window
    /// rather than having to open another. `closed` still follows whenever the
    /// window really does go.
    ///
    /// Absent for a plugin with nothing to lose, which is most of them.
    pub closing: Option<IcViewerClosingFn>,
}

/// Asked before a viewer's window closes. `IC_OK` lets it go; anything else
/// keeps it open.
pub type IcViewerClosingFn = extern "C" fn(instance: u64, user_data: *mut c_void) -> c_int;

pub type IcConnectionOpenFn =
    extern "C" fn(settings: *const u8, settings_len: u64, user_data: *mut c_void) -> IcFsHandle;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcConnectionVTable {
    pub struct_size: u32,
    pub open: IcConnectionOpenFn,
    pub fs: *const IcFsVTable,
    /// Answers with the form to put in front of the user, in place of the
    /// document given at registration. A kind that offers this is asked every
    /// time the form opens, so it can show what it knows now rather than what
    /// it knew at startup.
    ///
    /// Called on whichever thread opened the form, which is not always the
    /// frontend's: from here only `log_warn` is safe to call back.
    pub describe: Option<IcViewDescribeFn>,
    /// What the form's buttons and fields reach. Without it a connection form
    /// is one-way — the user types and the host commits — and nothing the
    /// plugin puts on the form can act. The answer is the same shape a view
    /// answers with: `put`, `set`, `clipboard`, `redescribe`.
    pub on_event: Option<IcViewEventFn>,
}

pub const IC_ENABLE_ALWAYS: u32 = 0;
pub const IC_ENABLE_ON_FILE: u32 = 1 << 0;
pub const IC_ENABLE_ON_DIR: u32 = 1 << 1;
pub const IC_ENABLE_ON_EMPTY: u32 = 1 << 2;

pub fn enabled_for(flags: u32, files: u32, dirs: u32) -> bool {
    if flags == IC_ENABLE_ALWAYS {
        return true;
    }
    if files > 0 && flags & IC_ENABLE_ON_FILE != 0 {
        return true;
    }
    if dirs > 0 && flags & IC_ENABLE_ON_DIR != 0 {
        return true;
    }
    if files == 0 && dirs == 0 && flags & IC_ENABLE_ON_EMPTY != 0 {
        return true;
    }
    false
}

/// What a button scoped to a filesystem is: one that simply fires, or one that
/// stays pressed. A plugin says which at registration, because the host has to
/// draw the widget before any mount is open to ask.
pub const IC_ACTION_BUTTON: u32 = 0;
pub const IC_ACTION_TOGGLE: u32 = 1;

/// What such a button looks like right now, as answered by `action_state`.
pub const IC_ACTION_SHOWN: u32 = 1 << 0;
pub const IC_ACTION_ENABLED: u32 = 1 << 1;
/// Pressed. Only means anything for `IC_ACTION_TOGGLE`.
pub const IC_ACTION_ON: u32 = 1 << 2;

/// What a mount that answers nothing is taken to mean: there and usable, and
/// not pressed.
pub const IC_ACTION_DEFAULT: u32 = IC_ACTION_SHOWN | IC_ACTION_ENABLED;

pub const IC_SIDE_LEFT: u32 = 0;
pub const IC_SIDE_RIGHT: u32 = 1;

/// The application's side of the boundary: one table of function pointers,
/// built once and handed to every plugin as it starts.
///
/// After the header — the magic, the size and the ABI version — every field is
/// a pointer of the same width, so what a plugin is given is an array of
/// function pointers that happens to have names. The names are what keeps each
/// slot's signature honest and what the growth test holds to its offset; the
/// memory is the flat array either way.
///
/// **There is one table for the whole process and it outlives every plugin.**
/// A plugin may keep the pointer it was handed in `ic_plugin_init` and call
/// through it later, including from a thread of its own — which is how a
/// plugin with work of its own says `drives_changed` or `fs_invalidate` long
/// after init returned. Which slots are safe from which thread is said slot by
/// slot below; the rest are for the thread that called init.
#[repr(C)]
pub struct IcHost {
    pub magic: u64,
    pub struct_size: u32,
    pub abi_version: u32,
    pub host_version: extern "C" fn() -> *const c_char,
    pub log_warn: extern "C" fn(message: *const c_char),
    pub add_toolbar_button: extern "C" fn(
        id: *const c_char,
        svg: *const c_char,
        tooltip: *const c_char,
        side: u32,
        priority: i32,
        enable_flags: u32,
        on_click: IcClickFn,
        user_data: *mut c_void,
    ) -> c_int,
    pub selection: extern "C" fn() -> IcSelection,
    pub register_panel_source: extern "C" fn(
        id: *const c_char,
        title: *const c_char,
        svg: *const c_char,
        rows: IcTableFn,
        user_data: *mut c_void,
    ) -> c_int,
    pub open_panel_source: extern "C" fn(id: *const c_char) -> c_int,
    pub close_panel_source: extern "C" fn() -> c_int,
    pub add_header_button: extern "C" fn(
        id: *const c_char,
        svg: *const c_char,
        label: *const c_char,
        tooltip: *const c_char,
        side: u32,
        priority: i32,
        on_click: IcClickFn,
        user_data: *mut c_void,
    ) -> c_int,
    // Called on a worker thread; from there only `log_warn` is safe to call back.
    pub register_filesystem: extern "C" fn(
        extensions: *const c_char,
        vtable: *const IcFsVTable,
        user_data: *mut c_void,
    ) -> c_int,
    pub register_panel_action: extern "C" fn(
        source_id: *const c_char,
        action_id: *const c_char,
        svg: *const c_char,
        tooltip: *const c_char,
        enable_flags: u32,
        on_click: IcClickFn,
        user_data: *mut c_void,
    ) -> c_int,
    /// An empty `schema` declares a kind that only mounts and is never offered to create.
    pub register_connection_kind: extern "C" fn(
        id: *const c_char,
        schema: *const u8,
        schema_len: u64,
        vtable: *const IcConnectionVTable,
        user_data: *mut c_void,
    ) -> c_int,
    pub register_view: extern "C" fn(
        id: *const c_char,
        title: *const c_char,
        vtable: *const IcViewVTable,
        user_data: *mut c_void,
    ) -> c_int,
    pub open_view: extern "C" fn(id: *const c_char, arg: *const u8, arg_len: u64) -> c_int,
    pub view_invalidate: extern "C" fn(id: *const c_char) -> c_int,
    pub register_locales:
        extern "C" fn(language: *const c_char, catalogue: *const u8, catalogue_len: u64) -> c_int,
    /// The older, unowned form: every plugin shares one table of names, so two
    /// plugins registering the same name overwrite each other. Use
    /// `register_plugin_asset`.
    pub register_asset:
        extern "C" fn(name: *const c_char, bytes: *const u8, bytes_len: u64) -> c_int,
    /// Reads back what this plugin stored. Empty when there is nothing under
    /// that key. The bytes stay valid until the next call on this thread.
    pub settings_read: extern "C" fn(plugin_id: *const c_char, key: *const c_char) -> IcBytes,
    /// Stores a value under the plugin's own name. A null `bytes` forgets the
    /// key. `flags` is `IC_SETTING_PLAIN` or `IC_SETTING_SECRET`.
    ///
    /// The id is the plugin's own, taken from its `IcAbout`. The host uses it
    /// to keep plugins out of each other's keys; it is a tidiness boundary, not
    /// a security one — a plugin shares the process and can reach anything.
    pub settings_write: extern "C" fn(
        plugin_id: *const c_char,
        key: *const c_char,
        bytes: *const u8,
        len: u64,
        flags: u32,
    ) -> c_int,
    /// A panel whose rows can be walked into, for a plugin whose contents are
    /// discovered rather than stored. Otherwise the same as a panel source, and
    /// it appears in the same place.
    pub register_panel_tree: extern "C" fn(
        id: *const c_char,
        title: *const c_char,
        svg: *const c_char,
        rows: IcTreeFn,
        user_data: *mut c_void,
    ) -> c_int,
    /// Replaces the text beside a header button this plugin added. An empty
    /// label leaves the button with nothing but its icon. Safe to call from a
    /// thread of the plugin's own: the host carries it to the frontend.
    pub set_header_label: extern "C" fn(id: *const c_char, label: *const c_char) -> c_int,
    /// Entries to show beside the local disks and the saved connections, in the
    /// drives dropdown and on the start page alike. `kind` names the connection
    /// kind that mounts them, so picking one goes through the same `open` as a
    /// connection the user saved.
    pub register_drive_source: extern "C" fn(
        kind: *const c_char,
        // One picture for every entry; empty leaves the application to draw reachability.
        svg: *const c_char,
        rows: IcDrivesFn,
        user_data: *mut c_void,
    ) -> c_int,
    /// Says the answer to `IcDrivesFn` has changed, so the lists are worth
    /// drawing again. Safe to call from a thread of the plugin's own: the host
    /// carries it to the frontend.
    pub drives_changed: extern "C" fn() -> c_int,
    /// A toolbar button that belongs to a filesystem of this plugin's rather
    /// than to the application. It appears only while the user stands inside a
    /// mount of one of `extensions`, and goes away on the way out.
    ///
    /// `extensions` is written the same way as in `register_filesystem`, and
    /// naming an extension the plugin never registered leaves a button that is
    /// never shown.
    pub register_fs_action: extern "C" fn(
        extensions: *const c_char,
        action_id: *const c_char,
        svg: *const c_char,
        tooltip: *const c_char,
        enable_flags: u32,
        // `IC_ACTION_BUTTON` or `IC_ACTION_TOGGLE`.
        kind: u32,
        on_click: IcFsClickFn,
        user_data: *mut c_void,
    ) -> c_int,
    /// Says the mount's listing is out of date, so the panel standing in it is
    /// worth listing again. Safe to call from a thread of the plugin's own.
    pub fs_invalidate: extern "C" fn(extensions: *const c_char) -> c_int,
    /// Shows or hides a header button this plugin added. A button that has
    /// nothing to say is better absent than greyed out, and an indicator is
    /// the case for it: it appears when there is activity and goes when there
    /// is none. Safe to call from a thread of the plugin's own.
    pub set_header_visible: extern "C" fn(id: *const c_char, shown: c_int) -> c_int,
    /// Whether the panel's own toolbar buttons are shown while standing in a
    /// filesystem of this plugin's, or whether the plugin draws the toolbar
    /// itself out of what `register_fs_action` put there.
    ///
    /// Declared here rather than asked of the filesystem: what a toolbar
    /// looks like is no business of a thing that stores files, and the same
    /// filesystem answers a web frontend and a console that have no toolbar
    /// at all. `extensions` is spelled as in `register_filesystem`.
    ///
    /// A plugin that says nothing keeps the toolbar it always had.
    pub set_default_toolbar_visible:
        extern "C" fn(extensions: *const c_char, shown: c_int) -> c_int,
    /// The language the application is showing, as a two-letter code.
    ///
    /// A plugin registers its catalogue with `register_locales` and the host
    /// translates what it is handed as a key. Text a plugin puts *inside* a
    /// listing is never a key, so it has to pick the phrase itself, and this is
    /// how it knows which one. The string stays valid for the process.
    pub language: extern "C" fn() -> *const c_char,
    /// Replaces the picture on a header button this plugin added.
    ///
    /// Fixed at registration would mean a button that cannot say what state
    /// it is in — signed in or out, reachable or not — and an indicator that
    /// cannot is half an indicator. Safe to call from a thread of the
    /// plugin's own: the host carries it to the frontend.
    pub set_header_icon: extern "C" fn(id: *const c_char, svg: *const c_char) -> c_int,
    /// Registering the same `id` again replaces the entries.
    pub register_pinned_connections: extern "C" fn(
        id: *const c_char,
        rows: IcPinnedConnectionsFn,
        user_data: *mut c_void,
    ) -> c_int,
    /// Safe to call from a thread of the plugin's own.
    pub pinned_connections_changed: extern "C" fn() -> c_int,
    /// A picture or other file this plugin brings, kept under its own name.
    /// Whatever refers to it says `asset:<plugin_id>/<name>`, so one plugin's
    /// names can never stand in for another's.
    ///
    /// Last on purpose. This table grows only at the end: a field put in the
    /// middle moves every one after it, and a plugin built against the older
    /// layout then calls through the wrong offsets.
    pub register_plugin_asset: extern "C" fn(
        plugin_id: *const c_char,
        name: *const c_char,
        bytes: *const u8,
        bytes_len: u64,
    ) -> c_int,

    // ── Reading a filesystem the host holds ──────────────────────────────
    //
    // The first slots through which a plugin asks the application for
    // something rather than being handed it. A source arrives with
    // `IcFsVTable::open_in`; it is the host's, and it is good for as long as
    // the mount that was opened on it.
    //
    // These are called from the thread the source was handed to, while that
    // call is running. A plugin that keeps a source and calls from a thread
    // of its own is asking the host to read a filesystem from somewhere it
    // does not own.
    /// What this source does cheaply: `IC_FS_SEEK`, `IC_FS_SEQUENTIAL` and the
    /// rest. A hint for choosing an algorithm, not a permission.
    pub fs_caps: extern "C" fn(source: IcFsSource) -> u32,
    /// Opens one file on that filesystem. `mode` is `IC_OPEN_READ`,
    /// `IC_OPEN_WRITE` or `IC_OPEN_UPDATE`. Null when it cannot be opened.
    pub fs_open: extern "C" fn(source: IcFsSource, path: *const c_char, mode: u32) -> IcStream,
    /// Fills `into` with at most `len` bytes and says how many arrived. Zero
    /// is the end of the file, -1 an error; a short answer is not the end.
    pub fs_read: extern "C" fn(stream: IcStream, into: *mut u8, len: u64) -> i64,
    /// Moves the read along and answers the new position, or -1. `whence` is
    /// `IC_SEEK_SET`, `IC_SEEK_CURRENT` or `IC_SEEK_END`; seeking to the end
    /// is how the length of a file is asked for.
    ///
    /// A source that says nothing about `IC_FS_SEEK` still answers this —
    /// the host pays for it, by fetching what it has to. That is deliberate:
    /// a plugin should not carry a copy of the file about just because the
    /// thing underneath it happens to be a slow network.
    pub fs_seek: extern "C" fn(stream: IcStream, offset: i64, whence: u32) -> i64,
    /// Writes from `from` and says how many bytes were taken, or -1.
    pub fs_write: extern "C" fn(stream: IcStream, from: *const u8, len: u64) -> i64,
    /// Cuts the file to `len` bytes.
    pub fs_truncate: extern "C" fn(stream: IcStream, len: u64) -> c_int,
    /// Finishes with the stream. Anything written is on its way to where it
    /// belongs by the time this returns.
    pub fs_close: extern "C" fn(stream: IcStream),
    /// What that filesystem holds at `path`. The rows belong to the host and
    /// are good until the next call on this thread.
    pub fs_list: extern "C" fn(source: IcFsSource, path: *const c_char) -> IcListing,
    /// A real path on this machine for something that needs one — a player, a
    /// renderer that opens files itself. The host makes a copy when it has to,
    /// and the copy lasts as long as the source. Null when it cannot.
    pub fs_local_path: extern "C" fn(source: IcFsSource, path: *const c_char) -> *const c_char,
    /// Says what the plugin changed at `path` on that filesystem, so whatever
    /// the host remembers about it is thrown away. An empty path means the
    /// whole of it.
    pub fs_changed: extern "C" fn(source: IcFsSource, path: *const c_char) -> c_int,

    /// Offers to show the files named by these extensions — `".png,.jpg,.nef"`,
    /// spelled as `register_filesystem` spells them.
    ///
    /// By extension and nothing else: the application does not read a file to
    /// find out what it is. Reading the first bytes of everything on a server
    /// costs a request per file for a name it already has, and a file that
    /// lies about its extension is shown as what it called itself — which is
    /// the honest answer and what the built-in viewer does anyway.
    ///
    /// `priority` settles it when two plugins offer the same extension; the
    /// larger wins, and the application's own text and hex viewer is always
    /// the last resort, including when no plugin offers anything at all.
    pub register_viewer: extern "C" fn(
        viewer_id: *const c_char,
        extensions: *const c_char,
        priority: i32,
        vtable: *const IcViewerVTable,
        user_data: *mut c_void,
    ) -> c_int,
}

pub const IC_SETTING_PLAIN: u32 = 0;
/// Stored through the application's secret store rather than in plain text.
pub const IC_SETTING_SECRET: u32 = 1 << 0;

/// Which application a plugin has been loaded into.
///
/// A plugin registers by it, and may decline to load at all — anything but `IC_OK` from `init`.
pub const IC_HOST_GTK: &str = "gtk";
pub const IC_HOST_WEB: &str = "web";
pub const IC_HOST_CONSOLE: &str = "console";

pub type IcPluginInit = extern "C" fn(host: *const IcHost, kind: *const c_char) -> c_int;

pub const IC_PLUGIN_INIT_SYMBOL: &[u8] = b"ic_plugin_init";

/// Called once on an orderly exit, before the libraries are let go. Optional:
/// a plugin that owns no background work needs none. The host gives it a short
/// budget and carries on regardless, so a hung plugin cannot block the exit.
pub type IcPluginShutdown = extern "C" fn();

pub const IC_PLUGIN_SHUTDOWN_SYMBOL: &[u8] = b"ic_plugin_shutdown";

pub const IC_ABOUT_MAGIC: u64 = 0x4943_5f41_424f_5554;

/// What a plugin says about itself. Read before `ic_plugin_init`, so the host can
/// list a plugin it is not going to load. Grows by appending only: the reader
/// trusts nothing past `struct_size`.
#[repr(C)]
pub struct IcAbout {
    pub magic: u64,
    pub struct_size: u32,
    pub abi_version: u32,
    /// Stable machine identity, not the file name.
    pub id: *const c_char,
    /// Plain Latin display name, hardcoded by the plugin. Not a translation key:
    /// a plugin cannot rely on the host holding a dictionary for it.
    pub name: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
}

pub type IcPluginAbout = extern "C" fn() -> *const IcAbout;

pub const IC_PLUGIN_ABOUT_SYMBOL: &[u8] = b"ic_plugin_about";

/// The two facts every plugin must be able to state, each as a function of its
/// own. A plugin can export just these and nothing else: no struct to get
/// right, no host table to hold. `ic_plugin_about` is the fuller answer, and
/// what it fills in wins; these fill whatever it left empty.
pub type IcPluginText = extern "C" fn() -> *const c_char;

pub const IC_PLUGIN_NAME_SYMBOL: &[u8] = b"ic_plugin_name";
pub const IC_PLUGIN_VERSION_SYMBOL: &[u8] = b"ic_plugin_version";

impl IcAbout {
    /// Builds the struct a plugin hands back. The pointers must outlive the
    /// call, so in practice they are `b"...\0"` literals.
    pub const fn new(
        id: *const c_char,
        name: *const c_char,
        version: *const c_char,
        description: *const c_char,
    ) -> IcAbout {
        IcAbout {
            magic: IC_ABOUT_MAGIC,
            struct_size: std::mem::size_of::<IcAbout>() as u32,
            abi_version: IC_ABI_VERSION,
            id,
            name,
            version,
            description,
        }
    }
}

/// Lets `IcAbout` sit in a `static`. Sound only because every pointer in it
/// points at a `'static` literal, which [`declare_about!`] is what guarantees.
#[repr(transparent)]
pub struct AboutSlot(pub IcAbout);

unsafe impl Sync for AboutSlot {}

/// Exports `ic_plugin_about`, `ic_plugin_name` and `ic_plugin_version` together.
///
/// Every argument must expand to a string literal, so a generated
/// `macro_rules!` returning one is a valid version — which is how the plugin
/// repository feeds in the version its build wrote.
#[macro_export]
macro_rules! declare_about {
    ($id:expr, $name:expr, $version:expr, $description:expr) => {
        #[cfg_attr(feature = "export-abi", no_mangle)]
        pub extern "C" fn ic_plugin_about() -> *const $crate::IcAbout {
            static ABOUT: $crate::AboutSlot = $crate::AboutSlot($crate::IcAbout::new(
                concat!($id, "\0").as_ptr() as *const std::os::raw::c_char,
                concat!($name, "\0").as_ptr() as *const std::os::raw::c_char,
                concat!($version, "\0").as_ptr() as *const std::os::raw::c_char,
                concat!($description, "\0").as_ptr() as *const std::os::raw::c_char,
            ));
            &ABOUT.0
        }

        #[cfg_attr(feature = "export-abi", no_mangle)]
        pub extern "C" fn ic_plugin_name() -> *const std::os::raw::c_char {
            concat!($name, "\0").as_ptr() as *const std::os::raw::c_char
        }

        #[cfg_attr(feature = "export-abi", no_mangle)]
        pub extern "C" fn ic_plugin_version() -> *const std::os::raw::c_char {
            concat!($version, "\0").as_ptr() as *const std::os::raw::c_char
        }
    };
}

/// A field of `IcAbout` the plugin actually claimed to have filled in.
pub fn about_field<T: Copy>(about: *const IcAbout, offset: usize) -> Option<T> {
    if about.is_null() {
        return None;
    }
    let raw = about as *const u8;
    let magic = unsafe { std::ptr::read_unaligned(raw as *const u64) };
    if magic != IC_ABOUT_MAGIC {
        return None;
    }
    let claimed = unsafe { std::ptr::read_unaligned(raw.add(8) as *const u32) } as usize;
    let end = offset.checked_add(std::mem::size_of::<T>())?;
    if end > claimed || end > std::mem::size_of::<IcAbout>() {
        return None;
    }
    Some(unsafe { std::ptr::read_unaligned(raw.add(offset) as *const T) })
}

impl IcHost {
    pub fn offset_of_end() -> u32 {
        std::mem::size_of::<IcHost>() as u32
    }
}

#[derive(Clone)]
pub struct BoundedVTable {
    claimed: u32,
    bytes: Vec<u8>,
}

impl BoundedVTable {
    /// # Safety
    ///
    /// `vtable` must either be null or point to at least `claimed` readable
    /// bytes, where `claimed` is the `u32` it starts with. That is the plugin's
    /// side of the bargain: a table says how big it is and the host reads no
    /// further than the smaller of what it claims and what the host knows.
    pub unsafe fn adopt(vtable: *const u8, known: usize) -> Option<BoundedVTable> {
        if vtable.is_null() || known < std::mem::size_of::<u32>() {
            return None;
        }
        let claimed = unsafe { std::ptr::read_unaligned(vtable as *const u32) };
        let usable = (claimed as usize).min(known);
        if usable < std::mem::size_of::<u32>() {
            return None;
        }
        let mut bytes = vec![0u8; known];
        unsafe {
            std::ptr::copy_nonoverlapping(vtable, bytes.as_mut_ptr(), usable);
        }
        Some(BoundedVTable { claimed, bytes })
    }

    pub fn field<T: Copy>(&self, offset: usize) -> Option<T> {
        let end = offset.checked_add(std::mem::size_of::<T>())?;
        if end > self.claimed as usize || end > self.bytes.len() {
            return None;
        }
        Some(unsafe { std::ptr::read_unaligned(self.bytes.as_ptr().add(offset) as *const T) })
    }

    pub fn claimed(&self) -> u32 {
        self.claimed
    }
}

pub fn needs_up_to(offset: usize) -> u32 {
    (offset + std::mem::size_of::<usize>()) as u32
}

pub fn needs_connection_kinds() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, register_connection_kind))
}

pub fn needs_views() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, view_invalidate))
}

pub fn needs_locales() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, register_locales))
}

pub fn needs_assets() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, register_asset))
}

pub fn needs_settings() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, settings_write))
}

pub fn needs_panel_tree() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, register_panel_tree))
}

pub fn needs_header_label() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, set_header_label))
}

pub fn needs_drive_source() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, drives_changed))
}

pub fn needs_fs_action() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, fs_invalidate))
}

pub fn needs_language() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, language))
}

pub fn needs_header_visible() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, set_header_visible))
}

pub fn needs_own_toolbar() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, set_default_toolbar_visible))
}

pub fn needs_header_icon() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, set_header_icon))
}

pub fn needs_pinned_connections() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, pinned_connections_changed))
}

pub fn needs_plugin_assets() -> u32 {
    needs_up_to(std::mem::offset_of!(IcHost, register_plugin_asset))
}

pub fn host_has(host: *const IcHost, needs: u32) -> bool {
    if host.is_null() {
        return false;
    }
    let magic = unsafe { std::ptr::read_unaligned(host as *const u64) };
    if magic != IC_HOST_MAGIC {
        return false;
    }
    let struct_size = unsafe { std::ptr::read_unaligned((host as *const u8).add(8) as *const u32) };
    struct_size >= needs
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostCheck {
    Ok,
    WrongMagic,
    TooOld { needs: u32, found: u32 },
    Truncated { needs: u32, found: u32 },
}

pub fn check_host(host: *const IcHost, needs_abi: u32, needs_size: u32) -> HostCheck {
    if host.is_null() {
        return HostCheck::WrongMagic;
    }
    let magic = unsafe { std::ptr::read_unaligned(host as *const u64) };
    if magic != IC_HOST_MAGIC {
        return HostCheck::WrongMagic;
    }
    let struct_size = unsafe { std::ptr::read_unaligned((host as *const u8).add(8) as *const u32) };
    let abi_version =
        unsafe { std::ptr::read_unaligned((host as *const u8).add(12) as *const u32) };
    if abi_version < needs_abi {
        return HostCheck::TooOld {
            needs: needs_abi,
            found: abi_version,
        };
    }
    if struct_size < needs_size {
        return HostCheck::Truncated {
            needs: needs_size,
            found: struct_size,
        };
    }
    HostCheck::Ok
}

/// Pieces for a plugin's own tests: a host table that answers everything and
/// does nothing.
///
/// Behind the `testing` feature, so nothing of it reaches a shipped plugin.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    extern "C" fn version() -> *const c_char {
        c"test".as_ptr()
    }
    extern "C" fn warn(_: *const c_char) {}
    extern "C" fn selection() -> IcSelection {
        IcSelection::EMPTY
    }
    extern "C" fn reg_source(
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: IcTableFn,
        _: *mut c_void,
    ) -> c_int {
        IC_OK
    }
    extern "C" fn open_source(_: *const c_char) -> c_int {
        IC_OK
    }
    extern "C" fn close_source() -> c_int {
        IC_OK
    }
    extern "C" fn add_header(
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: u32,
        _: i32,
        _: IcClickFn,
        _: *mut c_void,
    ) -> c_int {
        IC_OK
    }
    extern "C" fn reg_fs(_: *const c_char, _: *const IcFsVTable, _: *mut c_void) -> c_int {
        IC_OK
    }
    extern "C" fn reg_action(
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: u32,
        _: IcClickFn,
        _: *mut c_void,
    ) -> c_int {
        IC_OK
    }
    extern "C" fn add_button(
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: u32,
        _: i32,
        _: u32,
        _: IcClickFn,
        _: *mut c_void,
    ) -> c_int {
        IC_OK
    }

    extern "C" fn reg_kind(
        _: *const c_char,
        _: *const u8,
        _: u64,
        _: *const IcConnectionVTable,
        _: *mut c_void,
    ) -> c_int {
        IC_OK
    }

    extern "C" fn reg_view(
        _: *const c_char,
        _: *const c_char,
        _: *const IcViewVTable,
        _: *mut c_void,
    ) -> c_int {
        IC_OK
    }
    extern "C" fn open_view(_: *const c_char, _: *const u8, _: u64) -> c_int {
        IC_OK
    }
    extern "C" fn invalidate(_: *const c_char) -> c_int {
        IC_OK
    }
    extern "C" fn reg_locales(_: *const c_char, _: *const u8, _: u64) -> c_int {
        IC_OK
    }
    extern "C" fn reg_asset(_: *const c_char, _: *const u8, _: u64) -> c_int {
        IC_OK
    }

    extern "C" fn read_setting(_: *const c_char, _: *const c_char) -> IcBytes {
        IcBytes::EMPTY
    }

    extern "C" fn reg_tree(
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: IcTreeFn,
        _: *mut c_void,
    ) -> c_int {
        IC_OK
    }

    extern "C" fn set_label(_: *const c_char, _: *const c_char) -> c_int {
        IC_OK
    }

    extern "C" fn drives_moved() -> c_int {
        IC_OK
    }

    extern "C" fn fs_went_stale(_: *const c_char) -> c_int {
        IC_OK
    }

    extern "C" fn speaks() -> *const c_char {
        c"en".as_ptr()
    }

    extern "C" fn show_header(_: *const c_char, _: c_int) -> c_int {
        IC_OK
    }

    extern "C" fn repaint_header(_: *const c_char, _: *const c_char) -> c_int {
        IC_OK
    }

    extern "C" fn show_default_toolbar(_: *const c_char, _: c_int) -> c_int {
        IC_OK
    }

    extern "C" fn reg_fs_action(
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: u32,
        _: u32,
        _: IcFsClickFn,
        _: *mut c_void,
    ) -> c_int {
        IC_OK
    }

    extern "C" fn reg_pinned(_: *const c_char, _: IcPinnedConnectionsFn, _: *mut c_void) -> c_int {
        IC_OK
    }

    extern "C" fn reg_owned_asset(
        _: *const c_char,
        _: *const c_char,
        _: *const u8,
        _: u64,
    ) -> c_int {
        IC_OK
    }

    extern "C" fn reg_drives(
        _: *const c_char,
        _: *const c_char,
        _: IcDrivesFn,
        _: *mut c_void,
    ) -> c_int {
        IC_OK
    }

    extern "C" fn write_setting(
        _: *const c_char,
        _: *const c_char,
        _: *const u8,
        _: u64,
        _: u32,
    ) -> c_int {
        IC_OK
    }

    extern "C" fn host_fs_caps(_: IcFsSource) -> u32 {
        IC_FS_SEEK | IC_FS_LIST
    }

    extern "C" fn host_fs_open(_: IcFsSource, _: *const c_char, _: u32) -> IcStream {
        std::ptr::null_mut()
    }

    extern "C" fn host_fs_read(_: IcStream, _: *mut u8, _: u64) -> i64 {
        0
    }

    extern "C" fn host_fs_seek(_: IcStream, _: i64, _: u32) -> i64 {
        0
    }

    extern "C" fn host_fs_write(_: IcStream, _: *const u8, _: u64) -> i64 {
        -1
    }

    extern "C" fn host_fs_truncate(_: IcStream, _: u64) -> c_int {
        IC_ERR_IO
    }

    extern "C" fn host_fs_close(_: IcStream) {}

    extern "C" fn host_fs_list(_: IcFsSource, _: *const c_char) -> IcListing {
        IcListing::EMPTY
    }

    extern "C" fn host_fs_local_path(_: IcFsSource, _: *const c_char) -> *const c_char {
        std::ptr::null()
    }

    extern "C" fn host_fs_changed(_: IcFsSource, _: *const c_char) -> c_int {
        IC_OK
    }

    extern "C" fn reg_viewer(
        _: *const c_char,
        _: *const c_char,
        _: i32,
        _: *const IcViewerVTable,
        _: *mut c_void,
    ) -> c_int {
        IC_OK
    }

    /// A host table with every slot filled in and nothing behind any of them.
    ///
    /// For a plugin's own tests: `init` can be called with this, and the few
    /// calls a test cares about replaced one field at a time —
    ///
    /// ```ignore
    /// let mut host = ic_plugin_api::testing::silent_host();
    /// host.fs_open = my_fs_open;
    /// ```
    pub fn silent_host() -> IcHost {
        IcHost {
            magic: IC_HOST_MAGIC,
            struct_size: std::mem::size_of::<IcHost>() as u32,
            abi_version: IC_ABI_VERSION,
            host_version: version,
            log_warn: warn,
            add_toolbar_button: add_button,
            selection,
            register_panel_source: reg_source,
            open_panel_source: open_source,
            close_panel_source: close_source,
            add_header_button: add_header,
            register_filesystem: reg_fs,
            register_panel_action: reg_action,
            register_connection_kind: reg_kind,
            register_view: reg_view,
            open_view,
            view_invalidate: invalidate,
            register_locales: reg_locales,
            register_asset: reg_asset,
            settings_read: read_setting,
            settings_write: write_setting,
            register_panel_tree: reg_tree,
            set_header_label: set_label,
            register_drive_source: reg_drives,
            drives_changed: drives_moved,
            register_fs_action: reg_fs_action,
            fs_invalidate: fs_went_stale,
            set_header_visible: show_header,
            set_default_toolbar_visible: show_default_toolbar,
            language: speaks,
            set_header_icon: repaint_header,
            register_pinned_connections: reg_pinned,
            pinned_connections_changed: drives_moved,
            register_plugin_asset: reg_owned_asset,
            fs_caps: host_fs_caps,
            fs_open: host_fs_open,
            fs_read: host_fs_read,
            fs_seek: host_fs_seek,
            fs_write: host_fs_write,
            fs_truncate: host_fs_truncate,
            fs_close: host_fs_close,
            fs_list: host_fs_list,
            fs_local_path: host_fs_local_path,
            fs_changed: host_fs_changed,
            register_viewer: reg_viewer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::silent_host as host;
    use super::*;
    use std::ffi::CString;

    /// The table grows only at the end, and this is what holds it to that.
    ///
    /// A field put in the middle moves every one after it. Nothing complains
    /// at compile time — a plugin built against the older layout simply calls
    /// through the wrong offsets and takes the application down with it. The
    /// numbers below are the layout as it shipped; adding a field appends a
    /// line, and changing an existing one is the mistake this catches.
    #[test]
    fn nothing_already_in_the_table_ever_moves() {
        let laid_out: Vec<(&str, usize)> = vec![
            ("magic", std::mem::offset_of!(IcHost, magic)),
            ("struct_size", std::mem::offset_of!(IcHost, struct_size)),
            ("abi_version", std::mem::offset_of!(IcHost, abi_version)),
            ("host_version", std::mem::offset_of!(IcHost, host_version)),
            ("log_warn", std::mem::offset_of!(IcHost, log_warn)),
            (
                "add_toolbar_button",
                std::mem::offset_of!(IcHost, add_toolbar_button),
            ),
            ("selection", std::mem::offset_of!(IcHost, selection)),
            (
                "register_panel_source",
                std::mem::offset_of!(IcHost, register_panel_source),
            ),
            (
                "open_panel_source",
                std::mem::offset_of!(IcHost, open_panel_source),
            ),
            (
                "close_panel_source",
                std::mem::offset_of!(IcHost, close_panel_source),
            ),
            (
                "add_header_button",
                std::mem::offset_of!(IcHost, add_header_button),
            ),
            (
                "register_filesystem",
                std::mem::offset_of!(IcHost, register_filesystem),
            ),
            (
                "register_panel_action",
                std::mem::offset_of!(IcHost, register_panel_action),
            ),
            (
                "register_connection_kind",
                std::mem::offset_of!(IcHost, register_connection_kind),
            ),
            ("register_view", std::mem::offset_of!(IcHost, register_view)),
            ("open_view", std::mem::offset_of!(IcHost, open_view)),
            (
                "view_invalidate",
                std::mem::offset_of!(IcHost, view_invalidate),
            ),
            (
                "register_locales",
                std::mem::offset_of!(IcHost, register_locales),
            ),
            (
                "register_asset",
                std::mem::offset_of!(IcHost, register_asset),
            ),
            ("settings_read", std::mem::offset_of!(IcHost, settings_read)),
            (
                "settings_write",
                std::mem::offset_of!(IcHost, settings_write),
            ),
            (
                "register_panel_tree",
                std::mem::offset_of!(IcHost, register_panel_tree),
            ),
            (
                "set_header_label",
                std::mem::offset_of!(IcHost, set_header_label),
            ),
            (
                "register_drive_source",
                std::mem::offset_of!(IcHost, register_drive_source),
            ),
            (
                "drives_changed",
                std::mem::offset_of!(IcHost, drives_changed),
            ),
            (
                "register_fs_action",
                std::mem::offset_of!(IcHost, register_fs_action),
            ),
            ("fs_invalidate", std::mem::offset_of!(IcHost, fs_invalidate)),
            (
                "set_header_visible",
                std::mem::offset_of!(IcHost, set_header_visible),
            ),
            (
                "set_default_toolbar_visible",
                std::mem::offset_of!(IcHost, set_default_toolbar_visible),
            ),
            ("language", std::mem::offset_of!(IcHost, language)),
            (
                "set_header_icon",
                std::mem::offset_of!(IcHost, set_header_icon),
            ),
            (
                "register_pinned_connections",
                std::mem::offset_of!(IcHost, register_pinned_connections),
            ),
            (
                "pinned_connections_changed",
                std::mem::offset_of!(IcHost, pinned_connections_changed),
            ),
            (
                "register_plugin_asset",
                std::mem::offset_of!(IcHost, register_plugin_asset),
            ),
            ("fs_caps", std::mem::offset_of!(IcHost, fs_caps)),
            ("fs_open", std::mem::offset_of!(IcHost, fs_open)),
            ("fs_read", std::mem::offset_of!(IcHost, fs_read)),
            ("fs_seek", std::mem::offset_of!(IcHost, fs_seek)),
            ("fs_write", std::mem::offset_of!(IcHost, fs_write)),
            ("fs_truncate", std::mem::offset_of!(IcHost, fs_truncate)),
            ("fs_close", std::mem::offset_of!(IcHost, fs_close)),
            ("fs_list", std::mem::offset_of!(IcHost, fs_list)),
            ("fs_local_path", std::mem::offset_of!(IcHost, fs_local_path)),
            ("fs_changed", std::mem::offset_of!(IcHost, fs_changed)),
            (
                "register_viewer",
                std::mem::offset_of!(IcHost, register_viewer),
            ),
        ];
        let mut expected = 0usize;
        for (name, at) in &laid_out {
            assert_eq!(
                *at, expected,
                "`{name}` sits at {at} and used to sit at {expected}: something was put in front of it"
            );
            expected += match *name {
                "magic" => std::mem::size_of::<u64>(),
                "struct_size" | "abi_version" => std::mem::size_of::<u32>(),
                _ => std::mem::size_of::<usize>(),
            };
        }
        assert_eq!(
            expected,
            std::mem::size_of::<IcHost>(),
            "the table has a field this test does not know about"
        );
    }

    #[test]
    fn the_first_three_fields_never_move() {
        let h = host();
        let base = &h as *const IcHost as usize;
        assert_eq!(&h.magic as *const u64 as usize - base, 0);
        assert_eq!(&h.struct_size as *const u32 as usize - base, 8);
        assert_eq!(&h.abi_version as *const u32 as usize - base, 12);
    }

    extern "C" fn fs_open_in(_: IcFsSource, _: *const c_char, _: *mut c_void) -> IcFsHandle {
        1usize as IcFsHandle
    }
    extern "C" fn fs_close(_: IcFsHandle) {}
    extern "C" fn fs_list(_: IcFsHandle, _: *const c_char) -> IcListing {
        IcListing::EMPTY
    }
    extern "C" fn fs_read(_: IcFsHandle, _: *const c_char) -> IcBytes {
        IcBytes::EMPTY
    }
    extern "C" fn fs_flag(_: IcFsHandle) -> c_int {
        1
    }
    extern "C" fn fs_error(_: IcFsHandle) -> *const c_char {
        std::ptr::null()
    }
    extern "C" fn fs_write(_: IcFsHandle, _: *const c_char, _: *const u8, _: u64) -> c_int {
        IC_OK
    }

    fn writable_fs() -> IcFsVTable {
        IcFsVTable {
            struct_size: std::mem::size_of::<IcFsVTable>() as u32,
            open_in: fs_open_in,
            close: fs_close,
            list: fs_list,
            read: fs_read,
            is_read_only: fs_flag,
            last_error: fs_error,
            write: Some(fs_write),
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

    extern "C" fn fs_columns(_: IcFsHandle) -> IcColumns {
        IcColumns::EMPTY
    }

    extern "C" fn fs_rows(_: IcFsHandle, _: *const c_char) -> IcRows {
        IcRows::EMPTY
    }

    #[test]
    fn a_vtable_from_before_the_columns_were_added_offers_neither_of_them() {
        let mut table = writable_fs();
        table.struct_size = std::mem::offset_of!(IcFsVTable, columns) as u32;
        let bounded = unsafe {
            BoundedVTable::adopt(
                &table as *const IcFsVTable as *const u8,
                std::mem::size_of::<IcFsVTable>(),
            )
        }
        .expect("adopted");
        assert!(bounded
            .field::<Option<IcFsColumnsFn>>(std::mem::offset_of!(IcFsVTable, columns))
            .is_none());
        assert!(bounded
            .field::<Option<IcFsRowsFn>>(std::mem::offset_of!(IcFsVTable, list_rows))
            .is_none());
    }

    #[test]
    fn a_vtable_that_fills_them_in_hands_both_back() {
        let mut table = writable_fs();
        table.columns = Some(fs_columns);
        table.list_rows = Some(fs_rows);
        let bounded = unsafe {
            BoundedVTable::adopt(
                &table as *const IcFsVTable as *const u8,
                std::mem::size_of::<IcFsVTable>(),
            )
        }
        .expect("adopted");
        assert!(bounded
            .field::<Option<IcFsColumnsFn>>(std::mem::offset_of!(IcFsVTable, columns))
            .flatten()
            .is_some());
        assert!(bounded
            .field::<Option<IcFsRowsFn>>(std::mem::offset_of!(IcFsVTable, list_rows))
            .flatten()
            .is_some());
    }

    #[test]
    fn a_column_says_whether_it_is_text_or_a_tick() {
        let plain = IcFsColumn {
            key: c"status".as_ptr(),
            title: c"Status".as_ptr(),
            width: 110,
            kind: IC_COLUMN_TEXT,
        };
        assert!(!plain.is_check());
        assert_eq!(plain.key_string(), "status");
        assert_eq!(plain.title_string(), "Status");
        let ticked = IcFsColumn {
            kind: IC_COLUMN_CHECK,
            ..plain
        };
        assert!(ticked.is_check());
    }

    #[test]
    fn a_column_that_says_nothing_about_its_kind_is_text() {
        // Zero is what a plugin that never heard of ticks leaves behind.
        assert_eq!(IC_COLUMN_TEXT, 0);
    }

    #[test]
    fn a_column_with_no_strings_reads_as_empty_rather_than_crashing() {
        let nothing = IcFsColumn {
            key: std::ptr::null(),
            title: std::ptr::null(),
            width: 0,
            kind: IC_COLUMN_TEXT,
        };
        assert_eq!(nothing.key_string(), "");
        assert_eq!(nothing.title_string(), "");
    }

    #[test]
    fn only_the_one_agreed_spelling_counts_as_ticked() {
        assert!(cell_is_ticked(IC_CELL_TICKED));
        assert!(cell_is_ticked("1"));
        // Anything else is not ticked, rather than being undefined.
        assert!(!cell_is_ticked("0"));
        assert!(!cell_is_ticked(""));
        assert!(!cell_is_ticked("true"));
        assert!(!cell_is_ticked("yes"));
        assert!(!cell_is_ticked(" 1"));
    }

    #[test]
    fn a_vtable_from_before_the_ticks_existed_offers_no_click() {
        let mut table = writable_fs();
        table.struct_size = std::mem::offset_of!(IcFsVTable, cell_clicked) as u32;
        let bounded = unsafe {
            BoundedVTable::adopt(
                &table as *const IcFsVTable as *const u8,
                std::mem::size_of::<IcFsVTable>(),
            )
        }
        .expect("adopted");
        assert!(bounded
            .field::<Option<IcFsCellClickedFn>>(std::mem::offset_of!(IcFsVTable, cell_clicked))
            .is_none());
        // What came before it is still reachable.
        assert!(bounded
            .field::<Option<IcFsColumnsFn>>(std::mem::offset_of!(IcFsVTable, columns))
            .is_some());
    }

    /// A filesystem is opened on the filesystem its file lives on, and never
    /// on a copy of that file. This is the slot that says so.
    #[test]
    fn a_mount_is_opened_on_a_source_and_a_path() {
        let table = writable_fs();
        let bounded = unsafe {
            BoundedVTable::adopt(
                &table as *const IcFsVTable as *const u8,
                std::mem::size_of::<IcFsVTable>(),
            )
        }
        .expect("adopted");
        assert!(bounded
            .field::<IcFsOpenInFn>(std::mem::offset_of!(IcFsVTable, open_in))
            .is_some());
    }

    #[test]
    fn empty_columns_and_rows_are_sound_and_carry_nothing() {
        assert!(IcColumns::EMPTY.is_sound());
        assert!(IcColumns::EMPTY.as_slice().is_empty());
        assert!(IcRows::EMPTY.is_sound());
        assert!(IcRows::EMPTY.as_slice().is_empty());
    }

    #[test]
    fn columns_and_rows_without_the_right_magic_are_read_as_empty() {
        let lying = IcColumns {
            magic: 0,
            ..IcColumns::EMPTY
        };
        assert!(!lying.is_sound());
        assert!(lying.as_slice().is_empty());
        let short = IcRows {
            struct_size: 4,
            ..IcRows::EMPTY
        };
        assert!(!short.is_sound());
        assert!(short.as_slice().is_empty());
    }

    #[test]
    fn a_row_reads_its_cells_and_stops_at_what_was_declared() {
        let first = CString::new("seeding").expect("no nul");
        let second = CString::new("41%").expect("no nul");
        let cells: [*const c_char; 2] = [first.as_ptr(), second.as_ptr()];
        let row = IcRow {
            entry: IcDirEntry {
                name: std::ptr::null(),
                is_dir: 0,
                size: 0,
                modified: 0,
                permissions: 0,
                has_permissions: 0,
            },
            extra: cells.as_ptr(),
            extra_count: 2,
        };
        assert_eq!(row.extra_at(0), "seeding");
        assert_eq!(row.extra_at(1), "41%");
        assert_eq!(row.extra_at(7), "");
    }

    #[test]
    fn a_row_that_stops_short_is_never_read_past_its_own_count() {
        let only = CString::new("seeding").expect("no nul");
        // The array really holds one string; the row says so.
        let cells: [*const c_char; 1] = [only.as_ptr()];
        let row = IcRow {
            entry: IcDirEntry {
                name: std::ptr::null(),
                is_dir: 0,
                size: 0,
                modified: 0,
                permissions: 0,
                has_permissions: 0,
            },
            extra: cells.as_ptr(),
            extra_count: 1,
        };
        assert_eq!(row.extra_at(0), "seeding");
        assert_eq!(row.extra_at(1), "");
    }

    #[test]
    fn a_row_with_no_cells_at_all_reads_as_empty() {
        let row = IcRow {
            entry: IcDirEntry {
                name: std::ptr::null(),
                is_dir: 0,
                size: 0,
                modified: 0,
                permissions: 0,
                has_permissions: 0,
            },
            extra: std::ptr::null(),
            extra_count: 3,
        };
        assert_eq!(row.extra_at(0), "");
    }

    #[test]
    fn a_vtable_that_claims_its_whole_size_exposes_every_call() {
        let table = writable_fs();
        let bounded = unsafe {
            BoundedVTable::adopt(
                &table as *const IcFsVTable as *const u8,
                std::mem::size_of::<IcFsVTable>(),
            )
        }
        .expect("adopted");
        assert_eq!(bounded.claimed(), std::mem::size_of::<IcFsVTable>() as u32);
        assert!(bounded
            .field::<IcFsOpenInFn>(std::mem::offset_of!(IcFsVTable, open_in))
            .is_some());
        assert!(bounded
            .field::<Option<IcFsWriteFn>>(std::mem::offset_of!(IcFsVTable, write))
            .flatten()
            .is_some());
        assert!(
            bounded
                .field::<Option<IcFsPathFn>>(std::mem::offset_of!(IcFsVTable, create_dir))
                .flatten()
                .is_none(),
            "a null entry reads as absent, not as a call"
        );
    }

    #[test]
    fn a_vtable_built_before_the_write_side_existed_hides_it_instead_of_reading_past_the_end() {
        let mut table = writable_fs();
        table.struct_size = std::mem::offset_of!(IcFsVTable, write) as u32;
        let bounded = unsafe {
            BoundedVTable::adopt(
                &table as *const IcFsVTable as *const u8,
                std::mem::size_of::<IcFsVTable>(),
            )
        }
        .expect("adopted");
        assert!(bounded
            .field::<Option<IcFsErrorFn>>(std::mem::offset_of!(IcFsVTable, last_error))
            .flatten()
            .is_some());
        assert!(bounded
            .field::<Option<IcFsWriteFn>>(std::mem::offset_of!(IcFsVTable, write))
            .is_none());
    }

    #[test]
    fn a_vtable_claiming_more_than_the_host_knows_is_clamped() {
        let mut table = writable_fs();
        table.struct_size = u32::MAX;
        let bounded = unsafe {
            BoundedVTable::adopt(
                &table as *const IcFsVTable as *const u8,
                std::mem::size_of::<IcFsVTable>(),
            )
        }
        .expect("adopted");
        assert!(bounded
            .field::<Option<IcFsWriteFn>>(std::mem::offset_of!(IcFsVTable, write))
            .flatten()
            .is_some());
        assert!(bounded
            .field::<Option<IcFsWriteFn>>(std::mem::size_of::<IcFsVTable>())
            .is_none());
    }

    #[test]
    fn nothing_is_adopted_from_a_null_or_impossibly_small_vtable() {
        assert!(unsafe { BoundedVTable::adopt(std::ptr::null(), 64) }.is_none());
        let table = writable_fs();
        assert!(
            unsafe { BoundedVTable::adopt(&table as *const IcFsVTable as *const u8, 2) }.is_none()
        );
        let mut truncated = writable_fs();
        truncated.struct_size = 1;
        assert!(unsafe {
            BoundedVTable::adopt(
                &truncated as *const IcFsVTable as *const u8,
                std::mem::size_of::<IcFsVTable>(),
            )
        }
        .is_none());
    }

    #[test]
    fn an_optional_call_is_detected_without_refusing_a_shorter_host() {
        let full = host();
        assert!(host_has(&full, needs_connection_kinds()));
        let mut shortened = host();
        shortened.struct_size = std::mem::offset_of!(IcHost, register_connection_kind) as u32;
        assert!(!host_has(&shortened, needs_connection_kinds()));
        assert!(host_has(
            &shortened,
            needs_up_to(std::mem::offset_of!(IcHost, register_filesystem))
        ));
    }

    #[test]
    fn a_plugin_needing_views_refuses_a_host_that_only_knows_connections() {
        let mut older = host();
        older.struct_size = needs_connection_kinds();
        assert!(host_has(&older, needs_connection_kinds()));
        assert!(!host_has(&older, needs_views()));
        assert!(host_has(&host(), needs_views()));
    }

    #[test]
    fn a_foreign_table_is_never_treated_as_capable() {
        let mut impostor = host();
        impostor.magic = 0;
        assert!(!host_has(&impostor, needs_connection_kinds()));
        assert!(!host_has(std::ptr::null(), needs_connection_kinds()));
    }

    #[test]
    fn a_matching_host_is_accepted() {
        let h = host();
        assert_eq!(
            check_host(&h, IC_ABI_VERSION, std::mem::size_of::<IcHost>() as u32),
            HostCheck::Ok
        );
    }

    #[test]
    fn a_plugin_from_the_future_refuses_an_older_host() {
        let h = host();
        assert_eq!(
            check_host(&h, IC_ABI_VERSION + 1, 0),
            HostCheck::TooOld {
                needs: IC_ABI_VERSION + 1,
                found: IC_ABI_VERSION
            }
        );
    }

    #[test]
    fn a_plugin_needing_a_longer_table_refuses_a_shorter_one() {
        let mut h = host();
        h.struct_size = 16;
        assert!(matches!(
            check_host(&h, IC_ABI_VERSION, 64),
            HostCheck::Truncated { .. }
        ));
    }

    #[test]
    fn rubbish_in_place_of_a_host_is_rejected_without_touching_it() {
        assert_eq!(check_host(std::ptr::null(), 1, 0), HostCheck::WrongMagic);
        let not_a_host: u64 = 0xdead_beef;
        assert_eq!(
            check_host(&not_a_host as *const u64 as *const IcHost, 1, 0),
            HostCheck::WrongMagic
        );
    }
    #[test]
    fn an_empty_selection_reads_as_an_empty_slice() {
        assert!(IcSelection::EMPTY.as_slice().is_empty());
        let lying = IcSelection {
            items: std::ptr::null(),
            count: 7,
        };
        assert!(lying.as_slice().is_empty());
    }

    #[test]
    fn items_are_read_back_as_a_plain_slice() {
        let a = c"/a/dir";
        let b = c"/a/file.bin";
        let items = [
            IcSelectionItem {
                path: a.as_ptr(),
                key: a.as_ptr(),
                is_dir: 1,
            },
            IcSelectionItem {
                path: b.as_ptr(),
                key: b.as_ptr(),
                is_dir: 0,
            },
        ];
        let sel = IcSelection {
            items: items.as_ptr(),
            count: 2,
        };
        let got = sel.as_slice();
        assert_eq!(got.len(), 2);
        assert!(got[0].is_directory());
        assert!(!got[1].is_directory());
        assert_eq!(got[1].path_string().as_deref(), Some("/a/file.bin"));
    }

    #[test]
    fn a_button_with_no_flags_is_always_available() {
        assert!(enabled_for(IC_ENABLE_ALWAYS, 0, 0));
        assert!(enabled_for(IC_ENABLE_ALWAYS, 3, 2));
    }

    #[test]
    fn a_file_only_button_waits_for_a_file() {
        assert!(!enabled_for(IC_ENABLE_ON_FILE, 0, 0));
        assert!(!enabled_for(IC_ENABLE_ON_FILE, 0, 5));
        assert!(enabled_for(IC_ENABLE_ON_FILE, 1, 0));
        assert!(enabled_for(IC_ENABLE_ON_FILE, 1, 5));
    }

    #[test]
    fn a_button_can_ask_for_either_kind() {
        let both = IC_ENABLE_ON_FILE | IC_ENABLE_ON_DIR;
        assert!(enabled_for(both, 1, 0));
        assert!(enabled_for(both, 0, 1));
        assert!(!enabled_for(both, 0, 0));
    }

    #[test]
    fn a_button_can_ask_for_an_empty_selection_only() {
        assert!(enabled_for(IC_ENABLE_ON_EMPTY, 0, 0));
        assert!(!enabled_for(IC_ENABLE_ON_EMPTY, 1, 0));
        assert!(!enabled_for(IC_ENABLE_ON_EMPTY, 0, 1));
    }

    #[test]
    fn an_empty_table_yields_nothing_rather_than_reading() {
        let t = IcTable::EMPTY;
        assert!(t.columns_slice().is_empty());
        assert_eq!(t.cell(0, 0), None);
    }

    #[test]
    fn cells_are_addressed_row_by_row() {
        let a = c"1";
        let b = c"init";
        let c = c"2";
        let d = c"bash";
        let cells = [a.as_ptr(), b.as_ptr(), c.as_ptr(), d.as_ptr()];
        let cols = [
            IcColumn {
                key: c"pid".as_ptr(),
                title: c"PID".as_ptr(),
                width: 80,
            },
            IcColumn {
                key: c"name".as_ptr(),
                title: c"Name".as_ptr(),
                width: 200,
            },
        ];
        let t = IcTable {
            columns: cols.as_ptr(),
            column_count: 2,
            cells: cells.as_ptr(),
            row_count: 2,
            key_column: 0,
        };
        assert_eq!(t.cell(0, 0).as_deref(), Some("1"));
        assert_eq!(t.cell(0, 1).as_deref(), Some("init"));
        assert_eq!(t.cell(1, 1).as_deref(), Some("bash"));
        assert_eq!(t.columns_slice().len(), 2);
        assert_eq!(t.columns_slice()[1].title_string(), "Name");
    }

    #[test]
    fn an_index_outside_the_table_is_refused() {
        let a = c"x";
        let cells = [a.as_ptr()];
        let cols = [IcColumn {
            key: c"k".as_ptr(),
            title: c"K".as_ptr(),
            width: 10,
        }];
        let t = IcTable {
            columns: cols.as_ptr(),
            column_count: 1,
            cells: cells.as_ptr(),
            row_count: 1,
            key_column: 0,
        };
        assert_eq!(t.cell(1, 0), None);
        assert_eq!(t.cell(0, 1), None);
    }

    #[test]
    fn an_empty_listing_and_empty_bytes_read_as_nothing() {
        assert!(IcListing::EMPTY.as_slice().is_empty());
        assert!(IcBytes::EMPTY.as_slice().is_empty());
        let lying = IcListing {
            items: std::ptr::null(),
            count: 9,
        };
        assert!(lying.as_slice().is_empty());
        let lying_bytes = IcBytes {
            data: std::ptr::null(),
            len: 9,
        };
        assert!(lying_bytes.as_slice().is_empty());
    }

    #[test]
    fn a_directory_entry_reports_what_it_knows() {
        let name = c"readme.txt";
        let e = IcDirEntry {
            name: name.as_ptr(),
            is_dir: 0,
            size: 120,
            modified: 1700,
            permissions: 0o644,
            has_permissions: 1,
        };
        assert_eq!(e.name_string(), "readme.txt");
        assert!(!e.is_directory());
        assert_eq!(e.permissions_opt(), Some(0o644));
        let without = IcDirEntry {
            has_permissions: 0,
            ..e
        };
        assert_eq!(without.permissions_opt(), None);
    }
}
