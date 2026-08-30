use fm_core::rpc::FileSystemRpc as RouterRpc;
use gtk::glib;

pub use panel_server::*;

#[derive(Clone)]
pub struct TerminalBridge {
    pub open: std::rc::Rc<dyn Fn()>,
    pub input_tx: std::rc::Rc<std::cell::RefCell<Option<tokio::sync::mpsc::Sender<Vec<u8>>>>>,
}
impl From<ApiConnection> for crate::connection_manager::FtpConnection {
    fn from(c: ApiConnection) -> Self {
        Self {
            name: c.name,
            folder: None,
            protocol: c.protocol,
            host: c.host,
            port: c.port,
            user: c.user,
            pass: c.pass,
            auth_type: c.auth_type,
            key_path: c.key_path,
            passphrase: c.passphrase,
            remote_path: c.remote_path,
            use_tunnel: c.use_tunnel,
            tunnel_host: c.tunnel_host,
            tunnel_port: c.tunnel_port,
            tunnel_user: c.tunnel_user,
            tunnel_auth_type: c.tunnel_auth_type,
            tunnel_pass: c.tunnel_pass,
            tunnel_key_path: c.tunnel_key_path,
            tunnel_passphrase: c.tunnel_passphrase,
        }
    }
}
fn read_panel_state(router: &panel_router::PanelRouter) -> ApiPanelState {
    let path_ref = router.state.path.borrow();
    let api_levels: Vec<ApiLevel> = path_ref.levels().iter().enumerate().map(|(i, l)| {
        let label = if i == 0 { l.fs.display_name() } else { None };
        let icon = label.as_ref().map(|_| {
            let ic = l.fs.get_icon("/");
            ic.rsplit('/').next().unwrap_or(&ic).to_string()
        });
        ApiLevel {
            name: l.name.clone(),
            is_archive: i > 0 && panel_router::nav::is_archive(&l.name),
            label,
            icon,
        }
    }).collect();
    let cur_path = path_ref.active().relative_path.clone();
    let display = path_ref.absolute_path();
    let entries_data = path_ref.active().entries.clone();
    drop(path_ref);
    use chrono::TimeZone;
    let api_entries: Vec<ApiFileEntry> = entries_data
        .iter()
        .filter(|e| !router.should_skip_entry(&e.name))
        .map(|e| {
            let path = if display == "/" {
                format!("/{}", e.name)
            } else {
                format!("{}/{}", display, e.name)
            };
            let modified = match chrono::Local.timestamp_opt(e.modified as i64, 0) {
                chrono::LocalResult::Single(dt) if e.modified != 0 => {
                    Some(dt.format("%Y-%m-%d %H:%M:%S").to_string())
                }
                _ => None,
            };
            ApiFileEntry {
                path,
                is_dir: e.is_dir,
                size: if e.is_dir { None } else { Some(e.size) },
                modified,
                name: e.name.clone(),
            }
        })
        .collect();
    ApiPanelState {
        levels: api_levels,
        path: cur_path,
        entries: api_entries,
        showing_selector: router.is_showing_selector(),
        view_mode: router.view_mode(),
        selected: router.core_selected_name(),
        tabs: Vec::new(),
        active_tab: 0,
    }
}

fn read_panel_state_info(info: &crate::panel_builder::PanelInfo) -> ApiPanelState {
    let mut state = read_panel_state(&info.active_router());
    state.tabs = info.read_tabs();
    state.active_tab = info.active_tab_id();
    state
}

pub fn notify_side(side: &str, info: &crate::panel_builder::PanelInfo) {
    if !notifier_active() {
        return;
    }
    notify_panel_state(side, read_panel_state_info(info));
}
pub struct GtkBackend {
    pub left: crate::panel_builder::PanelInfo,
    pub right: crate::panel_builder::PanelInfo,
    pub config: client_config::AppConfig,
    pub selector_updaters: std::rc::Rc<std::cell::RefCell<Vec<std::rc::Rc<dyn Fn()>>>>,
}

