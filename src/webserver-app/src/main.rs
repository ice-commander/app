use std::cell::RefCell;
use std::rc::Rc;

use fm_core::rpc::FileSystemRpc;
use ic_platform::terminal::{spawn_pty_command, spawn_pty_session, PtySession};
use panel_core::RouterState;
use panel_server::{
    dispatch_core, init_notifier, notify_panel_state, notify_terminal_expanded,
    notify_terminal_opened, start_api_server, ApiCmd, ApiConnection, ApiDrive, ApiFileContent,
    ApiFileEntry, ApiLevel, ApiPanelState, ApiResult, ApiTab, PanelBackend, PanelSide, WsSessions,
    NEEDS_PASSWORD, PUBLIC_SETTINGS,
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
    let display = path_ref.absolute_path();
    let entries_data = path_ref.active().entries();
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
                enterable: !e.is_dir && panel_core::nav::is_archive(&e.name),
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
        path: display,
        entries: api_entries,
        showing_selector: core.showing_selector.get(),
        view_mode: "list".to_string(),
        selected: core.active_selected(),
        tabs: vec![ApiTab {
            id: 0,
            title: tab_title,
            icon: None,
        }],
        active_tab: 0,
    }
}

struct ConsoleBackend {
    left: Rc<RouterState>,
    right: Rc<RouterState>,
    config: client_config::AppConfig,
    views: RefCell<ic_view_session::Hub>,
    viewers: RefCell<std::collections::BTreeMap<u64, OpenViewer>>,
}

struct OpenViewer {
    viewer: String,
    client: String,
    source: ic_plugin_api::IcFsSource,
    _staged: fm_core::host_fs::Staged,
}

fn next_instance() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn sealed(viewer: &str, event: &serde_json::Value) -> serde_json::Value {
    let text = |key: &str| event.get(key).and_then(|held| held.as_str());
    ic_view_session::envelope(
        viewer,
        "null",
        text("type").unwrap_or("change"),
        text("node"),
        text("bind"),
        event.get("value").cloned(),
        event
            .get("values")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
        &facts(),
    )
}

fn facts() -> ic_view_session::HostFacts {
    ic_view_session::HostFacts::new(
        ic_plugin_api::IC_HOST_WEB,
        ic_i18n::current_lang(),
        &["copy"],
    )
}

/// The two records carry the same fields, so serde is an exact bridge between them.
fn record(c: &ApiConnection) -> connection_form::Connection {
    serde_json::to_value(c)
        .and_then(serde_json::from_value)
        .unwrap_or_default()
}

fn wire_of(held: &connection_form::Connection) -> Option<ApiConnection> {
    serde_json::to_value(held)
        .and_then(serde_json::from_value)
        .ok()
}

fn restore(c: &mut ApiConnection, held: &connection_form::Connection) {
    if let Ok(updated) = serde_json::to_value(held).and_then(serde_json::from_value) {
        *c = updated;
    }
}

/// Rewrites whichever fields the plugin declared secret, wherever they are kept.
fn over_secrets(c: &mut ApiConnection, f: impl FnMut(&str) -> Option<String>) {
    let kind = c.kind.clone();
    let mut held = record(c);
    connection_form::map_secrets(&mut held, &kind, f);
    restore(c, &held);
}

fn seal_api_conn(c: &mut ApiConnection) {
    over_secrets(c, |plain| Some(secret_store::encrypt_secret(plain)));
}

fn open_api_conn(mut c: ApiConnection) -> ApiConnection {
    over_secrets(&mut c, secret_store::decrypt_secret);
    c
}

fn blank_secrets(c: &mut ApiConnection) {
    over_secrets(c, |_| Some(String::new()));
}

fn carry_stored(incoming: &mut ApiConnection, stored: &ApiConnection) {
    let kind = incoming.kind.clone();
    let mut fresh = record(incoming);
    connection_form::carry_secrets(&mut fresh, &record(stored), &kind);
    restore(incoming, &fresh);
}

impl ConsoleBackend {
    fn core(&self, side: PanelSide) -> &Rc<RouterState> {
        match side {
            PanelSide::Left => &self.left,
            PanelSide::Right => &self.right,
        }
    }

    /// Turns the path the browser shows into the one this panel's filesystem answers to.
    fn provider(&self, side: PanelSide) -> Rc<dyn FileSystemRpc> {
        Rc::new(panel_core::RoutingProvider::snapshot(self.core(side)))
    }

    fn display_path(&self, side: PanelSide) -> String {
        self.core(side).path.borrow().absolute_path()
    }

    fn showing(&self, instance: u64) -> ApiResult<String> {
        self.viewers
            .borrow()
            .get(&instance)
            .map(|open| open.viewer.clone())
            .ok_or_else(|| format!("no viewer {instance} is open"))
    }

    fn let_go(&self, instance: u64) {
        let Some(open) = self.viewers.borrow_mut().remove(&instance) else {
            return;
        };
        ic_plugin_host::viewer_closed(&open.viewer, instance);
        fm_core::host_fs::HostSource::close(open.source);
    }

    /// The list, written back the one way it is read.
    fn keep(&self, conns: Vec<ApiConnection>) {
        let held: Vec<connection_form::Connection> = conns.iter().map(record).collect();
        connection_form::save_connections(&self.config, held);
    }

    fn stored_connections(&self) -> Vec<ApiConnection> {
        connection_form::stored_connections(&self.config)
            .iter()
            .filter_map(wire_of)
            .collect()
    }

