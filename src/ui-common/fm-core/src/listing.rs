//! What the application remembers about the contents of directories.
//!
//! One place, close to the filesystems rather than to the panels. A panel, a
//! tab, the terminal interface and the browser all reach the same memory, so
//! two of them looking at one directory cannot disagree about it, and walking
//! into a path does not read every folder along the way twice.
//!
//! It is kept beside the spawner and the icon provider — on the thread the
//! application runs its filesystems on — because every provider here is an
//! `Rc` and every call is `?Send`. Nothing in it is shared between threads,
//! and nothing needs to be.

use crate::rpc::{ColumnSpec, FileSystemRpc, RemoteFileEntry};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Whether an answer may come from memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// From memory when it is there. What walking a path and stepping back up
    /// want: the folder was read a moment ago and nothing has said otherwise.
    Remembered,
    /// From the filesystem, and what comes back replaces what was remembered.
    /// What a refresh, a copy's conflict check and anything that writes want.
    Fresh,
}

/// What was last seen in a directory, and whether it is still to be believed.
///
/// Being told that a directory changed does not throw the rows away: something
/// is drawing them, and an empty panel is a worse answer than one that is a
/// moment out of date. It marks them stale instead — they stay good enough to
/// paint, and the next time anybody *asks* for the directory it is read again.
struct Seen {
    rows: Rc<Vec<RemoteFileEntry>>,
    /// How this filesystem asked to be shown when it answered these rows.
    ///
    /// It belongs with the listing, not with the object that read it: a
    /// filesystem is built afresh on every walk down a path, and one that is
    /// handed its rows from here never opens the plugin behind it, so asking
    /// *it* gets the answers of a mount that has not been opened — no columns,
    /// and read-only.
    shown: Shown,
    stale: bool,
    /// When this directory was last looked at, so the ones nobody has been
    /// near go first when there is no more room.
    used: u64,
}

/// How much is worth keeping. A session that walks a large tree would
/// otherwise remember every folder it ever passed through until it closed.
/// Both limits are held at once: a few enormous directories cost as much as
/// many small ones.
const MOST_DIRECTORIES: usize = 256;
const MOST_ROWS: usize = 100_000;

thread_local! {
    static REMEMBERED: RefCell<HashMap<(String, String), Seen>> = RefCell::new(HashMap::new());
    static CLOCK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A number that only goes up, for saying which of two directories was looked
/// at more recently.
fn now() -> u64 {
    CLOCK.with(|clock| {
        let next = clock.get() + 1;
        clock.set(next);
        next
    })
}

/// Throws away the least recently looked at directories until what is left
/// fits. Called after something new is remembered, so the newest is never the
/// one thrown away.
fn make_room(held: &mut HashMap<(String, String), Seen>) {
    loop {
        let rows: usize = held.values().map(|seen| seen.rows.len()).sum();
        if held.len() <= MOST_DIRECTORIES && rows <= MOST_ROWS {
            return;
        }
        let Some(oldest) = held
            .iter()
            .min_by_key(|(_, seen)| seen.used)
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        held.remove(&oldest);
    }
}

/// What a filesystem said about itself when it answered a listing: the columns
/// it adds, whether they replace the panel's own, whether anything may be
/// written, and whether it wants the quick filter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown {
    pub columns: Vec<ColumnSpec>,
    pub columns_replace_defaults: bool,
    pub read_only: bool,
    pub wants_quick_filter: bool,
}

impl Shown {
    fn of(fs: &dyn FileSystemRpc) -> Shown {
        Shown {
            columns: fs.extra_columns(),
            columns_replace_defaults: fs.columns_replace_defaults(),
            read_only: fs.is_read_only(),
            wants_quick_filter: fs.wants_quick_filter(),
        }
    }
}

/// A directory is one path on one filesystem. The same server opened in two
/// tabs is one key; `/tmp` on the disk and `/tmp` on a server never are.
fn key(fs: &dyn FileSystemRpc, path: &str) -> Option<(String, String)> {
    let id = fs.fs_id();
    if id.is_empty() {
        return None;
    }
    Some((id, tidy(path)))
}

