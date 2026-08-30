use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use fm_core::rpc::FileSystemRpc;
use ic_platform::terminal::{spawn_pty_command, spawn_pty_session, PtySession};
use panel_core::RouterState;
use panel_server::{
    dispatch_core, init_notifier, notify_panel_state, notify_terminal_expanded,
    notify_terminal_opened, start_api_server, ApiCmd, ApiConnection, ApiDrive, ApiFileContent,
    NEEDS_PASSWORD, PUBLIC_SETTINGS,
    ApiFileEntry, ApiLevel, ApiPanelState, ApiResult, ApiTab, PanelBackend, PanelSide, WsSessions,
};
use tokio::sync::{broadcast, mpsc};

fn side_str(side: PanelSide) -> &'static str {
    match side {
        PanelSide::Left => "left",
        PanelSide::Right => "right",
    }
}

fn read_state(core: &RouterState) -> ApiPanelState {
    let path_ref = core.path.borrow();
    let api_levels: Vec<ApiLevel> = path_ref
        .levels()
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let label = if i == 0 { l.fs.display_name() } else { None };
            let icon = label.as_ref().map(|_| {
                let ic = l.fs.get_icon("/");
                ic.rsplit('/').next().unwrap_or(&ic).to_string()
            });
            ApiLevel {
                name: l.name.clone(),
                is_archive: i > 0 && panel_core::nav::is_archive(&l.name),
                label,
                icon,
            }
        })
        .collect();
    let cur_path = path_ref.active().relative_path.clone();
    let display = path_ref.absolute_path();
    let entries_data = path_ref.active().entries.clone();
    drop(path_ref);
    use chrono::TimeZone;
    let api_entries: Vec<ApiFileEntry> = entries_data
        .iter()
        .filter(|e| !(cfg!(unix) && e.name.starts_with('.')))
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
    let tab_title = if core.showing_selector.get() {
        "/".to_string()
    } else {
        api_levels
            .last()
            .map(|l| l.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "/".to_string())
    };
    ApiPanelState {
        levels: api_levels,
        path: cur_path,
        entries: api_entries,
        showing_selector: core.showing_selector.get(),
        view_mode: "list".to_string(),
        selected: core.active_selected(),
        tabs: vec![ApiTab { id: 0, title: tab_title, icon: None }],
        active_tab: 0,
    }
}

struct ConsoleBackend {
    left: Rc<RouterState>,
    right: Rc<RouterState>,
    config: client_config::AppConfig,
}

fn seal_api_conn(c: &mut ApiConnection) {
    for f in [&mut c.pass, &mut c.passphrase, &mut c.tunnel_pass, &mut c.tunnel_passphrase] {
        if let Some(v) = f.as_deref() {
            *f = Some(secret_store::encrypt_secret(v));
        }
    }
}

fn open_api_conn(mut c: ApiConnection) -> ApiConnection {
    for f in [&mut c.pass, &mut c.passphrase, &mut c.tunnel_pass, &mut c.tunnel_passphrase] {
        if let Some(v) = f.as_deref() {
            *f = secret_store::decrypt_secret(v).or_else(|| f.clone());
        }
    }
    c
}

impl ConsoleBackend {
    fn core(&self, side: PanelSide) -> &Rc<RouterState> {
        match side {
            PanelSide::Left => &self.left,
            PanelSide::Right => &self.right,
        }
    }

    fn stored_connections(&self) -> Vec<ApiConnection> {
        self.config.get::<Vec<ApiConnection>>("ui.ftp_connections").unwrap_or_default()
    }

