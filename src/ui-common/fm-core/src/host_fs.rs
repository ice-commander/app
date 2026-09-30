//! What is behind the ten `fs_*` slots a plugin is given.
//!
//! A plugin is never handed the bytes of a file. It is handed a source — the
//! filesystem the file lives on — and a path on it, and it reads what it
//! needs: a zip's central directory from the end, a tar streamed forwards, the
//! folder a song sits in so a player can build a playlist.
//!
//! A source is a directory on this machine. For a file that is already on a
//! disk that is its own folder, and nothing is copied at all. For a file on a
//! server the host fetches it once, into a directory of its own, and the
//! plugin reads that — one fetch in place of the one the application used to
//! do anyway, and the plugin is none the wiser.
//!
//! Everything here is called from whichever thread the plugin was driven on,
//! so nothing here touches the asynchronous side of the application: a source
//! is a path and a standard file, both of which cross threads safely.

// Every `fs_*` below is called from C with raw pointers: that is what the
// boundary is, and the buffer a plugin hands over is the plugin's own.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use ic_plugin_api::{
    IcDirEntry, IcFsSource, IcListing, IcStream, IC_ERR_INIT_FAILED, IC_FS_LIST, IC_FS_LOCAL_PATH,
    IC_FS_SEEK, IC_FS_WRITE, IC_OK, IC_OPEN_UPDATE, IC_OPEN_WRITE, IC_SEEK_CURRENT, IC_SEEK_END,
    IC_SEEK_SET,
};

/// A filesystem a plugin was let into, from the host's side.
pub struct HostSource {
    /// The directory the plugin sees as `/`.
    root: PathBuf,
    /// Which filesystem this stands for, so a plugin saying it changed
    /// something throws away what the application remembers about it. Empty
    /// for a source nothing is cached under.
    fs_id: String,
    /// Where on that filesystem `root` stands, for the same reason.
    within: String,
}

impl HostSource {
    /// A source on a real directory. `fs_id` and `within` are what the host
    /// knows the directory as, and are only used to answer `fs_changed`.
    pub fn rooted_at(root: impl Into<PathBuf>, fs_id: &str, within: &str) -> HostSource {
        HostSource {
            root: root.into(),
            fs_id: fs_id.to_string(),
            within: within.to_string(),
        }
    }

    /// Hands it to a plugin. The source stays alive until `close` takes it
    /// back, and a plugin that keeps the pointer past that is answered with
    /// nothing rather than with somebody else's memory.
    pub fn open(self) -> IcFsSource {
        let handed = Box::into_raw(Box::new(self));
        live().insert(handed as usize);
        handed as IcFsSource
    }

    /// Takes it back. Every stream the plugin left open is its own to close;
    /// they hold nothing but a file of their own.
    pub fn close(source: IcFsSource) {
        if source.is_null() || !live().remove(&(source as usize)) {
            return;
        }
        drop(unsafe { Box::from_raw(source as *mut HostSource) });
    }

    /// Where a path on this source really is. `None` when it climbs out of the
    /// root — a plugin asking for `../../etc/passwd` is answered with nothing.
    fn real(&self, path: &str) -> Option<PathBuf> {
        let mut here = self.root.clone();
        for segment in path.split(['/', '\\']) {
            match segment {
                "" | "." => continue,
                ".." => {
                    if !here.pop() || !here.starts_with(&self.root) {
                        return None;
                    }
                }
                named => here.push(named),
            }
        }
        here.starts_with(&self.root).then_some(here)
    }
}

/// A directory of the host's own, swept away when the mount that needed it is
/// closed. What it holds is a file fetched from somewhere the plugin cannot
/// read directly.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn made() -> std::io::Result<Scratch> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let at = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ice-commander-mount-{}-{at}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Scratch(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Where a mount's file stands while the plugin reads it: a directory on this
/// machine and the name of the file in it.
///
/// A file already on a disk is read where it lies — nothing is copied, and the
/// plugin sees the folder it sits in, which is how a player finds the rest of
/// an album. A file on a server is fetched once into a directory of the host's
/// own; that is the same single fetch the application did before it had
/// streaming at all, and it is swept away when the mount closes.
pub struct Staged {
    pub root: std::path::PathBuf,
    pub name: String,
    /// Set when the file was copied here, and then what the mount changes has
    /// to be carried back to where it came from.
    copied: Option<Scratch>,
    /// How long the copy was and when it was last written, as it was handed
    /// over. A mount that changed nothing is not carried back over a file that
    /// is already right.
    stamped: Option<(u64, std::time::SystemTime)>,
}

/// What a file looks like from the outside, for telling whether the plugin
/// wrote to it.
fn stamp_of(path: &std::path::Path) -> Option<(u64, std::time::SystemTime)> {
    let held = std::fs::metadata(path).ok()?;
    Some((held.len(), held.modified().ok()?))
}