impl GtkBackend {
    fn router(&self, side: PanelSide) -> std::rc::Rc<panel_router::PanelRouter> {
        match side {
            PanelSide::Left => self.left.active_router(),
            PanelSide::Right => self.right.active_router(),
        }
    }
    fn info(&self, side: PanelSide) -> &crate::panel_builder::PanelInfo {
        match side {
            PanelSide::Left => &self.left,
            PanelSide::Right => &self.right,
        }
    }
    fn notify(&self, side: PanelSide) {
        let side_str = match side { PanelSide::Left => "left", PanelSide::Right => "right" };
        notify_side(side_str, self.info(side));
    }
    fn refresh_selectors(&self) {
        for updater in self.selector_updaters.borrow().iter() {
            updater();
        }
    }
}

#[async_trait::async_trait(?Send)]
impl PanelBackend for GtkBackend {
    async fn enter(&self, side: PanelSide, name: String) -> ApiResult<()> {
        let router = self.router(side);
        router.switch_to_selector(false);
        let _ = router.enter(&name).await;
        self.notify(side);
        Ok(())
    }
    async fn go_up(&self, side: PanelSide) -> ApiResult<()> {
        let _ = self.router(side).go_up().await;
        self.notify(side);
        Ok(())
    }
    async fn go_back(&self, side: PanelSide) -> ApiResult<()> {
        let _ = self.router(side).go_back().await;
        self.notify(side);
        Ok(())
    }
    async fn go_forward(&self, side: PanelSide) -> ApiResult<()> {
        let _ = self.router(side).go_forward().await;
        self.notify(side);
        Ok(())
    }
    async fn go_to_level(&self, side: PanelSide, level: usize) -> ApiResult<()> {
        let _ = self.router(side).go_to_level(level).await;
        self.notify(side);
        Ok(())
    }
    async fn go_home(&self, side: PanelSide) -> ApiResult<()> {
        let router = self.router(side);
        router.reset_to_base();
        router.switch_to_selector(true);
        self.notify(side);
        Ok(())
    }

    async fn add_tab(&self, side: PanelSide) -> ApiResult<()> {
        self.info(side).add_tab(None);
        self.notify(side);
        Ok(())
    }
    async fn close_tab(&self, side: PanelSide, id: u32) -> ApiResult<()> {
        self.info(side).close_tab_by_id(id);
        self.notify(side);
        Ok(())
    }
    async fn switch_tab(&self, side: PanelSide, id: u32) -> ApiResult<()> {
        self.info(side).switch_tab_by_id(id);
        self.notify(side);
        Ok(())
    }

    fn read_state(&self, side: PanelSide) -> ApiPanelState {
        read_panel_state_info(self.info(side))
    }