/// One spelling for one directory: no trailing slash, and the root is "/".
fn tidy(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// The contents of `path`, from memory or from the filesystem.
///
/// Only a listing that succeeded is remembered: a server that answered with an
/// error must not be remembered as an empty folder.
pub async fn list(
    fs: &Rc<dyn FileSystemRpc>,
    path: &str,
    freshness: Freshness,
) -> Result<Rc<Vec<RemoteFileEntry>>, common::AppError> {
    let key = key(fs.as_ref(), path);
    if freshness == Freshness::Remembered {
        if let Some(key) = &key {
            let stamp = now();
            let known = REMEMBERED.with(|held| {
                let mut held = held.borrow_mut();
                let seen = held.get_mut(key)?;
                if seen.stale {
                    return None;
                }
                seen.used = stamp;
                Some(seen.rows.clone())
            });
            if let Some(entries) = known {
                return Ok(entries);
            }
        }
    }
    let read = fs.list_dir(path.to_string()).await;
    let entries = match read {
        Ok(rows) => Rc::new(rows),
        Err(why) => {
            // We went to look and could not. What is remembered is no longer
            // something anybody should act on, but it is still the last thing
            // seen, so it stays drawable.
            if let Some(key) = &key {
                REMEMBERED.with(|held| {
                    if let Some(seen) = held.borrow_mut().get_mut(key) {
                        seen.stale = true;
                    }
                });
            }
            return Err(why);
        }
    };
    if let Some(key) = key {
        // Asked for after the listing, while the filesystem still knows.
        let shown = Shown::of(fs.as_ref());
        let stamp = now();
        REMEMBERED.with(|held| {
            let mut held = held.borrow_mut();
            held.insert(
                key,
                Seen {
                    rows: entries.clone(),
                    shown,
                    stale: false,
                    used: stamp,
                },
            );
            make_room(&mut held);
        });
    }
    Ok(entries)
}

/// What is remembered about a directory, without asking the filesystem.
///
/// For the parts of the application that are not allowed to wait: drawing a
/// panel, answering which rows are selected, filling a status bar. They ask
/// about a folder the application has just listed, and `None` means it has
/// not been listed yet rather than that it is empty.
pub fn remembered_of(fs: &dyn FileSystemRpc, path: &str) -> Option<Rc<Vec<RemoteFileEntry>>> {
    let key = key(fs, path)?;
    REMEMBERED.with(|held| held.borrow().get(&key).map(|seen| seen.rows.clone()))
}

/// How this directory asked to be shown, as it did when it was read.
///
/// `None` when nothing is remembered about it, and the caller must then ask
/// the filesystem itself.
pub fn shown_as(fs: &dyn FileSystemRpc, path: &str) -> Option<Shown> {
    let key = key(fs, path)?;
    REMEMBERED.with(|held| held.borrow().get(&key).map(|seen| seen.shown.clone()))
}

/// Whether this directory can be shown without asking the filesystem again.
/// False for one that has never been read and for one that has been told it
/// changed — the rows are still there to draw, but somebody must go and look.
pub fn is_remembered(fs: &dyn FileSystemRpc, path: &str) -> bool {
    let Some(key) = key(fs, path) else {
        return false;
    };
    REMEMBERED.with(|held| held.borrow().get(&key).is_some_and(|seen| !seen.stale))
}

/// Says that one directory is no longer what we remember — because the
/// application wrote to it, or the plugin behind it said so.
pub fn forget(fs: &dyn FileSystemRpc, path: &str) {
    let Some(key) = key(fs, path) else {
        return;
    };
    REMEMBERED.with(|held| {
        if let Some(seen) = held.borrow_mut().get_mut(&key) {
            seen.stale = true;
        }
    });
}

/// Says that a directory and everything under it is no longer what we
/// remember: a folder that was deleted takes its children with it, and a tree
/// copied into a folder changes what is below it as well as the folder itself.
pub fn forget_within(fs: &dyn FileSystemRpc, path: &str) {
    let Some((id, here)) = key(fs, path) else {
        return;
    };
    let below = if here == "/" {
        "/".to_string()
    } else {
        format!("{here}/")
    };
    REMEMBERED.with(|held| {
        for ((other, at), seen) in held.borrow_mut().iter_mut() {
            if *other == id && (*at == here || at.starts_with(&below)) {
                seen.stale = true;
            }
        }
    });
}

/// The same as `forget_within`, for a filesystem named rather than held.
///
/// A plugin says what it changed through the source it was let into, and what
/// arrives is the name of that filesystem and a path on it — there is no
/// provider object at that point, and there need not be: two panels looking at
/// the same server share one memory, filed under the name.
pub fn forget_id_within(fs_id: &str, path: &str) {
    if fs_id.is_empty() {
        return;
    }
    let here = tidy(path);
    let below = if here == "/" {
        "/".to_string()
    } else {
        format!("{here}/")
    };
    REMEMBERED.with(|held| {
        for ((other, at), seen) in held.borrow_mut().iter_mut() {
            if other == fs_id && (*at == here || at.starts_with(&below)) {
                seen.stale = true;
            }
        }
    });
}

/// The same for a whole filesystem and everything mounted inside it: a mount
/// inside a file carries the name of what holds it, so one server going stale
/// takes the archives on it with it.
pub fn forget_below(fs_id: &str) {
    if fs_id.is_empty() {
        return;
    }
    REMEMBERED.with(|held| {
        for ((id, _), seen) in held.borrow_mut().iter_mut() {
            if id == fs_id || id.starts_with(&format!("{fs_id}/")) {
                seen.stale = true;
            }
        }
    });
}

/// Forgets every directory inside the filesystems a plugin has named.
///
/// This is what a plugin means when it says its filesystem moved on: a torrent
/// whose files arrived, an archive something was written into, a peer that
/// added a file to a share. It names either
///
/// - an **extension**, like `.zip` — then everything opened through a file of
///   that name is forgotten, because a mount is named after the file that
///   holds it; or
/// - a **connection kind**, like `node-in-net` — then every mount of that kind
///   is forgotten. A connection is not reached through a file and so has no
///   extension to be named by, which used to leave it with no way of saying
///   anything at all.
pub fn forget_mounts_of(names: &[String]) {
    if names.is_empty() {
        return;
    }
    REMEMBERED.with(|held| {
        for ((id, _), seen) in held.borrow_mut().iter_mut() {
            if names.iter().any(|name| names_this(id, name)) {
                seen.stale = true;
            }
        }
    });
}

/// Whether a filesystem's name is one the plugin just named.
fn names_this(id: &str, name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    if name.starts_with('.') {
        return id.split('/').any(|part| ends_with_extension(part, name));
    }
    // A connection, and everything mounted inside one: `conn:<kind>:<...>`.
    id.starts_with(&format!("conn:{name}:"))
}

/// Whether this part of a name is a file of that extension, matched whole:
/// `.zip` matches `books.zip` and not `books.zipped`.
fn ends_with_extension(part: &str, extension: &str) -> bool {
    let extension = extension.trim();
    if extension.is_empty() {
        return false;
    }
    let lowered = part.to_lowercase();
    let wanted = extension.to_lowercase();
    lowered.len() > wanted.len() && lowered.ends_with(&wanted)
}

/// Everything, for a test or for a frontend starting over.
pub fn forget_everything() {
    REMEMBERED.with(|held| held.borrow_mut().clear());
}

/// How many directories are remembered, stale ones included. For tests and
/// for saying so in a log.
pub fn remembered() -> usize {
    REMEMBERED.with(|held| held.borrow().len())
}

/// How many of them would be read again if they were asked for.
pub fn stale() -> usize {
    REMEMBERED.with(|held| held.borrow().values().filter(|seen| seen.stale).count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Counting {
        id: String,
        reads: std::cell::Cell<usize>,
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Counting {
        async fn list_dir(&self, path: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
            self.reads.set(self.reads.get() + 1);
            Ok(vec![RemoteFileEntry {
                name: format!("in{path}"),
                is_dir: false,
                size: 0,
                modified: 0,
                permissions: None,
                extra: Vec::new(),
            }])
        }
        fn fs_id(&self) -> String {
            self.id.clone()
        }
    }

    fn counting(id: &str) -> Rc<Counting> {
        Rc::new(Counting {
            id: id.to_string(),
            reads: std::cell::Cell::new(0),
        })
    }

    #[tokio::test]
    async fn a_directory_read_once_is_not_read_again() {
        forget_everything();
        let fs = counting("disk");
        let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
        list(&as_trait, "/a", Freshness::Fresh).await.expect("read");
        list(&as_trait, "/a", Freshness::Remembered)
            .await
            .expect("remembered");
        assert_eq!(
            fs.reads.get(),
            1,
            "the second answer came from the filesystem"
        );
    }

    #[tokio::test]
    async fn asking_for_fresh_goes_past_what_is_remembered() {
        forget_everything();
        let fs = counting("disk");
        let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
        list(&as_trait, "/a", Freshness::Fresh).await.expect("read");
        list(&as_trait, "/a", Freshness::Fresh).await.expect("read");
        assert_eq!(fs.reads.get(), 2);
    }

    /// The whole reason the key carries the filesystem: a path is only a name
    /// inside one of them.
    #[tokio::test]
    async fn the_same_path_on_two_filesystems_is_two_directories() {
        forget_everything();
        let disk = counting("disk");
        let server = counting("conn:sftp:1");
        let disk_t: Rc<dyn FileSystemRpc> = disk.clone();
        let server_t: Rc<dyn FileSystemRpc> = server.clone();
        list(&disk_t, "/tmp", Freshness::Fresh).await.expect("read");
        list(&server_t, "/tmp", Freshness::Remembered)
            .await
            .expect("read");
        assert_eq!(server.reads.get(), 1, "it answered with the disk's /tmp");
    }

    #[tokio::test]
    async fn a_filesystem_with_no_name_is_never_remembered() {
        forget_everything();
        let nameless = counting("");
        let as_trait: Rc<dyn FileSystemRpc> = nameless.clone();
        list(&as_trait, "/a", Freshness::Fresh).await.expect("read");
        list(&as_trait, "/a", Freshness::Remembered)
            .await
            .expect("read");
        assert_eq!(nameless.reads.get(), 2);
        assert_eq!(remembered(), 0);
    }

    #[tokio::test]
    async fn forgetting_one_directory_leaves_its_neighbours_alone() {
        forget_everything();
        let fs = counting("disk");
        let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
        list(&as_trait, "/a", Freshness::Fresh).await.expect("read");
        list(&as_trait, "/b", Freshness::Fresh).await.expect("read");
        forget(as_trait.as_ref(), "/a");
        list(&as_trait, "/a", Freshness::Remembered)
            .await
            .expect("read");
        list(&as_trait, "/b", Freshness::Remembered)
            .await
            .expect("read");
        assert_eq!(fs.reads.get(), 3, "only /a should have been read again");
        assert_eq!(stale(), 0, "reading it again makes it current");
    }

    /// An archive is named after the file that holds it, so a server going
    /// stale takes what is mounted inside it along.
    #[tokio::test]
    async fn forgetting_a_filesystem_forgets_what_is_mounted_inside_it() {
        forget_everything();
        let server = counting("conn:sftp:1");
        let inside = counting("conn:sftp:1/docs/notes.zip");
        let elsewhere = counting("local");
        let server_t: Rc<dyn FileSystemRpc> = server.clone();
        let inside_t: Rc<dyn FileSystemRpc> = inside.clone();
        let elsewhere_t: Rc<dyn FileSystemRpc> = elsewhere.clone();
        list(&server_t, "/docs", Freshness::Fresh)
            .await
            .expect("read");
        list(&inside_t, "/", Freshness::Fresh).await.expect("read");
        list(&elsewhere_t, "/", Freshness::Fresh)
            .await
            .expect("read");

        forget_below("conn:sftp:1");
        assert!(!is_remembered(server_t.as_ref(), "/docs"));
        assert!(
            !is_remembered(inside_t.as_ref(), "/"),
            "an archive on that server goes stale with it"
        );
        assert!(
            is_remembered(elsewhere_t.as_ref(), "/"),
            "the local disk is nothing to do with it"
        );
    }

    #[tokio::test]
    async fn a_trailing_slash_is_the_same_directory() {
        forget_everything();
        let fs = counting("disk");
        let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
        list(&as_trait, "/a", Freshness::Fresh).await.expect("read");
        list(&as_trait, "/a/", Freshness::Remembered)
            .await
            .expect("read");
        assert_eq!(fs.reads.get(), 1);
    }

    /// When a plugin says its kind of filesystem moved on, everything opened
    /// through a file of that name is forgotten — in every panel and every
    /// frontend at once, because there is only one memory to forget.
    #[tokio::test]
    async fn a_plugin_saying_its_filesystem_moved_on_empties_those_mounts() {
        forget_everything();
        let archive = counting("local/books.zip");
        let deeper = counting("local/books.zip/chapters");
        let torrent = counting("local/film.torrent");
        let disk = counting("local");
        for fs in [&archive, &deeper, &torrent, &disk] {
            let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
            list(&as_trait, "/", Freshness::Fresh).await.expect("read");
        }
        assert_eq!(remembered(), 4);

        forget_mounts_of(&[".zip".to_string()]);
        assert_eq!(stale(), 2, "the archive and what is inside it");
        assert!(
            !is_remembered(archive.as_ref(), "/"),
            "it must be read again"
        );
        assert!(
            remembered_of(archive.as_ref(), "/").is_some(),
            "and it must still be drawable in the meantime"
        );
        assert!(is_remembered(disk.as_ref(), "/"), "the disk is untouched");

        forget_mounts_of(&[".torrent".to_string()]);
        assert_eq!(stale(), 3);
        assert!(is_remembered(disk.as_ref(), "/"));
    }

    /// A connection has no extension to be named by, so it names its kind.
    /// Before this there was no way for a peer-to-peer share or a server to
    /// say that anything had changed.
    #[tokio::test]
    async fn a_connection_can_say_that_its_own_filesystem_moved_on() {
        forget_everything();
        let share = counting("conn:node-in-net:00ff");
        let inside = counting("conn:node-in-net:00ff/backup.zip");
        let other_share = counting("conn:sftp:1234");
        let disk = counting("local");
        for fs in [&share, &inside, &other_share, &disk] {
            let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
            list(&as_trait, "/", Freshness::Fresh).await.expect("read");
        }

        forget_mounts_of(&["node-in-net".to_string()]);
        assert!(!is_remembered(share.as_ref(), "/"));
        assert!(
            !is_remembered(inside.as_ref(), "/"),
            "and what is inside it"
        );
        assert!(
            is_remembered(other_share.as_ref(), "/"),
            "a different server"
        );
        assert!(is_remembered(disk.as_ref(), "/"));

        forget_mounts_of(&["sftp".to_string()]);
        assert!(!is_remembered(other_share.as_ref(), "/"));
        assert!(is_remembered(disk.as_ref(), "/"));
    }

    /// A folder called `books.zipped` is not an archive.
    #[tokio::test]
    async fn a_longer_name_that_merely_ends_the_same_way_is_left_alone() {
        forget_everything();
        let nearly = counting("local/books.zipped");
        let as_trait: Rc<dyn FileSystemRpc> = nearly.clone();
        list(&as_trait, "/", Freshness::Fresh).await.expect("read");
        forget_mounts_of(&[".zip".to_string()]);
        assert!(
            is_remembered(nearly.as_ref(), "/"),
            "a folder is not an archive"
        );
    }

    /// Being told that a directory changed must not blank it: a frontend that
    /// does not re-read of its own accord — the browser, the terminal one —
    /// would draw an empty folder, which is a worse answer than one that is a
    /// moment out of date.
    #[tokio::test]
    async fn a_directory_told_it_changed_is_still_drawable_until_it_is_read_again() {
        forget_everything();
        let fs = counting("disk");
        let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
        list(&as_trait, "/a", Freshness::Fresh).await.expect("read");

        forget(as_trait.as_ref(), "/a");
        let still_there = remembered_of(as_trait.as_ref(), "/a").expect("rows to draw");
        assert_eq!(
            still_there.len(),
            1,
            "the panel was left with nothing to show"
        );
        assert!(
            !is_remembered(as_trait.as_ref(), "/a"),
            "but it must be read again"
        );

        // And asking does read it again, rather than handing back the old rows.
        list(&as_trait, "/a", Freshness::Remembered)
            .await
            .expect("read");
        assert_eq!(fs.reads.get(), 2);
        assert!(is_remembered(as_trait.as_ref(), "/a"));
    }

    /// A folder that was deleted takes its children with it, and a tree copied
    /// into a folder changes what is below it as well. Forgetting one folder
    /// on its own used to leave the application sure about children that were
    /// no longer there.
    #[tokio::test]
    async fn forgetting_a_folder_takes_everything_inside_it() {
        forget_everything();
        let fs = counting("disk");
        let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
        for path in ["/x", "/x/sub", "/x/sub/deeper", "/xenon", "/y"] {
            list(&as_trait, path, Freshness::Fresh).await.expect("read");
        }

        forget_within(as_trait.as_ref(), "/x");
        assert!(!is_remembered(as_trait.as_ref(), "/x"));
        assert!(!is_remembered(as_trait.as_ref(), "/x/sub"));
        assert!(!is_remembered(as_trait.as_ref(), "/x/sub/deeper"));
        assert!(
            is_remembered(as_trait.as_ref(), "/xenon"),
            "a folder whose name merely begins the same way is a different folder"
        );
        assert!(is_remembered(as_trait.as_ref(), "/y"));
    }

    /// Forgetting the root is forgetting the whole of that filesystem.
    #[tokio::test]
    async fn forgetting_the_root_takes_the_whole_filesystem() {
        forget_everything();
        let fs = counting("disk");
        let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
        for path in ["/", "/a", "/a/b"] {
            list(&as_trait, path, Freshness::Fresh).await.expect("read");
        }
        forget_within(as_trait.as_ref(), "/");
        assert_eq!(stale(), 3);
    }

    /// Going to look and failing says the old answer is no longer to be acted
    /// on — a server that stopped answering must not leave the application
    /// certain about what it holds.
    #[tokio::test]
    async fn a_read_that_failed_stops_the_old_answer_being_trusted() {
        struct Fickle {
            fail: std::cell::Cell<bool>,
        }
        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for Fickle {
            async fn list_dir(&self, _: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
                if self.fail.get() {
                    return Err(common::AppError::Other("the server went away".into()));
                }
                Ok(vec![RemoteFileEntry {
                    name: "was-here".to_string(),
                    is_dir: false,
                    size: 0,
                    modified: 0,
                    permissions: None,
                    extra: Vec::new(),
                }])
            }
            fn fs_id(&self) -> String {
                "conn:sftp:3".to_string()
            }
        }
        forget_everything();
        let fickle = Rc::new(Fickle {
            fail: std::cell::Cell::new(false),
        });
        let fs: Rc<dyn FileSystemRpc> = fickle.clone();
        list(&fs, "/docs", Freshness::Fresh).await.expect("read");
        assert!(is_remembered(fs.as_ref(), "/docs"));

        // Now it stops answering.
        fickle.fail.set(true);
        assert!(list(&fs, "/docs", Freshness::Fresh).await.is_err());
        assert!(
            !is_remembered(fs.as_ref(), "/docs"),
            "the application went on trusting a listing it had just failed to confirm"
        );
        assert!(
            remembered_of(fs.as_ref(), "/docs").is_some(),
            "and it should still have the last thing it saw to draw"
        );
    }

    /// A session that walks a large tree must not remember all of it. The
    /// folders nobody has been near for longest go first, and the one just
    /// read is never the one thrown away.
    #[tokio::test]
    async fn the_oldest_directories_are_let_go_when_there_is_no_more_room() {
        forget_everything();
        let fs = counting("disk");
        let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
        for at in 0..(super::MOST_DIRECTORIES + 20) {
            list(&as_trait, &format!("/f{at}"), Freshness::Fresh)
                .await
                .expect("read");
        }
        assert_eq!(remembered(), super::MOST_DIRECTORIES);
        assert!(
            is_remembered(
                as_trait.as_ref(),
                &format!("/f{}", super::MOST_DIRECTORIES + 19)
            ),
            "the newest should be the last to go, not the first"
        );
        assert!(
            !is_remembered(as_trait.as_ref(), "/f0"),
            "the oldest should have been let go"
        );
    }

    /// Looking at a folder again keeps it: what counts is when it was last
    /// wanted, not when it was first read.
    #[tokio::test]
    async fn a_directory_that_keeps_being_looked_at_is_kept() {
        forget_everything();
        let fs = counting("disk");
        let as_trait: Rc<dyn FileSystemRpc> = fs.clone();
        list(&as_trait, "/old", Freshness::Fresh)
            .await
            .expect("read");
        for at in 0..(super::MOST_DIRECTORIES + 20) {
            list(&as_trait, &format!("/f{at}"), Freshness::Fresh)
                .await
                .expect("read");
            // Kept in use all the while.
            list(&as_trait, "/old", Freshness::Remembered)
                .await
                .expect("read");
        }
        assert!(
            is_remembered(as_trait.as_ref(), "/old"),
            "a folder in constant use was thrown away"
        );
    }

    /// A few enormous directories cost as much room as many small ones.
    #[tokio::test]
    async fn a_handful_of_huge_directories_is_bounded_too() {
        struct Huge;
        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for Huge {
            async fn list_dir(&self, _: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
                Ok((0..30_000)
                    .map(|at| RemoteFileEntry {
                        name: format!("f{at}"),
                        is_dir: false,
                        size: 0,
                        modified: 0,
                        permissions: None,
                        extra: Vec::new(),
                    })
                    .collect())
            }
            fn fs_id(&self) -> String {
                "huge".to_string()
            }
        }
        forget_everything();
        let fs: Rc<dyn FileSystemRpc> = Rc::new(Huge);
        for at in 0..8 {
            list(&fs, &format!("/big{at}"), Freshness::Fresh)
                .await
                .expect("read");
        }
        assert!(
            remembered() <= 4,
            "four directories of thirty thousand rows is already the limit, kept {}",
            remembered()
        );
    }

    /// A listing that failed says nothing about the directory, and must not be
    /// remembered as an empty one.
    #[tokio::test]
    async fn a_filesystem_that_refused_is_not_remembered_as_empty() {
        struct Refusing;
        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for Refusing {
            async fn list_dir(&self, _: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
                Err(common::AppError::Other("the server went away".into()))
            }
            fn fs_id(&self) -> String {
                "conn:sftp:2".to_string()
            }
        }
        forget_everything();
        let fs: Rc<dyn FileSystemRpc> = Rc::new(Refusing);
        assert!(list(&fs, "/a", Freshness::Fresh).await.is_err());
        assert_eq!(remembered(), 0);
    }
}