    async fn connect_provider(&self, side: PanelSide, conn: &ApiConnection) -> ApiResult<()> {
        let conn = &open_api_conn(conn.clone());
        let core = self.core(side);
        let provider: Rc<dyn FileSystemRpc> = match conn.protocol.to_uppercase().as_str() {
            "FTP" => Rc::new(virtualfs::ftp_rpc::LocalFtpRpc {
                name: conn.name.clone(),
                host: conn.host.clone(),
                port: conn.port,
                user: conn.user.clone(),
                pass: conn.pass.clone().unwrap_or_default(),
                ftp_session: Arc::new(Mutex::new(None)),
            }),
            "WEBDAV" => Rc::new(virtualfs::webdav_rpc::LocalWebDavRpc {
                name: conn.name.clone(),
                url: conn.host.clone(),
                user: if conn.user.is_empty() { None } else { Some(conn.user.clone()) },
                pass: conn.pass.clone(),
                remote_path: conn.remote_path.clone(),
            }),
            "SFTP" => Rc::new(virtualfs::sftp_rpc::LocalSftpRpc {
                name: conn.name.clone(),
                host: conn.host.clone(),
                port: conn.port,
                user: conn.user.clone(),
                pass: conn.pass.clone(),
                auth_type: conn.auth_type.clone().unwrap_or_else(|| "password".to_string()),
                key_path: conn.key_path.clone(),
                passphrase: conn.passphrase.clone(),
                use_tunnel: conn.use_tunnel,
                tunnel_host: conn.tunnel_host.clone(),
                tunnel_port: conn.tunnel_port,
                tunnel_user: conn.tunnel_user.clone(),
                tunnel_auth_type: conn.tunnel_auth_type.clone(),
                tunnel_pass: conn.tunnel_pass.clone(),
                tunnel_key_path: conn.tunnel_key_path.clone(),
                tunnel_passphrase: conn.tunnel_passphrase.clone(),
                sftp_session: Arc::new(Mutex::new(None)),
                tunnel: Arc::new(Mutex::new(None)),
            }),
            other => return Err(format!("unknown protocol: {other}")),
        };
        core.set_active_provider(provider.clone(), String::new());
        core.showing_selector.set(false);
        if let Some(rp) = conn.remote_path.clone().filter(|p| !p.is_empty() && p != "/") {
            let segs = panel_core::parse_path_to_segments(&rp);
            let levels = panel_core::nav::build_levels(&segs, provider.clone());
            *core.path.borrow_mut() = panel_core::nav::NavPath::from_levels(levels, provider);
        }
        core.list_active().await.map_err(|e| e.to_string())
    }
}

#[async_trait::async_trait(?Send)]
impl PanelBackend for ConsoleBackend {
    async fn enter(&self, side: PanelSide, name: String) -> ApiResult<()> {
        let _ = self.core(side).enter(&name).await;
        Ok(())
    }
    async fn go_up(&self, side: PanelSide) -> ApiResult<()> {
        let _ = self.core(side).go_up().await;
        Ok(())
    }
    async fn go_back(&self, side: PanelSide) -> ApiResult<()> {
        let _ = self.core(side).go_back().await;
        Ok(())
    }
    async fn go_forward(&self, side: PanelSide) -> ApiResult<()> {
        let _ = self.core(side).go_forward().await;
        Ok(())
    }
    async fn go_to_level(&self, side: PanelSide, level: usize) -> ApiResult<()> {
        let _ = self.core(side).go_to_level(level).await;
        Ok(())
    }
    async fn go_home(&self, side: PanelSide) -> ApiResult<()> {
        let core = self.core(side);
        core.reset_to_base();
        core.showing_selector.set(true);
        let _ = core.list_active().await;
        Ok(())
    }

    fn read_state(&self, side: PanelSide) -> ApiPanelState {
        read_state(self.core(side))
    }

    async fn add_tab(&self, _side: PanelSide) -> ApiResult<()> {
        Ok(())
    }
    async fn close_tab(&self, _side: PanelSide, _id: u32) -> ApiResult<()> {
        Ok(())
    }
    async fn switch_tab(&self, _side: PanelSide, _id: u32) -> ApiResult<()> {
        Ok(())
    }

