use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::AppError;
use fm_core::rpc::FileSystemRpc;

use crate::nav::{NavPath, PathLevel};

use crate::PathSegment;

pub struct History {
    pub snapshots: Vec<NavPath>,
    pub index: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            snapshots: Vec::new(),
            index: 0,
        }
    }

    pub fn record(&mut self, nav: NavPath) {
        if let Some(cur) = self.snapshots.get(self.index) {
            if cur.absolute_path() == nav.absolute_path() {
                self.snapshots[self.index] = nav;
                return;
            }
        }
        if !self.snapshots.is_empty() {
            self.snapshots.truncate(self.index + 1);
        }
        self.snapshots.push(nav);
        self.index = self.snapshots.len() - 1;
    }

    pub fn back(&mut self) -> Option<NavPath> {
        if self.can_back() {
            self.index -= 1;
            Some(self.snapshots[self.index].clone())
        } else {
            None
        }
    }

    pub fn forward(&mut self) -> Option<NavPath> {
        if self.can_forward() {
            self.index += 1;
            Some(self.snapshots[self.index].clone())
        } else {
            None
        }
    }

    pub fn can_back(&self) -> bool {
        !self.snapshots.is_empty() && self.index > 0
    }

    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.snapshots.len()
    }

    pub fn clear(&mut self) {
        self.snapshots.clear();
        self.index = 0;
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RouterState {
    pub local_provider: Rc<dyn FileSystemRpc>,
    pub base_provider: Rc<dyn FileSystemRpc>,
    pub history: Rc<RefCell<History>>,
    pub last_selected: Rc<RefCell<std::collections::HashMap<String, String>>>,
    pub showing_selector: Rc<Cell<bool>>,
    pub resolving: Cell<bool>,
    pub path: Rc<RefCell<crate::nav::NavPath>>,
    pub on_changed: Rc<RefCell<Option<Box<dyn Fn()>>>>,
}

impl RouterState {
    pub fn new(
        base_provider: Rc<dyn FileSystemRpc>,
        local_provider: Rc<dyn FileSystemRpc>,
        _mount_point: String,
    ) -> Self {
        let root_fs = base_provider.clone();
        Self {
            local_provider,
            base_provider,
            history: Rc::new(RefCell::new(History::new())),
            last_selected: Rc::new(RefCell::new(std::collections::HashMap::new())),
            showing_selector: Rc::new(Cell::new(false)),
            resolving: Cell::new(false),
            path: Rc::new(RefCell::new(crate::nav::NavPath::new(
                crate::nav::PathLevel::new(
                    root_fs.display_name().unwrap_or_default(),
                    "/",
                    root_fs,
                ),
            ))),
            on_changed: Rc::new(RefCell::new(None)),
        }
    }

    pub fn set_on_changed(&self, f: impl Fn() + 'static) {
        *self.on_changed.borrow_mut() = Some(Box::new(f));
    }

    fn notify_changed(&self) {
        if let Some(cb) = self.on_changed.borrow().as_ref() {
            cb();
        }
    }

    pub async fn list_active(&self) -> Result<(), AppError> {
        self.show_active_contents(fm_core::listing::Freshness::Fresh)
            .await
    }

    /// The same, for a caller that has just read this directory itself — the
    /// walk that checks a typed path ends on the folder the panel is about to
    /// show, and reading it twice in one action is waste, not carefulness.
    async fn show_active_contents(
        &self,
        freshness: fm_core::listing::Freshness,
    ) -> Result<(), AppError> {
        let (fs, rel) = {
            let path = self.path.borrow();
            let a = path.active();
            (a.fs.clone(), a.relative_path.clone())
        };
        // The one memory holds it; the level is a window onto that.
        fm_core::listing::list(&fs, &rel, freshness).await?;
        self.notify_changed();
        Ok(())
    }

    /// Reads this folder again, and stops trusting what is remembered about
    /// anything inside it.
    ///
    /// A refresh follows every write the application makes — a copy into this
    /// folder, a delete, a rename — and a tree copied in changes what is below
    /// it as much as the folder itself. It is also what the user asks for with
    /// F5, and there it means the same thing: believe nothing you kept.
    pub async fn refresh(&self) -> Result<(), AppError> {
        let (fs, rel) = {
            let path = self.path.borrow();
            let a = path.active();
            (a.fs.clone(), a.relative_path.clone())
        };
        fm_core::listing::forget_within(fs.as_ref(), &rel);
        self.list_active().await
    }

    pub(crate) async fn show_active(&self) -> Result<(), AppError> {
        let loaded = self.path.borrow().active().has_been_read();
        if loaded {
            self.notify_changed();
            Ok(())
        } else {
            self.list_active().await
        }
    }

    pub async fn navigate_typed(&self, input: String) -> Result<bool, AppError> {
        if self.resolving.get() {
            return Ok(true);
        }
        self.resolving.set(true);
        let result = self.navigate_typed_inner(input).await;
        self.resolving.set(false); // cleared on EVERY exit, before any `?` can escape
        result
    }

    async fn navigate_typed_inner(&self, input: String) -> Result<bool, AppError> {
        if input.trim().is_empty() {
            return Ok(true);
        }
        let segs = Self::normalize_typed_segments(crate::parse_path_to_segments(input.trim()));

        let f0 = self.path.borrow().levels()[0].fs.clone();
        let walk_base = if f0.is_root_fs() {
            self.local_provider.clone()
        } else {
            f0.clone()
        };

        let leaving = self.path.borrow().absolute_path();
        if let Some(name) = self.active_selected() {
            self.save_last_selected(leaving, name);
        }

        let mut valid = 0usize;
        for i in 0..segs.len() {
            let levels = crate::nav::build_levels(&segs[..=i], walk_base.clone());
            let (fs, rel) = {
                let last = levels
                    .last()
                    .expect("build_levels always yields the root level");
                (last.fs.clone(), last.relative_path.clone())
            };
            // Only asking whether this much of the path exists. A folder read
            // a moment ago on the way in is the same folder now, and on a
            // server inside an archive each of these used to be the whole
            // container over the network again.
            match fm_core::listing::list(&fs, &rel, fm_core::listing::Freshness::Remembered).await {
                Ok(_) => valid = i + 1,
                Err(_) => break,
            }
        }

        let commit_base = if valid == 0 { f0 } else { walk_base };
        let final_levels = crate::nav::build_levels(&segs[..valid], commit_base.clone());
        *self.path.borrow_mut() = crate::nav::NavPath::from_levels(final_levels, commit_base);
        self.showing_selector.set(false);
        // The walk above ended on this very folder; showing it does not need
        // to ask again.
        self.show_active_contents(fm_core::listing::Freshness::Remembered)
            .await?;
        self.record_history_snapshot();

        Ok(valid == segs.len())
    }

    fn normalize_typed_segments(segs: Vec<crate::PathSegment>) -> Vec<crate::PathSegment> {
        let mut out: Vec<crate::PathSegment> = Vec::with_capacity(segs.len());
        for s in segs {
            match s.name.as_str() {
                "." => {}
                ".." => {
                    out.pop();
                }
                _ => {
                    #[cfg(target_os = "windows")]
                    let s = {
                        let mut s = s;
                        if out.is_empty()
                            && s.name.len() == 2
                            && s.name.ends_with(':')
                            && s.name
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_ascii_alphabetic())
                        {
                            s.name = s.name.to_uppercase();
                        }
                        s
                    };
                    out.push(s);
                }
            }
        }
        out
    }

    pub async fn go_up(&self) -> Result<(), AppError> {
        if !self.path.borrow_mut().pop() {
            return Ok(());
        }
        self.show_active().await?;
        self.record_history_snapshot();
        Ok(())
    }

    pub async fn go_to_level(&self, idx: usize) -> Result<(), AppError> {
        self.path.borrow_mut().truncate_to(idx);
        self.show_active().await?;
        self.record_history_snapshot();
        Ok(())
    }

    pub async fn enter(&self, name: &str) -> Result<(), AppError> {
        let parent_abs = self.path.borrow().absolute_path();
        self.save_last_selected(parent_abs, name.to_string());
        self.set_selected(Some(name.to_string()));
        {
            let mut path = self.path.borrow_mut();
            let (parent_fs, parent_rel) = {
                let a = path.active();
                (a.fs.clone(), a.relative_path.clone())
            };
            let child_rel = {
                let mut parts = fm_core::path::split_joined(&parent_rel);
                parts.push(name);
                fm_core::path::join_segment_names(&parts)
            };
            if let Some(plugin) = fm_core::plugin_fs::filesystem_for(name) {
                let provider = Rc::new(fm_core::plugin_fs::PluginFsRpc::new(
                    plugin, child_rel, parent_fs,
                ));
                path.push(PathLevel::new(name, "/", provider));
            } else {
                path.push(PathLevel::new(name, child_rel, parent_fs));
            }
        }
        self.list_active().await?;
        self.record_history_snapshot();
        Ok(())
    }

    pub fn active_provider(&self) -> Rc<dyn FileSystemRpc> {
        self.path.borrow().active().fs.clone()
    }

    /// Where the panel stands, as its own filesystem spells it.
    pub fn active_relative_path(&self) -> String {
        self.path.borrow().active().relative_path.clone()
    }

    pub fn resolve_relative(&self, abs: &str) -> String {
        let path = self.path.borrow();
        let display = path.absolute_path();
        let rel = path.active().relative_path.clone();
        let tail = abs.strip_prefix(&display).unwrap_or(abs);
        let mut parts = fm_core::path::split_joined(&rel);
        parts.extend(fm_core::path::split_joined(tail));
        fm_core::path::join_segment_names(&parts)
    }

    pub fn breadcrumb_segments(&self) -> Vec<PathSegment> {
        let path = self.path.borrow();
        let mut acc: Vec<&str> = Vec::new();
        path.levels()[1..]
            .iter()
            .map(|l| {
                acc.push(l.name.as_str());
                PathSegment {
                    name: l.name.clone(),
                    path: fm_core::path::join_segment_names(&acc),
                }
            })
            .collect()
    }

    fn set_root_level(&self, provider: Rc<dyn FileSystemRpc>) {
        let name = provider.display_name().unwrap_or_default();
        *self.path.borrow_mut() =
            crate::nav::NavPath::new(crate::nav::PathLevel::new(name, "/", provider));
    }

    pub fn reset_to_base(&self) {
        self.set_root_level(self.base_provider.clone());
    }

    pub fn set_active_provider(&self, provider: Rc<dyn FileSystemRpc>, _mount_point: String) {
        self.set_root_level(provider);
    }

    pub fn record_history_snapshot(&self) {
        let nav = self.path.borrow().clone();
        self.history.borrow_mut().record(nav);
    }

    pub async fn go_back(&self) -> Result<(), AppError> {
        let restored = self.history.borrow_mut().back();
        if let Some(nav) = restored {
            *self.path.borrow_mut() = nav;
            self.show_active().await?;
        }
        Ok(())
    }

    pub async fn go_forward(&self) -> Result<(), AppError> {
        let restored = self.history.borrow_mut().forward();
        if let Some(nav) = restored {
            *self.path.borrow_mut() = nav;
            self.show_active().await?;
        }
        Ok(())
    }

    pub fn can_go_back(&self) -> bool {
        self.history.borrow().can_back()
    }

    pub fn can_go_forward(&self) -> bool {
        self.history.borrow().can_forward()
    }

    pub fn clear_history_state(&self) {
        self.history.borrow_mut().clear();
    }

    pub fn set_selected(&self, name: Option<String>) {
        self.path.borrow_mut().active_mut().selected = name;
    }

    pub fn active_selected(&self) -> Option<String> {
        self.path.borrow().active().selected.clone()
    }

    pub fn save_last_selected(&self, path: String, name: String) {
        self.last_selected.borrow_mut().insert(path, name);
    }

    pub fn get_last_selected(&self, path: &str) -> Option<String> {
        self.last_selected.borrow().get(path).cloned()
    }
}