pub async fn stage(
    parent: &std::rc::Rc<dyn crate::rpc::FileSystemRpc>,
    relative_path: &str,
) -> Result<Staged, common::AppError> {
    let trimmed = relative_path.trim_end_matches(['/', '\\']);
    let name = trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_string();
    if parent.is_local() {
        let holds = trimmed.len() - name.len();
        let root = trimmed[..holds].trim_end_matches(['/', '\\']);
        let root = if root.is_empty() { "/" } else { root };
        return Ok(Staged {
            root: PathBuf::from(root),
            name,
            copied: None,
            stamped: None,
        });
    }
    let bytes = parent.read_file(relative_path.to_string(), None).await?;
    let scratch = Scratch::made().map_err(|e| common::AppError::Other(e.to_string()))?;
    let at = scratch.path().join(&name);
    std::fs::write(&at, bytes).map_err(|e| common::AppError::Other(e.to_string()))?;
    let stamped = stamp_of(&at);
    Ok(Staged {
        root: scratch.path().to_path_buf(),
        name,
        copied: Some(scratch),
        stamped,
    })
}

impl Staged {
    /// The file the plugin was opened on, where it now stands.
    pub fn at(&self) -> PathBuf {
        self.root.join(&self.name)
    }

    /// Whether this is a copy that would have to be carried back, and the
    /// plugin has written to it. A mount standing on the real file has
    /// written where it stands and answers false.
    pub fn was_written(&self) -> bool {
        self.copied.is_some() && stamp_of(&self.at()) != self.stamped
    }
}

/// The sources handed out and not yet taken back.
///
/// A plugin holding a source it was told is finished with would otherwise read
/// freed memory, and that is a crash of the whole application rather than a
/// misbehaving plugin. One lookup against a small set is nothing beside the
/// read that follows.
fn live() -> std::sync::MutexGuard<'static, HashSet<usize>> {
    static HELD: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn source_of<'a>(source: IcFsSource) -> Option<&'a HostSource> {
    if source.is_null() || !live().contains(&(source as usize)) {
        return None;
    }
    Some(unsafe { &*(source as *const HostSource) })
}

fn asked(path: *const c_char) -> String {
    if path.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(path) }
        .to_string_lossy()
        .into_owned()
}

/// An open file, as the plugin holds it.
struct Stream {
    file: std::fs::File,
}

fn stream_of<'a>(stream: IcStream) -> Option<&'a mut Stream> {
    if stream.is_null() {
        return None;
    }
    Some(unsafe { &mut *(stream as *mut Stream) })
}

pub extern "C" fn fs_caps(source: IcFsSource) -> u32 {
    if source_of(source).is_none() {
        return 0;
    }
    // A directory on this machine: everything is cheap, including the seek to
    // the end that asks a file how long it is.
    IC_FS_SEEK | IC_FS_WRITE | IC_FS_LIST | IC_FS_LOCAL_PATH
}

pub extern "C" fn fs_open(source: IcFsSource, path: *const c_char, mode: u32) -> IcStream {
    let Some(source) = source_of(source) else {
        return std::ptr::null_mut();
    };
    let Some(real) = source.real(&asked(path)) else {
        return std::ptr::null_mut();
    };
    let opened = match mode {
        IC_OPEN_WRITE => std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&real),
        IC_OPEN_UPDATE => std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&real),
        // Anything else is a read: a plugin built against a newer contract
        // asking for something this host does not know still gets its bytes.
        _ => std::fs::File::open(&real),
    };
    match opened {
        Ok(file) => Box::into_raw(Box::new(Stream { file })) as IcStream,
        Err(_) => std::ptr::null_mut(),
    }
}

pub extern "C" fn fs_read(stream: IcStream, into: *mut u8, len: u64) -> i64 {
    let Some(stream) = stream_of(stream) else {
        return -1;
    };
    if into.is_null() {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    let room = unsafe { std::slice::from_raw_parts_mut(into, len as usize) };
    match stream.file.read(room) {
        Ok(read) => read as i64,
        Err(_) => -1,
    }
}

pub extern "C" fn fs_seek(stream: IcStream, offset: i64, whence: u32) -> i64 {
    let Some(stream) = stream_of(stream) else {
        return -1;
    };
    let wanted = match whence {
        IC_SEEK_SET => SeekFrom::Start(offset.max(0) as u64),
        IC_SEEK_CURRENT => SeekFrom::Current(offset),
        IC_SEEK_END => SeekFrom::End(offset),
        _ => return -1,
    };
    match stream.file.seek(wanted) {
        Ok(at) => at as i64,
        Err(_) => -1,
    }
}

pub extern "C" fn fs_write(stream: IcStream, from: *const u8, len: u64) -> i64 {
    let Some(stream) = stream_of(stream) else {
        return -1;
    };
    if from.is_null() {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    let held = unsafe { std::slice::from_raw_parts(from, len as usize) };
    match stream.file.write(held) {
        Ok(written) => written as i64,
        Err(_) => -1,
    }
}

pub extern "C" fn fs_truncate(stream: IcStream, len: u64) -> c_int {
    let Some(stream) = stream_of(stream) else {
        return IC_ERR_INIT_FAILED;
    };
    match stream.file.set_len(len) {
        Ok(()) => IC_OK,
        Err(_) => IC_ERR_INIT_FAILED,
    }
}

pub extern "C" fn fs_close(stream: IcStream) {
    if stream.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(stream as *mut Stream) });
}

