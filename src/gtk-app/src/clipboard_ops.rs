use fm_core::clipboard::{ClipKind, Clipboard};
use fm_core::rpc::FileSystemRpc;
use std::rc::Rc;

fn plan_names(
    items: &[fm_core::clipboard::ClipItem],
    mut taken: Vec<String>,
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for it in items {
        let renamed = fm_core::names::keep_or_number(&it.name, it.is_dir, &taken, |s, e, n| {
            format!("{s} ({n}){e}")
        });
        taken.push(renamed.clone());
        map.insert(it.name.clone(), renamed);
    }
    map
}

fn same_filesystem(src: Option<&Rc<dyn FileSystemRpc>>, dest: &Rc<dyn FileSystemRpc>) -> bool {
    match src {
        Some(src) => Rc::ptr_eq(src, dest) || (src.is_local() && dest.is_local()),
        None => false,
    }
}

pub fn take(
    clipboard: &Rc<Clipboard>,
    fm: &Rc<panel_router::PanelRouter>,
    kind: ClipKind,
) -> bool {
    let items: Vec<_> = fm
        .selected_entries()
        .into_iter()
        .filter(|e| e.name() != "..")
        .map(|e| fm_core::clipboard::ClipItem {
            name: e.name(),
            is_dir: e.is_dir(),
            size: e.size(),
            permissions: e.permissions(),
        })
        .collect();
    if items.is_empty() {
        return false;
    }
    let provider = fm.provider();
    clipboard.set(fm_core::clipboard::Clip {
        kind,
        side: fm.panel_id().to_string(),
        source_path: fm.current_path_string(),
        items,
        source: provider.clone(),
        anchor: Rc::downgrade(&fm.state.active_provider()),
        rebuildable: crate::transfer_plan::EndpointPlan::of(&provider).is_some(),
        refresh_source: Some({
            let r = fm.clone();
            Rc::new(move || r.refresh_spawned())
        }),
    });
    true
}

pub fn paste_into(
    window: &gtk::Window,
    clipboard: &Rc<Clipboard>,
    dest: &Rc<panel_router::PanelRouter>,
    into_subdir: Option<String>,
) {
    use gtk::prelude::Cast;
    let Ok(app_window) = window.clone().downcast::<adw::ApplicationWindow>() else {
        return;
    };
    let Some((kind, source_path, items, provider, refresh_source, source_level)) = clipboard.with(|c| {
        (
            c.kind,
            c.source_path.clone(),
            c.items.clone(),
            c.source.clone(),
            c.refresh_source.clone(),
            c.anchor.upgrade(),
        )
    }) else {
        return;
    };
    let dest_path = match &into_subdir {
        Some(name) => transfer_core::join_path(&dest.current_path_string(), name),
        None => dest.current_path_string(),
    };

    let same_place = source_path == dest_path
        && same_filesystem(source_level.as_ref(), &dest.state.active_provider());
    if same_place && kind == ClipKind::Cut {
        clipboard.clear();
        return;
    }

    let dest_names = if same_place {
        plan_names(&items, dest.current_entry_names())
    } else {
        std::collections::HashMap::new()
    };

    let source = crate::file_operations::TransferSource {
        items: items
            .iter()
            .map(|i| (i.name.clone(), i.is_dir, i.size, i.permissions))
            .collect(),
        parent: source_path,
        provider,
        refresh: None,
        dest_names,
        on_done: Some({
            let clipboard = clipboard.clone();
            Rc::new(move || {
                if let Some(f) = &refresh_source {
                    f();
                }
                if kind == ClipKind::Cut {
                    clipboard.clear();
                }
            })
        }),
    };
    crate::file_operations::show_transfer_dialog(
        &app_window,
        source,
        dest,
        kind == ClipKind::Cut,
        Some(dest_path),
    );
}

#[cfg(test)]
mod tests {
    use super::same_filesystem;
    use fm_core::rpc::FileSystemRpc;
    use std::rc::Rc;

    struct Fs(bool);

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Fs {
        fn is_local(&self) -> bool {
            self.0
        }
    }