    async fn delete(&self, side: PanelSide, paths: Vec<String>) -> ApiResult<()> {
        let router = self.router(side);
        let provider = router.provider();
        if provider.delete_entries(paths).await.is_ok() {
            let _ = router.refresh().await;
        }
        Ok(())
    }
    async fn mkdir(&self, side: PanelSide, name: String) -> ApiResult<()> {
        let router = self.router(side);
        let provider = router.provider();
        let parent = router.current_path_string();
        if provider.create_directory(parent, name, None).await.is_ok() {
            let _ = router.refresh().await;
        }
        Ok(())
    }
    async fn rename(&self, side: PanelSide, old_path: String, new_name: String) -> ApiResult<()> {
        let router = self.router(side);
        let provider = router.provider();
        let parent = old_path.rsplitn(2, '/').nth(1).unwrap_or("").to_string();
        let new_path = format!("{}/{}", parent.trim_end_matches('/'), new_name);
        if provider.rename_entry(old_path, new_path).await.is_ok() {
            let _ = router.refresh().await;
        }
        Ok(())
    }
    async fn copy(&self, src: PanelSide, dst: PanelSide, paths: Vec<String>) -> ApiResult<()> {
        self.transfer_paths(src, dst, paths, false).await
    }
    async fn move_entries(&self, src: PanelSide, dst: PanelSide, paths: Vec<String>) -> ApiResult<()> {
        self.transfer_paths(src, dst, paths, true).await
    }
    async fn read_file(&self, side: PanelSide, path: String) -> ApiResult<ApiFileContent> {
        let provider = self.router(side).provider();
        const MAX: usize = 2 * 1024 * 1024;
        match provider.read_file(path.clone(), None).await {
            Ok(bytes) if bytes.len() > MAX => {
                Ok(ApiFileContent { path, content: String::new(), is_binary: true })
            }
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Ok(ApiFileContent { path, content: text, is_binary: false }),
                Err(_) => Ok(ApiFileContent { path, content: String::new(), is_binary: true }),
            },
            Err(e) => Err(e.to_string()),
        }
    }
    async fn stream_file(&self, side: PanelSide, path: String) -> ApiResult<Vec<u8>> {
        const MAX: usize = 100 * 1024 * 1024;
        let provider = self.router(side).provider();
        match provider.read_file(path, None).await {
            Ok(bytes) if bytes.len() > MAX => {
                Err(format!("file too large to stream ({} MB max)", MAX / 1024 / 1024))
            }
            Ok(bytes) => Ok(bytes),
            Err(e) => Err(e.to_string()),
        }
    }
    async fn write_file(&self, side: PanelSide, path: String, content: String) -> ApiResult<()> {
        let router = self.router(side);
        let provider = router.provider();
        let result = provider
            .write_file(path, content.into_bytes(), None, None)
            .await
            .map_err(|e| e.to_string());
        if result.is_ok() {
            let _ = router.refresh().await;
        }
        result
    }
    async fn upload_file(&self, side: PanelSide, path: String, data: Vec<u8>) -> ApiResult<()> {
        let router = self.router(side);
        let provider = router.provider();
        let result = provider
            .write_file(path, data, None, None)
            .await
            .map_err(|e| e.to_string());
        if result.is_ok() {
            let _ = router.refresh().await;
        }
        result
    }

    fn get_drives(&self) -> Vec<ApiDrive> {
        let all = crate::drives::get_all_app_drives(&self.config);
        all.iter()
            .map(|d| {
                let (kind, path) = match &d.item {
                    crate::drives::AppDriveItem::RootFs => ("root", "/".to_string()),
                    crate::drives::AppDriveItem::UserHome => (
                        "home",
                        dirs::home_dir().unwrap_or_default().to_string_lossy().into_owned(),
                    ),
                    crate::drives::AppDriveItem::LocalDrive(p) => ("drive", p.clone()),
                    crate::drives::AppDriveItem::Volume(_) => ("volume", String::new()),
                    crate::drives::AppDriveItem::NetConnection(_) => ("net", String::new()),
                };
                let icon = d.icon.rsplit('/').next().unwrap_or(&d.icon).to_string();
                ApiDrive {
                    name: d.name.clone(),
                    key: d.key.clone(),
                    path,
                    icon,
                    kind: kind.to_string(),
                    subtitle: d.subtitle.clone(),
                    is_favorite: d.is_favorite,
                    is_online: d.is_online,
                }
            })
            .collect()
    }
    async fn activate_source(&self, side: PanelSide, key: String) -> ApiResult<()> {
        let router = self.router(side);
        let all = crate::drives::get_all_app_drives(&self.config);
        match all.iter().find(|d| d.key == key) {
            Some(d) => match crate::drives::activate_drive_item(&d.item, &router) {
                crate::drives::DriveActivation::Shown => {
                    router.switch_to_selector(false);
                    Ok(())
                }
                crate::drives::DriveActivation::NeedsAsyncMount(_) => {
                    Err("Mount this drive in the desktop app first".to_string())
                }
            },
            None => Err(format!("unknown source key: {key}")),
        }
    }
    fn get_connections(&self) -> Vec<ApiConnection> {
        let conns: Vec<crate::connection_manager::FtpConnection> =
            self.config.get("ui.ftp_connections").unwrap_or_default();
        conns.into_iter().map(|c| ApiConnection {
            name: c.name,
            protocol: c.protocol,
            host: c.host,
            port: c.port,
            user: c.user,
            pass: None,
            auth_type: c.auth_type,
            key_path: c.key_path,
            passphrase: None,
            remote_path: c.remote_path,
            use_tunnel: c.use_tunnel,
            tunnel_host: c.tunnel_host,
            tunnel_port: c.tunnel_port,
            tunnel_user: c.tunnel_user,
            tunnel_auth_type: c.tunnel_auth_type,
            tunnel_pass: None,
            tunnel_key_path: c.tunnel_key_path,
            tunnel_passphrase: None,
        }).collect()
    }
    fn save_connection(&self, connection: ApiConnection) -> ApiResult<()> {
        let mut conns: Vec<crate::connection_manager::FtpConnection> =
            self.config.get("ui.ftp_connections").unwrap_or_default();
        let mut new_conn: crate::connection_manager::FtpConnection = connection.into();
        if let Some(existing) = conns.iter().find(|c| c.name == new_conn.name) {
            for (incoming, stored) in [
                (&mut new_conn.pass, &existing.pass),
                (&mut new_conn.passphrase, &existing.passphrase),
                (&mut new_conn.tunnel_pass, &existing.tunnel_pass),
                (&mut new_conn.tunnel_passphrase, &existing.tunnel_passphrase),
            ] {
                if incoming.as_deref().map(|v| v.is_empty()).unwrap_or(true) {
                    *incoming = stored.clone();
                }
            }
        }
        crate::secret_store::seal_connection(&self.config, &mut new_conn);
        match conns.iter_mut().find(|c| c.name == new_conn.name) {
            Some(existing) => *existing = new_conn,
            None => conns.push(new_conn),
        }
        self.config.set("ui.ftp_connections", conns);
        self.config.save();
        self.refresh_selectors();
        Ok(())
    }
    fn delete_connection(&self, name: String) -> ApiResult<()> {
        let mut conns: Vec<crate::connection_manager::FtpConnection> =
            self.config.get("ui.ftp_connections").unwrap_or_default();
        let before = conns.len();
        conns.retain(|c| c.name != name);
        if conns.len() == before {
            Err(format!("no saved connection named {name:?}"))
        } else {
            self.config.set("ui.ftp_connections", conns);
            self.config.save();
            self.refresh_selectors();
            Ok(())
        }
    }
    async fn refresh_panel(&self, side: PanelSide) -> ApiResult<()> {
        self.router(side).refresh().await.map_err(|e| e.to_string())
    }

    fn get_settings(&self) -> ApiResult<serde_json::Value> {
        let mut out = serde_json::Map::new();
        for key in PUBLIC_SETTINGS {
            if let Some(v) = self.config.get::<serde_json::Value>(key) {
                out.insert((*key).to_string(), v);
            }
        }
        Ok(serde_json::Value::Object(out))
    }

    fn set_settings(&self, values: serde_json::Value) -> ApiResult<()> {
        let obj = values.as_object().ok_or("settings must be a JSON object")?;
        for key in obj.keys() {
            if !PUBLIC_SETTINGS.contains(&key.as_str()) {
                return Err(format!("{key:?} is not a settable setting"));
            }
        }
        for (key, value) in obj {
            self.config.set(key, value.clone());
        }
        self.config.save();
        Ok(())
    }

    fn viewer_content(&self) -> ApiResult<Option<(String, String, String)>> {
        Ok(crate::viewer_probe::content())
    }

    fn set_connections_dialog(&self, open: bool) -> ApiResult<()> {
        if !open {
            crate::connection_manager::close_manage_ftp_dialog();
            return Ok(());
        }
        if crate::connection_manager::connections_dialog_open() {
            return Ok(());
        }
        let Some(window) = self.left.active_router().window() else {
            return Err("no window to parent the dialog on".to_string());
        };
        let updaters = self.selector_updaters.clone();
        crate::connection_manager::show_manage_ftp_dialog(
            &window,
            std::rc::Rc::new(move || {
                for u in updaters.borrow().iter() {
                    u();
                }
            }),
            self.config.clone(),
            None,
        );
        Ok(())
    }

    fn export_connections(&self, password: Option<String>) -> ApiResult<String> {
        let conns: Vec<crate::connection_manager::FtpConnection> =
            self.config.get("ui.ftp_connections").unwrap_or_default();
        if conns.is_empty() {
            return Err("no saved connections to export".to_string());
        }
        let pw = password.filter(|p| !p.is_empty());
        Ok(crate::secret_store::export_connections(&conns, pw.as_deref()))
    }

    fn import_connections(&self, data: String, password: Option<String>) -> ApiResult<usize> {
        let pw = password.filter(|p| !p.is_empty());
        let incoming = crate::secret_store::parse_import(&data, pw.as_deref()).map_err(|e| {
            match e {
                ::secret_store::ImportError::NeedsPassword => NEEDS_PASSWORD.to_string(),
                ::secret_store::ImportError::WrongPassword => "wrong password".to_string(),
                ::secret_store::ImportError::Malformed => {
                    "not an ice-commander connections file".to_string()
                }
            }
        })?;
        let mut conns: Vec<crate::connection_manager::FtpConnection> =
            self.config.get("ui.ftp_connections").unwrap_or_default();
        for mut c in incoming.into_iter() {
            crate::secret_store::seal_connection(&self.config, &mut c);
            match conns.iter_mut().find(|e| e.name == c.name) {
                Some(existing) => *existing = c,
                None => conns.push(c),
            }
        }
        let n = conns.len();
        self.config.set("ui.ftp_connections", conns);
        self.config.save();
        self.refresh_selectors();
        Ok(n)
    }

    async fn connect_to(&self, side: PanelSide, connection: ApiConnection) -> ApiResult<()> {
        let connection = {
            let mut c = connection;
            let stored: Vec<crate::connection_manager::FtpConnection> =
                self.config.get("ui.ftp_connections").unwrap_or_default();
            let saved = stored
                .iter()
                .find(|s| s.name == c.name)
                .map(crate::secret_store::opened);
            let s = saved.as_ref();
            for (field, from) in [
                (&mut c.pass, s.and_then(|s| s.pass.clone())),
                (&mut c.passphrase, s.and_then(|s| s.passphrase.clone())),
                (&mut c.tunnel_pass, s.and_then(|s| s.tunnel_pass.clone())),
                (&mut c.tunnel_passphrase, s.and_then(|s| s.tunnel_passphrase.clone())),
            ] {
                if field.as_deref().map(|v| v.is_empty()).unwrap_or(true) {
                    *field = from;
                } else if let Some(v) = field.as_deref() {
                    *field = crate::secret_store::decrypt_secret(v).or_else(|| field.clone());
                }
            }
            c
        };
        let router = self.router(side);
        let remote_path = connection.remote_path.clone().unwrap_or_else(|| "/".to_string());
        let proto = connection.protocol.to_uppercase();
        let result: Result<(), String> = if proto == "FTP" {
            let rpc = std::rc::Rc::new(virtualfs::ftp_rpc::LocalFtpRpc {
                name: connection.name.clone(),
                host: connection.host.clone(),
                port: connection.port,
                user: connection.user.clone(),
                pass: connection.pass.clone().unwrap_or_default(),
                ftp_session: std::sync::Arc::new(std::sync::Mutex::new(None)),
            });
            router.mount_provider(rpc, "ftp", remote_path.clone());
            Ok(())
        } else if proto == "WEBDAV" {
            let rpc = std::rc::Rc::new(virtualfs::webdav_rpc::LocalWebDavRpc {
                name: connection.name.clone(),
                url: connection.host.clone(),
                user: if connection.user.is_empty() { None } else { Some(connection.user.clone()) },
                pass: connection.pass.clone(),
                remote_path: connection.remote_path.clone(),
            });
            router.mount_provider(rpc, "webdav", remote_path.clone());
            Ok(())
        } else if proto == "SFTP" {
            let rpc = std::rc::Rc::new(virtualfs::sftp_rpc::LocalSftpRpc {
                name: connection.name.clone(),
                host: connection.host.clone(),
                port: connection.port,
                user: connection.user.clone(),
                pass: connection.pass.clone(),
                auth_type: connection.auth_type.clone().unwrap_or_else(|| "password".to_string()),
                key_path: connection.key_path.clone(),
                passphrase: connection.passphrase.clone(),
                use_tunnel: connection.use_tunnel,
                tunnel_host: connection.tunnel_host.clone(),
                tunnel_port: connection.tunnel_port,
                tunnel_user: connection.tunnel_user.clone(),
                tunnel_auth_type: connection.tunnel_auth_type.clone(),
                tunnel_pass: connection.tunnel_pass.clone(),
                tunnel_key_path: connection.tunnel_key_path.clone(),
                tunnel_passphrase: connection.tunnel_passphrase.clone(),
                sftp_session: std::sync::Arc::new(std::sync::Mutex::new(None)),
                tunnel: std::sync::Arc::new(std::sync::Mutex::new(None)),
            });
            router.mount_provider(rpc, "sftp", remote_path.clone());
            Ok(())
        } else {
            Err(format!("Unknown protocol: {}", connection.protocol))
        };
        if result.is_ok() {
            router.switch_to_selector(false);
        }
        result
    }

    fn toggle_favorite(&self, path: String) {
        crate::favorites::toggle_favorite(&self.config, &path);
        self.refresh_selectors();
    }
    fn get_favorites_only(&self) -> bool {
        crate::favorites::is_favorites_only(&self.config)
    }
    fn set_favorites_only(&self, value: bool) {
        crate::favorites::set_favorites_only(&self.config, value);
        self.refresh_selectors();
    }
}