thread_local! {
    /// What the last `fs_list` on this thread answered. The contract says the
    /// rows are good until the next call, which is what this holds them for.
    static LISTED: std::cell::RefCell<(Vec<CString>, Vec<IcDirEntry>)> =
        const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
    /// The same for the one path `fs_local_path` last answered with.
    static LOCAL: std::cell::RefCell<CString> = std::cell::RefCell::new(CString::default());
}

pub extern "C" fn fs_list(source: IcFsSource, path: *const c_char) -> IcListing {
    let Some(source) = source_of(source) else {
        return IcListing::EMPTY;
    };
    let Some(real) = source.real(&asked(path)) else {
        return IcListing::EMPTY;
    };
    let Ok(read) = std::fs::read_dir(&real) else {
        return IcListing::EMPTY;
    };
    let mut names: Vec<CString> = Vec::new();
    let mut rows: Vec<IcDirEntry> = Vec::new();
    for entry in read.flatten() {
        let Ok(named) = CString::new(entry.file_name().to_string_lossy().into_owned()) else {
            continue;
        };
        let held = entry.metadata().ok();
        names.push(named);
        rows.push(IcDirEntry {
            // Filled in below: the strings move into the thread's own store
            // first, and only then is their address any good.
            name: std::ptr::null(),
            is_dir: held.as_ref().map(|m| m.is_dir()).unwrap_or(false) as c_int,
            size: held.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: held
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_secs())
                .unwrap_or(0),
            permissions: permissions_of(held.as_ref()),
            has_permissions: cfg!(unix) as c_int,
        });
    }
    LISTED.with(|held| {
        let mut held = held.borrow_mut();
        *held = (names, rows);
        let (names, rows) = &mut *held;
        for (row, named) in rows.iter_mut().zip(names.iter()) {
            row.name = named.as_ptr();
        }
        IcListing {
            items: rows.as_ptr(),
            count: rows.len() as u32,
        }
    })
}

#[cfg(unix)]
fn permissions_of(held: Option<&std::fs::Metadata>) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    held.map(|m| m.permissions().mode() & 0o7777).unwrap_or(0)
}

#[cfg(not(unix))]
fn permissions_of(_held: Option<&std::fs::Metadata>) -> u32 {
    0
}

pub extern "C" fn fs_local_path(source: IcFsSource, path: *const c_char) -> *const c_char {
    let Some(source) = source_of(source) else {
        return std::ptr::null();
    };
    let Some(real) = source.real(&asked(path)) else {
        return std::ptr::null();
    };
    if !real.exists() {
        return std::ptr::null();
    }
    let Ok(named) = CString::new(real.to_string_lossy().into_owned()) else {
        return std::ptr::null();
    };
    LOCAL.with(|held| {
        *held.borrow_mut() = named;
        held.borrow().as_ptr()
    })
}

