pub mod nav;
pub mod provider;
pub mod state;

pub use fm_core::rpc::PathSegment;
pub use provider::RoutingProvider;
pub use state::{History, RouterState};

pub fn parse_path_to_segments(path: &str) -> Vec<PathSegment> {
    let parts = fm_core::path::split_path(path);
    let mut result = Vec::new();
    for i in 0..parts.len() {
        result.push(PathSegment {
            name: parts[i].to_string(),
            path: fm_core::path::join_segment_names(&parts[..=i]),
        });
    }
    result
}

pub fn build_segments_to_path(segments: &[PathSegment]) -> String {
    let names: Vec<&str> = segments.iter().map(|s| s.name.as_str()).collect();
    fm_core::path::join_segment_names(&names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_core::rpc::FileSystemRpc;
    use std::rc::Rc;

    struct MockRpc;
    #[async_trait::async_trait(?Send)]
    impl fm_core::rpc::FileSystemRpc for MockRpc {}

    /// A level that brings columns of its own, the way an opened torrent does.
    struct ColumnedRpc;
    #[async_trait::async_trait(?Send)]
    impl fm_core::rpc::FileSystemRpc for ColumnedRpc {
        fn extra_columns(&self) -> Vec<fm_core::rpc::ColumnSpec> {
            vec![fm_core::rpc::ColumnSpec {
                key: "status".to_string(),
                title: "Status".to_string(),
                width: Some(120),
                kind: fm_core::rpc::ColumnKind::Text,
            }]
        }
    }

    #[test]
    fn the_columns_come_from_the_level_being_stood_in_and_leave_with_it() {
        let base: Rc<dyn FileSystemRpc> = Rc::new(MockRpc);
        let mut nav = NavPath::new(PathLevel::new("root", "/", base));
        assert!(nav.active().fs.extra_columns().is_empty());
        nav.push(PathLevel::new("a.torrent", "/", Rc::new(ColumnedRpc)));
        assert_eq!(nav.active().fs.extra_columns().len(), 1);
        assert_eq!(nav.active().fs.extra_columns()[0].key, "status");
        assert!(!nav.active().fs.columns_replace_defaults());
        assert!(nav.pop());
        assert!(
            nav.active().fs.extra_columns().is_empty(),
            "stepping back out takes the plugin's columns with it"
        );
    }

    #[test]
    fn test_parse_and_build_path() {
        let path = "/home/user/archive.zip";
        let segs = parse_path_to_segments(path);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].name, "home");
        assert_eq!(segs[1].name, "user");
        assert_eq!(segs[2].name, "archive.zip");

        let reconstructed = build_segments_to_path(&segs);
        assert_eq!(reconstructed, "/home/user/archive.zip");
    }

    use crate::nav::{build_levels, NavPath, PathLevel};

    #[test]
    fn build_levels_local_path() {
        let base: Rc<dyn FileSystemRpc> = Rc::new(MockRpc);
        let levels = build_levels(&parse_path_to_segments("/home/user"), base.clone());
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0].relative_path, "/");
        assert_eq!(levels[1].name, "home");
        assert_eq!(levels[1].relative_path, "/home");
        assert_eq!(levels[2].name, "user");
        assert_eq!(levels[2].relative_path, "/home/user");
        assert!(Rc::ptr_eq(&levels[2].fs, &base));
    }

    #[test]
    fn build_levels_root_only() {
        let base: Rc<dyn FileSystemRpc> = Rc::new(MockRpc);
        let levels = build_levels(&[], base.clone());
        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0].relative_path, "/");
        assert!(Rc::ptr_eq(&levels[0].fs, &base));
    }

    fn register_stub_archives() -> fm_core::plugin_fs::RegistryLease {
        use std::os::raw::{c_char, c_int, c_void};
        extern "C" fn open_in(
            _: ic_plugin_api::IcFsSource,
            _: *const c_char,
            _: *mut c_void,
        ) -> ic_plugin_api::IcFsHandle {
            1usize as ic_plugin_api::IcFsHandle
        }
        extern "C" fn close(_: ic_plugin_api::IcFsHandle) {}
        extern "C" fn list(
            _: ic_plugin_api::IcFsHandle,
            _: *const c_char,
        ) -> ic_plugin_api::IcListing {
            ic_plugin_api::IcListing::EMPTY
        }
        extern "C" fn read(
            _: ic_plugin_api::IcFsHandle,
            _: *const c_char,
        ) -> ic_plugin_api::IcBytes {
            ic_plugin_api::IcBytes::EMPTY
        }
        extern "C" fn read_only(_: ic_plugin_api::IcFsHandle) -> c_int {
            1
        }
        extern "C" fn last_error(_: ic_plugin_api::IcFsHandle) -> *const c_char {
            std::ptr::null()
        }
        let table = ic_plugin_api::IcFsVTable {
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
        };
        let exts = std::ffi::CString::new(".zip,.tar,.tar.gz,.tgz,.tar.bz2,.tbz2,.tbz").unwrap();
        let lease = fm_core::plugin_fs::lease_registry_for_test();
        fm_core::plugin_fs::register(exts.as_ptr(), &table, std::ptr::null_mut());
        lease
    }

    #[test]
    fn build_levels_archive_switches_provider() {
        let _lease = register_stub_archives();
        let base: Rc<dyn FileSystemRpc> = Rc::new(MockRpc);
        let levels = build_levels(&parse_path_to_segments("/docs/a.zip/inner"), base.clone());
        assert_eq!(levels.len(), 4);
        assert_eq!(levels[1].name, "docs");
        assert!(Rc::ptr_eq(&levels[1].fs, &base));
        assert_eq!(levels[2].name, "a.zip");
        assert_eq!(levels[2].relative_path, "/");
        assert!(!Rc::ptr_eq(&levels[2].fs, &base));
        assert_eq!(levels[3].name, "inner");
        assert_eq!(levels[3].relative_path, "/inner");
        assert!(Rc::ptr_eq(&levels[3].fs, &levels[2].fs));
    }

    #[test]
    fn build_levels_nested_archive() {
        let _lease = register_stub_archives();
        let base: Rc<dyn FileSystemRpc> = Rc::new(MockRpc);
        let levels = build_levels(&parse_path_to_segments("/a.zip/b.tar.gz/x"), base.clone());
        assert_eq!(levels.len(), 4);
        assert_eq!(levels[1].name, "a.zip");
        assert_eq!(levels[1].relative_path, "/");
        assert_eq!(levels[2].name, "b.tar.gz");
        assert_eq!(levels[2].relative_path, "/");
        assert!(!Rc::ptr_eq(&levels[2].fs, &levels[1].fs));
        assert_eq!(levels[3].name, "x");
        assert_eq!(levels[3].relative_path, "/x");
        assert!(Rc::ptr_eq(&levels[3].fs, &levels[2].fs));
    }

    #[test]
    fn navpath_push_pop_truncate() {
        let base: Rc<dyn FileSystemRpc> = Rc::new(MockRpc);
        let mut p = NavPath::new(PathLevel::new("root", "/", base.clone()));
        assert_eq!(p.depth(), 1);
        p.push(PathLevel::new("a", "/a", base.clone()));
        p.push(PathLevel::new("b", "/a/b", base.clone()));
        assert_eq!(p.depth(), 3);
        assert_eq!(p.active().name, "b");

        p.truncate_to(1);
        assert_eq!(p.depth(), 2);
        assert_eq!(p.active().name, "a");

        assert!(p.pop());
        assert_eq!(p.active().name, "root");
        assert!(!p.pop());
        assert_eq!(p.depth(), 1);
    }

    #[test]
    fn empty_string_returns_no_segments() {
        assert!(parse_path_to_segments("").is_empty());
    }

    #[test]
    fn root_slash_returns_no_segments() {
        assert!(parse_path_to_segments("/").is_empty());
    }

    #[test]
    fn trailing_slash_same_as_without() {
        assert_eq!(
            parse_path_to_segments("/home/user/"),
            parse_path_to_segments("/home/user")
        );
    }

    #[test]
    fn double_slashes_are_collapsed() {
        let segs = parse_path_to_segments("//home//user");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].name, "home");
        assert_eq!(segs[1].name, "user");
    }

    #[test]
    fn backslash_separator_also_splits() {
        let segs = parse_path_to_segments("home\\user\\docs");
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].name, "home");
        assert_eq!(segs[1].name, "user");
        assert_eq!(segs[2].name, "docs");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn cumulative_paths_are_correct_unix() {
        let segs = parse_path_to_segments("/a/b/c");
        assert_eq!(segs[0].path, "/a");
        assert_eq!(segs[1].path, "/a/b");
        assert_eq!(segs[2].path, "/a/b/c");
    }

    #[test]
    fn build_empty_slice_returns_root() {
        assert_eq!(build_segments_to_path(&[]), "/");
    }

    #[test]
    fn router_state_reset_to_base_collapses_to_root() {
        let provider = Rc::new(MockRpc);
        let state = RouterState::new(provider.clone(), provider.clone(), "/".to_string());

        state
            .path
            .borrow_mut()
            .push(PathLevel::new("a", "/a", provider.clone()));
        assert_eq!(state.path.borrow().depth(), 2);

        state.reset_to_base();
        assert_eq!(state.path.borrow().depth(), 1);
    }

    #[test]
    fn router_state_history_records_navpath_snapshots() {
        let p = Rc::new(MockRpc);
        let state = RouterState::new(p.clone(), p.clone(), "/".to_string());

        assert!(!state.can_go_back());
        assert!(!state.can_go_forward());

        state.record_history_snapshot();
        state
            .path
            .borrow_mut()
            .push(PathLevel::new("a", "/a", p.clone()));
        state.record_history_snapshot();
        state
            .path
            .borrow_mut()
            .push(PathLevel::new("b", "/a/b", p.clone()));
        state.record_history_snapshot();

        assert!(state.can_go_back());
        assert!(!state.can_go_forward());

        let prev = state.history.borrow_mut().back().unwrap();
        assert_eq!(prev.absolute_path(), "/a");
        assert!(state.can_go_forward());

        let prev2 = state.history.borrow_mut().back().unwrap();
        assert_eq!(prev2.absolute_path(), "/");

        let next = state.history.borrow_mut().forward().unwrap();
        assert_eq!(next.absolute_path(), "/a");
    }

    #[test]
    fn record_history_snapshot_dedups_same_path() {
        let p = Rc::new(MockRpc);
        let state = RouterState::new(p.clone(), p.clone(), "/".to_string());

        state
            .path
            .borrow_mut()
            .push(PathLevel::new("a", "/a", p.clone()));
        state.record_history_snapshot();
        state.record_history_snapshot();
        assert_eq!(state.history.borrow().snapshots.len(), 1);
        assert!(!state.can_go_forward());

        state
            .path
            .borrow_mut()
            .push(PathLevel::new("b", "/a/b", p.clone()));
        state.record_history_snapshot();
        assert_eq!(state.history.borrow().snapshots.len(), 2);
        assert!(state.can_go_back());
    }

    #[test]
    fn new_navigation_after_back_truncates_forward() {
        let p = Rc::new(MockRpc);
        let state = RouterState::new(p.clone(), p.clone(), "/".to_string());

        state.record_history_snapshot();
        state
            .path
            .borrow_mut()
            .push(PathLevel::new("a", "/a", p.clone()));
        state.record_history_snapshot();
        state
            .path
            .borrow_mut()
            .push(PathLevel::new("b", "/a/b", p.clone()));
        state.record_history_snapshot();

        let back = state.history.borrow_mut().back().unwrap();
        *state.path.borrow_mut() = back;
        assert!(state.can_go_forward());

        state
            .path
            .borrow_mut()
            .push(PathLevel::new("c", "/a/c", p.clone()));
        state.record_history_snapshot();
        assert!(
            !state.can_go_forward(),
            "new nav after back must drop the forward branch"
        );
        assert_eq!(state.history.borrow().snapshots.len(), 3);
    }

    #[test]
    fn breadcrumb_segments_follow_nav_path_after_enter() {
        let p = Rc::new(MockRpc);
        let state = RouterState::new(p.clone(), p.clone(), "/".to_string());

        state.path.borrow_mut().push(crate::nav::PathLevel::new(
            "folderA",
            "/folderA",
            Rc::new(MockRpc),
        ));
        state.path.borrow_mut().push(crate::nav::PathLevel::new(
            "sub",
            "/folderA/sub",
            Rc::new(MockRpc),
        ));

        let names: Vec<String> = state
            .breadcrumb_segments()
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(
            names,
            vec!["folderA".to_string(), "sub".to_string()],
            "breadcrumbs must reflect NavPath, not the stale `stack`"
        );
    }

    #[test]
    fn breadcrumb_segments_shrink_after_truncate() {
        let p = Rc::new(MockRpc);
        let state = RouterState::new(p.clone(), p.clone(), "/".to_string());
        {
            let mut path = state.path.borrow_mut();
            path.push(crate::nav::PathLevel::new("a", "/a", Rc::new(MockRpc)));
            path.push(crate::nav::PathLevel::new("b", "/a/b", Rc::new(MockRpc)));
            path.push(crate::nav::PathLevel::new("c", "/a/b/c", Rc::new(MockRpc)));
        }
        state.path.borrow_mut().truncate_to(1);

        let names: Vec<String> = state
            .breadcrumb_segments()
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(
            names,
            vec!["a".to_string()],
            "jump to 'a' must drop deeper crumbs"
        );
    }

    #[test]
    fn active_provider_follows_navpath() {
        let base = Rc::new(MockRpc);
        let state = RouterState::new(base.clone(), base.clone(), "/".to_string());
        let archive: Rc<dyn FileSystemRpc> = Rc::new(MockRpc);

        state
            .path
            .borrow_mut()
            .push(crate::nav::PathLevel::new("a.zip", "/", archive.clone()));

        assert!(Rc::ptr_eq(&state.active_provider(), &archive));
    }

    #[test]
    fn resolve_relative_local_is_identity() {
        let p = Rc::new(MockRpc);
        let state = RouterState::new(p.clone(), p.clone(), "/".to_string());
        {
            let mut path = state.path.borrow_mut();
            path.push(crate::nav::PathLevel::new(
                "tests",
                "/tests",
                Rc::new(MockRpc),
            ));
            path.push(crate::nav::PathLevel::new(
                "foo",
                "/tests/foo",
                Rc::new(MockRpc),
            ));
        }
        assert_eq!(
            state.resolve_relative("/tests/foo/file.txt"),
            "/tests/foo/file.txt"
        );
        assert_eq!(state.resolve_relative("/tests/foo"), "/tests/foo");
    }

    #[test]
    fn resolve_relative_strips_archive_mount() {
        let p = Rc::new(MockRpc);
        let state = RouterState::new(p.clone(), p.clone(), "/".to_string());
        let archive = Rc::new(MockRpc);
        {
            let mut path = state.path.borrow_mut();
            path.push(crate::nav::PathLevel::new("a.zip", "/", archive.clone()));
            path.push(crate::nav::PathLevel::new(
                "inner",
                "/inner",
                archive.clone(),
            ));
        }
        assert_eq!(state.resolve_relative("/a.zip/inner/file"), "/inner/file");
        assert_eq!(state.resolve_relative("/a.zip/inner"), "/inner");
    }

    fn segments(names: &[&str]) -> Vec<PathSegment> {
        names
            .iter()
            .map(|n| PathSegment {
                name: (*n).to_string(),
                path: String::new(),
            })
            .collect()
    }

    // Build the fixture with the production functions; hand-written prefixes hide the case where
    // absolute_path() and the level relative_path disagree.
    fn resolve_relative_round_trip(dir_names: &[&str], file: &str) -> (String, String) {
        let base: Rc<dyn FileSystemRpc> = Rc::new(MockRpc);
        let state = RouterState::new(base.clone(), base.clone(), "/".to_string());
        *state.path.borrow_mut() = crate::nav::NavPath::from_levels(
            crate::nav::build_levels(&segments(dir_names), base.clone()),
            base,
        );

        let display = state.path.borrow().absolute_path();
        let mut parts = fm_core::path::split_joined(&display);
        parts.push(file);
        let entry_path = fm_core::path::join_segment_names(&parts);

        let mut expected = dir_names.to_vec();
        expected.push(file);
        (
            state.resolve_relative(&entry_path),
            build_segments_to_path(&segments(&expected)),
        )
    }

    #[test]
    fn resolve_relative_returns_the_level_relative_path() {
        let (got, want) = resolve_relative_round_trip(&["home", "me", "docs"], "file.txt");
        assert_eq!(got, want);
    }

    #[test]
    fn resolve_relative_of_a_drive_rooted_path_is_usable_by_the_local_fs() {
        let (got, want) = resolve_relative_round_trip(&["C:", "msys64", "home"], "file.txt");
        assert_eq!(got, want);
    }

    // Stepping into a child must land on the same relative path that navigating to the full
    // address would build, or the two entry points leave the panel in different states.
    #[test]
    fn entering_a_child_matches_what_address_navigation_builds() {
        for (parent, child) in [
            (vec!["home", "me"], "docs"),
            (vec![], "home"),
            (vec![], "C:"),
            (vec!["C:", "msys64"], "home"),
        ] {
            let parent_rel = build_segments_to_path(&segments(&parent));
            let mut parts = fm_core::path::split_joined(&parent_rel);
            parts.push(child);
            let entered = fm_core::path::join_segment_names(&parts);

            let mut all = parent.clone();
            all.push(child);
            assert_eq!(
                entered,
                build_segments_to_path(&segments(&all)),
                "{parent:?} + {child:?}"
            );
        }
    }
}

