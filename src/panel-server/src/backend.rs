use crate::{
    ApiCmd, ApiConnection, ApiDrive, ApiFileContent, ApiPanelState, ApiResult, PanelSide,
};

#[async_trait::async_trait(?Send)]
pub trait PanelBackend {
    async fn enter(&self, side: PanelSide, name: String) -> ApiResult<()>;
    async fn go_up(&self, side: PanelSide) -> ApiResult<()>;
    async fn go_back(&self, side: PanelSide) -> ApiResult<()>;
    async fn go_forward(&self, side: PanelSide) -> ApiResult<()>;
    async fn go_to_level(&self, side: PanelSide, level: usize) -> ApiResult<()>;
    async fn go_home(&self, side: PanelSide) -> ApiResult<()>;

    async fn add_tab(&self, side: PanelSide) -> ApiResult<()>;
    async fn close_tab(&self, side: PanelSide, id: u32) -> ApiResult<()>;
    async fn switch_tab(&self, side: PanelSide, id: u32) -> ApiResult<()>;

    fn read_state(&self, side: PanelSide) -> ApiPanelState;

    async fn delete(&self, side: PanelSide, paths: Vec<String>) -> ApiResult<()>;
    async fn mkdir(&self, side: PanelSide, name: String) -> ApiResult<()>;
    async fn rename(&self, side: PanelSide, old_path: String, new_name: String) -> ApiResult<()>;
    async fn copy(&self, src: PanelSide, dst: PanelSide, paths: Vec<String>) -> ApiResult<()>;
    async fn move_entries(&self, src: PanelSide, dst: PanelSide, paths: Vec<String>) -> ApiResult<()>;
    async fn read_file(&self, side: PanelSide, path: String) -> ApiResult<ApiFileContent>;
    async fn stream_file(&self, side: PanelSide, path: String) -> ApiResult<Vec<u8>>;
    async fn write_file(&self, side: PanelSide, path: String, content: String) -> ApiResult<()>;
    async fn upload_file(&self, side: PanelSide, path: String, data: Vec<u8>) -> ApiResult<()>;

    fn get_drives(&self) -> Vec<ApiDrive>;
    async fn activate_source(&self, side: PanelSide, key: String) -> ApiResult<()>;
    fn get_connections(&self) -> Vec<ApiConnection>;
    fn save_connection(&self, connection: ApiConnection) -> ApiResult<()>;
    fn delete_connection(&self, name: String) -> ApiResult<()>;
    async fn connect_to(&self, side: PanelSide, connection: ApiConnection) -> ApiResult<()>;
    async fn refresh_panel(&self, side: PanelSide) -> ApiResult<()>;
    fn get_settings(&self) -> ApiResult<serde_json::Value>;
    fn set_settings(&self, values: serde_json::Value) -> ApiResult<()>;
    fn viewer_content(&self) -> ApiResult<Option<(String, String, String)>>;
    fn set_connections_dialog(&self, open: bool) -> ApiResult<()>;
    fn export_connections(&self, password: Option<String>) -> ApiResult<String>;
    fn import_connections(&self, data: String, password: Option<String>) -> ApiResult<usize>;

    fn toggle_favorite(&self, path: String);
    fn get_favorites_only(&self) -> bool;
    fn set_favorites_only(&self, value: bool);
}