    async fn delete(&self, side: PanelSide, paths: Vec<String>) -> ApiResult<()> {
        let core = self.core(side);
        if core.active_provider().delete_entries(paths).await.is_ok() {
            let _ = core.refresh().await;
        }
        Ok(())
    }
    async fn mkdir(&self, side: PanelSide, name: String) -> ApiResult<()> {
        let core = self.core(side);
        let active_path = core.path.borrow().active().relative_path.clone();
        if core.active_provider().create_directory(active_path, name, None).await.is_ok() {
            let _ = core.refresh().await;
        }
        Ok(())
    }
    async fn rename(&self, side: PanelSide, old_path: String, new_name: String) -> ApiResult<()> {
        let core = self.core(side);
        let parent = old_path.rsplitn(2, '/').nth(1).unwrap_or("").to_string();
        let new_path = format!("{}/{}", parent.trim_end_matches('/'), new_name);
        if core.active_provider().rename_entry(old_path, new_path).await.is_ok() {
            let _ = core.refresh().await;
        }
        Ok(())
    }
    async fn copy(&self, src: PanelSide, dst: PanelSide, paths: Vec<String>) -> ApiResult<()> {
        let src_provider = self.core(src).active_provider();
        let dst_core = self.core(dst);
        let dst_provider = dst_core.active_provider();
        let dst_path = dst_core.path.borrow().active().relative_path.clone();
        for src_path in paths {
            let file_name = src_path.rsplitn(2, '/').next().unwrap_or(&src_path).to_string();
            let dst_file = format!("{}/{}", dst_path.trim_end_matches('/'), file_name);
            if let Ok(data) = src_provider.read_file(src_path.clone(), None).await {
                let _ = dst_provider.write_file(dst_file, data, None, None).await;
            }
        }
        let _ = dst_core.refresh().await;
        Ok(())
    }
    async fn move_entries(&self, src: PanelSide, dst: PanelSide, paths: Vec<String>) -> ApiResult<()> {
        let src_core = self.core(src);
        let src_provider = src_core.active_provider();
        let dst_core = self.core(dst);
        let dst_provider = dst_core.active_provider();
        let dst_path = dst_core.path.borrow().active().relative_path.clone();
        for src_path in paths {
            let file_name = src_path.rsplitn(2, '/').next().unwrap_or(&src_path).to_string();
            let dst_file = format!("{}/{}", dst_path.trim_end_matches('/'), file_name);
            if let Ok(data) = src_provider.read_file(src_path.clone(), None).await {
                if dst_provider.write_file(dst_file, data, None, None).await.is_ok() {
                    let _ = src_provider.delete_entries(vec![src_path]).await;
                }
            }
        }
        let _ = dst_core.refresh().await;
        let _ = src_core.refresh().await;
        Ok(())
    }
    async fn read_file(&self, side: PanelSide, path: String) -> ApiResult<ApiFileContent> {
        let provider = self.core(side).active_provider();
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
        let provider = self.core(side).active_provider();
        match provider.read_file(path, None).await {
            Ok(bytes) if bytes.len() > MAX => {
                Err(format!("file too large to stream ({} MB max)", MAX / 1024 / 1024))
            }
            Ok(bytes) => Ok(bytes),
            Err(e) => Err(e.to_string()),
        }
    }
    async fn write_file(&self, side: PanelSide, path: String, content: String) -> ApiResult<()> {
        let core = self.core(side);
        let result = core
            .active_provider()
            .write_file(path, content.into_bytes(), None, None)
            .await
            .map_err(|e| e.to_string());
        if result.is_ok() {
            let _ = core.refresh().await;
        }
        result
    }
    async fn upload_file(&self, side: PanelSide, path: String, data: Vec<u8>) -> ApiResult<()> {
        let core = self.core(side);
        let result = core
            .active_provider()
            .write_file(path, data, None, None)
            .await
            .map_err(|e| e.to_string());
        if result.is_ok() {
            let _ = core.refresh().await;
        }
        result
    }