pub fn start_api_dispatcher(
    mut rx: tokio::sync::mpsc::Receiver<ApiCmd>,
    left_info: crate::panel_builder::PanelInfo,
    right_info: crate::panel_builder::PanelInfo,
    config: client_config::AppConfig,
    selector_updaters: std::rc::Rc<std::cell::RefCell<Vec<std::rc::Rc<dyn Fn()>>>>,
    left_term: TerminalBridge,
    right_term: TerminalBridge,
    term_expand: std::rc::Rc<dyn Fn(PanelSide, bool)>,
) {
    let backend = std::rc::Rc::new(GtkBackend {
        left: left_info,
        right: right_info,
        config,
        selector_updaters,
    });

    let (gui_tx, gui_rx) = std::sync::mpsc::channel::<ApiCmd>();
    tokio::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            if gui_tx.send(cmd).is_err() {
                break;
            }
        }
    });

    glib::timeout_add_local(std::time::Duration::from_millis(10), move || {
        while let Ok(cmd) = gui_rx.try_recv() {
            let backend = backend.clone();
            let left_term = left_term.clone();
            let right_term = right_term.clone();
            let term_expand = term_expand.clone();
            glib::spawn_future_local(async move {
                if let Some(cmd) = dispatch_core(&*backend, cmd).await {
                    handle_gtk_only(cmd, &backend, &left_term, &right_term, &term_expand);
                }
            });
        }
        glib::ControlFlow::Continue
    });
}