    async fn connect_provider(&self, side: PanelSide, conn: &ApiConnection) -> ApiResult<()> {
        let conn = &open_api_conn(conn.clone());
        let core = self.core(side);
        let kind = conn.kind.clone();
        let Some(document) = ic_plugin_host::connection_document(&kind) else {
            return Err(format!("no plugin serves {kind} connections"));
        };
        let settings = connection_form::plugin_mount_settings(&document, &record(conn));
        let Some(provider) = ic_plugin_host::mount_connection(&kind, &settings) else {
            return Err(format!("no plugin serves {kind} connections"));
        };
        core.set_active_provider(provider.clone(), String::new());
        core.showing_selector.set(false);
        if let Some(rp) =
            connection_form::opening_path_in(&kind, &settings).filter(|p| !p.is_empty() && p != "/")
        {
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
        let result = self
            .provider(side)
            .delete_entries(paths)
            .await
            .map_err(|e| e.to_string());
        let _ = self.core(side).refresh().await;
        result
    }
    async fn mkdir(&self, side: PanelSide, name: String) -> ApiResult<()> {
        let result = self
            .provider(side)
            .create_directory(self.display_path(side), name, None)
            .await
            .map_err(|e| e.to_string());
        let _ = self.core(side).refresh().await;
        result
    }
    async fn rename(&self, side: PanelSide, old_path: String, new_name: String) -> ApiResult<()> {
        let parent = old_path.rsplitn(2, '/').nth(1).unwrap_or("").to_string();
        let new_path = format!("{}/{}", parent.trim_end_matches('/'), new_name);
        let result = self
            .provider(side)
            .rename_entry(old_path, new_path)
            .await
            .map_err(|e| e.to_string());
        let _ = self.core(side).refresh().await;
        result
    }
    async fn copy(&self, src: PanelSide, dst: PanelSide, paths: Vec<String>) -> ApiResult<()> {
        let src_provider = self.provider(src);
        let dst_provider = self.provider(dst);
        let dst_path = self.display_path(dst);
        let mut failed = Ok(());
        for src_path in paths {
            let file_name = src_path
                .rsplitn(2, '/')
                .next()
                .unwrap_or(&src_path)
                .to_string();
            let dst_file = format!("{}/{}", dst_path.trim_end_matches('/'), file_name);
            let copied = match src_provider.read_file(src_path.clone(), None).await {
                Ok(data) => dst_provider.write_file(dst_file, data, None, None).await,
                Err(e) => Err(e),
            };
            if let Err(e) = copied {
                failed = Err(format!("{src_path}: {e}"));
            }
        }
        let _ = self.core(dst).refresh().await;
        failed
    }
    async fn move_entries(
        &self,
        src: PanelSide,
        dst: PanelSide,
        paths: Vec<String>,
    ) -> ApiResult<()> {
        let src_provider = self.provider(src);
        let dst_provider = self.provider(dst);
        let dst_path = self.display_path(dst);
        let mut failed = Ok(());
        for src_path in paths {
            let file_name = src_path
                .rsplitn(2, '/')
                .next()
                .unwrap_or(&src_path)
                .to_string();
            let dst_file = format!("{}/{}", dst_path.trim_end_matches('/'), file_name);
            let moved = match src_provider.read_file(src_path.clone(), None).await {
                Ok(data) => match dst_provider.write_file(dst_file, data, None, None).await {
                    Ok(()) => src_provider.delete_entries(vec![src_path.clone()]).await,
                    Err(e) => Err(e),
                },
                Err(e) => Err(e),
            };
            if let Err(e) = moved {
                failed = Err(format!("{src_path}: {e}"));
            }
        }
        let _ = self.core(dst).refresh().await;
        let _ = self.core(src).refresh().await;
        failed
    }
    async fn read_file(&self, side: PanelSide, path: String) -> ApiResult<ApiFileContent> {
        let provider = self.provider(side);
        const MAX: usize = 2 * 1024 * 1024;
        match provider.read_file(path.clone(), None).await {
            Ok(bytes) if bytes.len() > MAX => Ok(ApiFileContent {
                path,
                content: String::new(),
                is_binary: true,
            }),
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Ok(ApiFileContent {
                    path,
                    content: text,
                    is_binary: false,
                }),
                Err(_) => Ok(ApiFileContent {
                    path,
                    content: String::new(),
                    is_binary: true,
                }),
            },
            Err(e) => Err(e.to_string()),
        }
    }
    async fn stream_file(&self, side: PanelSide, path: String) -> ApiResult<Vec<u8>> {
        const MAX: usize = 100 * 1024 * 1024;
        let provider = self.provider(side);
        match provider.read_file(path, None).await {
            Ok(bytes) if bytes.len() > MAX => Err(format!(
                "file too large to stream ({} MB max)",
                MAX / 1024 / 1024
            )),
            Ok(bytes) => Ok(bytes),
            Err(e) => Err(e.to_string()),
        }
    }
    async fn write_file(&self, side: PanelSide, path: String, content: String) -> ApiResult<()> {
        let core = self.core(side);
        let result = self
            .provider(side)
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
        let result = self
            .provider(side)
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
        for d in localfs::utils::get_drives() {
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
            let record = record(&conn);
            drives.push(ApiDrive {
                name: conn.name.clone(),
                key: format!("net:{}", conn.name),
                path: String::new(),
                icon: connection_form::kind_icon_ref(&conn.kind)
                    .unwrap_or_else(|| "connect.svg".to_string()),
                kind: "net".to_string(),
                subtitle: connection_form::kind_summary(&record).unwrap_or_default(),
                is_favorite: false,
                is_online: true,
            });
        }
        for drive in ic_plugin_host::plugin_drives() {
            drives.push(ApiDrive {
                name: drive.name,
                key: format!("offered:{}", drive.key),
                path: String::new(),
                icon: if drive.online {
                    "connect.svg".to_string()
                } else {
                    "disconnect.svg".to_string()
                },
                kind: "net".to_string(),
                subtitle: drive.subtitle,
                is_favorite: false,
                is_online: drive.online,
            });
        }
        drives
    }
    async fn activate_source(&self, side: PanelSide, key: String) -> ApiResult<()> {
        if let Some(offered) = key.strip_prefix("offered:") {
            let Some(drive) = ic_plugin_host::plugin_drives()
                .into_iter()
                .find(|held| held.key == offered)
            else {
                return Err(format!("nothing offers {offered:?} any more"));
            };
            let Some(provider) = ic_plugin_host::mount_connection(&drive.kind, &drive.settings)
            else {
                return Err(format!("no plugin serves {}", drive.kind));
            };
            self.core(side).set_active_provider(provider, String::new());
            return Ok(());
        }
        if let Some(name) = key.strip_prefix("net:") {
            return match self
                .stored_connections()
                .into_iter()
                .find(|c| c.name == name)
            {
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
                let held = connection_form::stored_secrets(&record(&c), &c.kind);
                blank_secrets(&mut c);
                c.stored_secrets = held;
                c
            })
            .collect()
    }
    fn save_connection(&self, connection: ApiConnection) -> ApiResult<()> {
        let mut conns = self.stored_connections();
        let mut connection = connection;
        connection.stored_secrets.clear();
        if ic_plugin_host::connection_document(&connection.kind).is_none() {
            return Err(format!("no plugin serves {} connections", connection.kind));
        }
        if let Some(existing) = conns.iter().find(|c| c.name == connection.name).cloned() {
            carry_stored(&mut connection, &existing);
        }
        seal_api_conn(&mut connection);
        match conns.iter_mut().find(|c| c.name == connection.name) {
            Some(existing) => *existing = connection,
            None => conns.push(connection),
        }
        self.keep(conns);
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
        self.keep(conns);
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
        self.keep(conns);
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
            carry_stored(&mut connection, &saved);
        }
        self.connect_provider(side, &connection).await
    }
    fn toggle_favorite(&self, _path: String) {}
    fn get_favorites_only(&self) -> bool {
        false
    }
    fn set_favorites_only(&self, _value: bool) {}

    fn connection_kinds(&self) -> ApiResult<serde_json::Value> {
        let offered: Vec<serde_json::Value> = connection_form::kind_table()
            .into_iter()
            .map(|entry| {
                serde_json::json!({
                    "id": entry.protocol.to_lowercase(),
                    "label": entry.label,
                    "document": entry.document,
                    "events": ic_plugin_host::connection_takes_events(&entry.protocol),
                })
            })
            .collect();
        Ok(serde_json::json!({ "host": facts().to_json(), "kinds": offered }))
    }

    async fn submit_connection_form(
        &self,
        form: panel_server::ApiConnectionForm,
    ) -> ApiResult<serde_json::Value> {
        let kind = form.kind.to_lowercase();
        let Some(document) = ic_plugin_host::connection_document(&kind) else {
            return Err(format!("no plugin serves {kind} connections"));
        };
        let values = form.values.into_iter().collect();
        let already_held =
            connection_form::stored_secrets_of(&self.config, form.editing.as_deref(), &kind);
        let collected = match connection_form::check_plugin_form(
            &document,
            values,
            form.touched,
            &already_held,
        ) {
            connection_form::FormOutcome::Ready(collected) => collected,
            connection_form::FormOutcome::Incomplete(missing) => {
                return Ok(serde_json::json!({ "ok": false, "missing": missing }))
            }
        };
        let editing = form.editing.as_deref().and_then(|name| {
            connection_form::stored_connections(&self.config)
                .iter()
                .position(|held| held.name == name)
        });
        let at = connection_form::store_connection(&self.config, &collected, &kind, editing)?;
        let Some(saved) = connection_form::stored_connections(&self.config)
            .get(at)
            .cloned()
        else {
            return Err("the record vanished as it was written".to_string());
        };
        if let Some(side) = form.connect {
            // connect_provider decrypts on its own, so the sealed record goes in as stored
            let wire = wire_of(&saved).ok_or("the saved record could not be read back")?;
            self.connect_provider(side, &wire).await?;
        }
        Ok(serde_json::json!({ "ok": true, "name": saved.name }))
    }

    fn connection_form_event(
        &self,
        kind: String,
        event: serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        let kind = kind.to_lowercase();
        connection_form::form_event(&kind, &event, &facts())
            .ok_or_else(|| format!("no plugin serves {kind} connections"))
    }

    fn plugin_views(&self) -> ApiResult<serde_json::Value> {
        let offered: Vec<serde_json::Value> = ic_plugin_host::view_ids()
            .into_iter()
            .map(|id| {
                let named = ic_plugin_host::view_title(&id).unwrap_or_else(|| id.clone());
                let shown = connection_form::translate_optional(&named).unwrap_or(named);
                serde_json::json!({ "id": id, "title": shown })
            })
            .collect();
        Ok(serde_json::json!(offered))
    }

    fn open_plugin_view(&self, id: String, argument: String) -> ApiResult<serde_json::Value> {
        self.views
            .borrow_mut()
            .open(&id, &argument, &ic_plugin_host::Views, &facts())
            .ok_or_else(|| format!("{id} cannot describe itself"))
    }

    fn plugin_view_event(
        &self,
        id: String,
        event: serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        self.views
            .borrow_mut()
            .event(&id, &event, &ic_plugin_host::Views, &facts())
            .ok_or_else(|| format!("{id} is not open"))
    }

    fn close_plugin_view(&self, id: String) -> ApiResult<()> {
        if self.views.borrow_mut().close(&id) {
            ic_plugin_host::view_closed(&id, 1);
        }
        Ok(())
    }

    fn plugin_asset(&self, name: String) -> ApiResult<Vec<u8>> {
        ic_plugin_host::asset(&name).ok_or_else(|| format!("no plugin offers {name}"))
    }

    async fn open_viewer(
        &self,
        side: PanelSide,
        path: String,
        client: String,
    ) -> ApiResult<Option<serde_json::Value>> {
        let named = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
        let Some(viewer) = ic_plugin_host::viewer_for(&named) else {
            return Ok(None);
        };
        let provider = self.provider(side);
        let staged = fm_core::host_fs::stage(&provider, &path)
            .await
            .map_err(|e| e.to_string())?;
        let source =
            fm_core::host_fs::HostSource::rooted_at(&staged.root, &provider.fs_id(), "").open();
        let instance = next_instance();
        if !ic_plugin_host::open_viewer(&viewer, instance, source, &staged.name) {
            fm_core::host_fs::HostSource::close(source);
            return Err(format!("{viewer} could not open {named}"));
        }
        let context = ic_view_session::context(&facts(), "null");
        let Some(document) = ic_plugin_host::describe_viewer(&viewer, instance, &context) else {
            ic_plugin_host::viewer_closed(&viewer, instance);
            fm_core::host_fs::HostSource::close(source);
            return Err(format!("{viewer} cannot describe itself"));
        };
        let answered = serde_json::json!({
            "instance": instance,
            "viewer": viewer,
            "document": document,
        });
        self.viewers.borrow_mut().insert(
            instance,
            OpenViewer {
                viewer,
                client,
                source,
                _staged: staged,
            },
        );
        Ok(Some(answered))
    }

    fn viewer_event(
        &self,
        instance: u64,
        event: serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        let viewer = self.showing(instance)?;
        let sent = sealed(&viewer, &event);
        let Some(answer) = ic_plugin_host::viewer_event(&viewer, instance, &sent) else {
            return Err(format!("{viewer} did not answer"));
        };
        let again = answer
            .get("redescribe")
            .and_then(|again| again.as_bool())
            .unwrap_or(false);
        let document = again
            .then(|| {
                ic_plugin_host::describe_viewer(
                    &viewer,
                    instance,
                    &ic_view_session::context(&facts(), "null"),
                )
            })
            .flatten();
        Ok(serde_json::json!({ "answer": answer, "document": document }))
    }

    fn viewer_part(&self, instance: u64, name: String) -> ApiResult<Vec<u8>> {
        let viewer = self.showing(instance)?;
        ic_plugin_host::viewer_part(&viewer, instance, &name)
            .ok_or_else(|| format!("{viewer} offers no {name}"))
    }

    fn close_viewer(&self, instance: u64, force: bool) -> ApiResult<serde_json::Value> {
        let Ok(viewer) = self.showing(instance) else {
            return Ok(serde_json::json!({ "closed": true }));
        };
        if !force && !ic_plugin_host::viewer_may_close(&viewer, instance) {
            let document = ic_plugin_host::describe_viewer(
                &viewer,
                instance,
                &ic_view_session::context(&facts(), "null"),
            );
            return Ok(serde_json::json!({ "closed": false, "document": document }));
        }
        self.let_go(instance);
        Ok(serde_json::json!({ "closed": true }))
    }

    fn drop_viewers_of(&self, client: String) {
        let left: Vec<u64> = self
            .viewers
            .borrow()
            .iter()
            .filter(|(_, open)| open.client == client)
            .map(|(instance, _)| *instance)
            .collect();
        for instance in left {
            self.let_go(instance);
        }
    }

    fn translations(&self) -> ApiResult<serde_json::Value> {
        let lang = ic_i18n::current_lang();
        Ok(serde_json::json!({
            "lang": lang,
            "keys": ic_i18n::dictionary(lang),
        }))
    }
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

    fn handle(&self, cmd: ApiCmd) -> Option<ApiCmd> {
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
            other => return Some(other),
        }
        None
    }

    fn open(&self, side: PanelSide) {
        if self.slot(side).borrow().is_some() {
            return;
        }
        let core = self.core(side);
        let cwd = core.path.borrow().active().relative_path.clone();
        let spawned = match core.active_provider().open_shell(&cwd, 24, 80) {
            Some(session) => Ok(session),
            None => match core.active_provider().get_ssh_connection_command(&cwd) {
                Some(args) => spawn_pty_command(args, None),
                None => spawn_pty_session(Some(cwd)),
            },
        };
        match spawned {
            Ok(PtySession {
                input_tx,
                mut output_rx,
                resize_tx,
            }) => {
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

/// Answers the commands this frontend has no implementation for. Without this the
/// reply channel is dropped and the caller sees a bare 503 with nothing in the log.
fn refuse(cmd: ApiCmd) {
    const WHY: &str = "the web server does not implement this";
    match cmd {
        ApiCmd::SetViewMode { reply, .. } | ApiCmd::SetSort { reply, .. } => {
            let _ = reply.send(Err(WHY.to_string()));
        }
        ApiCmd::OpenNative { reply, .. } => {
            let _ = reply.send(Err(WHY.to_string()));
        }
        ApiCmd::ListWindows { reply } => {
            let _ = reply.send(Err(WHY.to_string()));
        }
        ApiCmd::CloseExtraWindows { reply } => {
            let _ = reply.send(Err(WHY.to_string()));
        }
        _ => eprintln!("web server: a command reached the end of the loop unhandled"),
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
    ic_i18n::register_locales!("../../gtk-app/locales");
    ic_i18n::set_lang(
        &config
            .get::<String>("ui.language")
            .unwrap_or_else(|| "en".to_string()),
    );
    ic_plugin_host::set_settings_config(config.clone());
    // A plugin may ask from its own thread; the loop below takes it to the owning one.
    let (plugin_tx, mut plugin_rx) =
        tokio::sync::mpsc::unbounded_channel::<ic_plugin_host::Wanted>();
    ic_plugin_host::set_waker(std::sync::Arc::new(move |wanted| {
        let _ = plugin_tx.send(wanted);
    }));
    // After the application's own dictionaries, so a plugin's keys sit on top.
    ic_plugin_host::set_host_kind(ic_plugin_api::IC_HOST_WEB);
    for attempt in ic_plugin_host::loader::load_for(&config) {
        if attempt.went_wrong() {
            eprintln!("installed plugin {} did not load", attempt.name);
        }
    }

    let make_core = || {
        let local: Rc<dyn FileSystemRpc> =
            Rc::new(localfs::local_rpc::LocalFileSystemRpc::new(config.clone()));
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

    let backend = Rc::new(ConsoleBackend {
        left,
        right,
        config: config.clone(),
        views: RefCell::new(ic_view_session::Hub::default()),
        viewers: RefCell::new(std::collections::BTreeMap::new()),
    });
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
            loop {
                let cmd = tokio::select! {
                    asked = plugin_rx.recv() => {
                        if let Some(wanted) = asked {
                            ic_plugin_host::deliver(wanted);
                        }
                        continue;
                    }
                    cmd = rx.recv() => match cmd {
                        Some(cmd) => cmd,
                        None => break,
                    },
                };
                let backend = backend.clone();
                let terminals = terminals.clone();
                tokio::task::spawn_local(async move {
                    if let Some(cmd) = dispatch_core(&*backend, cmd).await {
                        if let Some(cmd) = terminals.handle(cmd) {
                            refuse(cmd);
                        }
                    }
                });
            }
        })
        .await;
}

#[cfg(test)]
mod what_the_browser_sees {
    use super::read_state;
    use fm_core::rpc::{FileSystemRpc, RemoteFileEntry};
    use panel_core::RouterState;
    use std::rc::Rc;

    struct Shared;

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Shared {
        async fn list_dir(&self, _: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
            Ok(vec![RemoteFileEntry {
                name: "film.mkv".to_string(),
                is_dir: false,
                size: 10,
                modified: 0,
                permissions: None,
                extra: Vec::new(),
            }])
        }
        fn fs_id(&self) -> String {
            "local/film.torrent".to_string()
        }
    }

    /// Nothing here re-reads a folder on its own: the browser is served from
    /// what the application last saw. So a plugin saying its filesystem moved
    /// on must not leave that empty — a browser showing an empty folder is a
    /// worse answer than one showing what was there a moment ago.
    #[tokio::test]
    async fn a_stale_folder_is_still_served_until_something_reads_it_again() {
        fm_core::listing::forget_everything();
        let fs: Rc<dyn FileSystemRpc> = Rc::new(Shared);
        let core = RouterState::new(fs.clone(), fs.clone(), "/".to_string());
        core.list_active().await.expect("a listing");
        assert_eq!(read_state(&core).entries.len(), 1);

        ic_plugin_host::deliver(ic_plugin_host::Wanted::FsInvalidate {
            extensions: vec![".torrent".to_string()],
        });

        assert_eq!(
            read_state(&core).entries.len(),
            1,
            "the browser was handed an empty folder"
        );
        assert!(
            !fm_core::listing::is_remembered(fs.as_ref(), "/"),
            "and it must be read again when anything asks"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{blank_secrets, carry_stored, record, restore};
    use panel_server::{ApiConnection, PanelBackend};

    fn declare(kind: &str, secrets: &[&str]) {
        let fields: Vec<serde_json::Value> = ["name", "host", "user", "pass", "api_token"]
            .iter()
            .map(|bind| {
                serde_json::json!({
                    "bind": bind,
                    "type": "text",
                    "scope": if *bind == "name" { "record" } else { "settings" },
                    "required": matches!(*bind, "name" | "host"),
                    "secret": secrets.contains(bind),
                })
            })
            .collect();
        let document = serde_json::json!({
            "schema": 1,
            "kind": kind,
            "fields": fields,
            "form": { "t": "column", "children": [] },
        })
        .to_string();
        let host = ic_plugin_host::host_table();
        let id = std::ffi::CString::new(kind).expect("a kind id");
        assert_eq!(
            (host.register_connection_kind)(
                id.as_ptr(),
                document.as_ptr(),
                document.len() as u64,
                std::ptr::null(),
                std::ptr::null_mut(),
            ),
            ic_plugin_api::IC_OK
        );
    }

    fn sample(kind: &str) -> ApiConnection {
        serde_json::from_value(serde_json::json!({
            "name": "work",
            "folder": "team",
            "kind": kind.to_lowercase(),
            "settings": {
                "host": "h.example.org",
                "port": "2222",
                "user": "ivan",
                "pass": "hunter2",
                "passphrase": "phrase",
                "api_token": "t-0007",
            },
        }))
        .expect("a record")
    }

    fn backend(config: client_config::AppConfig) -> super::ConsoleBackend {
        let make = || {
            let local: std::rc::Rc<dyn fm_core::rpc::FileSystemRpc> =
                std::rc::Rc::new(localfs::local_rpc::LocalFileSystemRpc::new(config.clone()));
            std::rc::Rc::new(panel_core::RouterState::new(
                local.clone(),
                local,
                "/".to_string(),
            ))
        };
        super::ConsoleBackend {
            left: make(),
            right: make(),
            config,
            views: std::cell::RefCell::new(ic_view_session::Hub::default()),
            viewers: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        }
    }

    fn posted(kind: &str, typed: &[(&str, &str)]) -> panel_server::ApiConnectionForm {
        panel_server::ApiConnectionForm {
            kind: kind.to_string(),
            values: typed
                .iter()
                .map(|(bind, value)| ((*bind).to_string(), serde_json::json!(value)))
                .collect(),
            touched: typed.iter().map(|(bind, _)| (*bind).to_string()).collect(),
            editing: None,
            connect: None,
        }
    }

    #[tokio::test]
    async fn the_browser_saves_a_connection_without_naming_a_protocol() {
        declare("webform", &["pass"]);
        let config = client_config::AppConfig::new("ice-commander-web-form");
        config.set(
            connection_form::CONNECTIONS_KEY,
            Vec::<connection_form::Connection>::new(),
        );
        let backend = backend(config.clone());

        let offered = backend.connection_kinds().expect("the kinds are listed");
        assert_eq!(
            offered["host"]["kind"],
            serde_json::json!("web"),
            "the browser is told which host it is talking to, so host.* reads the same there"
        );
        let listed = offered["kinds"].as_array().expect("an array of kinds");
        assert!(
            listed
                .iter()
                .any(|entry| entry["id"] == serde_json::json!("webform")),
            "a declared kind reaches the browser"
        );
        assert!(
            listed
                .iter()
                .all(|entry| entry["document"]["fields"].is_array()),
            "and each one carries the document the browser renders"
        );

        let answered = backend
            .submit_connection_form(posted(
                "webform",
                &[
                    ("name", "work"),
                    ("host", "h.example.org"),
                    ("pass", "hunter2"),
                ],
            ))
            .await
            .expect("the form is accepted");
        assert_eq!(answered["ok"], serde_json::json!(true));
        assert_eq!(answered["name"], serde_json::json!("work"));

        let stored = connection_form::stored_connections(&config);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].value("host").as_deref(), Some("h.example.org"));
        assert_ne!(
            stored[0].value("pass").as_deref(),
            Some("hunter2"),
            "the password must not be stored as typed"
        );
    }

    #[tokio::test]
    async fn an_incomplete_form_comes_back_with_what_is_missing() {
        declare("webmissing", &["pass"]);
        let config = client_config::AppConfig::new("ice-commander-web-missing");
        config.set(
            connection_form::CONNECTIONS_KEY,
            Vec::<connection_form::Connection>::new(),
        );
        let backend = backend(config.clone());

        let answered = backend
            .submit_connection_form(posted("webmissing", &[("name", "work")]))
            .await
            .expect("the call itself succeeds");
        assert_eq!(answered["ok"], serde_json::json!(false));
        assert_eq!(answered["missing"], serde_json::json!(["host"]));
        assert!(
            connection_form::stored_connections(&config).is_empty(),
            "nothing is written until the form is complete"
        );
    }

    /// A kind whose password is required and kept: the form cannot commit without
    /// one, and an empty box means the stored one stands.
    fn declare_guarding(kind: &str) {
        let document = serde_json::json!({
            "schema": 1,
            "kind": kind,
            "fields": [
                { "bind": "name", "type": "text", "scope": "record", "required": true },
                { "bind": "host", "type": "text", "scope": "settings", "required": true },
                { "bind": "pass", "type": "text", "scope": "settings", "secret": true,
                  "required": true, "empty_as_absent": true, "empty_keeps_stored": true }
            ],
            "form": { "t": "column", "children": [] },
        })
        .to_string();
        let host = ic_plugin_host::host_table();
        let id = std::ffi::CString::new(kind).expect("a kind id");
        assert_eq!(
            (host.register_connection_kind)(
                id.as_ptr(),
                document.as_ptr(),
                document.len() as u64,
                std::ptr::null(),
                std::ptr::null_mut(),
            ),
            ic_plugin_api::IC_OK
        );
    }

    #[tokio::test]
    async fn a_password_the_browser_is_never_handed_does_not_have_to_be_typed_again_to_save() {
        declare_guarding("webguard");
        let config = client_config::AppConfig::new("ice-commander-web-guard");
        config.set(
            connection_form::CONNECTIONS_KEY,
            Vec::<connection_form::Connection>::new(),
        );
        let backend = backend(config.clone());

        let answered = backend
            .submit_connection_form(posted(
                "webguard",
                &[
                    ("name", "work"),
                    ("host", "h.example.org"),
                    ("pass", "hunter2"),
                ],
            ))
            .await
            .expect("the form is accepted");
        assert_eq!(answered["ok"], serde_json::json!(true));

        let listed = backend.get_connections();
        let shown = listed
            .iter()
            .find(|held| held.name == "work")
            .expect("the record is listed");
        assert_eq!(
            shown.stored_secrets,
            vec!["pass".to_string()],
            "the browser is told which binds the host already holds a secret for"
        );
        assert_eq!(
            shown.settings.get("pass").map(String::as_str),
            Some(""),
            "and is handed none of them all the same"
        );

        let mut again = posted("webguard", &[("name", "work"), ("host", "h2.example.org")]);
        again.editing = Some("work".to_string());
        let answered = backend
            .submit_connection_form(again)
            .await
            .expect("the form is accepted");
        assert_eq!(
            answered["ok"],
            serde_json::json!(true),
            "a password the record already holds is not demanded again"
        );

        let stored = connection_form::stored_connections(&config);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].value("host").as_deref(), Some("h2.example.org"));
        let mut opened = stored[0].clone();
        connection_form::unseal(&mut opened);
        assert_eq!(
            opened.value("pass").as_deref(),
            Some("hunter2"),
            "and is still the one stored before the edit"
        );
    }

    #[tokio::test]
    async fn a_kind_no_plugin_serves_is_refused_rather_than_stored() {
        let config = client_config::AppConfig::new("ice-commander-web-unknown");
        let backend = backend(config);
        assert!(backend
            .submit_connection_form(posted("nosuchkind", &[("name", "x")]))
            .await
            .is_err());
    }

    #[test]
    fn the_bridge_between_the_two_records_loses_nothing() {
        let before = sample("SFTP");
        let mut after = ApiConnection::default();
        restore(&mut after, &record(&before));
        assert_eq!(
            serde_json::to_value(&before).expect("encodes"),
            serde_json::to_value(&after).expect("encodes"),
            "every field must survive the round trip through Connection"
        );
    }

    #[test]
    fn the_browser_is_never_handed_a_secret_the_plugin_declared() {
        declare("vaultweb", &["pass", "api_token"]);
        let mut shown = sample("VAULTWEB");
        blank_secrets(&mut shown);
        assert_eq!(shown.settings.get("pass").map(String::as_str), Some(""));
        assert_eq!(
            shown.settings.get("api_token").map(String::as_str),
            Some(""),
            "a secret kept in the settings map must be blanked too"
        );
        assert_eq!(
            shown.settings.get("user").map(String::as_str),
            Some("ivan"),
            "a field the plugin did not call secret still reaches the browser"
        );
        assert_eq!(
            shown.settings.get("passphrase").map(String::as_str),
            Some("phrase"),
            "and neither does a field it did not declare at all"
        );
    }

    #[test]
    fn a_save_that_leaves_a_secret_blank_keeps_what_was_stored() {
        declare("keeperweb", &["pass", "api_token"]);
        let stored = sample("KEEPERWEB");
        let mut incoming = sample("KEEPERWEB");
        blank_secrets(&mut incoming);
        carry_stored(&mut incoming, &stored);
        assert_eq!(
            incoming.settings.get("pass").map(String::as_str),
            Some("hunter2")
        );
        assert_eq!(
            incoming.settings.get("api_token").map(String::as_str),
            Some("t-0007")
        );
    }

    const SIGN_IN: &str = r#"{
        "schema": 1,
        "kind": "weblistens",
        "identity": "name",
        "fields": [
            { "bind": "name", "type": "text", "scope": "record", "required": true },
            { "bind": "host", "type": "text", "scope": "settings", "required": true },
            { "bind": "pass", "type": "text", "scope": "settings", "secret": true }
        ],
        "form": { "t": "column", "children": [
            { "t": "input", "id": "host", "bind": "host" },
            { "t": "input", "id": "pass", "bind": "pass" },
            { "t": "button", "id": "check", "intent": { "do": "emit", "node": "check" } } ] }
    }"#;

    const SIGNED_IN: &str = r#"{
        "schema": 1,
        "kind": "weblistens",
        "identity": "name",
        "fields": [
            { "bind": "name", "type": "text", "scope": "record", "required": true },
            { "bind": "host", "type": "text", "scope": "settings", "required": true },
            { "bind": "pass", "type": "text", "scope": "settings", "secret": true }
        ],
        "form": { "t": "column", "children": [
            { "t": "text", "id": "already", "text": "signed in" } ] }
    }"#;

    static SIGNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static HEARD: std::sync::Mutex<Vec<serde_json::Value>> = std::sync::Mutex::new(Vec::new());

    thread_local! {
        static ANSWER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn answer(source: &str) -> ic_plugin_api::IcBytes {
        ANSWER.with(|slot| {
            *slot.borrow_mut() = source.as_bytes().to_vec();
            let held = slot.borrow();
            ic_plugin_api::IcBytes {
                data: held.as_ptr(),
                len: held.len() as u64,
            }
        })
    }

    extern "C" fn kind_describes(
        _: *const u8,
        _: u64,
        _: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        answer(if SIGNED.load(std::sync::atomic::Ordering::Relaxed) {
            SIGNED_IN
        } else {
            SIGN_IN
        })
    }

    extern "C" fn kind_hears(
        event: *const u8,
        len: u64,
        _: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        let source = unsafe { std::slice::from_raw_parts(event, len as usize) };
        let mut node = String::new();
        if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(source) {
            node = parsed["node"].as_str().unwrap_or_default().to_string();
            HEARD.lock().expect("the log").push(parsed);
        }
        if node == "signin" {
            SIGNED.store(true, std::sync::atomic::Ordering::Relaxed);
            return answer(r#"{ "redescribe": true }"#);
        }
        answer(
            r#"{ "put": { "host": "found.example.org" },
                 "set": { "data.checked": true },
                 "clipboard": "t-0007" }"#,
        )
    }

    fn declare_listening() {
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: kind_opens,
            fs: std::ptr::null(),
            describe: Some(kind_describes),
            on_event: Some(kind_hears),
        };
        let host = ic_plugin_host::host_table();
        let id = std::ffi::CString::new("weblistens").expect("a kind id");
        assert_eq!(
            (host.register_connection_kind)(
                id.as_ptr(),
                SIGN_IN.as_ptr(),
                SIGN_IN.len() as u64,
                &table,
                std::ptr::null_mut(),
            ),
            ic_plugin_api::IC_OK
        );
    }

    extern "C" fn kind_opens(
        _: *const u8,
        _: u64,
        _: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcFsHandle {
        std::ptr::null_mut()
    }

    fn form_event(node: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "activate",
            "node": node,
            "values": { "name": "work", "host": "h.example.org", "pass": "hunter2" },
            "touched": ["host"],
        })
    }

    #[test]
    fn a_form_button_reaches_the_plugin_and_what_it_answers_comes_back() {
        declare_listening();
        let config = client_config::AppConfig::new("ice-commander-web-listens");
        let backend = backend(config);

        let offered = backend.connection_kinds().expect("the kinds are listed");
        let listed = offered["kinds"]
            .as_array()
            .expect("an array of kinds")
            .iter()
            .find(|entry| entry["id"] == serde_json::json!("weblistens"))
            .expect("the listening kind is offered")
            .clone();
        assert_eq!(
            listed["events"],
            serde_json::json!(true),
            "the browser is told the form is worth wiring"
        );

        HEARD.lock().expect("the log").clear();
        let answered = backend
            .connection_form_event("WebListens".to_string(), form_event("check"))
            .expect("the plugin answered");

        let seen = HEARD
            .lock()
            .expect("the log")
            .last()
            .cloned()
            .expect("the plugin was reached");
        assert_eq!(seen["type"], serde_json::json!("activate"));
        assert_eq!(seen["node"], serde_json::json!("check"));
        assert_eq!(seen["values"]["host"], serde_json::json!("h.example.org"));
        assert!(
            seen["values"].get("pass").is_none(),
            "a secret never leaves the host"
        );
        assert_eq!(
            seen["host"]["kind"],
            serde_json::json!("web"),
            "the plugin is told which frontend the form is drawn on"
        );

        assert_eq!(
            answered["put"]["host"],
            serde_json::json!("found.example.org"),
            "what the plugin wrote is keyed by node id, for the browser to place"
        );
        assert_eq!(
            answered["state"]["data"]["checked"],
            serde_json::json!(true)
        );
        assert_eq!(answered["clipboard"], serde_json::json!("t-0007"));
        assert!(
            answered["state"]["state"].get("pass").is_none(),
            "nor is a secret handed back to the browser"
        );

        assert_eq!(
            answered["document"], listed["document"],
            "an event that does not ask for it leaves the form as it was"
        );

        let again = backend
            .connection_form_event("weblistens".to_string(), form_event("signin"))
            .expect("the plugin answered");
        let mut rebuilt = false;
        let document: ic_view::Document =
            serde_json::from_value(again["document"].clone()).expect("a document");
        document
            .form
            .walk(&mut |node| rebuilt |= node.id.as_deref() == Some("already"));
        assert!(
            rebuilt,
            "redescribe answers with the form the plugin now wants shown"
        );
    }

    #[test]
    fn a_kind_that_takes_no_events_answers_a_form_event_without_changing_anything() {
        declare("webdeaf", &["pass"]);
        let config = client_config::AppConfig::new("ice-commander-web-deaf");
        let backend = backend(config);

        let offered = backend.connection_kinds().expect("the kinds are listed");
        let listed = offered["kinds"]
            .as_array()
            .expect("an array of kinds")
            .iter()
            .find(|entry| entry["id"] == serde_json::json!("webdeaf"))
            .expect("the kind is offered")
            .clone();
        assert_eq!(listed["events"], serde_json::json!(false));

        let answered = backend
            .connection_form_event("webdeaf".to_string(), form_event("check"))
            .expect("the call itself succeeds");
        assert_eq!(answered["put"], serde_json::json!({}));
        assert_eq!(answered["clipboard"], serde_json::Value::Null);
        assert_eq!(
            answered["document"], listed["document"],
            "the form the browser already draws is left exactly as it was"
        );
    }

    #[test]
    fn a_form_event_for_a_kind_no_plugin_serves_is_refused() {
        let config = client_config::AppConfig::new("ice-commander-web-noevent");
        let backend = backend(config);
        assert!(backend
            .connection_form_event("nosuchkind".to_string(), form_event("check"))
            .is_err());
    }

    const SHOWS_FILE: &str = r#"{
        "schema": 1,
        "fields": [],
        "form": { "t": "view", "surface": "window", "children": [
            { "t": "text", "id": "head", "text": "the first line" } ] }
    }"#;

    struct Watched {
        opened: std::sync::Mutex<Vec<String>>,
        seen: std::sync::Mutex<Vec<serde_json::Value>>,
        closes: std::sync::atomic::AtomicUsize,
        refuses: std::sync::atomic::AtomicBool,
    }

    thread_local! {
        static SPOKEN: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn spoken(source: &str) -> ic_plugin_api::IcBytes {
        SPOKEN.with(|slot| {
            *slot.borrow_mut() = source.as_bytes().to_vec();
            let held = slot.borrow();
            ic_plugin_api::IcBytes {
                data: held.as_ptr(),
                len: held.len() as u64,
            }
        })
    }

    extern "C" fn file_opens(
        _: u64,
        _: ic_plugin_api::IcFsSource,
        path: *const std::os::raw::c_char,
        user_data: *mut std::ffi::c_void,
    ) -> std::os::raw::c_int {
        let watched = unsafe { &*(user_data as *const Watched) };
        watched.opened.lock().expect("the viewer").push(
            unsafe { std::ffi::CStr::from_ptr(path) }
                .to_string_lossy()
                .into_owned(),
        );
        ic_plugin_api::IC_OK
    }

    extern "C" fn file_describes(
        _: *const u8,
        _: u64,
        _: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        spoken(SHOWS_FILE)
    }

    extern "C" fn file_hears(
        event: *const u8,
        len: u64,
        user_data: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        let watched = unsafe { &*(user_data as *const Watched) };
        let raw = unsafe { std::slice::from_raw_parts(event, len as usize) };
        watched
            .seen
            .lock()
            .expect("the viewer")
            .push(serde_json::from_slice(raw).expect("the host sends json"));
        spoken("null")
    }

    extern "C" fn file_closing(_: u64, user_data: *mut std::ffi::c_void) -> std::os::raw::c_int {
        let watched = unsafe { &*(user_data as *const Watched) };
        if watched
            .refuses
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            ic_plugin_api::IC_ERR_INIT_FAILED
        } else {
            ic_plugin_api::IC_OK
        }
    }

    extern "C" fn file_was_closed(_: u64, user_data: *mut std::ffi::c_void) {
        let watched = unsafe { &*(user_data as *const Watched) };
        watched
            .closes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn declare_viewer(id: &str, extensions: &str) -> &'static Watched {
        let watched: &'static Watched = Box::leak(Box::new(Watched {
            opened: std::sync::Mutex::new(Vec::new()),
            seen: std::sync::Mutex::new(Vec::new()),
            closes: std::sync::atomic::AtomicUsize::new(0),
            refuses: std::sync::atomic::AtomicBool::new(false),
        }));
        let window = ic_plugin_api::IcViewVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcViewVTable>() as u32,
            describe: file_describes,
            on_event: Some(file_hears),
            closed: None,
        };
        let table = ic_plugin_api::IcViewerVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcViewerVTable>() as u32,
            view: &window,
            open: file_opens,
            closed: Some(file_was_closed),
            content: None,
            closing: Some(file_closing),
            canvas_ready: None,
            canvas_draw: None,
            canvas_gone: None,
        };
        let host = ic_plugin_host::host_table();
        let named = std::ffi::CString::new(id).expect("a viewer id");
        let claims = std::ffi::CString::new(extensions).expect("the extensions");
        assert_eq!(
            (host.register_viewer)(
                named.as_ptr(),
                claims.as_ptr(),
                0,
                &table,
                watched as *const Watched as *mut std::ffi::c_void,
            ),
            ic_plugin_api::IC_OK
        );
        watched
    }

    async fn shown(
        backend: &super::ConsoleBackend,
        name: &str,
        client: &str,
    ) -> (u64, serde_json::Value) {
        let at = std::env::temp_dir().join(name);
        std::fs::write(&at, b"the first line\n").expect("a file");
        let answered = backend
            .open_viewer(
                panel_server::PanelSide::Left,
                at.to_string_lossy().into_owned(),
                client.to_string(),
            )
            .await
            .expect("the plugin opens it")
            .expect("a plugin claims it");
        (
            answered["instance"].as_u64().expect("an instance"),
            answered,
        )
    }

    #[tokio::test]
    async fn the_plugin_hears_the_same_envelope_the_desktop_sends_it() {
        let watched = declare_viewer("webviewer", ".webview");
        let backend = backend(client_config::AppConfig::new("ice-commander-web-viewer"));
        let (instance, answered) = shown(&backend, "notes.webview", "browser-1").await;

        assert_eq!(answered["viewer"], serde_json::json!("webviewer"));
        assert_eq!(
            watched.opened.lock().expect("the viewer").as_slice(),
            ["notes.webview".to_string()],
            "the plugin is given the name on the filesystem it was let into, not the bytes"
        );

        backend
            .viewer_event(
                instance,
                serde_json::json!({ "type": "opened", "values": {} }),
            )
            .expect("the plugin answers");
        let opened = watched.seen.lock().expect("the viewer")[0].clone();
        assert_eq!(opened["v"], serde_json::json!(1));
        assert_eq!(opened["view"], serde_json::json!("webviewer"));
        assert_eq!(opened["type"], serde_json::json!("opened"));
        assert_eq!(opened["gesture"], serde_json::json!(false));
        assert_eq!(opened["arg"], serde_json::Value::Null);
        assert_eq!(
            opened["host"]["kind"],
            serde_json::json!(ic_plugin_api::IC_HOST_WEB),
            "the same word init was given"
        );

        backend
            .viewer_event(
                instance,
                serde_json::json!({ "type": "activate", "node": "head", "values": {} }),
            )
            .expect("the plugin answers");
        let pressed = watched
            .seen
            .lock()
            .expect("the viewer")
            .last()
            .cloned()
            .expect("an event");
        assert_eq!(pressed["gesture"], serde_json::json!(true));
        assert_eq!(pressed["node"], serde_json::json!("head"));

        assert_eq!(
            backend
                .close_viewer(instance, false)
                .expect("the window closes")["closed"],
            serde_json::json!(true)
        );
    }

    #[tokio::test]
    async fn a_plugin_that_says_no_keeps_the_window_and_draws_into_it() {
        let watched = declare_viewer("webrefuser", ".webrefuse");
        let backend = backend(client_config::AppConfig::new("ice-commander-web-refuse"));
        let (instance, _) = shown(&backend, "draft.webrefuse", "browser-1").await;

        watched
            .refuses
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let refused = backend
            .close_viewer(instance, false)
            .expect("the call itself succeeds");
        assert_eq!(refused["closed"], serde_json::json!(false));
        assert!(
            refused["document"]["form"].is_object(),
            "a refusal comes back with the window described again"
        );
        assert_eq!(watched.closes.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(
            backend
                .viewer_event(instance, serde_json::json!({ "type": "opened" }))
                .is_ok(),
            "the window the plugin kept is still open"
        );

        let closed = backend
            .close_viewer(instance, false)
            .expect("the window closes");
        assert_eq!(closed["closed"], serde_json::json!(true));
        assert_eq!(watched.closes.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_browser_that_stops_watching_takes_what_it_left_open_with_it() {
        let watched = declare_viewer("weblost", ".weblost");
        let backend = backend(client_config::AppConfig::new("ice-commander-web-lost"));
        let (mine, _) = shown(&backend, "mine.weblost", "browser-1").await;
        let (theirs, _) = shown(&backend, "theirs.weblost", "browser-2").await;

        watched
            .refuses
            .store(true, std::sync::atomic::Ordering::SeqCst);
        backend.drop_viewers_of("browser-1".to_string());
        assert_eq!(
            watched.closes.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "a window nobody is looking at any more is not asked, it is told"
        );
        assert!(
            backend
                .viewer_event(mine, serde_json::json!({ "type": "opened" }))
                .is_err(),
            "and the instance is gone with it"
        );
        assert!(
            backend
                .viewer_event(theirs, serde_json::json!({ "type": "opened" }))
                .is_ok(),
            "the other browser keeps what it opened"
        );
    }
}

#[cfg(test)]
mod the_path_the_browser_sends {
    use super::{read_state, ConsoleBackend};
    use fm_core::rpc::{FileSystemRpc, RemoteFileEntry};
    use panel_core::nav::PathLevel;
    use panel_core::RouterState;
    use panel_server::{PanelBackend, PanelSide};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Answers paths from its own root only, the way the archive plugin does.
    #[derive(Default)]
    struct Mounted {
        calls: RefCell<Vec<String>>,
    }

    impl Mounted {
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
        fn held(path: &str) -> Option<&'static [u8]> {
            match path {
                "/note.txt" => Some(b"at the root"),
                "/sub/note.txt" => Some(b"one level in"),
                _ => None,
            }
        }
    }

    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Mounted {
        async fn list_dir(&self, path: String) -> Result<Vec<RemoteFileEntry>, common::AppError> {
            self.calls.borrow_mut().push(format!("list {path}"));
            if path != "/" && path != "/sub" {
                return Err(common::AppError::Other(format!(
                    "File not found in archive: {path}"
                )));
            }
            Ok(vec![RemoteFileEntry {
                name: "note.txt".to_string(),
                is_dir: false,
                size: 11,
                modified: 0,
                permissions: None,
                extra: Vec::new(),
            }])
        }
        async fn read_file(
            &self,
            path: String,
            _progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<Vec<u8>, common::AppError> {
            self.calls.borrow_mut().push(format!("read {path}"));
            Self::held(&path).map(|b| b.to_vec()).ok_or_else(|| {
                common::AppError::Other(format!("File not found in archive: {path}"))
            })
        }
        async fn write_file(
            &self,
            path: String,
            content: Vec<u8>,
            _permissions: Option<u32>,
            _progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<(), common::AppError> {
            self.calls
                .borrow_mut()
                .push(format!("write {path} +{}", content.len()));
            Ok(())
        }
        async fn delete_entries(&self, paths: Vec<String>) -> Result<(), common::AppError> {
            self.calls
                .borrow_mut()
                .push(format!("delete {}", paths.join(" ")));
            Ok(())
        }
        async fn rename_entry(
            &self,
            path: String,
            new_path: String,
        ) -> Result<(), common::AppError> {
            self.calls
                .borrow_mut()
                .push(format!("rename {path} -> {new_path}"));
            Ok(())
        }
        async fn create_directory(
            &self,
            parent_path: String,
            dir_name: String,
            _permissions: Option<u32>,
        ) -> Result<(), common::AppError> {
            self.calls
                .borrow_mut()
                .push(format!("mkdir {parent_path} | {dir_name}"));
            Ok(())
        }
        fn is_local(&self) -> bool {
            false
        }
        fn fs_id(&self) -> String {
            "archive/bundle.zip".to_string()
        }
    }

    /// Answers with the very path it was handed, so a test can see what came through.
    struct Local;
    #[async_trait::async_trait(?Send)]
    impl FileSystemRpc for Local {
        async fn read_file(
            &self,
            path: String,
            _progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
        ) -> Result<Vec<u8>, common::AppError> {
            Ok(path.into_bytes())
        }
        fn is_local(&self) -> bool {
            true
        }
    }

    /// `deep` matters: at the mount root a missing prefix and a doubled one look alike.
    fn standing_in_the_archive(deep: bool) -> (ConsoleBackend, Rc<Mounted>, String) {
        let mount = Rc::new(Mounted::default());
        let inside: Rc<dyn FileSystemRpc> = mount.clone();
        let local: Rc<dyn FileSystemRpc> = Rc::new(Local);
        let make = || {
            Rc::new(RouterState::new(
                local.clone(),
                local.clone(),
                "/".to_string(),
            ))
        };
        let left = make();
        {
            let mut nav = left.path.borrow_mut();
            nav.push(PathLevel::new("work", "/work", local.clone()));
            nav.push(PathLevel::new("bundle.zip", "/", inside.clone()));
            if deep {
                nav.push(PathLevel::new("sub", "/sub", inside.clone()));
            }
        }
        let displayed = left.path.borrow().absolute_path();
        let backend = ConsoleBackend {
            left,
            right: make(),
            config: client_config::AppConfig::new("ice-commander-web-path-test"),
            views: RefCell::new(ic_view_session::Hub::default()),
            viewers: RefCell::new(std::collections::BTreeMap::new()),
        };
        (backend, mount, displayed)
    }

    #[tokio::test]
    async fn f3_reads_a_file_named_by_the_path_the_panel_shows() {
        for (deep, want) in [(false, "at the root"), (true, "one level in")] {
            let (backend, _mount, displayed) = standing_in_the_archive(deep);
            let asked = format!("{displayed}/note.txt");
            let answered = backend
                .read_file(PanelSide::Left, asked.clone())
                .await
                .unwrap_or_else(|e| panic!("{asked}: {e}"));
            assert_eq!(answered.content, want);
            assert!(!answered.is_binary);
        }
    }

    /// F3 on an image or a plugin document goes through `host_fs::stage`.
    #[tokio::test]
    async fn the_viewer_stages_a_file_named_by_the_path_the_panel_shows() {
        let (backend, _mount, displayed) = standing_in_the_archive(true);
        let provider = backend.provider(PanelSide::Left);
        let staged = fm_core::host_fs::stage(&provider, &format!("{displayed}/note.txt"))
            .await
            .expect("the viewer stages what the browser asked for");
        assert_eq!(staged.name, "note.txt");
        assert_eq!(
            std::fs::read(staged.root.join(&staged.name)).expect("the staged copy"),
            b"one level in"
        );
    }

    #[tokio::test]
    async fn streaming_a_file_takes_the_shown_path_too() {
        let (backend, _mount, displayed) = standing_in_the_archive(true);
        let bytes = backend
            .stream_file(PanelSide::Left, format!("{displayed}/note.txt"))
            .await
            .expect("the stream route");
        assert_eq!(bytes, b"one level in");
    }

    #[tokio::test]
    async fn editing_deleting_and_renaming_reach_the_mount_as_mount_relative_paths() {
        let (backend, mount, displayed) = standing_in_the_archive(true);
        backend
            .write_file(
                PanelSide::Left,
                format!("{displayed}/note.txt"),
                "edited".to_string(),
            )
            .await
            .expect("the editor saves");
        backend
            .delete(PanelSide::Left, vec![format!("{displayed}/note.txt")])
            .await
            .expect("the delete route");
        backend
            .rename(
                PanelSide::Left,
                format!("{displayed}/note.txt"),
                "renamed.txt".to_string(),
            )
            .await
            .expect("the rename route");
        let seen: Vec<String> = mount
            .calls()
            .into_iter()
            .filter(|c| !c.starts_with("list "))
            .collect();
        assert_eq!(
            seen,
            vec![
                "write /sub/note.txt +6".to_string(),
                "delete /sub/note.txt".to_string(),
                "rename /sub/note.txt -> /sub/renamed.txt".to_string(),
            ]
        );
    }

    /// The browser builds both targets from `ApiPanelState.path`, so it must be a display path too.
    #[tokio::test]
    async fn uploading_and_mkdir_land_where_the_panel_stands() {
        let (backend, mount, displayed) = standing_in_the_archive(true);
        let shown = read_state(backend.core(PanelSide::Left)).path;
        assert_eq!(shown, displayed);

        backend
            .upload_file(
                PanelSide::Left,
                format!("{shown}/dropped.bin"),
                vec![1, 2, 3],
            )
            .await
            .expect("the upload route");
        backend
            .mkdir(PanelSide::Left, "made".to_string())
            .await
            .expect("the mkdir route");
        let seen: Vec<String> = mount
            .calls()
            .into_iter()
            .filter(|c| !c.starts_with("list "))
            .collect();
        assert_eq!(
            seen,
            vec![
                "write /sub/dropped.bin +3".to_string(),
                "mkdir /sub | made".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn copying_out_of_the_archive_reads_the_source_and_reports_a_failure() {
        let (backend, mount, displayed) = standing_in_the_archive(true);
        backend
            .copy(
                PanelSide::Left,
                PanelSide::Right,
                vec![format!("{displayed}/note.txt")],
            )
            .await
            .expect_err("the local right-hand panel refuses the write, and says so");
        assert!(
            mount.calls().contains(&"read /sub/note.txt".to_string()),
            "the source inside the mount was read: {:?}",
            mount.calls()
        );
    }

    #[tokio::test]
    async fn a_panel_outside_a_mount_is_left_exactly_as_it_was() {
        let local: Rc<dyn FileSystemRpc> = Rc::new(Local);
        let core = Rc::new(RouterState::new(
            local.clone(),
            local.clone(),
            "/".to_string(),
        ));
        {
            let mut nav = core.path.borrow_mut();
            nav.push(PathLevel::new("home", "/home", local.clone()));
            nav.push(PathLevel::new("me", "/home/me", local.clone()));
        }
        assert_eq!(read_state(&core).path, "/home/me");

        let backend = ConsoleBackend {
            left: core,
            right: Rc::new(RouterState::new(local.clone(), local, "/".to_string())),
            config: client_config::AppConfig::new("ice-commander-web-path-test"),
            views: RefCell::new(ic_view_session::Hub::default()),
            viewers: RefCell::new(std::collections::BTreeMap::new()),
        };
        for asked in ["/home/me/file.txt", "/home/me/sub/deeper.txt", "/home/me"] {
            let answered = backend
                .read_file(PanelSide::Left, asked.to_string())
                .await
                .expect("a local read");
            assert_eq!(
                answered.content, asked,
                "a local path must reach the disk untouched"
            );
        }
    }
}