    fn local() -> Rc<dyn FileSystemRpc> {
        Rc::new(Fs(true))
    }

    fn remote() -> Rc<dyn FileSystemRpc> {
        Rc::new(Fs(false))
    }

    #[test]
    fn one_provider_is_the_same_filesystem_as_itself() {
        let fs = remote();
        assert!(same_filesystem(Some(&fs), &fs));
    }

    #[test]
    fn two_local_panels_share_a_filesystem_despite_separate_instances() {
        assert!(same_filesystem(Some(&local()), &local()));
    }

    #[test]
    fn a_local_and_a_remote_at_the_same_path_are_not_the_same_place() {
        assert!(!same_filesystem(Some(&local()), &remote()));
        assert!(!same_filesystem(Some(&remote()), &local()));
    }

    #[test]
    fn two_distinct_remotes_are_not_the_same_place() {
        assert!(!same_filesystem(Some(&remote()), &remote()));
    }

    #[test]
    fn a_lost_source_level_is_never_the_same_place() {
        assert!(!same_filesystem(None, &local()));
    }

    fn unique_name(name: &str, is_dir: bool, taken: &[String]) -> String {
        fm_core::names::keep_or_number(name, is_dir, taken, |s, e, n| format!("{s} ({n}){e}"))
    }

    fn taken(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn ci(name: &str, is_dir: bool) -> fm_core::clipboard::ClipItem {
        fm_core::clipboard::ClipItem {
            name: name.to_string(),
            is_dir,
            size: 0,
            permissions: None,
        }
    }

    #[test]
    fn planning_maps_every_clashing_item_to_a_free_name() {
        let map = super::plan_names(&[ci("a.txt", false)], taken(&["a.txt"]));
        assert_eq!(map.get("a.txt").map(String::as_str), Some("a (1).txt"));
    }

    #[test]
    fn two_clashing_items_never_get_handed_the_same_name() {
        let map = super::plan_names(
            &[ci("a.txt", false), ci("a (1).txt", false)],
            taken(&["a.txt", "a (1).txt"]),
        );
        let first = map.get("a.txt").unwrap();
        let second = map.get("a (1).txt").unwrap();
        assert_ne!(first, second, "both were numbered against the same list");
        assert_eq!(first, "a (2).txt");
        assert_eq!(second, "a (1) (1).txt", "an existing counter is not parsed, another is appended");
    }

    #[test]
    fn planning_keeps_a_name_nothing_clashes_with() {
        let map = super::plan_names(&[ci("b.txt", false)], taken(&["a.txt"]));
        assert_eq!(map.get("b.txt").map(String::as_str), Some("b.txt"));
    }

    #[test]
    fn a_folder_is_numbered_without_being_split_at_its_dot() {
        let map = super::plan_names(&[ci("backup.2024", true)], taken(&["backup.2024"]));
        assert_eq!(map.get("backup.2024").map(String::as_str), Some("backup.2024 (1)"));
    }

    #[test]
    fn a_free_name_is_left_alone() {
        assert_eq!(unique_name("a.txt", false, &taken(&["b.txt"])), "a.txt");
    }

    #[test]
    fn a_clash_gets_a_counter_before_the_extension() {
        assert_eq!(unique_name("a.txt", false, &taken(&["a.txt"])), "a (1).txt");
        assert_eq!(
            unique_name("a.txt", false, &taken(&["a.txt", "a (1).txt"])),
            "a (2).txt"
        );
    }

    #[test]
    fn a_name_without_an_extension_still_counts_up() {
        assert_eq!(unique_name("README", false, &taken(&["README"])), "README (1)");
    }

    #[test]
    fn a_dotfile_keeps_its_leading_dot() {
        assert_eq!(unique_name(".bashrc", false, &taken(&[".bashrc"])), ".bashrc (1)");
    }

    #[test]
    fn a_double_extension_only_splits_at_the_last_dot() {
        assert_eq!(
            unique_name("app.tar.gz", false, &taken(&["app.tar.gz"])),
            "app.tar (1).gz"
        );
    }
}
