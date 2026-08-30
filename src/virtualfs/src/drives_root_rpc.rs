use common::AppError;
use fm_core::rpc::RemoteFileEntry;

pub struct DrivesRootRpc;

impl DrivesRootRpc {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait(?Send)]
impl fm_core::rpc::FileSystemRpc for DrivesRootRpc {
    async fn list_dir(&self, _path: String) -> Result<Vec<RemoteFileEntry>, AppError> {
        let res = tokio::task::spawn_blocking(move || {
            let drives = crate::utils::get_drives();
            let mut entries = Vec::new();

            #[cfg(target_os = "windows")]
            {
                for d in drives {
                    entries.push(RemoteFileEntry {
                        name: d.path,
                        is_dir: true,
                        size: 0,
                        modified: 0,
                        permissions: None,
                    });
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                entries.push(RemoteFileEntry {
                    name: "/".to_string(),
                    is_dir: true,
                    size: 0,
                    modified: 0,
                    permissions: None,
                });
                for d in drives {
                    if d.path != "/" && d.is_mounted {
                        entries.push(RemoteFileEntry {
                            name: d.path,
                            is_dir: true,
                            size: 0,
                            modified: 0,
                            permissions: None,
                        });
                    }
                }
            }
            entries
        })
        .await;

        res.map_err(|e| AppError::Other(e.to_string()))
    }

    async fn create_directory(
        &self,
        _parent_path: String,
        _dir_name: String,
        _permissions: Option<u32>,
    ) -> Result<(), AppError> {
        Err(AppError::Other("Cannot create directory in drives root".to_string()))
    }

    async fn rename_entry(&self, _path: String, _new_path: String) -> Result<(), AppError> {
        Err(AppError::Other("Cannot rename in drives root".to_string()))
    }

    async fn delete_entries(&self, _paths: Vec<String>) -> Result<(), AppError> {
        Err(AppError::Other("Cannot delete in drives root".to_string()))
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

    fn get_icon(&self, _path: &str) -> String {
        "/com/icecommander/gtk/home.svg".to_string()
    }

    fn is_root_fs(&self) -> bool {
        true
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use fm_core::rpc::FileSystemRpc;

    #[test]
    fn drives_root_reports_itself_as_the_root_filesystem() {
        assert!(DrivesRootRpc::new().is_root_fs());
    }

    #[test]
    fn every_path_gets_the_home_icon() {
        let rpc = DrivesRootRpc::new();
        assert!(rpc.get_icon("/").ends_with("/home.svg"));
        assert!(rpc.get_icon("/media/usb").ends_with("/home.svg"));
        assert_eq!(rpc.get_icon("/"), rpc.get_icon("C:\\"));
    }

    #[tokio::test]
    async fn creating_a_directory_in_the_drives_root_is_rejected() {
        let err = DrivesRootRpc::new()
            .create_directory("/".to_string(), "new".to_string(), None)
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "Cannot create directory in drives root");
    }

    #[tokio::test]
    async fn renaming_in_the_drives_root_is_rejected() {
        let err = DrivesRootRpc::new()
            .rename_entry("/media/usb".to_string(), "/media/stick".to_string())
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "Cannot rename in drives root");
    }

    #[tokio::test]
    async fn deleting_in_the_drives_root_is_rejected() {
        let err = DrivesRootRpc::new()
            .delete_entries(vec!["/media/usb".to_string()])
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "Cannot delete in drives root");
    }

    #[tokio::test]
    async fn deleting_an_empty_batch_is_still_rejected() {
        assert!(DrivesRootRpc::new().delete_entries(Vec::new()).await.is_err());
    }
}