/// Which filesystem this source stands for and where on it — what a plugin
/// saying it changed something is talking about. Empty when the source is not
/// something the application remembers anything about.
///
/// `fs_changed` itself is not here: throwing away what is remembered has to
/// happen on the thread that draws, and only the host knows how to get there.
pub fn where_it_stands(source: IcFsSource) -> Option<(String, String)> {
    let source = source_of(source)?;
    Some((source.fs_id.clone(), source.within.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ic_plugin_api::IC_OPEN_READ;

    fn a_directory(named: &str) -> PathBuf {
        let at = std::env::temp_dir().join(format!("ic-host-fs-{}-{named}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(&at).expect("a directory of our own");
        at
    }

    fn opened(root: &Path) -> IcFsSource {
        HostSource::rooted_at(root, "", "").open()
    }

    fn c(text: &str) -> CString {
        CString::new(text).expect("a path")
    }

    /// The whole point of the boundary: a plugin reads the piece it wants out
    /// of the middle of a file without the file ever being in memory.
    #[test]
    fn a_plugin_reads_a_stretch_out_of_the_middle() {
        let root = a_directory("middle");
        std::fs::write(root.join("digits.txt"), b"1234567890").expect("a file");
        let source = opened(&root);

        let stream = fs_open(source, c("digits.txt").as_ptr(), IC_OPEN_READ);
        assert!(!stream.is_null());
        assert_eq!(fs_seek(stream, 4, IC_SEEK_SET), 4);
        let mut into = [0u8; 3];
        assert_eq!(fs_read(stream, into.as_mut_ptr(), 3), 3);
        assert_eq!(&into, b"567");
        fs_close(stream);
        HostSource::close(source);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Seeking to the end is how a file says how long it is, and a zip asks
    /// that before it asks anything else.
    #[test]
    fn the_end_of_a_file_is_its_length() {
        let root = a_directory("length");
        std::fs::write(root.join("holiday.zip"), vec![0u8; 4096]).expect("a file");
        let source = opened(&root);

        let stream = fs_open(source, c("/holiday.zip").as_ptr(), IC_OPEN_READ);
        assert_eq!(fs_seek(stream, 0, IC_SEEK_END), 4096);
        assert_eq!(fs_seek(stream, -22, IC_SEEK_END), 4074, "a zip's tail");
        fs_close(stream);
        HostSource::close(source);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn what_is_written_through_a_stream_is_on_the_disk() {
        let root = a_directory("writing");
        let source = opened(&root);

        let stream = fs_open(source, c("made.txt").as_ptr(), IC_OPEN_WRITE);
        assert!(!stream.is_null());
        assert_eq!(fs_write(stream, b"hello".as_ptr(), 5), 5);
        assert_eq!(fs_truncate(stream, 4), IC_OK);
        fs_close(stream);

        assert_eq!(
            std::fs::read(root.join("made.txt")).expect("what was written"),
            b"hell"
        );
        HostSource::close(source);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_folder_a_file_sits_in_can_be_listed() {
        let root = a_directory("listing");
        std::fs::write(root.join("one.mp3"), b"..").expect("a file");
        std::fs::create_dir(root.join("inner")).expect("a folder");
        let source = opened(&root);

        let listed = fs_list(source, c("/").as_ptr());
        let mut named: Vec<String> = listed
            .as_slice()
            .iter()
            .map(|entry| entry.name_string())
            .collect();
        named.sort();
        assert_eq!(named, vec!["inner".to_string(), "one.mp3".to_string()]);
        let folder = listed
            .as_slice()
            .iter()
            .find(|entry| entry.name_string() == "inner")
            .expect("the folder");
        assert_eq!(folder.is_dir, 1);
        HostSource::close(source);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A plugin is let into one directory, not into the machine.
    #[test]
    fn a_path_that_climbs_out_of_the_source_is_refused() {
        let root = a_directory("climbing");
        std::fs::write(root.join("inside.txt"), b"..").expect("a file");
        let source = opened(&root);

        assert!(fs_open(source, c("../../etc/passwd").as_ptr(), IC_OPEN_READ).is_null());
        assert!(fs_local_path(source, c("..").as_ptr()).is_null());
        assert_eq!(fs_list(source, c("../..").as_ptr()).count, 0);
        HostSource::close(source);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A source that was taken back answers nothing. Reading through it would
    /// otherwise be a use of freed memory — a crash of the application, not of
    /// the plugin that asked.
    #[test]
    fn a_source_the_host_has_taken_back_answers_nothing() {
        let root = a_directory("closed");
        std::fs::write(root.join("gone.txt"), b"..").expect("a file");
        let source = opened(&root);
        HostSource::close(source);

        assert_eq!(fs_caps(source), 0);
        assert!(fs_open(source, c("gone.txt").as_ptr(), IC_OPEN_READ).is_null());
        assert_eq!(fs_list(source, c("/").as_ptr()).count, 0);
        assert!(fs_local_path(source, c("gone.txt").as_ptr()).is_null());
        // Closing it twice is a plugin's mistake, not a double free.
        HostSource::close(source);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_is_there_has_a_path_on_this_machine() {
        let root = a_directory("local");
        std::fs::write(root.join("film.mkv"), b"..").expect("a file");
        let source = opened(&root);

        let answered = fs_local_path(source, c("film.mkv").as_ptr());
        assert!(!answered.is_null());
        let held = unsafe { CStr::from_ptr(answered) }
            .to_string_lossy()
            .into_owned();
        assert_eq!(PathBuf::from(held), root.join("film.mkv"));
        assert!(
            fs_local_path(source, c("nowhere.mkv").as_ptr()).is_null(),
            "a file that is not there has no path"
        );
        HostSource::close(source);
        let _ = std::fs::remove_dir_all(&root);
    }
}