    fn get_drives(&self) -> Vec<ApiDrive> {
        let mut drives = vec![ApiDrive {
            name: "Root".to_string(),
            key: "root".to_string(),
            path: "/".to_string(),
            icon: "home.svg".to_string(),
            kind: "root".to_string(),
            subtitle: String::new(),
            is_favorite: false,
            is_online: true,
        }];
        if let Some(home) = dirs::home_dir() {
            drives.push(ApiDrive {
                name: "Home".to_string(),
                key: "home".to_string(),
                path: home.to_string_lossy().into_owned(),
                icon: "at-home.svg".to_string(),
                kind: "home".to_string(),
                subtitle: String::new(),
                is_favorite: false,
                is_online: true,
            });
        }
        for d in virtualfs::utils::get_drives() {
            drives.push(ApiDrive {
                name: d.name,
                key: d.path.clone(),
                path: d.path,
                icon: "ssd.svg".to_string(),
                kind: "drive".to_string(),
                subtitle: String::new(),
                is_favorite: false,
                is_online: d.is_mounted,
            });
        }
        for conn in self.get_connections() {
            drives.push(ApiDrive {
                name: conn.name.clone(),
                key: format!("net:{}", conn.name),
                path: String::new(),
                icon: format!("{}.svg", conn.protocol.to_lowercase()),
                kind: "net".to_string(),
                subtitle: format!("{}@{}", conn.user, conn.host),
                is_favorite: false,
                is_online: true,
            });
        }
        drives
    }
    async fn activate_source(&self, side: PanelSide, key: String) -> ApiResult<()> {
        if let Some(name) = key.strip_prefix("net:") {
            return match self.stored_connections().into_iter().find(|c| c.name == name) {
                Some(conn) => self.connect_provider(side, &conn).await,
                None => Err(format!("no saved connection named {name:?}")),
            };
        }
        let core = self.core(side);
        let path = match key.as_str() {
            "root" => "/".to_string(),
            "home" => dirs::home_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/".to_string()),
            p if p.starts_with('/') => p.to_string(),
            other => return Err(format!("unknown source key: {other}")),
        };
        let base = core.local_provider.clone();
        let segs = panel_core::parse_path_to_segments(&path);
        let levels = panel_core::nav::build_levels(&segs, base.clone());
        *core.path.borrow_mut() = panel_core::nav::NavPath::from_levels(levels, base);
        core.showing_selector.set(false);
        core.list_active().await.map_err(|e| e.to_string())
    }

    fn get_connections(&self) -> Vec<ApiConnection> {
        self.stored_connections()
            .into_iter()
            .map(|mut c| {
                c.pass = None;
                c.passphrase = None;
                c.tunnel_pass = None;
                c.tunnel_passphrase = None;
                c
            })
            .collect()
    }
    fn save_connection(&self, connection: ApiConnection) -> ApiResult<()> {
        let mut conns = self.stored_connections();
        let mut connection = connection;
        if let Some(existing) = conns.iter().find(|c| c.name == connection.name) {
            for (incoming, stored) in [
                (&mut connection.pass, &existing.pass),
                (&mut connection.passphrase, &existing.passphrase),
                (&mut connection.tunnel_pass, &existing.tunnel_pass),
                (&mut connection.tunnel_passphrase, &existing.tunnel_passphrase),
            ] {
                if incoming.as_deref().map(|v| v.is_empty()).unwrap_or(true) {
                    *incoming = stored.clone();
                }
            }
        }
        seal_api_conn(&mut connection);
        match conns.iter_mut().find(|c| c.name == connection.name) {
            Some(existing) => *existing = connection,
            None => conns.push(connection),
        }
        self.config.set("ui.ftp_connections", conns);
        self.config.save();
        Ok(())
    }
    fn delete_connection(&self, name: String) -> ApiResult<()> {
        let mut conns = self.stored_connections();
        let before = conns.len();
        conns.retain(|c| c.name != name);
        if conns.len() == before {
            return Err(format!("no saved connection named {name:?}"));
        }
        self.config.set("ui.ftp_connections", conns);
        self.config.save();
        Ok(())
    }
    async fn refresh_panel(&self, side: PanelSide) -> ApiResult<()> {
        self.core(side).refresh().await.map_err(|e| e.to_string())
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
        Ok(None)
    }

