use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::App;
use crate::overlay::Overlay;
use crate::util::centered_rect;
use crate::view::{Purpose, ViewPane};

fn load_connections(config: &client_config::AppConfig) -> Vec<connection_store::Connection> {
    let stored: Vec<connection_form::Connection> = connection_form::stored_connections(&config);
    stored
        .iter()
        .filter_map(|conn| {
            let kind = conn.kind.clone();
            let document = ic_plugin_host::connection_document(&kind)?;
            let mut settings = connection_form::plugin_mount_settings(&document, conn);
            for bind in connection_form::secret_binds(&kind) {
                let Some(sealed) = settings.get(&bind).cloned() else {
                    continue;
                };
                if let Some(plain) = secret_store::decrypt_secret(&sealed) {
                    settings.insert(bind, plain);
                }
            }
            Some(connection_store::Connection {
                id: format!("{kind}:{}", conn.name),
                name: conn.name.clone(),
                folder: conn.folder.clone(),
                kind,
                settings,
            })
        })
        .collect()
}

pub(crate) enum SourceKind {
    Local(String),
    Net(connection_store::Connection),
    /// Offered by a plugin rather than saved by the user, and mounted through
    /// the connection kind that published it.
    Offered {
        kind: String,
        settings: std::collections::BTreeMap<String, String>,
    },
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
        for d in localfs::utils::get_drives() {
            v.push(Source {
                label: d.name,
                subtitle: d.path.clone(),
                online: d.is_mounted,
                kind: SourceKind::Local(d.path),
            });
        }
        for c in load_connections(&self.config) {
            let subtitle = format!(
                "{} {}@{}",
                c.kind.to_lowercase(),
                c.get("user").unwrap_or_default(),
                c.get("url").or_else(|| c.get("host")).unwrap_or_default()
            );
            v.push(Source {
                label: c.name.clone(),
                subtitle,
                online: true,
                kind: SourceKind::Net(c),
            });
        }
        for drive in ic_plugin_host::plugin_drives() {
            v.push(Source {
                label: drive.name,
                subtitle: drive.subtitle,
                online: drive.online,
                kind: SourceKind::Offered {
                    kind: drive.kind,
                    settings: drive.settings,
                },
            });
        }
        v
    }

    /// Redraws the list a plugin has just changed, but only if it is on screen:
    /// a peer appearing must not pull the user out of whatever they are doing.
    pub(crate) fn refresh_sources(&mut self) {
        let Overlay::Sources { cursor, .. } = &self.overlay else {
            return;
        };
        let at = *cursor;
        let items = self.build_sources();
        let cursor = at.min(items.len().saturating_sub(1));
        self.overlay = Overlay::Sources { items, cursor };
    }

    pub(crate) fn open_sources(&mut self) {
        let items = self.build_sources();
        self.overlay = Overlay::Sources { items, cursor: 0 };
    }

    fn stored(&self) -> Vec<connection_form::Connection> {
        connection_form::stored_connections(&self.config)
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        self.stored().iter().position(|held| held.name == name)
    }

    /// Opens the editor over an existing record, or over a blank one of that kind.
    fn edit(&mut self, kind: &str, editing: Option<usize>) {
        let stored = self.stored();
        let record = match editing.and_then(|at| stored.get(at).cloned()) {
            Some(held) => held,
            None => connection_form::Connection::new(kind),
        };
        let mut opened = record.clone();
        connection_form::unseal(&mut opened);
        let held = connection_form::stored_secrets(&opened, kind);
        match ViewPane::for_connection(kind, &opened, editing, &held) {
            Some(pane) => self.overlay = Overlay::View(Box::new(pane)),
            None => self.set_message("Connections", format!("no plugin serves {kind}")),
        }
    }

    pub(crate) fn edit_selected_connection(&mut self) {
        let Overlay::Sources { items, cursor } = &self.overlay else {
            return;
        };
        let Some(SourceKind::Net(conn)) = items.get(*cursor).map(|source| &source.kind) else {
            return;
        };
        let (kind, name) = (conn.kind.clone(), conn.name.clone());
        let at = self.index_of(&name);
        if at.is_none() {
            return self.set_message("Connections", format!("{name} is no longer stored"));
        }
        self.edit(&kind, at);
    }

    pub(crate) fn offer_new_connection(&mut self) {
        let items: Vec<(String, String)> = connection_form::kind_table()
            .into_iter()
            .map(|entry| (entry.protocol.to_lowercase(), entry.label))
            .collect();
        if items.is_empty() {
            return self.set_message("Connections", "No plugin offers a connection kind");
        }
        self.overlay = Overlay::Kinds { items, cursor: 0 };
    }

    pub(crate) fn start_new_connection(&mut self, kind: &str) {
        self.edit(kind, None);
    }

    pub(crate) fn delete_selected_connection(&mut self) {
        let Overlay::Sources { items, cursor } = &self.overlay else {
            return;
        };
        let Some(SourceKind::Net(conn)) = items.get(*cursor).map(|source| &source.kind) else {
            return;
        };
        let name = conn.name.clone();
        let mut stored = self.stored();
        let before = stored.len();
        stored.retain(|held| held.name != name);
        if stored.len() == before {
            return;
        }
        connection_form::save_connections(&self.config, stored);
        self.config.save();
        self.open_sources();
    }

    /// Saves whatever the open editor has committed, and optionally connects to it.
    pub(crate) async fn save_open_editor(&mut self, connect: bool) {
        let Overlay::View(pane) = &self.overlay else {
            return;
        };
        let Purpose::Connection { kind, editing } = pane.purpose.clone() else {
            return;
        };
        // The document the form was filled in against; `describe` may have moved on since.
        let document = pane.session.document.clone();
        let form = match pane.committed() {
            Ok(form) => form,
            Err(missing) => {
                let body = if missing.is_empty() {
                    "the form is not complete".to_string()
                } else {
                    format!("still needed: {}", missing.join(", "))
                };
                return self.set_message("Connections", body);
            }
        };
        let at = match connection_form::store_connection(&self.config, &form, &kind, editing) {
            Ok(at) => at,
            Err(why) => return self.set_message("Connections", why),
        };
        self.overlay = Overlay::None;
        if !connect {
            return self.open_sources();
        }
        let Some(record) = self.stored().get(at).cloned() else {
            return;
        };
        let mut opened = record;
        connection_form::unseal(&mut opened);
        let settings = connection_form::plugin_mount_settings(&document, &opened);
        self.connect_net(connection_store::Connection {
            id: format!("{kind}:{}", opened.name),
            name: opened.name.clone(),
            folder: opened.folder.clone(),
            kind,
            settings,
        })
        .await;
    }

    pub(crate) fn revert_open_editor(&mut self) {
        let Overlay::View(pane) = &self.overlay else {
            return;
        };
        let Purpose::Connection { kind, editing } = pane.purpose.clone() else {
            return;
        };
        self.edit(&kind, editing);
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
            SourceKind::Offered { kind, settings } => {
                let core = self.panes[self.active].core.clone();
                let Some(provider) = ic_plugin_host::mount_connection(&kind, &settings) else {
                    return self.set_message("No plugin serves this drive", kind);
                };
                core.set_active_provider(provider, String::new());
                core.showing_selector.set(false);
                let _ = core.list_active().await;
                self.active_pane().table.select(Some(0));
            }
        }
    }

    pub(crate) async fn connect_net(&mut self, conn: connection_store::Connection) {
        let core = self.panes[self.active].core.clone();
        let Some(provider) = ic_plugin_host::mount_connection(&conn.kind, &conn.settings) else {
            return self.set_message("No plugin serves this connection", conn.kind.clone());
        };
        core.set_active_provider(provider.clone(), String::new());
        core.showing_selector.set(false);
        if let Some(rp) =
            connection_form::opening_path_in(&conn.kind, &conn.settings).filter(|p| p != "/")
        {
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
        .border_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    let list_items: Vec<ListItem> = items
        .iter()
        .map(|s| {
            let (icon, icon_color) = match &s.kind {
                SourceKind::Local(_) => ("▪", Color::Cyan),
                SourceKind::Net(_) => ("☁", Color::Yellow),
                SourceKind::Offered { .. } => ("⇄", Color::Green),
            };
            let label_style = if s.online {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
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
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, inner, &mut state);
}

#[cfg(test)]
mod tests {
    use super::load_connections;
    use connection_form::store_connection as store_record;
    use std::collections::BTreeMap;

    fn form(kind: &str, typed: &[(&str, &str)]) -> connection_form::PluginFormValues {
        let document = ic_plugin_host::connection_document(kind).expect("a declared kind");
        let values: BTreeMap<String, serde_json::Value> = typed
            .iter()
            .map(|(bind, value)| ((*bind).to_string(), serde_json::json!(value)))
            .collect();
        let touched = typed.iter().map(|(bind, _)| (*bind).to_string()).collect();
        connection_form::collect_plugin_form(
            &document,
            values,
            touched,
            &std::collections::BTreeSet::new(),
        )
        .expect("the form is complete")
    }

    fn editor_config(name: &str) -> client_config::AppConfig {
        let config = client_config::AppConfig::new(name);
        connection_form::save_connections(&config, Vec::new());
        config
    }

    #[test]
    fn a_saved_record_holds_only_what_the_document_declared() {
        declare("ftp");
        let config = editor_config("ice-commander-console-editor-new");
        let at = store_record(
            &config,
            &form(
                "ftp",
                &[
                    ("name", "work"),
                    ("host", "ftp.example.org"),
                    ("user", "ivan"),
                    ("pass", "hunter2"),
                ],
            ),
            "ftp",
            None,
        )
        .expect("saves");
        assert_eq!(at, 0);

        let stored: Vec<connection_form::Connection> = connection_form::stored_connections(&config);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].name, "work");
        assert_eq!(stored[0].kind, "ftp");
        assert_eq!(stored[0].value("host").as_deref(), Some("ftp.example.org"));
        assert_eq!(stored[0].value("user").as_deref(), Some("ivan"));
        assert_ne!(
            stored[0].value("pass").as_deref(),
            Some("hunter2"),
            "a password must not reach the config in plain text"
        );

        let listed = load_connections(&config);
        assert_eq!(listed.len(), 1, "the editor writes what the list can read");
        assert_eq!(listed[0].get("host"), Some("ftp.example.org"));
        assert_eq!(
            listed[0].get("pass"),
            Some("hunter2"),
            "and mounting gets the password back"
        );
    }

    #[test]
    fn editing_without_retyping_the_password_keeps_the_stored_one() {
        declare("ftp");
        let config = editor_config("ice-commander-console-editor-keep");
        store_record(
            &config,
            &form(
                "ftp",
                &[
                    ("name", "work"),
                    ("host", "ftp.example.org"),
                    ("user", "ivan"),
                    ("pass", "hunter2"),
                ],
            ),
            "ftp",
            None,
        )
        .expect("saves");

        store_record(
            &config,
            &form(
                "ftp",
                &[
                    ("name", "work"),
                    ("host", "ftp2.example.org"),
                    ("user", "ivan"),
                ],
            ),
            "ftp",
            Some(0),
        )
        .expect("saves again");

        let listed = load_connections(&config);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].get("host"), Some("ftp2.example.org"));
        assert_eq!(
            listed[0].get("pass"),
            Some("hunter2"),
            "an untouched password field must not wipe the stored password"
        );
    }

    #[test]
    fn two_records_may_not_share_a_name() {
        declare("ftp");
        let config = editor_config("ice-commander-console-editor-clash");
        let mine = form("ftp", &[("name", "work"), ("host", "h"), ("user", "u")]);
        store_record(&config, &mine, "ftp", None).expect("saves");
        assert!(
            store_record(&config, &mine, "ftp", None).is_err(),
            "a second record under the same name would shadow the first"
        );
        assert!(
            store_record(&config, &mine, "ftp", Some(0)).is_ok(),
            "saving the record being edited over itself is fine"
        );
    }

    fn declare(kind: &str) {
        let document = serde_json::json!({
            "schema": 1,
            "kind": kind,
            "fields": [
                { "bind": "name", "type": "text", "scope": "record" },
                { "bind": "host", "type": "text" },
                { "bind": "user", "type": "text" },
                { "bind": "pass", "type": "text", "secret": true },
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

    #[test]
    fn the_list_reads_a_record_an_older_version_wrote() {
        declare("ftp");
        let config = client_config::AppConfig::new("ice-commander-console-sources-test");
        config.set(
            connection_form::LEGACY_CONNECTIONS_KEY,
            serde_json::json!([{
                "name": "work",
                "protocol": "FTP",
                "host": "ftp.example.org",
                "port": 21,
                "user": "ivan",
                "pass": "plain"
            }]),
        );

        let listed = load_connections(&config);
        assert_eq!(
            listed.len(),
            1,
            "the record the dialog wrote must be visible"
        );
        assert_eq!(listed[0].name, "work");
        assert_eq!(listed[0].kind, "ftp");
        assert_eq!(listed[0].get("host"), Some("ftp.example.org"));
        assert_eq!(listed[0].get("user"), Some("ivan"));
    }

    #[test]
    fn a_record_whose_plugin_is_missing_is_not_offered() {
        let config = client_config::AppConfig::new("ice-commander-console-missing-test");
        let mut gone = connection_form::Connection::new("nosuchkind");
        gone.name = "gone".to_string();
        gone.put("host", "h".to_string());
        connection_form::save_connections(&config, vec![gone]);
        assert!(
            load_connections(&config).is_empty(),
            "without a plugin there is nothing to connect with"
        );
    }
}