fn handle_gtk_only(
    cmd: ApiCmd,
    backend: &GtkBackend,
    left_term: &TerminalBridge,
    right_term: &TerminalBridge,
    term_expand: &std::rc::Rc<dyn Fn(PanelSide, bool)>,
) {
    match cmd {
        ApiCmd::OpenTerminal { side } => {
            let bridge = match side { PanelSide::Left => left_term, PanelSide::Right => right_term };
            (bridge.open)();
        }
        ApiCmd::TerminalInput { side, data } => {
            let bridge = match side { PanelSide::Left => left_term, PanelSide::Right => right_term };
            if let Some(tx) = bridge.input_tx.borrow().as_ref() {
                let _ = tx.try_send(data);
            }
        }
        ApiCmd::SetTerminalExpanded { side, expanded } => {
            term_expand(side, expanded);
        }
        ApiCmd::SetViewMode { side, mode, reply } => {
            backend.router(side).set_view_mode(mode);
            let _ = reply.send(Ok(()));
        }
        ApiCmd::SetSort { side, column, descending, reply } => {
            backend.router(side).set_sort(column, descending);
            let _ = reply.send(Ok(()));
        }
        ApiCmd::OpenNative { side, path, reply } => {
            let router = backend.router(side);
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let entry = gtk_fm_ui::FileEntry::new(&name, &path, false, size, "", None);
            let result = match router.window() {
                Some(window) => {
                    crate::viewer::show_viewer(&window, entry, router.clone());
                    Ok(())
                }
                None => Err("panel has no window".to_string()),
            };
            let _ = reply.send(result);
        }
        ApiCmd::ListWindows { reply } => {
            use gtk::prelude::GtkWindowExt;
            let titles = extra_toplevel_windows(backend.left.active_router().window())
                .iter()
                .map(|w| w.title().map(|s| s.to_string()).unwrap_or_default())
                .collect();
            let _ = reply.send(Ok(titles));
        }
        ApiCmd::CloseExtraWindows { reply } => {
            use gtk::prelude::GtkWindowExt;
            let extras = extra_toplevel_windows(backend.left.active_router().window());
            let n = extras.len();
            for w in &extras {
                w.close();
            }
            let _ = reply.send(Ok(n));
        }
        _ => {}
    }
}

