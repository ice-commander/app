use std::rc::Rc;
use std::sync::{Arc, Mutex};

use fm_core::rpc::FileSystemRpc;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::App;
use crate::overlay::Overlay;
use crate::util::centered_rect;

#[derive(Clone, serde::Deserialize)]
pub(crate) struct SavedConnection {
    name: String,
    protocol: String,
    host: String,
    port: u16,
    user: String,
    #[serde(default)]
    pass: Option<String>,
    #[serde(default)]
    auth_type: Option<String>,
    #[serde(default)]
    key_path: Option<String>,
    #[serde(default)]
    passphrase: Option<String>,
    #[serde(default)]
    remote_path: Option<String>,
    #[serde(default)]
    use_tunnel: Option<bool>,
    #[serde(default)]
    tunnel_host: Option<String>,
    #[serde(default)]
    tunnel_port: Option<u16>,
    #[serde(default)]
    tunnel_user: Option<String>,
    #[serde(default)]
    tunnel_auth_type: Option<String>,
    #[serde(default)]
    tunnel_pass: Option<String>,
    #[serde(default)]
    tunnel_key_path: Option<String>,
    #[serde(default)]
    tunnel_passphrase: Option<String>,
}

fn load_connections(config: &client_config::AppConfig) -> Vec<SavedConnection> {
    config.get::<Vec<SavedConnection>>("ui.ftp_connections").unwrap_or_default()
}

pub(crate) enum SourceKind {
    Local(String),
    Net(SavedConnection),
}

pub(crate) struct Source {
    pub(crate) label: String,
    pub(crate) subtitle: String,
    pub(crate) online: bool,
    pub(crate) kind: SourceKind,
}

impl App {
    fn build_sources(&self) -> Vec<Source> {
        let mut v = vec![Source {
            label: "/".into(),
            subtitle: "root".into(),
            online: true,
            kind: SourceKind::Local("/".into()),
        }];
        if let Some(home) = dirs::home_dir() {
            let h = home.to_string_lossy().into_owned();
            v.push(Source {
                label: "~".into(),
                subtitle: h.clone(),
                online: true,
                kind: SourceKind::Local(h),
            });
        }
        for d in virtualfs::utils::get_drives() {
            v.push(Source {
                label: d.name,
                subtitle: d.path.clone(),
                online: d.is_mounted,
                kind: SourceKind::Local(d.path),
            });
        }
        for c in load_connections(&self.config) {
            let subtitle = format!("{} {}@{}", c.protocol.to_lowercase(), c.user, c.host);
            v.push(Source { label: c.name.clone(), subtitle, online: true, kind: SourceKind::Net(c) });
        }
        v
    }

    pub(crate) fn open_sources(&mut self) {
        let items = self.build_sources();
        self.overlay = Overlay::Sources { items, cursor: 0 };
    }

    pub(crate) async fn activate_source(&mut self, src: Source) {
        match src.kind {
            SourceKind::Local(path) => {
                let core = self.panes[self.active].core.clone();
                crate::goto_local(&core, &path);
                core.showing_selector.set(false);
                let _ = core.list_active().await;
                self.active_pane().table.select(Some(0));
            }
            SourceKind::Net(conn) => self.connect_net(conn).await,
        }
    }

    async fn connect_net(&mut self, conn: SavedConnection) {
        let mut conn = conn;
        for f in [
            &mut conn.pass,
            &mut conn.passphrase,
            &mut conn.tunnel_pass,
            &mut conn.tunnel_passphrase,
        ] {
            if let Some(v) = f.as_deref() {
                *f = secret_store::decrypt_secret(v).or_else(|| f.clone());
            }
        }
        let core = self.panes[self.active].core.clone();
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
            other => return self.set_message("Unknown protocol", other.to_string()),
        };
        core.set_active_provider(provider.clone(), String::new());
        core.showing_selector.set(false);
        if let Some(rp) = conn.remote_path.clone().filter(|p| !p.is_empty() && p != "/") {
            let segs = panel_core::parse_path_to_segments(&rp);
            let levels = panel_core::nav::build_levels(&segs, provider.clone());
            *core.path.borrow_mut() = panel_core::nav::NavPath::from_levels(levels, provider);
        }
        if let Err(e) = core.list_active().await {
            self.set_message("Connect failed", e.to_string());
        }
        self.active_pane().table.select(Some(0));
    }
}

pub(crate) fn draw_sources(f: &mut Frame, items: &[Source], cursor: usize) {
    let area = f.area();
    let h = (items.len() as u16 + 3).clamp(6, area.height.saturating_sub(2));
    let w = 60u16.min(area.width.saturating_sub(4)).max(30);
    let rect = centered_rect(w, h, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Sources — Enter open · Esc back ")
        .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    let list_items: Vec<ListItem> = items
        .iter()
        .map(|s| {
            let (icon, icon_color) = match &s.kind {
                SourceKind::Local(_) => ("▪", Color::Cyan),
                SourceKind::Net(_) => ("☁", Color::Yellow),
            };
            let label_style = if s.online {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
                Span::styled(format!("{:<14}", s.label), label_style),
                Span::styled(s.subtitle.clone(), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(cursor.min(items.len().saturating_sub(1))));
    let list = List::new(list_items)
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    f.render_stateful_widget(list, inner, &mut state);
}