#[cfg(test)]
mod walking_a_typed_path {
    use super::*;
    use fm_core::rpc::{FileSystemRpc, RemoteFileEntry};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A filesystem that writes down every directory it was asked to list.
    struct Counting {
        reads: RefCell<Vec<String>>,
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Counting {
        async fn list_dir(&self, path: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
            self.reads.borrow_mut().push(path.clone());
            Ok(vec![RemoteFileEntry {
                name: "child".to_string(),
                is_dir: true,
                size: 0,
                modified: 0,
                permissions: None,
                extra: Vec::new(),
            }])
        }
        fn fs_id(&self) -> String {
            "counting".to_string()
        }
    }

    /// Typing a path used to read every folder along it, and then read the one
    /// at the end a second time — the walk that checks the path exists and the
    /// listing that shows it did not know about each other. They share one
    /// memory now, so the folder the panel lands in is read once.
    #[tokio::test]
    async fn the_folder_landed_in_is_not_read_twice() {
        fm_core::listing::forget_everything();
        let fs = Rc::new(Counting {
            reads: RefCell::new(Vec::new()),
        });
        let root: Rc<dyn FileSystemRpc> = fs.clone();
        let state = RouterState::new(root.clone(), root, "/".to_string());

        assert!(state
            .navigate_typed("/a/b/c".to_string())
            .await
            .expect("the walk finished"));

        let reads = fs.reads.borrow().clone();
        let landed = reads
            .iter()
            .filter(|path| path.as_str() == "/a/b/c")
            .count();
        assert_eq!(
            landed, 1,
            "the folder the panel stands in was read {landed} times: {reads:?}"
        );
    }

