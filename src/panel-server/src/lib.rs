use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

mod backend;
pub use backend::{dispatch_core, PanelBackend};

pub enum ApiCmd {
    OpenNative {
        side: PanelSide,
        path: String,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    ListWindows {
        reply: oneshot::Sender<ApiResult<Vec<String>>>,
    },
    CloseExtraWindows {
        reply: oneshot::Sender<ApiResult<usize>>,
    },
    Breadcrumb {
        side: PanelSide,
        level: usize,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    Enter {
        side: PanelSide,
        name: String,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    GoUp {
        side: PanelSide,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    GoBack {
        side: PanelSide,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    GoForward {
        side: PanelSide,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    AddTab {
        side: PanelSide,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    CloseTab {
        side: PanelSide,
        id: u32,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    SwitchTab {
        side: PanelSide,
        id: u32,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    GetDrives {
        reply: oneshot::Sender<ApiResult<Vec<ApiDrive>>>,
    },
    ActivateSource {
        side: PanelSide,
        key: String,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    Delete {
        side: PanelSide,
        paths: Vec<String>,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    Mkdir {
        side: PanelSide,
        name: String,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    Rename {
        side: PanelSide,
        old_path: String,
        new_name: String,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    Copy {
        src_side: PanelSide,
        dst_side: PanelSide,
        paths: Vec<String>,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    Move {
        src_side: PanelSide,
        dst_side: PanelSide,
        paths: Vec<String>,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    GetPanelState {
        side: PanelSide,
        reply: oneshot::Sender<ApiResult<ApiPanelState>>,
    },
    GetConnections {
        reply: oneshot::Sender<ApiResult<Vec<ApiConnection>>>,
    },
    SaveConnection {
        connection: ApiConnection,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    DeleteConnection {
        name: String,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    ConnectTo {
        side: PanelSide,
        connection: ApiConnection,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    ExportConnections {
        password: Option<String>,
        reply: oneshot::Sender<ApiResult<String>>,
    },
    RefreshPanel {
        side: PanelSide,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    GetSettings {
        reply: oneshot::Sender<ApiResult<serde_json::Value>>,
    },
    SetSettings {
        values: serde_json::Value,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    GetViewerContent {
        reply: oneshot::Sender<ApiResult<Option<(String, String, String)>>>,
    },
    SetConnectionsDialog {
        open: bool,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    ImportConnections {
        data: String,
        password: Option<String>,
        reply: oneshot::Sender<ApiResult<usize>>,
    },
    GoHome {
        side: PanelSide,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    ToggleFavorite {
        path: String,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    ReadFile {
        side: PanelSide,
        path: String,
        reply: oneshot::Sender<ApiResult<ApiFileContent>>,
    },
    StreamFile {
        side: PanelSide,
        path: String,
        reply: oneshot::Sender<ApiResult<Vec<u8>>>,
    },
    WriteFile {
        side: PanelSide,
        path: String,
        content: String,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    UploadFile {
        side: PanelSide,
        path: String,
        data: Vec<u8>,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    GetFavoritesOnly {
        reply: oneshot::Sender<ApiResult<bool>>,
    },
    SetFavoritesOnly {
        value: bool,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    OpenTerminal {
        side: PanelSide,
    },
    TerminalInput {
        side: PanelSide,
        data: Vec<u8>,
    },
    TerminalResize {
        side: PanelSide,
        rows: u16,
        cols: u16,
    },
    SetTerminalExpanded {
        side: PanelSide,
        expanded: bool,
    },
    SetViewMode {
        side: PanelSide,
        mode: String,
        reply: oneshot::Sender<ApiResult<()>>,
    },
    SetSort {
        side: PanelSide,
        column: String,
        descending: bool,
        reply: oneshot::Sender<ApiResult<()>>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiLevel {
    pub name: String,
    pub is_archive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiTab {
    pub id: u32,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiPanelState {
    pub levels: Vec<ApiLevel>,
    pub path: String,
    pub entries: Vec<ApiFileEntry>,
    pub showing_selector: bool,
    pub view_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    #[serde(default)]
    pub tabs: Vec<ApiTab>,
    #[serde(default)]
    pub active_tab: u32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiDrive {
    pub name: String,
    pub key: String,
    pub path: String,
    pub icon: String,
    pub kind: String,
    pub subtitle: String,
    pub is_favorite: bool,
    pub is_online: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PanelSide {
    Left,
    Right,
}

impl std::str::FromStr for PanelSide {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "left" => Ok(PanelSide::Left),
            "right" => Ok(PanelSide::Right),
            other => Err(format!("unknown side: {other}")),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiFileContent {
    pub path: String,
    pub content: String,
    pub is_binary: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiConnection {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub pass: Option<String>,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
    #[serde(default)]
    pub remote_path: Option<String>,
    #[serde(default)]
    pub use_tunnel: Option<bool>,
    #[serde(default)]
    pub tunnel_host: Option<String>,
    #[serde(default)]
    pub tunnel_port: Option<u16>,
    #[serde(default)]
    pub tunnel_user: Option<String>,
    #[serde(default)]
    pub tunnel_auth_type: Option<String>,
    #[serde(default)]
    pub tunnel_pass: Option<String>,
    #[serde(default)]
    pub tunnel_key_path: Option<String>,
    #[serde(default)]
    pub tunnel_passphrase: Option<String>,
}

pub type ApiResult<T> = Result<T, String>;

pub const PUBLIC_SETTINGS: &[&str] = &[
    "net.connect_timeout_secs",
    "net.request_timeout_secs",
    "ui.bandwidth_limit",
    "ui.double_click_action",
    "ui.drives_toolbar_favorites_only",
    "ui.editor_type",
    "ui.enable_logging",
    "ui.external_editor_path",
    "ui.fast_save",
    "ui.fm_list_row_size",
    "ui.language",
    "ui.new_tab_focus_new",
    "ui.open_connection_target",
    "ui.show_hidden_files_shared",
    "ui.show_thumbnails",
    "ui.theme_index",
];

pub const NEEDS_PASSWORD: &str = "needs-password";

pub type WsSessions = Arc<Mutex<WsRegistry>>;

#[derive(Default)]
pub struct WsRegistry {
    next_id: u64,
    sessions: std::collections::HashMap<u64, actix_ws::Session>,
    term_open: TermOpenState,
}

#[derive(Default, Clone)]
struct TermOpenState {
    left_open: bool,
    right_open: bool,
    expanded_side: String,
    expanded: bool,
}

impl WsRegistry {
    fn insert(&mut self, session: actix_ws::Session) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.insert(id, session);
        id
    }

    fn snapshot(&self) -> Vec<(u64, actix_ws::Session)> {
        self.sessions.iter().map(|(id, s)| (*id, s.clone())).collect()
    }

    fn remove(&mut self, id: u64) {
        self.sessions.remove(&id);
    }

    fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

#[derive(Clone)]
pub struct ApiState {
    pub tx: tokio::sync::mpsc::Sender<ApiCmd>,
    pub ws_sessions: WsSessions,
    pub term_left: TermMirror,
    pub term_right: TermMirror,
    pub webui_js: Arc<Vec<u8>>,
    pub webui_css: Arc<Vec<u8>>,
}

struct TermScrollback {
    chunks: std::collections::VecDeque<(u64, Vec<u8>)>,
    next_seq: u64,
    total_bytes: usize,
}

const TERM_SCROLLBACK_CAP: usize = 256 * 1024;

#[derive(Clone)]
pub struct TermMirror {
    scrollback: Arc<Mutex<TermScrollback>>,
    tap: tokio::sync::broadcast::Sender<(u64, Vec<u8>)>,
}

impl TermMirror {
    fn start(raw: tokio::sync::broadcast::Sender<Vec<u8>>) -> TermMirror {
        let scrollback = Arc::new(Mutex::new(TermScrollback {
            chunks: std::collections::VecDeque::new(),
            next_seq: 0,
            total_bytes: 0,
        }));
        let (tap, _) = tokio::sync::broadcast::channel(256);
        let mirror = TermMirror { scrollback: scrollback.clone(), tap: tap.clone() };
        let mut raw_rx = raw.subscribe();
        tokio::spawn(async move {
            loop {
                match raw_rx.recv().await {
                    Ok(chunk) => {
                        let seq = {
                            let mut sb = scrollback.lock().unwrap();
                            let seq = sb.next_seq;
                            sb.next_seq += 1;
                            sb.total_bytes += chunk.len();
                            sb.chunks.push_back((seq, chunk.clone()));
                            while sb.total_bytes > TERM_SCROLLBACK_CAP {
                                if let Some((_, old)) = sb.chunks.pop_front() {
                                    sb.total_bytes -= old.len();
                                } else {
                                    break;
                                }
                            }
                            seq
                        };
                        let _ = tap.send((seq, chunk));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        mirror
    }

    fn attach(&self) -> (Vec<u8>, u64, tokio::sync::broadcast::Receiver<(u64, Vec<u8>)>) {
        let sb = self.scrollback.lock().unwrap();
        let rx = self.tap.subscribe();
        let mut replay = Vec::with_capacity(sb.total_bytes);
        for (_, chunk) in &sb.chunks {
            replay.extend_from_slice(chunk);
        }
        (replay, sb.next_seq, rx)
    }
}

#[derive(Clone)]
pub struct Notifier {
    sessions: WsSessions,
}

impl Notifier {
    fn has_clients(&self) -> bool {
        !self.sessions.lock().unwrap().is_empty()
    }

    pub fn panel_updated(&self, side: &str, state: ApiPanelState) {
        ws_broadcast_json(&self.sessions, serde_json::json!({
            "event": "panel_updated", "side": side, "state": state,
        }));
    }

    pub fn terminal_state(&self, side: &str, open: bool) {
        {
            let mut reg = self.sessions.lock().unwrap();
            match side {
                "left" => reg.term_open.left_open = open,
                "right" => reg.term_open.right_open = open,
                _ => {}
            }
        }
        ws_broadcast_json(&self.sessions, serde_json::json!({
            "event": "terminal_state", "side": side, "open": open,
        }));
    }

    pub fn terminal_expanded(&self, side: &str, expanded: bool) {
        {
            let mut reg = self.sessions.lock().unwrap();
            reg.term_open.expanded_side = side.to_string();
            reg.term_open.expanded = expanded;
        }
        ws_broadcast_json(&self.sessions, serde_json::json!({
            "event": "terminal_expanded", "side": side, "expanded": expanded,
        }));
    }

    pub fn view_mode(&self, side: &str, mode: &str) {
        if !self.has_clients() {
            return;
        }
        ws_broadcast_json(&self.sessions, serde_json::json!({
            "event": "view_mode", "side": side, "mode": mode,
        }));
    }

    pub fn viewer_opened(&self, side: &str, path: &str, mode: &str) {
        if !self.has_clients() {
            return;
        }
        ws_broadcast_json(&self.sessions, serde_json::json!({
            "event": "open_viewer", "side": side, "path": path, "mode": mode,
        }));
    }

    pub fn viewer_closed(&self) {
        if !self.has_clients() {
            return;
        }
        ws_broadcast_json(&self.sessions, serde_json::json!({ "event": "close_viewer" }));
    }

    pub fn connections_dialog(&self, open: bool) {
        if !self.has_clients() {
            return;
        }
        ws_broadcast_json(&self.sessions, serde_json::json!({
            "event": if open { "open_connections" } else { "close_connections" },
        }));
    }
}

static NOTIFIER: std::sync::OnceLock<Notifier> = std::sync::OnceLock::new();

pub fn init_notifier(sessions: WsSessions) {
    let _ = NOTIFIER.set(Notifier { sessions });
}

fn with_notifier(f: impl FnOnce(&Notifier)) {
    if let Some(n) = NOTIFIER.get() {
        f(n);
    }
}

pub fn notifier_active() -> bool {
    NOTIFIER.get().is_some_and(|n| n.has_clients())
}

pub fn notify_panel_state(side: &str, state: ApiPanelState) {
    with_notifier(|n| n.panel_updated(side, state));
}
pub fn notify_terminal_opened(side: &str) {
    with_notifier(|n| n.terminal_state(side, true));
}
pub fn notify_terminal_closed(side: &str) {
    with_notifier(|n| n.terminal_state(side, false));
}
pub fn notify_terminal_expanded(side: &str, expanded: bool) {
    with_notifier(|n| n.terminal_expanded(side, expanded));
}
pub fn notify_view_mode(side: &str, mode: &str) {
    with_notifier(|n| n.view_mode(side, mode));
}
pub fn notify_viewer_opened(side: &str, path: &str, mode: &str) {
    with_notifier(|n| n.viewer_opened(side, path, mode));
}
pub fn notify_viewer_closed() {
    with_notifier(|n| n.viewer_closed());
}
pub fn notify_connections_dialog(open: bool) {
    with_notifier(|n| n.connections_dialog(open));
}

fn ws_broadcast_json(sessions: &WsSessions, value: serde_json::Value) {
    let msg = value.to_string();
    let sessions_clone = sessions.clone();
    tokio::spawn(async move {
        let snapshot = sessions_clone.lock().unwrap().snapshot();
        let mut dead = Vec::new();
        for (id, mut s) in snapshot {
            if s.text(msg.clone()).await.is_err() {
                dead.push(id);
            }
        }
        if !dead.is_empty() {
            let mut reg = sessions_clone.lock().unwrap();
            for id in dead {
                reg.remove(id);
            }
        }
    });
}

#[derive(Serialize, Clone)]
pub struct ApiOperation {
    pub id: u64,
    pub kind: String,
    pub current_file: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub done_files: usize,
    pub total_files: usize,
    pub speed_bps: u64,
}

struct OpEntry {
    op: ApiOperation,
    started: std::time::Instant,
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Default)]
struct OpsState {
    next_id: u64,
    ops: std::collections::HashMap<u64, OpEntry>,
}

fn ops_state() -> &'static Mutex<OpsState> {
    static OPS: std::sync::OnceLock<Mutex<OpsState>> = std::sync::OnceLock::new();
    OPS.get_or_init(|| Mutex::new(OpsState::default()))
}

pub struct OpGuard {
    id: u64,
}

pub fn ops_begin(
    kind: &str,
    total_bytes: u64,
    total_files: usize,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> OpGuard {
    let mut st = ops_state().lock().unwrap();
    let id = st.next_id;
    st.next_id += 1;
    st.ops.insert(id, OpEntry {
        op: ApiOperation {
            id,
            kind: kind.to_string(),
            current_file: String::new(),
            done_bytes: 0,
            total_bytes,
            done_files: 0,
            total_files,
            speed_bps: 0,
        },
        started: std::time::Instant::now(),
        cancel,
    });
    OpGuard { id }
}

impl OpGuard {
    pub fn update(&self, current_file: &str, done_bytes: u64, done_files: usize) {
        let mut st = ops_state().lock().unwrap();
        if let Some(e) = st.ops.get_mut(&self.id) {
            e.op.current_file = current_file.to_string();
            e.op.done_bytes = done_bytes;
            e.op.done_files = done_files;
            let secs = e.started.elapsed().as_secs_f64();
            e.op.speed_bps = if secs > 0.2 { (done_bytes as f64 / secs) as u64 } else { 0 };
        }
    }
}

impl Drop for OpGuard {
    fn drop(&mut self) {
        ops_state().lock().unwrap().ops.remove(&self.id);
    }
}

async fn get_operations() -> impl Responder {
    let st = ops_state().lock().unwrap();
    let mut ops: Vec<ApiOperation> = st.ops.values().map(|e| e.op.clone()).collect();
    ops.sort_by_key(|o| o.id);
    HttpResponse::Ok().json(ops)
}

async fn cancel_operation(id: web::Path<u64>) -> impl Responder {
    let st = ops_state().lock().unwrap();
    match st.ops.get(&id) {
        Some(e) => {
            e.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
        }
        None => HttpResponse::NotFound()
            .json(serde_json::json!({ "error": format!("no operation {id}") })),
    }
}

async fn get_panel_state(
    side: web::Path<PanelSide>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::GetPanelState { side: *side, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(panel_state)) => HttpResponse::Ok().json(panel_state),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct EnterBody {
    name: String,
}

async fn enter(
    side: web::Path<PanelSide>,
    body: web::Json<EnterBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::Enter { side: *side, name: body.name.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct BreadcrumbBody {
    level: usize,
}

async fn breadcrumb(
    side: web::Path<PanelSide>,
    body: web::Json<BreadcrumbBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::Breadcrumb { side: *side, level: body.level, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn go_up(
    side: web::Path<PanelSide>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::GoUp { side: *side, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn add_tab(
    side: web::Path<PanelSide>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::AddTab { side: *side, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn close_tab(
    path: web::Path<(PanelSide, u32)>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (side, id) = *path;
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::CloseTab { side, id, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn switch_tab(
    path: web::Path<(PanelSide, u32)>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (side, id) = *path;
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::SwitchTab { side, id, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn go_back(
    side: web::Path<PanelSide>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::GoBack { side: *side, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn go_forward(
    side: web::Path<PanelSide>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::GoForward { side: *side, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct DeleteBody {
    paths: Vec<String>,
}

async fn delete_entries(
    side: web::Path<PanelSide>,
    body: web::Json<DeleteBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::Delete { side: *side, paths: body.paths.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct MkdirBody {
    name: String,
}

async fn mkdir(
    side: web::Path<PanelSide>,
    body: web::Json<MkdirBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::Mkdir { side: *side, name: body.name.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct RenameBody {
    old_path: String,
    new_name: String,
}

async fn rename_entry(
    side: web::Path<PanelSide>,
    body: web::Json<RenameBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::Rename {
        side: *side,
        old_path: body.old_path.clone(),
        new_name: body.new_name.clone(),
        reply: reply_tx,
    };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct TransferBody {
    src_side: PanelSide,
    paths: Vec<String>,
}

async fn copy_entries(
    dst_side: web::Path<PanelSide>,
    body: web::Json<TransferBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::Copy {
        src_side: body.src_side,
        dst_side: *dst_side,
        paths: body.paths.clone(),
        reply: reply_tx,
    };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn move_entries(
    dst_side: web::Path<PanelSide>,
    body: web::Json<TransferBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::Move {
        src_side: body.src_side,
        dst_side: *dst_side,
        paths: body.paths.clone(),
        reply: reply_tx,
    };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn go_home(
    side: web::Path<PanelSide>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::GoHome { side: *side, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct FileQuery {
    path: String,
}

async fn read_file(
    side: web::Path<PanelSide>,
    query: web::Query<FileQuery>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::ReadFile { side: *side, path: query.path.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(content)) => HttpResponse::Ok().json(content),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

fn media_content_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn parse_byte_range(header: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let spec = header.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    let (start, end) = if a.is_empty() {
        let suffix: u64 = b.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        (total.saturating_sub(suffix), total - 1)
    } else {
        let start: u64 = a.parse().ok()?;
        let end = if b.is_empty() { total - 1 } else { b.parse::<u64>().ok()?.min(total - 1) };
        (start, end)
    };
    if start > end || start >= total {
        return None;
    }
    Some((start, end))
}

async fn stream_file(
    req: HttpRequest,
    side: web::Path<PanelSide>,
    query: web::Query<FileQuery>,
    state: web::Data<ApiState>,
) -> impl Responder {
    use actix_web::http::header::{ACCEPT_RANGES, CONTENT_RANGE, RANGE};

    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::StreamFile { side: *side, path: query.path.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    let bytes = match reply_rx.await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => return HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => return HttpResponse::ServiceUnavailable().finish(),
    };

    let ctype = media_content_type(&query.path);
    let total = bytes.len() as u64;
    let range_hdr = req.headers().get(RANGE).and_then(|v| v.to_str().ok());
    let has_range = range_hdr.is_some();

    match range_hdr.and_then(|h| parse_byte_range(h, total)) {
        Some((start, end)) => HttpResponse::PartialContent()
            .content_type(ctype)
            .insert_header((ACCEPT_RANGES, "bytes"))
            .insert_header((CONTENT_RANGE, format!("bytes {start}-{end}/{total}")))
            .body(bytes[start as usize..=end as usize].to_vec()),
        None if has_range => HttpResponse::RangeNotSatisfiable()
            .insert_header((CONTENT_RANGE, format!("bytes */{total}")))
            .finish(),
        None => HttpResponse::Ok()
            .content_type(ctype)
            .insert_header((ACCEPT_RANGES, "bytes"))
            .body(bytes),
    }
}

#[derive(Deserialize)]
struct WriteFileBody {
    path: String,
    content: String,
}

async fn write_file(
    side: web::Path<PanelSide>,
    body: web::Json<WriteFileBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::WriteFile { side: *side, path: body.path.clone(), content: body.content.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct UploadQuery {
    path: String,
}

async fn upload_file(
    side: web::Path<PanelSide>,
    query: web::Query<UploadQuery>,
    body: web::Bytes,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::UploadFile {
        side: *side,
        path: query.path.clone(),
        data: body.to_vec(),
        reply: reply_tx,
    };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct ToggleFavoriteBody {
    path: String,
}

async fn toggle_favorite(body: web::Json<ToggleFavoriteBody>, state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::ToggleFavorite { path: body.path.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn get_favorites_only(state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state.tx.send(ApiCmd::GetFavoritesOnly { reply: reply_tx }).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(v)) => HttpResponse::Ok().json(serde_json::json!({ "value": v })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct FavoritesOnlyBody {
    value: bool,
}

async fn set_favorites_only(body: web::Json<FavoritesOnlyBody>, state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state.tx.send(ApiCmd::SetFavoritesOnly { value: body.value, reply: reply_tx }).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn get_connections(state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::GetConnections { reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(conns)) => HttpResponse::Ok().json(conns),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn save_connection(body: web::Json<ApiConnection>, state: web::Data<ApiState>) -> impl Responder {
    let conn = body.into_inner();
    if conn.name.trim().is_empty() || conn.host.trim().is_empty() {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": "name and host are required" }));
    }
    if !matches!(conn.protocol.to_uppercase().as_str(), "FTP" | "SFTP" | "WEBDAV") {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": format!("unknown protocol: {}", conn.protocol) }));
    }
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::SaveConnection { connection: conn, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn delete_connection(name: web::Path<String>, state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::DeleteConnection { name: name.into_inner(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::NotFound().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct ConnectToBody {
    side: PanelSide,
    connection: ApiConnection,
}

#[derive(serde::Deserialize)]
struct ExportBody {
    #[serde(default)]
    password: Option<String>,
}

#[derive(serde::Deserialize)]
struct ImportBody {
    data: String,
    #[serde(default)]
    password: Option<String>,
}

#[derive(serde::Deserialize)]
struct DialogBody {
    open: bool,
}

async fn refresh_panel(
    side: web::Path<PanelSide>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::RefreshPanel { side: *side, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn get_settings(state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state.tx.send(ApiCmd::GetSettings { reply: reply_tx }).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(v)) => HttpResponse::Ok().json(v),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn set_settings(
    body: web::Json<serde_json::Value>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::SetSettings { values: body.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn viewer_content(state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state.tx.send(ApiCmd::GetViewerContent { reply: reply_tx }).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(Some((path, mode, text)))) => HttpResponse::Ok().json(serde_json::json!({
            "path": path, "mode": mode, "content": text,
        })),
        Ok(Ok(None)) => {
            HttpResponse::NotFound().json(serde_json::json!({ "error": "no viewer open" }))
        }
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn set_connections_dialog(
    body: web::Json<DialogBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::SetConnectionsDialog { open: body.open, reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn export_connections(
    body: web::Json<ExportBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::ExportConnections { password: body.password.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(data)) => HttpResponse::Ok().json(serde_json::json!({ "data": data })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn import_connections(
    body: web::Json<ImportBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::ImportConnections {
        data: body.data.clone(),
        password: body.password.clone(),
        reply: reply_tx,
    };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(n)) => HttpResponse::Ok().json(serde_json::json!({ "imported": n })),
        Ok(Err(e)) => HttpResponse::BadRequest().json(serde_json::json!({
            "error": e,
            "needs_password": e == NEEDS_PASSWORD,
        })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn connect_to(body: web::Json<ConnectToBody>, state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::ConnectTo {
        side: body.side,
        connection: body.connection.clone(),
        reply: reply_tx,
    };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn get_drives(state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::GetDrives { reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(drives)) => HttpResponse::Ok().json(drives),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct TerminalExpandBody {
    expanded: bool,
}

#[derive(Deserialize)]
struct ViewModeBody {
    mode: String,
}

async fn set_view_mode(
    side: web::Path<String>,
    body: web::Json<ViewModeBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let side = match side.parse::<PanelSide>() {
        Ok(s) => s,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    };
    if !matches!(body.mode.as_str(), "list" | "small" | "large") {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": format!("bad view mode {:?}", body.mode) }));
    }
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::SetViewMode { side, mode: body.mode.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct SortBody {
    column: String,
    #[serde(default)]
    descending: bool,
}

async fn set_sort(
    side: web::Path<String>,
    body: web::Json<SortBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let side = match side.parse::<PanelSide>() {
        Ok(s) => s,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    };
    if !matches!(body.column.as_str(), "name" | "date" | "size") {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": format!("bad sort column {:?}", body.column) }));
    }
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::SetSort {
        side,
        column: body.column.clone(),
        descending: body.descending,
        reply: reply_tx,
    };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn set_terminal_expanded(
    side: web::Path<String>,
    body: web::Json<TerminalExpandBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let side = match side.parse::<PanelSide>() {
        Ok(s) => s,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    };
    let _ = state.tx.send(ApiCmd::SetTerminalExpanded { side, expanded: body.expanded }).await;
    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

#[derive(Deserialize)]
struct OpenViewerBody {
    path: String,
    #[serde(default)]
    mode: Option<String>,
}

async fn open_viewer(
    side: web::Path<String>,
    body: web::Json<OpenViewerBody>,
) -> impl Responder {
    let side = side.into_inner();
    if side.parse::<PanelSide>().is_err() {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": "bad side" }));
    }
    let mode = match body.mode.as_deref() {
        None | Some("view") => "view",
        Some("edit") => "edit",
        Some(other) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("bad mode {other:?}") }))
        }
    };
    notify_viewer_opened(&side, &body.path, mode);
    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

#[derive(Deserialize)]
struct OpenNativeBody {
    path: String,
}

async fn open_native(
    side: web::Path<String>,
    body: web::Json<OpenNativeBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let side = match side.parse::<PanelSide>() {
        Ok(s) => s,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    };
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::OpenNative { side, path: body.path.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[derive(Deserialize)]
struct ActivateBody {
    key: String,
}

async fn activate_source(
    side: web::Path<String>,
    body: web::Json<ActivateBody>,
    state: web::Data<ApiState>,
) -> impl Responder {
    let side = match side.parse::<PanelSide>() {
        Ok(s) => s,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    };
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = ApiCmd::ActivateSource { side, key: body.key.clone(), reply: reply_tx };
    if state.tx.send(cmd).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn ws_handler(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<ApiState>,
) -> Result<HttpResponse, actix_web::Error> {
    let (res, session, msg_stream) = actix_ws::handle(&req, stream)?;
    let term = {
        let mut reg = state.ws_sessions.lock().unwrap();
        reg.insert(session.clone());
        reg.term_open.clone()
    };
    actix_web::rt::spawn(async move {
        let mut s = session;
        for msg in [
            serde_json::json!({ "event": "terminal_state", "side": "left", "open": term.left_open }),
            serde_json::json!({ "event": "terminal_state", "side": "right", "open": term.right_open }),
            serde_json::json!({ "event": "terminal_expanded", "side": term.expanded_side, "expanded": term.expanded }),
        ] {
            if s.text(msg.to_string()).await.is_err() {
                break;
            }
        }
    });
    actix_web::rt::spawn(async move {
        let mut stream = msg_stream;
        while let Some(Ok(msg)) = stream.recv().await {
            if matches!(msg, actix_ws::Message::Close(_)) { break; }
        }
    });
    Ok(res)
}

async fn terminal_ws_handler(
    side: web::Path<String>,
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<ApiState>,
) -> Result<HttpResponse, actix_web::Error> {
    let side = match side.parse::<PanelSide>() {
        Ok(s) => s,
        Err(e) => return Ok(HttpResponse::BadRequest().json(serde_json::json!({ "error": e }))),
    };
    let mirror = match side {
        PanelSide::Left => state.term_left.clone(),
        PanelSide::Right => state.term_right.clone(),
    };
    let (replay, first_live_seq, mut out_rx) = mirror.attach();

    let (res, session, mut msg_stream) = actix_ws::handle(&req, stream)?;

    let _ = state.tx.send(ApiCmd::OpenTerminal { side }).await;

    let mut out_session = session.clone();
    actix_web::rt::spawn(async move {
        if !replay.is_empty() && out_session.binary(replay).await.is_err() {
            return;
        }
        loop {
            match out_rx.recv().await {
                Ok((seq, chunk)) => {
                    if seq < first_live_seq {
                        continue;
                    }
                    if out_session.binary(chunk).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
        let _ = out_session.close(None).await;
    });

    let tx = state.tx.clone();
    actix_web::rt::spawn(async move {
        while let Some(Ok(msg)) = msg_stream.recv().await {
            match msg {
                actix_ws::Message::Binary(b) => {
                    let _ = tx.send(ApiCmd::TerminalInput { side, data: b.to_vec() }).await;
                }
                actix_ws::Message::Text(t) => {
                    let resize = serde_json::from_str::<serde_json::Value>(&t)
                        .ok()
                        .and_then(|v| v.get("resize").cloned())
                        .map(|r| {
                            let cols = r.get("cols").and_then(|c| c.as_u64()).unwrap_or(80) as u16;
                            let rows = r.get("rows").and_then(|c| c.as_u64()).unwrap_or(24) as u16;
                            (rows, cols)
                        });
                    match resize {
                        Some((rows, cols)) => {
                            let _ = tx.send(ApiCmd::TerminalResize { side, rows, cols }).await;
                        }
                        None => {
                            let _ = tx.send(ApiCmd::TerminalInput { side, data: t.into_bytes().to_vec() }).await;
                        }
                    }
                }
                actix_ws::Message::Close(_) => break,
                _ => {}
            }
        }
    });

    Ok(res)
}

async fn health() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok" }))
}

async fn webui_index(state: web::Data<ApiState>) -> impl Responder {
    let css = String::from_utf8_lossy(&state.webui_css);
    let js = String::from_utf8_lossy(&state.webui_js);
    let html = format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Ice Commander — Web UI</title>
  <style>{css}</style>
</head>
<body>
  <div id="root"></div>
  <script>{js}</script>
</body>
</html>"#);
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .insert_header(("Cache-Control", "no-store"))
        .body(html)
}

async fn webui_bundle_js(state: web::Data<ApiState>) -> impl Responder {
    HttpResponse::Ok()
        .content_type("application/javascript; charset=utf-8")
        .insert_header(("Cache-Control", "public, max-age=3600"))
        .body((*state.webui_js).clone())
}

async fn webui_style_css(state: web::Data<ApiState>) -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/css; charset=utf-8")
        .insert_header(("Cache-Control", "public, max-age=3600"))
        .body((*state.webui_css).clone())
}

pub fn start_api_server(
    port: u16,
    webui: bool,
    host: String,
    tx: tokio::sync::mpsc::Sender<ApiCmd>,
    ws_sessions: WsSessions,
    term_out_left: tokio::sync::broadcast::Sender<Vec<u8>>,
    term_out_right: tokio::sync::broadcast::Sender<Vec<u8>>,
    webui_js: Vec<u8>,
    webui_css: Vec<u8>,
) {
    let webui_js = Arc::new(webui_js);
    let webui_css = Arc::new(webui_css);
    std::thread::spawn(move || {
        actix_web::rt::System::new().block_on(async move {
            let state = web::Data::new(ApiState {
                tx,
                ws_sessions,
                term_left: TermMirror::start(term_out_left),
                term_right: TermMirror::start(term_out_right),
                webui_js,
                webui_css,
            });
            let result = HttpServer::new(move || {
                let app = App::new()
                    .app_data(state.clone())
                    .app_data(web::PayloadConfig::new(512 * 1024 * 1024))
                    .route("/api/health", web::get().to(health))
                    .route("/api/ws", web::get().to(ws_handler))
                    .route("/api/panel/{side}/terminal/ws", web::get().to(terminal_ws_handler))
                    .route("/api/panel/{side}/terminal/expand", web::post().to(set_terminal_expanded))
                    .route("/api/panel/{side}/view-mode", web::post().to(set_view_mode))
                    .route("/api/panel/{side}/sort", web::post().to(set_sort))
                    .route("/api/operations", web::get().to(get_operations))
                    .route("/api/operations/{id}/cancel", web::post().to(cancel_operation))
                    .route("/api/drives", web::get().to(get_drives))
                    .route("/api/panel/{side}/activate", web::post().to(activate_source))
                    .route("/api/connections", web::get().to(get_connections))
                    .route("/api/connections", web::post().to(save_connection))
                    .route("/api/connections/{name}", web::delete().to(delete_connection))
                    .route("/api/panel/{side}/refresh", web::post().to(refresh_panel))
                    .route("/api/settings", web::get().to(get_settings))
                    .route("/api/settings", web::post().to(set_settings))
                    .route("/api/viewer/content", web::get().to(viewer_content))
                    .route("/api/connections/dialog", web::post().to(set_connections_dialog))
                    .route("/api/connections/export", web::post().to(export_connections))
                    .route("/api/connections/import", web::post().to(import_connections))
                    .route("/api/connect", web::post().to(connect_to))
                    .route("/api/favorites/toggle", web::post().to(toggle_favorite))
                    .route("/api/favorites/only", web::get().to(get_favorites_only))
                    .route("/api/favorites/only", web::post().to(set_favorites_only))
                    .route("/api/panel/{side}/home", web::post().to(go_home))
                    .route("/api/panel/{side}/state", web::get().to(get_panel_state))
                    .route("/api/panel/{side}/breadcrumb", web::post().to(breadcrumb))
                    .route("/api/panel/{side}/enter", web::post().to(enter))
                    .route("/api/panel/{side}/up", web::post().to(go_up))
                    .route("/api/panel/{side}/back", web::post().to(go_back))
                    .route("/api/panel/{side}/forward", web::post().to(go_forward))
                    .route("/api/panel/{side}/tabs/add", web::post().to(add_tab))
                    .route("/api/panel/{side}/tabs/{id}/close", web::post().to(close_tab))
                    .route("/api/panel/{side}/tabs/{id}/activate", web::post().to(switch_tab))
                    .route("/api/panel/{side}/delete", web::post().to(delete_entries))
                    .route("/api/panel/{side}/mkdir", web::post().to(mkdir))
                    .route("/api/panel/{side}/rename", web::post().to(rename_entry))
                    .route("/api/panel/{side}/file", web::get().to(read_file))
                    .route("/api/panel/{side}/file", web::put().to(write_file))
                    .route("/api/panel/{side}/file/stream", web::get().to(stream_file))
                    .route("/api/panel/{side}/upload", web::post().to(upload_file))
                    .route("/api/panel/{side}/view", web::post().to(open_viewer))
                    .route("/api/panel/{side}/open-native", web::post().to(open_native))
                    .route("/api/windows", web::get().to(list_windows))
                    .route("/api/windows/close-extra", web::post().to(close_extra_windows))
                    .route("/api/panel/{dst_side}/copy", web::post().to(copy_entries))
                    .route("/api/panel/{dst_side}/move", web::post().to(move_entries));
                if webui {
                    app
                        .route("/", web::get().to(webui_index))
                        .route("/webui/bundle.js", web::get().to(webui_bundle_js))
                        .route("/webui/style.css", web::get().to(webui_style_css))
                } else {
                    app
                }
            })
            .bind((host.as_str(), port));

            match result {
                Ok(s) => {
                    let display_host = if host == "0.0.0.0" { "localhost" } else { &host };
                    if webui {
                        println!("[API] Web UI available at http://{display_host}:{port}/");
                    }
                    println!("[API] REST API listening on http://{display_host}:{port}/api/");
                    s.run().await.ok();
                }
                Err(e) => eprintln!("[API] Failed to bind port {port}: {e}"),
            }
        });
    });
}

async fn list_windows(state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state.tx.send(ApiCmd::ListWindows { reply: reply_tx }).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(titles)) => HttpResponse::Ok().json(serde_json::json!({ "windows": titles })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

async fn close_extra_windows(state: web::Data<ApiState>) -> impl Responder {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state.tx.send(ApiCmd::CloseExtraWindows { reply: reply_tx }).await.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    match reply_rx.await {
        Ok(Ok(n)) => HttpResponse::Ok().json(serde_json::json!({ "closed": n })),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
        Err(_) => HttpResponse::ServiceUnavailable().finish(),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_byte_range;

    #[test]
    fn range_basic() {
        assert_eq!(parse_byte_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_byte_range("bytes=0-0", 1000), Some((0, 0)));
        assert_eq!(parse_byte_range("bytes=0-999", 1000), Some((0, 999)));
    }

    #[test]
    fn range_open_ended_and_suffix() {
        assert_eq!(parse_byte_range("bytes=100-", 1000), Some((100, 999)));
        assert_eq!(parse_byte_range("bytes=-500", 1000), Some((500, 999)));
    }

    #[test]
    fn range_end_is_clamped_to_last_byte() {
        assert_eq!(parse_byte_range("bytes=0-2000", 1000), Some((0, 999)));
    }

    #[test]
    fn range_only_first_of_multiple() {
        assert_eq!(parse_byte_range("bytes=0-99,200-299", 1000), Some((0, 99)));
    }

    #[test]
    fn range_unsatisfiable_is_none() {
        assert_eq!(parse_byte_range("bytes=2000-3000", 1000), None);
        assert_eq!(parse_byte_range("bytes=500-100", 1000), None);
        assert_eq!(parse_byte_range("bytes=-0", 1000), None);
        assert_eq!(parse_byte_range("bytes=0-10", 0), None);
    }

    #[test]
    fn range_malformed_is_none() {
        assert_eq!(parse_byte_range("bytes=abc", 1000), None);
        assert_eq!(parse_byte_range("nonsense", 1000), None);
        assert_eq!(parse_byte_range("bytes=", 1000), None);
    }
}