fn extra_toplevel_windows(main_win: Option<gtk::Window>) -> Vec<gtk::Window> {
    use gtk::prelude::*;
    let toplevels = gtk::Window::toplevels();
    let mut out = Vec::new();
    for i in 0..toplevels.n_items() {
        if let Some(w) = toplevels.item(i).and_then(|o| o.downcast::<gtk::Window>().ok()) {
            if main_win.as_ref() != Some(&w) {
                out.push(w);
            }
        }
    }
    out
}

async fn transfer_one(
    src_provider: &std::rc::Rc<dyn RouterRpc>,
    dst_provider: &std::rc::Rc<dyn RouterRpc>,
    src_path: String,
    dst_file: String,
    is_move: bool,
) -> u64 {
    match src_provider.read_file(src_path.clone(), None).await {
        Ok(data) => {
            let len = data.len() as u64;
            if dst_provider.write_file(dst_file, data, None, None).await.is_ok() && is_move {
                let _ = src_provider.delete_entries(vec![src_path]).await;
            }
            len
        }
        Err(_) => 0,
    }
}

async fn transfer_batch(
    src_provider: std::rc::Rc<dyn RouterRpc>,
    dst_provider: std::rc::Rc<dyn RouterRpc>,
    paths: Vec<String>,
    dst_path: String,
    is_move: bool,
    op: OpGuard,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let (mut done_files, mut done_bytes) = (0usize, 0u64);
    for src_path in paths {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        let file_name = src_path.rsplitn(2, '/').next().unwrap_or(&src_path).to_string();
        op.update(&file_name, done_bytes, done_files);
        let dst_file = format!("{}/{}", dst_path.trim_end_matches('/'), file_name);
        done_bytes += transfer_one(&src_provider, &dst_provider, src_path, dst_file, is_move).await;
        done_files += 1;
        op.update(&file_name, done_bytes, done_files);
    }
    drop(op);
}

