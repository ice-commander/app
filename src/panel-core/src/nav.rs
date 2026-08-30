use std::rc::Rc;

use fm_core::rpc::{FileSystemRpc, RemoteFileEntry};

#[derive(Clone)]
pub struct PathLevel {
    pub name: String,
    pub relative_path: String,
    pub fs: Rc<dyn FileSystemRpc>,
    pub entries: Rc<Vec<RemoteFileEntry>>,
    pub selected: Option<String>,
    pub loaded: bool,
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
            entries: Rc::new(Vec::new()),
            selected: None,
            loaded: false,
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
        if names.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", names.join("/"))
        }
    }
}

pub fn is_archive(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".zip")
        || n.ends_with(".tar")
        || n.ends_with(".tar.gz")
        || n.ends_with(".tgz")
        || n.ends_with(".tar.bz2")
        || n.ends_with(".tbz2")
        || n.ends_with(".tbz")
}

pub fn build_levels(target: &[crate::PathSegment], base: Rc<dyn FileSystemRpc>) -> Vec<PathLevel> {
    let mut levels = Vec::with_capacity(target.len() + 1);
    levels.push(PathLevel::new(base.display_name().unwrap_or_default(), "/", base.clone()));
    let mut current_fs = base;
    let mut fs_start = 0usize;

    for i in 0..target.len() {
        let name = &target[i].name;
        if is_archive(name) {
            let relative_in_parent = crate::build_segments_to_path(&target[fs_start..=i]);
            let provider = Rc::new(virtualfs::archive_rpc::ArchiveFileSystemRpc::new(
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
