use std::rc::Rc;

use fm_core::rpc::{FileSystemRpc, RemoteFileEntry};

#[derive(Clone)]
pub struct PathLevel {
    pub name: String,
    pub relative_path: String,
    pub fs: Rc<dyn FileSystemRpc>,
    pub selected: Option<String>,
}

impl PathLevel {
    pub fn new(
        name: impl Into<String>,
        relative_path: impl Into<String>,
        fs: Rc<dyn FileSystemRpc>,
    ) -> Self {
        Self {
            name: name.into(),
            relative_path: relative_path.into(),
            fs,
            selected: None,
        }
    }

    /// What this folder holds, as the application last read it.
    ///
    /// A level does not keep a listing of its own: one memory answers every
    /// panel, every tab and every frontend, and two of them cannot disagree
    /// about the same folder. Empty means it has not been read yet — the
    /// caller that cares asks `has_been_read`.
    pub fn entries(&self) -> Rc<Vec<RemoteFileEntry>> {
        fm_core::listing::remembered_of(self.fs.as_ref(), &self.relative_path)
            .unwrap_or_else(|| Rc::new(Vec::new()))
    }

    pub fn has_been_read(&self) -> bool {
        fm_core::listing::is_remembered(self.fs.as_ref(), &self.relative_path)
    }

    /// How this folder asked to be shown: the columns it adds, whether they
    /// replace the panel's own, whether it may be written to, whether it wants
    /// the quick filter.
    ///
    /// Taken from the listing rather than from the filesystem object, because
    /// a filesystem built afresh while walking a path is handed its rows from
    /// what the application remembers and never opens the plugin behind it.
    /// Asking that one answers as an unopened mount does — no columns, and
    /// read-only — and a torrent would lose its table, an archive the right to
    /// be written into.
    pub fn shown_as(&self) -> fm_core::listing::Shown {
        match fm_core::listing::shown_as(self.fs.as_ref(), &self.relative_path) {
            Some(shown) => shown,
            None => fm_core::listing::Shown {
                columns: self.fs.extra_columns(),
                columns_replace_defaults: self.fs.columns_replace_defaults(),
                read_only: self.fs.is_read_only(),
                wants_quick_filter: self.fs.wants_quick_filter(),
            },
        }
    }
}

#[derive(Clone)]
pub struct NavPath {
    levels: Vec<PathLevel>,
}

impl NavPath {
    pub fn new(root: PathLevel) -> Self {
        Self { levels: vec![root] }
    }

    pub fn from_levels(levels: Vec<PathLevel>, base: Rc<dyn FileSystemRpc>) -> Self {
        if levels.is_empty() {
            let name = base.display_name().unwrap_or_default();
            Self::new(PathLevel::new(name, "/", base))
        } else {
            Self { levels }
        }
    }

    pub fn levels(&self) -> &[PathLevel] {
        &self.levels
    }

    pub fn active(&self) -> &PathLevel {
        self.levels.last().expect("NavPath is never empty")
    }

    pub fn active_mut(&mut self) -> &mut PathLevel {
        self.levels.last_mut().expect("NavPath is never empty")
    }

    pub fn depth(&self) -> usize {
        self.levels.len()
    }

    pub fn push(&mut self, level: PathLevel) {
        self.levels.push(level);
    }

    pub fn pop(&mut self) -> bool {
        if self.levels.len() > 1 {
            self.levels.pop();
            true
        } else {
            false
        }
    }

    pub fn truncate_to(&mut self, idx: usize) {
        if idx + 1 < self.levels.len() {
            self.levels.truncate(idx + 1);
        }
    }

    pub fn absolute_path(&self) -> String {
        let names: Vec<&str> = self.levels[1..].iter().map(|l| l.name.as_str()).collect();
        fm_core::path::join_segment_names(&names)
    }
}

pub fn is_archive(name: &str) -> bool {
    fm_core::plugin_fs::handles_extension(name)
}

pub fn build_levels(target: &[crate::PathSegment], base: Rc<dyn FileSystemRpc>) -> Vec<PathLevel> {
    let mut levels = Vec::with_capacity(target.len() + 1);
    levels.push(PathLevel::new(
        base.display_name().unwrap_or_default(),
        "/",
        base.clone(),
    ));
    let mut current_fs = base;
    let mut fs_start = 0usize;

    for i in 0..target.len() {
        let name = &target[i].name;
        if let Some(plugin) = fm_core::plugin_fs::filesystem_for(name) {
            let relative_in_parent = crate::build_segments_to_path(&target[fs_start..=i]);
            let provider = Rc::new(fm_core::plugin_fs::PluginFsRpc::new(
                plugin,
                relative_in_parent,
                current_fs.clone(),
            ));
            current_fs = provider;
            fs_start = i + 1;
            levels.push(PathLevel::new(name.clone(), "/", current_fs.clone()));
        } else {
            let rel = crate::build_segments_to_path(&target[fs_start..=i]);
            levels.push(PathLevel::new(name.clone(), rel, current_fs.clone()));
        }
    }
    levels
}