    fn set_connections_dialog(&self, _open: bool) -> ApiResult<()> {
        Ok(())
    }

    fn export_connections(&self, password: Option<String>) -> ApiResult<String> {
        let conns = self.stored_connections();
        if conns.is_empty() {
            return Err("no saved connections to export".to_string());
        }
        let opened: Vec<ApiConnection> = conns.into_iter().map(open_api_conn).collect();
        let payload = serde_json::to_string(&opened).map_err(|e| e.to_string())?;
        let pw = password.filter(|p| !p.is_empty());
        Ok(secret_store::wrap_export(&payload, pw.as_deref()))
    }

    fn import_connections(&self, data: String, password: Option<String>) -> ApiResult<usize> {
        let pw = password.filter(|p| !p.is_empty());
        let payload = secret_store::unwrap_export(&data, pw.as_deref()).map_err(|e| match e {
            secret_store::ImportError::NeedsPassword => NEEDS_PASSWORD.to_string(),
            secret_store::ImportError::WrongPassword => "wrong password".to_string(),
            secret_store::ImportError::Malformed => {
                "not an ice-commander connections file".to_string()
            }
        })?;
        let incoming: Vec<ApiConnection> =
            serde_json::from_str(&payload).map_err(|_| "not an ice-commander connections file")?;

        let mut conns = self.stored_connections();
        for mut c in incoming.into_iter() {
            seal_api_conn(&mut c);
            match conns.iter_mut().find(|e| e.name == c.name) {
                Some(existing) => *existing = c,
                None => conns.push(c),
            }
        }
        let n = conns.len();
        self.config.set("ui.ftp_connections", conns);
        self.config.save();
        Ok(n)
    }

    async fn connect_to(&self, side: PanelSide, connection: ApiConnection) -> ApiResult<()> {
        let mut connection = connection;
        if let Some(saved) = self
            .stored_connections()
            .into_iter()
            .find(|s| s.name == connection.name)
        {
            for (field, from) in [
                (&mut connection.pass, saved.pass),
                (&mut connection.passphrase, saved.passphrase),
                (&mut connection.tunnel_pass, saved.tunnel_pass),
                (&mut connection.tunnel_passphrase, saved.tunnel_passphrase),
            ] {
                if field.as_deref().map(|v| v.is_empty()).unwrap_or(true) {
                    *field = from;
                }
            }
        }
        self.connect_provider(side, &connection).await
    }
    fn toggle_favorite(&self, _path: String) {}
    fn get_favorites_only(&self) -> bool {
        false
    }
    fn set_favorites_only(&self, _value: bool) {}
}

type TermSession = (mpsc::Sender<Vec<u8>>, mpsc::Sender<(u16, u16)>);

struct TerminalHub {
    left_in: Rc<RefCell<Option<TermSession>>>,
    right_in: Rc<RefCell<Option<TermSession>>>,
    term_out_left: broadcast::Sender<Vec<u8>>,
    term_out_right: broadcast::Sender<Vec<u8>>,
    left_core: Rc<RouterState>,
    right_core: Rc<RouterState>,
}

impl TerminalHub {
    fn slot(&self, side: PanelSide) -> &Rc<RefCell<Option<TermSession>>> {
        match side {
            PanelSide::Left => &self.left_in,
            PanelSide::Right => &self.right_in,
        }
    }
    fn out(&self, side: PanelSide) -> &broadcast::Sender<Vec<u8>> {
        match side {
            PanelSide::Left => &self.term_out_left,
            PanelSide::Right => &self.term_out_right,
        }
    }
    fn core(&self, side: PanelSide) -> &Rc<RouterState> {
        match side {
            PanelSide::Left => &self.left_core,
            PanelSide::Right => &self.right_core,
        }
    }

