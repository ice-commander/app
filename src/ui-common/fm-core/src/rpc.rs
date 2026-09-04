use common::AppError;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentWait {
    Infinite,
    Bounded(Duration),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathSegment {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteFileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64,
    pub permissions: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct SshShellTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: Option<String>,
    pub key_path: Option<String>,
    pub passphrase: Option<String>,
    pub remote_dir: String,
}

#[async_trait::async_trait(?Send)]
pub trait FileSystemRpc {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    async fn read_file_opt(
        &self,
        path: String,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        _blocking: bool,
    ) -> Result<Vec<u8>, AppError> {
        self.read_file(path, progress_callback).await
    }

    fn get_path_segments(&self, path: &str) -> Vec<PathSegment> {
        let parts: Vec<String> = path
            .split(['/', '\\'])
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        let mut result = Vec::new();
        let mut current = Vec::new();
        for p in parts {
            current.push(p.clone());
            let target_path = current.join("/");
            result.push(PathSegment {
                name: p,
                #[cfg(target_os = "windows")]
                path: if target_path.contains(':') {
                    if target_path.ends_with(':') {
                        format!("{}/", target_path)
                    } else {
                        target_path
                    }
                } else {
                    format!("/{}", target_path)
                },
                #[cfg(not(target_os = "windows"))]
                path: format!("/{}", target_path),
            });
        }
        result
    }

    async fn list_dir(&self, _path: String) -> Result<Vec<RemoteFileEntry>, AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    async fn create_directory(
        &self,
        _parent_path: String,
        _dir_name: String,
        _permissions: Option<u32>,
    ) -> Result<(), AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    async fn delete_entries(&self, _paths: Vec<String>) -> Result<(), AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    async fn rename_entry(&self, _path: String, _new_path: String) -> Result<(), AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    async fn duplicate_entry(&self, _src: String, _dst: String) -> Result<(), AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    async fn get_permissions(&self, _path: String) -> Result<u32, AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    async fn set_permissions(&self, _path: String, _permissions: u32) -> Result<(), AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    async fn read_file(
        &self,
        _path: String,
        _progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<Vec<u8>, AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    async fn write_file(
        &self,
        _path: String,
        _content: Vec<u8>,
        _permissions: Option<u32>,
        _progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<(), AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    async fn extract_archive(&self, _archive_path: String) -> Result<(), AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    async fn compress_to_archive(
        &self,
        _entry_path: String,
        _archive_path: String,
    ) -> Result<(), AppError> {
        Err(AppError::Other("Not implemented".to_string()))
    }

    fn request_file_download(&self, _file_path: String, _transfer_id: uuid::Uuid) {}

    fn trigger_file_upload(
        &self,
        _target_path: String,
        _file_name: String,
        _local_file_path: std::path::PathBuf,
        _transfer_id: uuid::Uuid,
    ) {
    }

    fn is_local(&self) -> bool {
        false
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_root_fs(&self) -> bool {
        false
    }

    fn connection_id(&self) -> Option<String> {
        None
    }

    fn display_name(&self) -> Option<String> {
        None
    }

    fn get_icon_svg(&self, _path: &str) -> Option<String> {
        None
    }

    fn get_icon(&self, _path: &str) -> String {
        "/com/fm-ui/gtk/folder.svg".to_string()
    }

    fn get_last_selected(&self, _path: &str) -> Option<String> {
        None
    }

    fn get_ssh_connection_command(&self, _remote_path: &str) -> Option<Vec<String>> {
        None
    }

    fn get_ssh_shell_target(&self, _remote_path: &str) -> Option<SshShellTarget> {
        None
    }

    fn content_wait(&self) -> ContentWait {
        ContentWait::Infinite
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRpc;

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for TestRpc {}

    fn rpc() -> TestRpc {
        TestRpc
    }


    #[test]
    fn empty_path_returns_no_segments() {
        assert!(rpc().get_path_segments("").is_empty());
    }

    #[test]
    fn root_slash_returns_no_segments() {
        assert!(rpc().get_path_segments("/").is_empty());
    }

    #[test]
    fn backslash_only_returns_no_segments() {
        assert!(rpc().get_path_segments("\\").is_empty());
    }


    #[test]
    fn single_component_name() {
        let segs = rpc().get_path_segments("/home");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].name, "home");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn single_component_path_unix() {
        let segs = rpc().get_path_segments("/home");
        assert_eq!(segs[0].path, "/home");
    }


    #[test]
    fn multi_component_names() {
        let segs = rpc().get_path_segments("/home/user/docs");
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].name, "home");
        assert_eq!(segs[1].name, "user");
        assert_eq!(segs[2].name, "docs");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn multi_component_cumulative_paths_unix() {
        let segs = rpc().get_path_segments("/home/user/docs");
        assert_eq!(segs[0].path, "/home");
        assert_eq!(segs[1].path, "/home/user");
        assert_eq!(segs[2].path, "/home/user/docs");
    }


    #[test]
    fn trailing_slash_same_as_without() {
        assert_eq!(
            rpc().get_path_segments("/home/user/"),
            rpc().get_path_segments("/home/user"),
        );
    }

    #[test]
    fn double_slashes_collapsed() {
        assert_eq!(
            rpc().get_path_segments("//home//user"),
            rpc().get_path_segments("/home/user"),
        );
    }

    #[test]
    fn backslash_treated_as_separator() {
        let segs = rpc().get_path_segments("home\\user\\docs");
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].name, "home");
        assert_eq!(segs[1].name, "user");
        assert_eq!(segs[2].name, "docs");
    }

    #[test]
    fn mixed_separators_yield_correct_count() {
        let segs = rpc().get_path_segments("/home\\user/docs");
        assert_eq!(segs.len(), 3);
    }


    #[cfg(target_os = "windows")]
    mod windows_paths {
        use super::*;

        #[test]
        fn drive_root_gets_trailing_slash() {
            let segs = rpc().get_path_segments("C:");
            assert_eq!(segs.len(), 1);
            assert_eq!(segs[0].name, "C:");
            assert_eq!(segs[0].path, "C:/");
        }

        #[test]
        fn drive_with_subdirs() {
            let segs = rpc().get_path_segments("C:\\Users\\foo");
            assert_eq!(segs.len(), 3);
            assert_eq!(segs[0].path, "C:/");
            assert_eq!(segs[1].path, "C:/Users");
            assert_eq!(segs[2].path, "C:/Users/foo");
        }

        #[test]
        fn unc_like_path_no_colon() {
            let segs = rpc().get_path_segments("/shared/folder");
            assert_eq!(segs[0].path, "/shared");
            assert_eq!(segs[1].path, "/shared/folder");
        }
    }


    #[test]
    fn path_segment_clone_eq() {
        let a = PathSegment {
            name: "foo".into(),
            path: "/foo".into(),
        };
        assert_eq!(a.clone(), a);
    }

    #[test]
    fn remote_file_entry_clone_eq() {
        let a = RemoteFileEntry {
            name: "f.txt".into(),
            is_dir: false,
            size: 42,
            modified: 0,
            permissions: None,
        };
        assert_eq!(a.clone(), a);
    }
}
