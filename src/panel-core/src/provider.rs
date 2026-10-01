use std::rc::Rc;

use common::AppError;
use fm_core::rpc::FileSystemRpc;

use crate::RouterState;

pub struct RoutingProvider {
    provider: Rc<dyn FileSystemRpc>,
    display_prefix: String,
    rel_prefix: String,
}

impl RoutingProvider {
    pub fn snapshot(state: &RouterState) -> Self {
        let nav = state.path.borrow();
        Self {
            provider: nav.active().fs.clone(),
            display_prefix: nav.absolute_path(),
            rel_prefix: nav.active().relative_path.clone(),
        }
    }

    pub fn from_parts(
        provider: Rc<dyn FileSystemRpc>,
        display_prefix: String,
        rel_prefix: String,
    ) -> Self {
        Self {
            provider,
            display_prefix,
            rel_prefix,
        }
    }

    pub fn inner(&self) -> Rc<dyn FileSystemRpc> {
        self.provider.clone()
    }
    pub fn display_prefix(&self) -> &str {
        &self.display_prefix
    }
    pub fn rel_prefix(&self) -> &str {
        &self.rel_prefix
    }

    pub fn resolve(&self, abs: &str) -> String {
        let tail = abs.strip_prefix(&self.display_prefix).unwrap_or(abs);
        let mut parts = fm_core::path::split_joined(&self.rel_prefix);
        parts.extend(fm_core::path::split_joined(tail));
        fm_core::path::join_segment_names(&parts)
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystemRpc for RoutingProvider {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    async fn list_dir(&self, path: String) -> Result<Vec<fm_core::rpc::RemoteFileEntry>, AppError> {
        self.provider.list_dir(self.resolve(&path)).await
    }
    async fn create_directory(
        &self,
        parent_path: String,
        dir_name: String,
        permissions: Option<u32>,
    ) -> Result<(), AppError> {
        self.provider
            .create_directory(self.resolve(&parent_path), dir_name, permissions)
            .await
    }
    async fn delete_entries(&self, paths: Vec<String>) -> Result<(), AppError> {
        let rel: Vec<String> = paths.iter().map(|p| self.resolve(p)).collect();
        self.provider.delete_entries(rel).await
    }
    async fn rename_entry(&self, path: String, new_path: String) -> Result<(), AppError> {
        self.provider
            .rename_entry(self.resolve(&path), self.resolve(&new_path))
            .await
    }
    async fn duplicate_entry(&self, src: String, dst: String) -> Result<(), AppError> {
        self.provider
            .duplicate_entry(self.resolve(&src), self.resolve(&dst))
            .await
    }
    async fn get_permissions(&self, path: String) -> Result<u32, AppError> {
        self.provider.get_permissions(self.resolve(&path)).await
    }
    async fn set_permissions(&self, path: String, permissions: u32) -> Result<(), AppError> {
        self.provider
            .set_permissions(self.resolve(&path), permissions)
            .await
    }
    fn supports_permissions(&self) -> bool {
        self.provider.supports_permissions()
    }
    async fn read_file(
        &self,
        path: String,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<Vec<u8>, AppError> {
        self.provider
            .read_file(self.resolve(&path), progress_callback)
            .await
    }
    async fn read_file_opt(
        &self,
        path: String,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        blocking: bool,
    ) -> Result<Vec<u8>, AppError> {
        self.provider
            .read_file_opt(self.resolve(&path), progress_callback, blocking)
            .await
    }
    async fn write_file(
        &self,
        path: String,
        content: Vec<u8>,
        permissions: Option<u32>,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<(), AppError> {
        self.provider
            .write_file(self.resolve(&path), content, permissions, progress_callback)
            .await
    }
    async fn read_at(&self, path: String, offset: u64, len: usize) -> Result<Vec<u8>, AppError> {
        self.provider
            .read_at(self.resolve(&path), offset, len)
            .await
    }
    async fn write_at(&self, path: String, offset: u64, data: Vec<u8>) -> Result<(), AppError> {
        self.provider
            .write_at(self.resolve(&path), offset, data)
            .await
    }
    async fn set_file_length(&self, path: String, len: u64) -> Result<(), AppError> {
        self.provider
            .set_file_length(self.resolve(&path), len)
            .await
    }
    fn supports_offset_io(&self) -> bool {
        self.provider.supports_offset_io()
    }
    fn extra_columns(&self) -> Vec<fm_core::rpc::ColumnSpec> {
        self.provider.extra_columns()
    }
    fn columns_replace_defaults(&self) -> bool {
        self.provider.columns_replace_defaults()
    }

    fn plugin_scope(&self) -> Option<String> {
        self.provider.plugin_scope()
    }

    fn plugin_mount(&self) -> Option<usize> {
        self.provider.plugin_mount()
    }

    fn plugin_action_state(&self, action_id: &str) -> u32 {
        self.provider.plugin_action_state(action_id)
    }

    fn toggle_cell(&self, dir: &str, name: &str, column: &str, ticked: bool) -> bool {
        self.provider.toggle_cell(dir, name, column, ticked)
    }
    fn wants_quick_filter(&self) -> bool {
        self.provider.wants_quick_filter()
    }
    fn request_file_download(&self, file_path: String, transfer_id: uuid::Uuid) {
        self.provider
            .request_file_download(self.resolve(&file_path), transfer_id);
    }
    fn trigger_file_upload(
        &self,
        target_path: String,
        file_name: String,
        local_file_path: std::path::PathBuf,
        transfer_id: uuid::Uuid,
    ) {
        self.provider.trigger_file_upload(
            self.resolve(&target_path),
            file_name,
            local_file_path,
            transfer_id,
        );
    }

    fn is_local(&self) -> bool {
        self.provider.is_local()
    }
    fn runs_off_thread(&self) -> bool {
        self.provider.runs_off_thread()
    }
    fn fs_id(&self) -> String {
        self.provider.fs_id()
    }
    fn is_read_only(&self) -> bool {
        self.provider.is_read_only()
    }
    fn is_root_fs(&self) -> bool {
        self.provider.is_root_fs()
    }
    fn connection_id(&self) -> Option<String> {
        self.provider.connection_id()
    }
    fn display_name(&self) -> Option<String> {
        self.provider.display_name()
    }
    fn get_icon(&self, path: &str) -> String {
        self.provider.get_icon(path)
    }
    fn get_last_selected(&self, path: &str) -> Option<String> {
        self.provider.get_last_selected(path)
    }
    fn get_ssh_connection_command(&self, remote_path: &str) -> Option<Vec<String>> {
        self.provider.get_ssh_connection_command(remote_path)
    }

    fn supports_terminal(&self) -> bool {
        self.provider.supports_terminal()
    }
    fn open_shell(
        &self,
        cwd: &str,
        rows: u16,
        cols: u16,
    ) -> Option<ic_platform::terminal::PtySession> {
        self.provider.open_shell(cwd, rows, cols)
    }
}
