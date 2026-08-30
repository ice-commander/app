use fm_core::rpc::FileSystemRpc;
use std::path::Path;
use std::rc::Rc;

pub(super) fn split_parent_child(path: &str) -> (String, String) {
    let path_norm = path.replace('\\', "/");
    if let Some(idx) = path_norm.rfind('/') {
        let parent = &path_norm[..idx];
        let parent = if parent.is_empty() { "/" } else { parent };
        let child = &path_norm[idx + 1..];
        (parent.to_string(), child.to_string())
    } else {
        ("".to_string(), path.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_path_splits_correctly() {
        let (parent, child) = split_parent_child("/home/user/file.txt");
        assert_eq!(parent, "/home/user");
        assert_eq!(child, "file.txt");
    }

    #[test]
    fn root_file_has_root_parent() {
        let (parent, child) = split_parent_child("/file.txt");
        assert_eq!(parent, "/");
        assert_eq!(child, "file.txt");
    }

    #[test]
    fn no_slash_returns_empty_parent() {
        let (parent, child) = split_parent_child("file.txt");
        assert_eq!(parent, "");
        assert_eq!(child, "file.txt");
    }

    #[test]
    fn windows_backslashes_are_normalized() {
        let (parent, child) = split_parent_child("C:\\Users\\user\\doc.txt");
        assert_eq!(parent, "C:/Users/user");
        assert_eq!(child, "doc.txt");
    }

    #[test]
    fn trailing_slash_gives_empty_child() {
        let (parent, child) = split_parent_child("/home/user/");
        assert_eq!(parent, "/home/user");
        assert_eq!(child, "");
    }

    #[test]
    fn empty_string_gives_empty_both() {
        let (parent, child) = split_parent_child("");
        assert_eq!(parent, "");
        assert_eq!(child, "");
    }
}

pub(crate) async fn query_file_metadata(
    path: String,
    provider: Option<Rc<dyn FileSystemRpc>>,
) -> Option<(u64, u64)> {
    if let Some(p) = provider {
        let (parent, filename) = split_parent_child(&path);
        match p.list_dir(parent).await {
            Ok(entries) => entries.into_iter().find(|e| e.name == filename).map(|e| (e.size, e.modified)),
            Err(_) => None,
        }
    } else {
        let metadata = std::fs::metadata(Path::new(&path));
        metadata.ok().map(|m| {
            let size = m.len();
            let modified = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (size, modified)
        })
    }
}