pub async fn dispatch_core<B: PanelBackend + ?Sized>(backend: &B, cmd: ApiCmd) -> Option<ApiCmd> {
    match cmd {
        ApiCmd::Enter { side, name, reply } => {
            let _ = backend.enter(side, name).await;
            let _ = reply.send(Ok(()));
        }
        ApiCmd::GoUp { side, reply } => {
            let _ = backend.go_up(side).await;
            let _ = reply.send(Ok(()));
        }
        ApiCmd::GoBack { side, reply } => {
            let _ = backend.go_back(side).await;
            let _ = reply.send(Ok(()));
        }
        ApiCmd::GoForward { side, reply } => {
            let _ = backend.go_forward(side).await;
            let _ = reply.send(Ok(()));
        }
        ApiCmd::Breadcrumb { side, level, reply } => {
            let _ = backend.go_to_level(side, level).await;
            let _ = reply.send(Ok(()));
        }
        ApiCmd::GoHome { side, reply } => {
            let _ = reply.send(backend.go_home(side).await);
        }

        ApiCmd::AddTab { side, reply } => {
            let _ = reply.send(backend.add_tab(side).await);
        }
        ApiCmd::CloseTab { side, id, reply } => {
            let _ = reply.send(backend.close_tab(side, id).await);
        }
        ApiCmd::SwitchTab { side, id, reply } => {
            let _ = reply.send(backend.switch_tab(side, id).await);
        }

        ApiCmd::GetPanelState { side, reply } => {
            let _ = reply.send(Ok(backend.read_state(side)));
        }

        ApiCmd::ReadFile { side, path, reply } => {
            let _ = reply.send(backend.read_file(side, path).await);
        }
        ApiCmd::StreamFile { side, path, reply } => {
            let _ = reply.send(backend.stream_file(side, path).await);
        }
        ApiCmd::WriteFile { side, path, content, reply } => {
            let _ = reply.send(backend.write_file(side, path, content).await);
        }
        ApiCmd::UploadFile { side, path, data, reply } => {
            let _ = reply.send(backend.upload_file(side, path, data).await);
        }

        ApiCmd::Delete { side, paths, reply } => {
            let _ = reply.send(Ok(()));
            let _ = backend.delete(side, paths).await;
        }
        ApiCmd::Mkdir { side, name, reply } => {
            let _ = reply.send(Ok(()));
            let _ = backend.mkdir(side, name).await;
        }
        ApiCmd::Rename { side, old_path, new_name, reply } => {
            let _ = reply.send(Ok(()));
            let _ = backend.rename(side, old_path, new_name).await;
        }
        ApiCmd::Copy { src_side, dst_side, paths, reply } => {
            let _ = reply.send(Ok(()));
            let _ = backend.copy(src_side, dst_side, paths).await;
        }
        ApiCmd::Move { src_side, dst_side, paths, reply } => {
            let _ = reply.send(Ok(()));
            let _ = backend.move_entries(src_side, dst_side, paths).await;
        }

        ApiCmd::GetDrives { reply } => {
            let _ = reply.send(Ok(backend.get_drives()));
        }
        ApiCmd::ActivateSource { side, key, reply } => {
            let _ = reply.send(backend.activate_source(side, key).await);
        }
        ApiCmd::GetConnections { reply } => {
            let _ = reply.send(Ok(backend.get_connections()));
        }
        ApiCmd::SaveConnection { connection, reply } => {
            let _ = reply.send(backend.save_connection(connection));
        }
        ApiCmd::DeleteConnection { name, reply } => {
            let _ = reply.send(backend.delete_connection(name));
        }
        ApiCmd::ConnectTo { side, connection, reply } => {
            let _ = reply.send(backend.connect_to(side, connection).await);
        }
        ApiCmd::RefreshPanel { side, reply } => {
            let _ = reply.send(backend.refresh_panel(side).await);
        }
        ApiCmd::GetSettings { reply } => {
            let _ = reply.send(backend.get_settings());
        }
        ApiCmd::SetSettings { values, reply } => {
            let _ = reply.send(backend.set_settings(values));
        }
        ApiCmd::GetViewerContent { reply } => {
            let _ = reply.send(backend.viewer_content());
        }
        ApiCmd::SetConnectionsDialog { open, reply } => {
            let _ = reply.send(backend.set_connections_dialog(open));
        }
        ApiCmd::ExportConnections { password, reply } => {
            let _ = reply.send(backend.export_connections(password));
        }
        ApiCmd::ImportConnections { data, password, reply } => {
            let _ = reply.send(backend.import_connections(data, password));
        }

        ApiCmd::ToggleFavorite { path, reply } => {
            backend.toggle_favorite(path);
            let _ = reply.send(Ok(()));
        }
        ApiCmd::GetFavoritesOnly { reply } => {
            let _ = reply.send(Ok(backend.get_favorites_only()));
        }
        ApiCmd::SetFavoritesOnly { value, reply } => {
            backend.set_favorites_only(value);
            let _ = reply.send(Ok(()));
        }

        other => return Some(other),
    }
    None
}