    fn handle(&self, cmd: ApiCmd) {
        match cmd {
            ApiCmd::OpenTerminal { side } => self.open(side),
            ApiCmd::TerminalInput { side, data } => {
                if let Some((input, _)) = self.slot(side).borrow().as_ref() {
                    let _ = input.try_send(data);
                }
            }
            ApiCmd::TerminalResize { side, rows, cols } => {
                if let Some((_, resize)) = self.slot(side).borrow().as_ref() {
                    let _ = resize.try_send((rows, cols));
                }
            }
            ApiCmd::SetTerminalExpanded { side, expanded } => {
                notify_terminal_expanded(side_str(side), expanded);
            }
            _ => {}
        }
    }

    fn open(&self, side: PanelSide) {
        if self.slot(side).borrow().is_some() {
            return;
        }
        let core = self.core(side);
        let cwd = core.path.borrow().active().relative_path.clone();
        let spawned = match core.active_provider().get_ssh_connection_command(&cwd) {
            Some(args) => spawn_pty_command(args, None),
            None => spawn_pty_session(Some(cwd)),
        };
        match spawned {
            Ok(PtySession { input_tx, mut output_rx, resize_tx }) => {
                *self.slot(side).borrow_mut() = Some((input_tx, resize_tx));
                let out = self.out(side).clone();
                let slot = self.slot(side).clone();
                tokio::task::spawn_local(async move {
                    while let Some(data) = output_rx.recv().await {
                        let _ = out.send(data);
                    }
                    slot.borrow_mut().take();
                });
                notify_terminal_opened(side_str(side));
            }
            Err(e) => {
                let _ = self
                    .out(side)
                    .send(format!("failed to spawn shell: {e}\r\n").into_bytes());
            }
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8090);
    let config = client_config::AppConfig::new("ice-commander");
    secret_store::harden_file_permissions(&config.config_path());

    let make_core = || {
        let local: Rc<dyn FileSystemRpc> =
            Rc::new(virtualfs::local_rpc::LocalFileSystemRpc::new(config.clone()));
        Rc::new(RouterState::new(local.clone(), local, "/".to_string()))
    };
    let left = make_core();
    let right = make_core();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<ApiCmd>(64);
    let ws_sessions = WsSessions::default();
    init_notifier(ws_sessions.clone());
    let term_out_left = broadcast::channel::<Vec<u8>>(1024).0;
    let term_out_right = broadcast::channel::<Vec<u8>>(1024).0;
    start_api_server(
        port,
        true,
        "127.0.0.1".to_string(),
        tx.clone(),
        ws_sessions.clone(),
        term_out_left.clone(),
        term_out_right.clone(),
        include_bytes!("../../gtk-app/assets/webui/bundle.js").to_vec(),
        include_bytes!("../../gtk-app/assets/webui/style.css").to_vec(),
    );

    for (core, side) in [(&left, "left"), (&right, "right")] {
        let weak = Rc::downgrade(core);
        core.set_on_changed(move || {
            if let Some(c) = weak.upgrade() {
                notify_panel_state(side, read_state(&c));
            }
        });
    }

    let _ = left.list_active().await;
    let _ = right.list_active().await;

    let backend = Rc::new(ConsoleBackend { left, right, config: config.clone() });
    let terminals = Rc::new(TerminalHub {
        left_in: Rc::new(RefCell::new(None)),
        right_in: Rc::new(RefCell::new(None)),
        term_out_left,
        term_out_right,
        left_core: backend.left.clone(),
        right_core: backend.right.clone(),
    });

    println!("Ice Commander console web-server → http://127.0.0.1:{port}");

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            while let Some(cmd) = rx.recv().await {
                let backend = backend.clone();
                let terminals = terminals.clone();
                tokio::task::spawn_local(async move {
                    if let Some(cmd) = dispatch_core(&*backend, cmd).await {
                        terminals.handle(cmd);
                    }
                });
            }
        })
        .await;
}
