pub mod nav;
pub mod state;

pub use fm_core::rpc::PathSegment;
pub use state::{History, RouterState};

pub fn parse_path_to_segments(path: &str) -> Vec<PathSegment> {
    let parts: Vec<&str> = path
        .split(|c| c == '/' || c == '\\')
        .filter(|s| !s.is_empty())
        .collect();
    let mut result = Vec::new();
    let mut current = Vec::new();
    for p in parts {
        current.push(p);
        let joined = current.join("/");
        let seg_path = if joined.is_empty() {
            "/".to_string()
        } else {
            #[cfg(target_os = "windows")]
            {
                if joined.contains(':') {
                    if joined.ends_with(':') {
                        format!("{}/", joined)
                    } else {
                        joined
                    }
                } else {
                    format!("/{}", joined)
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                format!("/{}", joined)
            }
        };
        result.push(PathSegment {
            name: p.to_string(),
            path: seg_path,
        });
    }
    result
}

pub fn build_segments_to_path(segments: &[PathSegment]) -> String {
    let joined = segments
        .iter()
        .map(|s| s.name.as_str())
        .collect::<Vec<&str>>()
        .join("/");
    if joined.is_empty() {
        "/".to_string()
    } else {
        #[cfg(target_os = "windows")]
        {
            if joined.contains(':') {
                if joined.ends_with(':') {
                    format!("{}/", joined)
                } else {
                    joined
                }
            } else {
                format!("/{}", joined)
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            format!("/{}", joined)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_core::rpc::FileSystemRpc;
    use std::rc::Rc;

    struct MockRpc;
    #[async_trait::async_trait(?Send)]
    impl fm_core::rpc::FileSystemRpc for MockRpc {}

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

    #[test]
    fn build_levels_archive_switches_provider() {
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

        state.path.borrow_mut().push(PathLevel::new("a", "/a", provider.clone()));
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
        state.path.borrow_mut().push(PathLevel::new("a", "/a", p.clone()));
        state.record_history_snapshot();
        state.path.borrow_mut().push(PathLevel::new("b", "/a/b", p.clone()));
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

        state.path.borrow_mut().push(PathLevel::new("a", "/a", p.clone()));
        state.record_history_snapshot();
        state.record_history_snapshot();
        assert_eq!(state.history.borrow().snapshots.len(), 1);
        assert!(!state.can_go_forward());

        state.path.borrow_mut().push(PathLevel::new("b", "/a/b", p.clone()));
        state.record_history_snapshot();
        assert_eq!(state.history.borrow().snapshots.len(), 2);
        assert!(state.can_go_back());
    }

    #[test]
    fn new_navigation_after_back_truncates_forward() {
        let p = Rc::new(MockRpc);
        let state = RouterState::new(p.clone(), p.clone(), "/".to_string());

        state.record_history_snapshot();
        state.path.borrow_mut().push(PathLevel::new("a", "/a", p.clone()));
        state.record_history_snapshot();
        state.path.borrow_mut().push(PathLevel::new("b", "/a/b", p.clone()));
        state.record_history_snapshot();

        let back = state.history.borrow_mut().back().unwrap();
        *state.path.borrow_mut() = back;
        assert!(state.can_go_forward());

        state.path.borrow_mut().push(PathLevel::new("c", "/a/c", p.clone()));
        state.record_history_snapshot();
        assert!(!state.can_go_forward(), "new nav after back must drop the forward branch");
        assert_eq!(state.history.borrow().snapshots.len(), 3);
    }

    #[test]
    fn breadcrumb_segments_follow_nav_path_after_enter() {
        let p = Rc::new(MockRpc);
        let state = RouterState::new(p.clone(), p.clone(), "/".to_string());

        state
            .path
            .borrow_mut()
            .push(crate::nav::PathLevel::new("folderA", "/folderA", Rc::new(MockRpc)));
        state
            .path
            .borrow_mut()
            .push(crate::nav::PathLevel::new("sub", "/folderA/sub", Rc::new(MockRpc)));

        let names: Vec<String> = state.breadcrumb_segments().iter().map(|s| s.name.clone()).collect();
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

        let names: Vec<String> = state.breadcrumb_segments().iter().map(|s| s.name.clone()).collect();
        assert_eq!(names, vec!["a".to_string()], "jump to 'a' must drop deeper crumbs");
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
            path.push(crate::nav::PathLevel::new("tests", "/tests", Rc::new(MockRpc)));
            path.push(crate::nav::PathLevel::new("foo", "/tests/foo", Rc::new(MockRpc)));
        }
        assert_eq!(state.resolve_relative("/tests/foo/file.txt"), "/tests/foo/file.txt");
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
            path.push(crate::nav::PathLevel::new("inner", "/inner", archive.clone()));
        }
        assert_eq!(state.resolve_relative("/a.zip/inner/file"), "/inner/file");
        assert_eq!(state.resolve_relative("/a.zip/inner"), "/inner");
    }

}