impl GtkBackend {
    async fn transfer_paths(
        &self,
        src: PanelSide,
        dst: PanelSide,
        paths: Vec<String>,
        is_move: bool,
    ) -> ApiResult<()> {
        let src_router = self.router(src);
        let dst_router = self.router(dst);
        let dst_path = dst_router.current_path_string();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let op = ops_begin(
            if is_move { "move" } else { "copy" },
            0,
            paths.len(),
            cancel.clone(),
        );

        let src_provider = src_router.provider();
        let dst_provider = dst_router.provider();
        let plans = crate::transfer_plan::EndpointPlan::of(&src_provider)
            .zip(crate::transfer_plan::EndpointPlan::of(&dst_provider));

        match plans {
            Some((src_plan, dst_plan)) => {
                let (src_factory, dst_factory) = (src_plan.into_factory(), dst_plan.into_factory());
                let handle = tokio::task::spawn_blocking(move || {
                    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build()
                    {
                        Ok(rt) => rt,
                        Err(_) => return,
                    };
                    let local = tokio::task::LocalSet::new();
                    local.block_on(&rt, async move {
                        transfer_batch(
                            src_factory(),
                            dst_factory(),
                            paths,
                            dst_path,
                            is_move,
                            op,
                            cancel,
                        )
                        .await;
                    });
                });
                let _ = handle.await;
            }
            None => {
                transfer_batch(src_provider, dst_provider, paths, dst_path, is_move, op, cancel)
                    .await;
            }
        }

        let _ = dst_router.refresh().await;
        let _ = src_router.refresh().await;
        Ok(())
    }
}
