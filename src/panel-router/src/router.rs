use std::cell::RefCell;
use std::rc::Rc;

use client_config::AppConfig;
use common::AppError;
use fm_core::rpc::{ContentWait, FileSystemRpc};
use gtk::glib;
use gtk_fm_ui::{BreadcrumbSegment, FileEntry, FmPanelInput, FmPanelModel, FmPanelOutput, SourceInfo};
use panel_core::RouterState;
use relm4::prelude::*;
use relm4::Controller;

fn join_display(dir: &str, name: &str) -> String {
    format!("/{}/{}", dir.trim_matches('/'), name)
        .replace("//", "/")
}

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
        Self { provider, display_prefix, rel_prefix }
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

    fn resolve(&self, abs: &str) -> String {
        let tail = abs.strip_prefix(&self.display_prefix).unwrap_or(abs);
        let mut parts: Vec<&str> = self.rel_prefix.split('/').filter(|s| !s.is_empty()).collect();
        parts.extend(tail.split('/').filter(|s| !s.is_empty()));
        format!("/{}", parts.join("/"))
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
    async fn extract_archive(&self, archive_path: String) -> Result<(), AppError> {
        self.provider.extract_archive(self.resolve(&archive_path)).await
    }
    async fn compress_to_archive(
        &self,
        entry_path: String,
        archive_path: String,
    ) -> Result<(), AppError> {
        self.provider
            .compress_to_archive(self.resolve(&entry_path), self.resolve(&archive_path))
            .await
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

    fn get_ssh_shell_target(&self, remote_path: &str) -> Option<fm_core::rpc::SshShellTarget> {
        self.provider.get_ssh_shell_target(remote_path)
    }
}

fn push_listing(state: &Rc<RouterState>, sender: &relm4::Sender<FmPanelInput>) {
    let (path, entries, root_fs, root_rel) = {
        let nav = state.path.borrow();
        let root = &nav.levels()[0];
        (
            nav.absolute_path(),
            (*nav.active().entries).clone(),
            root.fs.clone(),
            root.relative_path.clone(),
        )
    };
    let breadcrumb: Vec<BreadcrumbSegment> = {
        let nav = state.path.borrow();
        let mut acc = String::new();
        nav.levels()[1..]
            .iter()
            .map(|l| {
                acc.push('/');
                acc.push_str(&l.name);
                BreadcrumbSegment {
                    name: l.name.clone(),
                    path: acc.clone(),
                    icon: l.fs.get_icon(&l.relative_path),
                    icon_svg: l.fs.get_icon_svg(&l.relative_path),
                }
            })
            .collect()
    };
    let is_local = root_fs.is_local();
    let source = SourceInfo {
        is_local,
        display_name: root_fs.display_name(),
        fs_label: Some(if is_local { "Local FileSystem" } else { "Remote FileSystem" }.to_string()),
        root_icon: root_fs.get_icon(&root_rel),
        root_icon_svg: root_fs.get_icon_svg(&root_rel),
        connection_id: root_fs.connection_id(),
    };
    let select_name = {
        let nav = state.path.borrow();
        let abs = nav.absolute_path();
        nav.active().selected.clone().or_else(|| state.get_last_selected(&abs))
    };
    let _ = sender.send(FmPanelInput::Listing {
        path,
        entries,
        breadcrumb,
        source,
        select_name,
    });
    let _ = sender.send(FmPanelInput::SetHistory {
        can_back: state.can_go_back(),
        can_forward: state.can_go_forward(),
    });
}

async fn guarded_load<T>(
    state: &Rc<RouterState>,
    sender: &relm4::Sender<FmPanelInput>,
    fut: impl std::future::Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    let _ = sender.send(FmPanelInput::Loading);
    let res = match state.active_provider().content_wait() {
        ContentWait::Infinite => fut.await,
        ContentWait::Bounded(d) => {
            let timer = glib::timeout_future(d);
            futures::pin_mut!(fut);
            match futures::future::select(fut, timer).await {
                futures::future::Either::Left((r, _)) => r,
                futures::future::Either::Right((_, _)) => Err(AppError::Other(format!(
                    "Timed out after {}s waiting for the directory listing.",
                    d.as_secs()
                ))),
            }
        }
    };
    match res {
        Ok(v) => {
            push_listing(state, sender);
            Ok(v)
        }
        Err(e) => {
            let _ = sender.send(FmPanelInput::LoadFailed {
                message: e.to_string(),
            });
            Err(e)
        }
    }
}

pub struct PanelRouter {
    pub fm: Controller<FmPanelModel>,
    pub state: Rc<RouterState>,
    panel_id: String,
    config: AppConfig,
    resource_id: RefCell<String>,
    selection: RefCell<Vec<(String, bool)>>,
    show_selector_fn: RefCell<Option<Box<dyn Fn(bool)>>>,
}

impl PanelRouter {
    pub fn new(
        fm: Controller<FmPanelModel>,
        base_provider: Rc<dyn FileSystemRpc>,
        local_provider: Rc<dyn FileSystemRpc>,
        mount_point: String,
        panel_id: impl Into<String>,
        config: AppConfig,
    ) -> Rc<Self> {
        Rc::new(Self {
            fm,
            state: Rc::new(RouterState::new(base_provider, local_provider, mount_point)),
            panel_id: panel_id.into(),
            config,
            resource_id: RefCell::new("local_fs".to_string()),
            selection: RefCell::new(Vec::new()),
            show_selector_fn: RefCell::new(None),
        })
    }

    pub fn render(&self) {
        push_listing(&self.state, self.fm.sender());
    }

    pub async fn refresh(&self) -> Result<(), AppError> {
        guarded_load(&self.state, self.fm.sender(), self.state.refresh()).await
    }

    pub async fn enter(&self, name: &str) -> Result<(), AppError> {
        guarded_load(&self.state, self.fm.sender(), self.state.enter(name)).await
    }

    pub async fn go_up(&self) -> Result<(), AppError> {
        guarded_load(&self.state, self.fm.sender(), self.state.go_up()).await
    }

    pub async fn go_to_level(&self, idx: usize) -> Result<(), AppError> {
        guarded_load(&self.state, self.fm.sender(), self.state.go_to_level(idx)).await
    }

    pub async fn go_back(&self) -> Result<(), AppError> {
        guarded_load(&self.state, self.fm.sender(), self.state.go_back()).await
    }

    pub async fn go_forward(&self) -> Result<(), AppError> {
        guarded_load(&self.state, self.fm.sender(), self.state.go_forward()).await
    }

    pub async fn navigate_typed(&self, input: String) -> Result<bool, AppError> {
        guarded_load(&self.state, self.fm.sender(), self.state.navigate_typed(input)).await
    }

    pub fn reset_to_base(&self) {
        *self.resource_id.borrow_mut() = "local_fs".to_string();
        self.state.reset_to_base();
        self.relist_spawned();
    }

    pub fn set_active_provider(&self, provider: Rc<dyn FileSystemRpc>, resource_id: impl Into<String>) {
        *self.resource_id.borrow_mut() = resource_id.into();
        self.state.set_active_provider(provider, String::new());
        self.relist_spawned();
    }

    pub fn mount_provider(
        &self,
        provider: Rc<dyn FileSystemRpc>,
        resource_id: impl Into<String>,
        path: String,
    ) {
        *self.resource_id.borrow_mut() = resource_id.into();
        self.state.set_active_provider(provider, String::new());
        let state = self.state.clone();
        let sender = self.fm.sender().clone();
        glib::spawn_future_local(async move {
            let _ = guarded_load(&state, &sender, state.navigate_typed(path)).await;
        });
    }

    pub fn open_local_path(&self, path: String) {
        *self.resource_id.borrow_mut() = "local_fs".to_string();
        self.state.reset_to_base();
        let state = self.state.clone();
        let sender = self.fm.sender().clone();
        glib::spawn_future_local(async move {
            let _ = guarded_load(&state, &sender, state.navigate_typed(path)).await;
        });
    }

    fn relist_spawned(&self) {
        let state = self.state.clone();
        let sender = self.fm.sender().clone();
        glib::spawn_future_local(async move {
            let _ = guarded_load(&state, &sender, state.list_active()).await;
        });
    }

    async fn finish_mutation(&self, result: Result<(), AppError>, fail_title: &str) {
        match result {
            Ok(()) => {
                let _ = self.refresh().await;
            }
            Err(e) => {
                let _ = self.fm.sender().send(FmPanelInput::OpFailed {
                    title: fail_title.to_string(),
                    message: e.to_string(),
                });
            }
        }
    }

    pub async fn mkdir(&self, parent: String, name: String) {
        let rel = self.state.resolve_relative(&parent);
        let r = self.state.active_provider().create_directory(rel, name, None).await;
        self.finish_mutation(r, "Create Directory Failed").await;
    }

    pub async fn delete(&self, paths: Vec<String>) {
        let rel: Vec<String> = paths.iter().map(|p| self.state.resolve_relative(p)).collect();
        let r = self.state.active_provider().delete_entries(rel).await;
        self.finish_mutation(r, "Delete Failed").await;
    }

    pub async fn rename(&self, old_path: String, new_path: String) {
        let (o, n) = (self.state.resolve_relative(&old_path), self.state.resolve_relative(&new_path));
        let r = self.state.active_provider().rename_entry(o, n).await;
        self.finish_mutation(r, "Rename Failed").await;
    }

    pub async fn chmod(&self, path: String, mode: u32) {
        let rel = self.state.resolve_relative(&path);
        let r = self.state.active_provider().set_permissions(rel, mode).await;
        self.finish_mutation(r, "Change Permissions Failed").await;
    }

    pub async fn duplicate(&self, src: String, dst: String) {
        let (s, d) = (self.state.resolve_relative(&src), self.state.resolve_relative(&dst));
        let r = self.state.active_provider().duplicate_entry(s, d).await;
        self.finish_mutation(r, "Duplicate Failed").await;
    }

    pub async fn handle_output(&self, out: FmPanelOutput) -> Option<FmPanelOutput> {
        match out {
            FmPanelOutput::NavigateEnter(name) => {
                let _ = self.enter(&name).await;
                None
            }
            FmPanelOutput::NavigateUp => {
                let _ = self.go_up().await;
                None
            }
            FmPanelOutput::NavigateLevel(idx) => {
                let _ = self.go_to_level(idx).await;
                None
            }
            FmPanelOutput::NavigateTyped(input) => {
                if let Ok(false) = self.navigate_typed(input).await {
                    let _ = self.fm.sender().send(FmPanelInput::AddressNotResolved);
                }
                None
            }
            FmPanelOutput::HistoryBack => {
                let _ = self.go_back().await;
                None
            }
            FmPanelOutput::HistoryForward => {
                let _ = self.go_forward().await;
                None
            }
            FmPanelOutput::Refresh => {
                let _ = self.refresh().await;
                None
            }
            FmPanelOutput::Mkdir { parent, name } => {
                self.mkdir(parent, name).await;
                None
            }
            FmPanelOutput::Delete { paths, .. } => {
                self.delete(paths).await;
                None
            }
            FmPanelOutput::Rename { old_path, new_path } => {
                self.rename(old_path, new_path).await;
                None
            }
            FmPanelOutput::Chmod { path, mode } => {
                self.chmod(path, mode).await;
                None
            }
            FmPanelOutput::Duplicate { src, dst } => {
                self.duplicate(src, dst).await;
                None
            }
            FmPanelOutput::StateChanged { selected, cursor, .. } => {
                *self.selection.borrow_mut() = selected;
                self.state.set_selected(cursor);
                None
            }
            FmPanelOutput::ViewModeChanged(_) | FmPanelOutput::SortChanged { .. } => None,
            other => Some(other),
        }
    }

    pub async fn goto(&self, path: String) {
        let _ = self.navigate_typed(path).await;
    }

    pub fn start_rename(&self) {
        let _ = self.fm.sender().send(FmPanelInput::StartRename);
    }

    pub fn request_create_dir(&self) {
        let _ = self.fm.sender().send(FmPanelInput::RequestCreateDir);
    }

    pub fn request_delete(&self) {
        let _ = self.fm.sender().send(FmPanelInput::RequestDelete);
    }

    pub fn show_filter_bar(&self) {
        let _ = self.fm.sender().send(FmPanelInput::ShowFilterBar);
    }

    pub fn re_render(&self) {
        let _ = self.fm.sender().send(FmPanelInput::ReRender);
    }

    pub fn set_view_mode(&self, mode: String) {
        let _ = self.fm.sender().send(FmPanelInput::SetViewMode(mode));
    }

    pub fn current_path_string(&self) -> String {
        self.state.path.borrow().absolute_path()
    }

    pub fn provider(&self) -> Rc<dyn FileSystemRpc> {
        Rc::new(RoutingProvider::snapshot(&self.state))
    }

    pub fn window(&self) -> Option<gtk::Window> {
        use gtk::prelude::{Cast, WidgetExt};
        self.fm.widget().root().and_then(|r| r.downcast::<gtk::Window>().ok())
    }

    pub fn config(&self) -> AppConfig {
        self.config.clone()
    }

    pub fn show_hidden_config_key(&self) -> String {
        let shared = self
            .config
            .get::<bool>("ui.show_hidden_files_shared")
            .unwrap_or(true);
        if shared || self.panel_id == "default" {
            "ui.show_hidden_files".to_string()
        } else {
            format!("ui.show_hidden_files_{}", self.panel_id)
        }
    }

    fn icon_size_key(&self) -> String {
        if self.panel_id == "default" {
            "ui.fm_grid_icon_size".to_string()
        } else {
            format!("ui.fm_grid_icon_size_{}", self.panel_id)
        }
    }

    pub fn view_mode(&self) -> String {
        match self.config.get::<u32>(&self.icon_size_key()).unwrap_or(80) {
            30 => "list",
            40 => "small",
            _ => "large",
        }
        .to_string()
    }

    pub fn should_skip_entry(&self, name: &str) -> bool {
        let show_hidden = self
            .config
            .get::<bool>(&self.show_hidden_config_key())
            .unwrap_or(false);
        !show_hidden && cfg!(unix) && name.starts_with('.')
    }

    pub fn set_sort(&self, column: String, descending: bool) {
        let _ = self.fm.sender().send(FmPanelInput::SetSort { column, descending });
    }

    pub fn current_resource_id(&self) -> String {
        self.resource_id.borrow().clone()
    }

    pub fn core_selected_name(&self) -> Option<String> {
        let nav = self.state.path.borrow();
        let abs = nav.absolute_path();
        nav.active()
            .selected
            .clone()
            .or_else(|| self.state.get_last_selected(&abs))
    }

    pub fn set_clipboard_state(&self, count: usize, mine: bool, cut: bool) {
        let _ = self
            .fm
            .sender()
            .send(gtk_fm_ui::FmPanelInput::ClipboardState { count, mine, cut });
    }

    pub fn panel_id(&self) -> &str {
        &self.panel_id
    }

    pub fn current_entry_names(&self) -> Vec<String> {
        self.state
            .path
            .borrow()
            .active()
            .entries
            .iter()
            .map(|e| e.name.clone())
            .collect()
    }

    pub fn selected_entries(&self) -> Vec<FileEntry> {
        use chrono::TimeZone;
        let sel = self.selection.borrow();
        let nav = self.state.path.borrow();
        let dir = nav.absolute_path();
        let entries = nav.active().entries.clone();
        sel.iter()
            .map(|(name, is_dir)| {
                let full = join_display(&dir, name);
                let (size, date, perms) = entries
                    .iter()
                    .find(|e| &e.name == name)
                    .map(|e| {
                        let date = match chrono::Local.timestamp_opt(e.modified as i64, 0) {
                            chrono::LocalResult::Single(dt) if e.modified != 0 => {
                                dt.format("%Y-%m-%d %H:%M:%S").to_string()
                            }
                            _ => String::new(),
                        };
                        (e.size, date, e.permissions)
                    })
                    .unwrap_or((0, String::new(), None));
                FileEntry::new(name, &full, *is_dir, size, &date, perms)
            })
            .collect()
    }

    pub fn set_on_navigated(&self, f: impl Fn() + 'static) {
        self.state.set_on_changed(f);
    }

    pub fn refresh_spawned(self: &Rc<Self>) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            let _ = this.refresh().await;
        });
    }

    pub fn open_path(self: &Rc<Self>, path: String) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            this.goto(path).await;
        });
    }

    pub fn go_up_spawned(self: &Rc<Self>) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            let _ = this.go_up().await;
        });
    }

    pub fn cancel_editing(&self) {
        let _ = self.fm.sender().send(FmPanelInput::CancelEditing);
    }

    pub fn grab_focus(&self) {
        let _ = self.fm.sender().send(FmPanelInput::GrabFocus);
    }

    pub fn set_show_selector_fn(&self, f: impl Fn(bool) + 'static) {
        *self.show_selector_fn.borrow_mut() = Some(Box::new(f));
    }

    pub fn switch_to_selector(&self, show: bool) {
        self.state.showing_selector.set(show);
        if let Some(f) = self.show_selector_fn.borrow().as_ref() {
            f(show);
        }
    }

    pub fn is_showing_selector(&self) -> bool {
        self.state.showing_selector.get()
    }

    pub fn sync_selector_state(&self, is_selector: bool) {
        self.state.showing_selector.set(is_selector);
    }

    pub fn can_go_back(&self) -> bool {
        self.state.can_go_back()
    }

    pub fn can_go_forward(&self) -> bool {
        self.state.can_go_forward()
    }

    pub fn clear_history(&self) {
        self.state.clear_history_state();
        self.render();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use panel_core::nav::PathLevel;

    struct Dummy;
    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Dummy {}

    fn rp(display_prefix: &str, rel_prefix: &str) -> RoutingProvider {
        RoutingProvider {
            provider: Rc::new(Dummy),
            display_prefix: display_prefix.to_string(),
            rel_prefix: rel_prefix.to_string(),
        }
    }

    #[test]
    fn resolve_strips_archive_display_prefix_to_archive_relative() {
        let r = rp("/linux.txt.zip", "/");
        assert_eq!(r.resolve("/linux.txt.zip/readme.txt"), "/readme.txt");
        assert_eq!(r.resolve("/linux.txt.zip/sub/a.txt"), "/sub/a.txt");
        assert_eq!(r.resolve("/linux.txt.zip"), "/");
    }

    #[test]
    fn resolve_from_a_subdir_inside_the_archive() {
        let r = rp("/linux.txt.zip/sub", "/sub");
        assert_eq!(r.resolve("/linux.txt.zip/sub/a.txt"), "/sub/a.txt");
        assert_eq!(r.resolve("/linux.txt.zip/sub/deep/b.txt"), "/sub/deep/b.txt");
    }

    #[test]
    fn resolve_is_identity_on_a_plain_local_dir() {
        let r = rp("/home/me/docs", "/home/me/docs");
        assert_eq!(r.resolve("/home/me/docs/file.txt"), "/home/me/docs/file.txt");
        let root = rp("/", "/");
        assert_eq!(root.resolve("/etc/hosts"), "/etc/hosts");
    }

    #[test]
    fn resolve_collapses_duplicated_and_trailing_slashes() {
        let r = rp("/linux.txt.zip", "/");
        assert_eq!(r.resolve("/linux.txt.zip//sub///a.txt"), "/sub/a.txt");
        assert_eq!(r.resolve("/linux.txt.zip/sub/"), "/sub");
        assert_eq!(r.resolve("/linux.txt.zip/"), "/");

        let deep = rp("/linux.txt.zip/sub", "/sub");
        assert_eq!(deep.resolve("/linux.txt.zip/sub//deep//b.txt"), "/sub/deep/b.txt");
    }

    #[test]
    fn resolve_of_the_mount_point_itself_is_the_level_root() {
        let r = rp("/docs/a.zip/sub", "/sub");
        assert_eq!(r.resolve("/docs/a.zip/sub"), "/sub");
        assert_eq!(r.resolve("/docs/a.zip/sub/"), "/sub");
    }

    #[test]
    fn resolve_of_an_empty_path_is_the_level_root() {
        assert_eq!(rp("/a.zip", "/").resolve(""), "/");
        assert_eq!(rp("/a.zip/sub", "/sub").resolve(""), "/sub");
    }

    #[test]
    fn resolve_of_the_display_root_from_a_deeper_level_is_that_level() {
        let r = rp("/a.zip/sub", "/sub");
        assert_eq!(r.resolve("/"), "/sub");
    }

    #[test]
    fn resolve_keeps_dot_dot_segments_verbatim() {
        let r = rp("/a.zip/sub", "/sub");
        assert_eq!(r.resolve("/a.zip/sub/../other/x"), "/sub/../other/x");
        assert_eq!(r.resolve("/a.zip/sub/."), "/sub/.");
    }

    #[test]
    fn resolve_keeps_a_backslash_inside_one_component() {
        let r = rp("/a.zip", "/");
        assert_eq!(r.resolve("/a.zip/dir\\file.txt"), "/dir\\file.txt");
        assert_eq!(r.resolve("/a.zip/C:\\win\\file"), "/C:\\win\\file");
    }

    #[test]
    fn resolve_of_a_level_zero_path_is_left_untouched() {
        let r = rp("/docs/a.zip", "/");
        assert_eq!(r.resolve("/docs/readme.txt"), "/docs/readme.txt");
        assert_eq!(r.resolve("/docs/a.zip/readme.txt"), "/readme.txt");
    }

    #[test]
    fn resolve_inside_nested_archives_strips_the_whole_display_prefix() {
        let r = rp("/docs/outer.zip/inner.tar.gz/deep", "/deep");
        assert_eq!(r.resolve("/docs/outer.zip/inner.tar.gz/deep/f.txt"), "/deep/f.txt");
        assert_eq!(r.resolve("/docs/outer.zip/inner.tar.gz/deep/d/f.txt"), "/deep/d/f.txt");

        let at_mount = rp("/docs/outer.zip/inner.tar.gz", "/");
        assert_eq!(at_mount.resolve("/docs/outer.zip/inner.tar.gz/f.txt"), "/f.txt");
    }

    #[test]
    fn resolve_preserves_unicode_and_spaces_in_names() {
        let r = rp("/архив.zip", "/");
        assert_eq!(r.resolve("/архив.zip/документы/файл 1.txt"), "/документы/файл 1.txt");
        assert_eq!(r.resolve("/архив.zip/🙂 dir/🙂.bin"), "/🙂 dir/🙂.bin");
    }

    #[test]
    fn resolve_preserves_order_of_a_very_deep_path() {
        let deep: String = (0..100).map(|i| format!("/d{}", i)).collect();
        let r = rp("/mnt/big.zip", "/");
        assert_eq!(r.resolve(&format!("/mnt/big.zip{}/f.txt", deep)), format!("{}/f.txt", deep));
    }

    #[test]
    fn resolve_with_an_empty_relative_prefix_yields_the_display_tail() {
        let r = RoutingProvider::from_parts(Rc::new(Dummy), "/a.zip".to_string(), String::new());
        assert_eq!(r.rel_prefix(), "");
        assert_eq!(r.resolve("/a.zip/sub/x.txt"), "/sub/x.txt");
        assert_eq!(r.resolve("/a.zip"), "/");
    }

    #[test]
    fn resolve_tolerates_a_display_prefix_with_a_trailing_slash() {
        let r = rp("/a.zip/", "/");
        assert_eq!(r.resolve("/a.zip/sub/x.txt"), "/sub/x.txt");
    }

    struct Recorder {
        calls: RefCell<Vec<String>>,
    }

    impl Recorder {
        fn new() -> Rc<Self> {
            Rc::new(Self { calls: RefCell::new(Vec::new()) })
        }
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Recorder {
        async fn list_dir(&self, path: String) -> Result<Vec<fm_core::rpc::RemoteFileEntry>, AppError> {
            self.calls.borrow_mut().push(format!("list_dir {}", path));
            Ok(Vec::new())
        }
        async fn create_directory(
            &self,
            parent_path: String,
            dir_name: String,
            _permissions: Option<u32>,
        ) -> Result<(), AppError> {
            self.calls.borrow_mut().push(format!("mkdir {} | {}", parent_path, dir_name));
            Ok(())
        }
        async fn delete_entries(&self, paths: Vec<String>) -> Result<(), AppError> {
            self.calls.borrow_mut().push(format!("delete {}", paths.join(" ")));
            Ok(())
        }
        async fn rename_entry(&self, path: String, new_path: String) -> Result<(), AppError> {
            self.calls.borrow_mut().push(format!("rename {} -> {}", path, new_path));
            Ok(())
        }
        fn request_file_download(&self, file_path: String, _transfer_id: uuid::Uuid) {
            self.calls.borrow_mut().push(format!("download {}", file_path));
        }
        fn is_local(&self) -> bool {
            true
        }
        fn is_read_only(&self) -> bool {
            true
        }
        fn is_root_fs(&self) -> bool {
            true
        }
        fn connection_id(&self) -> Option<String> {
            Some("conn-7".to_string())
        }
        fn display_name(&self) -> Option<String> {
            Some("recorder".to_string())
        }
        fn get_icon(&self, path: &str) -> String {
            format!("icon:{}", path)
        }
    }

    fn recording(display_prefix: &str, rel_prefix: &str) -> (Rc<Recorder>, RoutingProvider) {
        let rec = Recorder::new();
        let rp = RoutingProvider::from_parts(
            rec.clone(),
            display_prefix.to_string(),
            rel_prefix.to_string(),
        );
        (rec, rp)
    }

    #[test]
    fn list_dir_reaches_the_inner_provider_with_a_level_relative_path() {
        let (rec, r) = recording("/docs/a.zip/sub", "/sub");
        let res = futures::executor::block_on(r.list_dir("/docs/a.zip/sub/inner".to_string()));
        assert!(res.is_ok());
        assert_eq!(rec.calls(), vec!["list_dir /sub/inner".to_string()]);
    }

    #[test]
    fn delete_entries_resolves_every_path_of_the_batch() {
        let (rec, r) = recording("/a.zip/sub", "/sub");
        let _ = futures::executor::block_on(r.delete_entries(vec![
            "/a.zip/sub/a.txt".to_string(),
            "/a.zip/sub//b.txt".to_string(),
        ]));
        assert_eq!(rec.calls(), vec!["delete /sub/a.txt /sub/b.txt".to_string()]);
    }

    #[test]
    fn rename_entry_resolves_both_sides() {
        let (rec, r) = recording("/a.zip/sub", "/sub");
        let _ = futures::executor::block_on(
            r.rename_entry("/a.zip/sub/old.txt".to_string(), "/a.zip/sub/new.txt".to_string()),
        );
        assert_eq!(rec.calls(), vec!["rename /sub/old.txt -> /sub/new.txt".to_string()]);
    }

    #[test]
    fn create_directory_resolves_the_parent_but_not_the_new_name() {
        let (rec, r) = recording("/a.zip/sub", "/sub");
        let _ = futures::executor::block_on(r.create_directory(
            "/a.zip/sub".to_string(),
            "new/name".to_string(),
            None,
        ));
        assert_eq!(rec.calls(), vec!["mkdir /sub | new/name".to_string()]);
    }

    #[test]
    fn request_file_download_resolves_the_display_path() {
        let (rec, r) = recording("/a.zip/sub", "/sub");
        r.request_file_download("/a.zip/sub/f.bin".to_string(), uuid::Uuid::nil());
        assert_eq!(rec.calls(), vec!["download /sub/f.bin".to_string()]);
    }

    #[test]
    fn capability_queries_are_delegated_to_the_inner_provider() {
        let (_rec, r) = recording("/a.zip", "/");
        assert!(r.is_local());
        assert!(r.is_read_only());
        assert!(r.is_root_fs());
        assert_eq!(r.connection_id().as_deref(), Some("conn-7"));
        assert_eq!(r.display_name().as_deref(), Some("recorder"));
        assert!(r.get_icon("/a.zip/x.txt").starts_with("icon:"));
    }

    #[test]
    fn inner_returns_the_wrapped_provider_itself() {
        let inner: Rc<dyn FileSystemRpc> = Rc::new(Dummy);
        let r = RoutingProvider::from_parts(inner.clone(), "/a.zip".to_string(), "/".to_string());
        assert!(Rc::ptr_eq(&r.inner(), &inner));
        assert_eq!(r.display_prefix(), "/a.zip");
        assert_eq!(r.rel_prefix(), "/");
    }

    fn state_with(levels: Vec<PathLevel>) -> Rc<RouterState> {
        let base: Rc<dyn FileSystemRpc> = Rc::new(Dummy);
        let state = Rc::new(RouterState::new(base.clone(), base, "/".to_string()));
        for l in levels {
            state.path.borrow_mut().push(l);
        }
        state
    }

    #[test]
    fn snapshot_of_an_untouched_panel_is_rooted() {
        let state = state_with(Vec::new());
        let snap = RoutingProvider::snapshot(&state);
        assert_eq!(snap.display_prefix(), "/");
        assert_eq!(snap.rel_prefix(), "/");
        assert_eq!(snap.resolve("/etc/hosts"), "/etc/hosts");
    }

    #[test]
    fn snapshot_takes_the_active_level_provider_and_prefixes() {
        let local: Rc<dyn FileSystemRpc> = Rc::new(Dummy);
        let archive: Rc<dyn FileSystemRpc> = Rc::new(Dummy);
        let state = state_with(vec![
            PathLevel::new("docs", "/docs", local.clone()),
            PathLevel::new("a.zip", "/", archive.clone()),
            PathLevel::new("sub", "/sub", archive.clone()),
        ]);
        let snap = RoutingProvider::snapshot(&state);
        assert_eq!(snap.display_prefix(), "/docs/a.zip/sub");
        assert_eq!(snap.rel_prefix(), "/sub");
        assert!(Rc::ptr_eq(&snap.inner(), &archive));
        assert_eq!(snap.resolve("/docs/a.zip/sub/f.txt"), "/sub/f.txt");
    }

    #[test]
    fn snapshot_is_frozen_against_later_navigation() {
        let archive: Rc<dyn FileSystemRpc> = Rc::new(Dummy);
        let state = state_with(vec![PathLevel::new("a.zip", "/", archive.clone())]);
        let snap = RoutingProvider::snapshot(&state);

        state.path.borrow_mut().push(PathLevel::new("sub", "/sub", archive.clone()));
        state.path.borrow_mut().push(PathLevel::new("deeper", "/sub/deeper", archive));

        assert_eq!(snap.display_prefix(), "/a.zip");
        assert_eq!(snap.rel_prefix(), "/");
        assert_eq!(snap.resolve("/a.zip/f.txt"), "/f.txt");
    }

    #[test]
    fn join_display_from_the_root_directory_does_not_double_the_slash() {
        assert_eq!(join_display("/", "etc"), "/etc");
        assert_eq!(join_display("", "etc"), "/etc");
    }

    #[test]
    fn join_display_trims_and_collapses_separators() {
        assert_eq!(join_display("/a/b", "c.txt"), "/a/b/c.txt");
        assert_eq!(join_display("/a/b/", "c.txt"), "/a/b/c.txt");
        assert_eq!(join_display("/a//b", "c.txt"), "/a/b/c.txt");
        assert_eq!(join_display("/a/b", "файл 1.txt"), "/a/b/файл 1.txt");
    }
}