    /// Walking a path reads each folder along it once, and the one it lands on
    /// is not read a second time to be shown.
    #[tokio::test]
    async fn each_folder_along_a_typed_path_is_read_once() {
        fm_core::listing::forget_everything();
        let fs = Rc::new(Counting {
            reads: RefCell::new(Vec::new()),
        });
        let root: Rc<dyn FileSystemRpc> = fs.clone();
        let state = RouterState::new(root.clone(), root, "/".to_string());

        assert!(state
            .navigate_typed("/a/b/c".to_string())
            .await
            .expect("the walk finished"));
        assert_eq!(fs.reads.borrow().as_slice(), ["/a", "/a/b", "/a/b/c"]);
    }

    /// And walking it again reads nothing at all: every folder on the way is
    /// one the application looked at a moment ago, and nothing has said it
    /// changed. A user who disagrees presses refresh, which goes past all of
    /// this.
    #[tokio::test]
    async fn walking_the_same_path_again_reads_nothing() {
        fm_core::listing::forget_everything();
        let fs = Rc::new(Counting {
            reads: RefCell::new(Vec::new()),
        });
        let root: Rc<dyn FileSystemRpc> = fs.clone();
        let state = RouterState::new(root.clone(), root, "/".to_string());

        assert!(state
            .navigate_typed("/a/b/c".to_string())
            .await
            .expect("the walk finished"));
        let first = fs.reads.borrow().len();
        assert!(state
            .navigate_typed("/a/b/c".to_string())
            .await
            .expect("the walk finished"));
        assert_eq!(fs.reads.borrow().len(), first, "it read a folder again");

        // Until the application itself changes something there.
        state.refresh().await.expect("a refresh reads for real");
        assert_eq!(fs.reads.borrow().len(), first + 1);
    }
}

#[cfg(test)]
mod one_memory {
    use super::*;
    use fm_core::rpc::{FileSystemRpc, RemoteFileEntry};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Counting {
        reads: RefCell<usize>,
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Counting {
        async fn list_dir(&self, path: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
            *self.reads.borrow_mut() += 1;
            Ok(vec![RemoteFileEntry {
                name: format!("seen{}", path),
                is_dir: false,
                size: 0,
                modified: 0,
                permissions: None,
                extra: Vec::new(),
            }])
        }
        fn fs_id(&self) -> String {
            "shared-disk".to_string()
        }
    }

    fn panel(fs: Rc<dyn FileSystemRpc>) -> RouterState {
        RouterState::new(fs.clone(), fs, "/".to_string())
    }

    /// Two panels are two navigations over the same disk. They used to keep a
    /// listing each, which is how two views of one folder come to disagree.
    /// Now there is one, so the second panel costs nothing and shows the same
    /// thing.
    #[tokio::test]
    async fn two_panels_looking_at_one_folder_read_it_once() {
        fm_core::listing::forget_everything();
        let fs = Rc::new(Counting {
            reads: RefCell::new(0),
        });
        let shared: Rc<dyn FileSystemRpc> = fs.clone();

        let left = panel(shared.clone());
        let right = panel(shared);
        left.list_active().await.expect("the left panel lists");
        assert_eq!(*fs.reads.borrow(), 1);

        // The right panel stands in the same folder and is shown from memory.
        right.show_active().await.expect("the right panel shows");
        assert_eq!(*fs.reads.borrow(), 1, "the second panel read it again");
        assert_eq!(
            left.path.borrow().active().entries(),
            right.path.borrow().active().entries(),
            "two panels in one folder disagreed about what is in it"
        );
    }

    /// A filesystem that brings columns of its own — a torrent with its
    /// progress column, an archive with its packed size — is built afresh
    /// every time a path is walked. When its rows come from what the
    /// application remembers, it is never asked for anything, so asking *it*
    /// for the columns answers "none" and the panel quietly loses its table.
    /// The columns belong with the listing.
    #[tokio::test]
    async fn a_folder_shown_from_memory_keeps_the_columns_it_was_read_with() {
        use fm_core::rpc::{ColumnKind, ColumnSpec};

        struct WithColumns {
            asked: RefCell<usize>,
        }

        #[async_trait::async_trait(?Send)]
        impl FileSystemRpc for WithColumns {
            async fn list_dir(&self, _: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
                *self.asked.borrow_mut() += 1;
                Ok(vec![RemoteFileEntry {
                    name: "film.mkv".to_string(),
                    is_dir: false,
                    size: 0,
                    modified: 0,
                    permissions: None,
                    extra: vec!["61%".to_string()],
                }])
            }
            fn extra_columns(&self) -> Vec<ColumnSpec> {
                // Only a filesystem that has listed something has columns to
                // declare, exactly as a plugin mount behaves.
                if *self.asked.borrow() == 0 {
                    return Vec::new();
                }
                vec![ColumnSpec {
                    key: "progress".to_string(),
                    title: "Progress".to_string(),
                    width: Some(90),
                    kind: ColumnKind::Text,
                }]
            }
            fn is_read_only(&self) -> bool {
                // As a plugin mount does: an unopened one says it cannot be
                // written to, because it does not know yet.
                *self.asked.borrow() == 0
            }
            fn fs_id(&self) -> String {
                "local/film.torrent".to_string()
            }
        }

        fm_core::listing::forget_everything();
        let first = Rc::new(WithColumns {
            asked: RefCell::new(0),
        });
        let mounted: Rc<dyn FileSystemRpc> = first.clone();
        let panel = RouterState::new(mounted.clone(), mounted, "/".to_string());
        panel.list_active().await.expect("a listing");
        assert_eq!(
            panel.path.borrow().active().shown_as().columns.len(),
            1,
            "the mount that read it declares its own column"
        );
        assert!(!panel.path.borrow().active().shown_as().read_only);

        // Walking a path builds the mount again; this one has never listed
        // anything, and its rows come from what was remembered.
        let rebuilt = Rc::new(WithColumns {
            asked: RefCell::new(0),
        });
        let rebuilt_fs: Rc<dyn FileSystemRpc> = rebuilt.clone();
        let again = RouterState::new(rebuilt_fs.clone(), rebuilt_fs, "/".to_string());
        again.show_active().await.expect("shown from memory");
        assert_eq!(*rebuilt.asked.borrow(), 0, "it was shown from memory");
        assert_eq!(
            again.path.borrow().active().shown_as().columns.len(),
            1,
            "the panel lost the column the listing was read with"
        );
        assert!(
            !again.path.borrow().active().shown_as().read_only,
            "a torrent shown from memory was taken for read-only, so nothing could be written into it"
        );
    }

    /// A refresh follows every write the application makes — a copy into this
    /// folder, a delete, a rename — and a tree copied in changes what is below
    /// it as much as the folder itself. So a refresh stops trusting the whole
    /// of what is inside, not only the folder on screen. The same is what the
    /// user means by pressing F5.
    #[tokio::test]
    async fn refreshing_a_folder_stops_trusting_what_is_inside_it() {
        fm_core::listing::forget_everything();
        let fs = Rc::new(Counting {
            reads: RefCell::new(0),
        });
        let shared: Rc<dyn FileSystemRpc> = fs.clone();
        let panel = RouterState::new(shared.clone(), shared.clone(), "/".to_string());

        // Something below was read earlier — the panel was in there a moment ago.
        fm_core::listing::list(&shared, "/sub", fm_core::listing::Freshness::Fresh)
            .await
            .expect("a listing");
        panel
            .list_active()
            .await
            .expect("the panel lists its own folder");
        assert!(fm_core::listing::is_remembered(shared.as_ref(), "/sub"));

        panel.refresh().await.expect("a refresh");

        assert!(
            !fm_core::listing::is_remembered(shared.as_ref(), "/sub"),
            "a copy into this folder can have written inside it, and that was still trusted"
        );
        assert!(
            fm_core::listing::remembered_of(shared.as_ref(), "/sub").is_some(),
            "and there should still be something to draw until it is read again"
        );
    }

    /// And a level keeps nothing of its own: say the folder changed and both
    /// panels know at once, because there was only ever one copy. What they
    /// keep is the right to draw what they last saw until one of them looks
    /// again — an empty panel would be a worse answer than an old one.
    #[tokio::test]
    async fn saying_a_folder_changed_reaches_every_panel_at_once() {
        fm_core::listing::forget_everything();
        let fs = Rc::new(Counting {
            reads: RefCell::new(0),
        });
        let shared: Rc<dyn FileSystemRpc> = fs.clone();
        let left = panel(shared.clone());
        let right = panel(shared.clone());
        left.list_active().await.expect("the left panel lists");
        assert!(right.path.borrow().active().has_been_read());

        fm_core::listing::forget(shared.as_ref(), "/");
        assert!(!left.path.borrow().active().has_been_read());
        assert!(!right.path.borrow().active().has_been_read());
        assert!(
            !left.path.borrow().active().entries().is_empty(),
            "both panels were left with nothing to draw"
        );

        // And the next panel to ask reads it for real.
        left.show_active()
            .await
            .expect("the left panel looks again");
        assert_eq!(*fs.reads.borrow(), 2);
        assert!(right.path.borrow().active().has_been_read());
    }
}
