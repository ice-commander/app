/// A saved connection. The application owns the name and where it sits in the
/// list; everything else is whatever the kind's own form asked for, under the
/// names the plugin gave its fields.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Connection {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    pub kind: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub settings: std::collections::BTreeMap<String, String>,
}

impl Connection {
    pub fn new(kind: &str) -> Self {
        Connection {
            kind: kind.to_lowercase(),
            ..Default::default()
        }
    }

    /// What this record holds for one of the document's binds. `name` and
    /// `folder` are the application's own columns; a plugin's field is read
    /// from where the plugin's field names say it is.
    pub fn value(&self, bind: &str) -> Option<String> {
        match bind {
            "name" => Some(self.name.clone()),
            "folder" => self.folder.clone(),
            other => self.settings.get(other).cloned(),
        }
    }

    /// The exact inverse of [`Connection::value`].
    pub fn put(&mut self, bind: &str, value: String) {
        match bind {
            "name" => self.name = value,
            "folder" => self.folder = Some(value),
            other => {
                self.settings.insert(other.to_string(), value);
            }
        }
    }

    pub fn text(&self, bind: &str) -> Option<String> {
        self.value(bind).filter(|held| !held.is_empty())
    }
}

/// The shape a connection was stored in before a record became the kind's own
/// business: FTP's fields as columns of their own, anything a later plugin
/// wanted in a bag beside them. Read once, so a list saved by an older version
/// opens here — see [`carried_over`].
#[derive(serde::Deserialize, Default)]
struct Legacy {
    #[serde(default)]
    name: String,
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    protocol: String,
    #[serde(default)]
    host: String,
    #[serde(default)]
    port: u16,
    #[serde(default)]
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
    #[serde(default)]
    settings: std::collections::BTreeMap<String, String>,
}

/// An old record read as a new one. Every column keeps the name it had, which
/// is the name the forms already bind to; the host is written under `url` as
/// well, because WebDAV's field is called that and asking a plugin which name
/// it prefers would mean no old record could be read before its plugin loads.
fn carried_over(old: Legacy) -> Connection {
    let mut carried = Connection {
        name: old.name,
        folder: old.folder,
        kind: old.protocol.to_lowercase(),
        settings: std::collections::BTreeMap::new(),
    };
    let mut put = |bind: &str, value: Option<String>| {
        if let Some(value) = value.filter(|held| !held.is_empty()) {
            carried.settings.insert(bind.to_string(), value);
        }
    };
    put("host", Some(old.host.clone()));
    put("url", Some(old.host));
    put("port", (old.port != 0).then(|| old.port.to_string()));
    put("user", Some(old.user));
    put("pass", old.pass);
    put("auth_type", old.auth_type);
    put("key_path", old.key_path);
    put("passphrase", old.passphrase);
    put("remote_path", old.remote_path);
    put("use_tunnel", old.use_tunnel.map(|on| on.to_string()));
    put("tunnel_host", old.tunnel_host);
    put("tunnel_port", old.tunnel_port.map(|port| port.to_string()));
    put("tunnel_user", old.tunnel_user);
    put("tunnel_auth_type", old.tunnel_auth_type);
    put("tunnel_pass", old.tunnel_pass);
    put("tunnel_key_path", old.tunnel_key_path);
    put("tunnel_passphrase", old.tunnel_passphrase);
    carried.settings.extend(old.settings);
    carried
}

pub fn secret_binds(kind: &str) -> Vec<String> {
    ic_plugin_host::connection_document(kind)
        .map(|document| plugin_secret_binds(&document))
        .unwrap_or_default()
}

fn plugin_secret_binds(document: &ic_view::Document) -> Vec<String> {
    document
        .fields
        .iter()
        .filter(|field| field.secret)
        .map(|field| field.bind.clone())
        .collect()
}

/// Rewrites every field the plugin declared secret, wherever the record keeps it.
pub fn map_secrets(conn: &mut Connection, kind: &str, mut f: impl FnMut(&str) -> Option<String>) {
    for bind in secret_binds(kind) {
        let Some(current) = conn.text(&bind) else {
            continue;
        };
        if let Some(mapped) = f(&current) {
            conn.put(&bind, mapped);
        }
    }
}

/// Which of the declared secrets this record actually holds a value for.
pub fn stored_secrets(record: &Connection, kind: &str) -> Vec<String> {
    secret_binds(kind)
        .into_iter()
        .filter(|bind| record.text(bind).is_some())
        .collect()
}

/// The markers for the record a posted form is editing, for a frontend that
/// holds no state of its own: it never sends a secret back, so the commit would
/// otherwise take an untouched password box for a cleared one.
pub fn stored_secrets_of(
    config: &client_config::AppConfig,
    editing: Option<&str>,
    kind: &str,
) -> std::collections::BTreeSet<String> {
    let Some(name) = editing else {
        return std::collections::BTreeSet::new();
    };
    stored_connections(config)
        .iter()
        .find(|held| held.name == name)
        .map(|record| stored_secrets(record, kind).into_iter().collect())
        .unwrap_or_default()
}

/// Keeps whatever secret is already stored when the incoming record leaves it blank.
pub fn carry_secrets(incoming: &mut Connection, stored: &Connection, kind: &str) {
    for bind in secret_binds(kind) {
        if incoming.text(&bind).is_some() {
            continue;
        }
        if let Some(kept) = stored.value(&bind) {
            incoming.put(&bind, kept);
        }
    }
}

/// Seeds a form from a record. A bind the record holds nothing for is left out
/// rather than seeded blank, so the document's own default still applies: a
/// connection being created seeds nothing at all.
pub fn plugin_state_from_record(document: &ic_view::Document, conn: &Connection) -> ic_view::State {
    let mut state = ic_view::State::default();
    for field in &document.fields {
        if let Some(text) = conn.text(&field.bind) {
            state.set_state(&field.bind, serde_json::Value::String(text));
        }
    }
    state
}

pub fn plugin_mount_settings(
    document: &ic_view::Document,
    conn: &Connection,
) -> std::collections::BTreeMap<String, String> {
    let mut settings = std::collections::BTreeMap::new();
    for field in &document.fields {
        if field.scope != ic_view::Scope::Settings {
            continue;
        }
        if let Some(text) = conn.text(&field.bind) {
            settings.insert(field.bind.clone(), text);
        }
    }
    settings
}

/// A filled-in form as the plugin's own document defines it: `record` is what
/// the application keeps a column for, and `values` every bind the form
/// committed, secrets included.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginFormValues {
    pub record: std::collections::BTreeMap<String, String>,
    pub keep_stored: std::collections::BTreeSet<String>,
    pub name: String,
    pub values: std::collections::BTreeMap<String, String>,
}

impl PluginFormValues {
    pub fn text(&self, key: &str) -> Option<String> {
        self.values
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
    }

    pub fn flag(&self, key: &str) -> Option<bool> {
        self.text(key).map(|value| value == "true")
    }

    pub fn number(&self, key: &str) -> Option<u16> {
        self.text(key).and_then(|value| value.parse().ok())
    }
}

/// What a filled-in form commits from. `stored` names the secrets the record
/// behind the form already holds, without which `empty_keeps_stored` cannot
/// fire and a required password would have to be typed again to save.
fn commit_state(
    values: std::collections::BTreeMap<String, serde_json::Value>,
    touched: Vec<String>,
    stored: &std::collections::BTreeSet<String>,
) -> ic_view::State {
    let mut state = ic_view::State::default();
    for (bind, value) in values {
        state.set_state(&bind, value);
    }
    for bind in touched {
        state.touched.insert(bind);
    }
    state.stored_secrets = stored.clone();
    state
}

/// What a form commits from: what the session holds, with what the widgets show
/// on top. A value the plugin set that nothing draws — a peer chosen from a
/// list — would otherwise be dropped the moment the user saves, and a field
/// declared `required` would keep the form from committing at all.
pub fn values_to_commit(
    held: &ic_view::State,
    shown: std::collections::BTreeMap<String, serde_json::Value>,
) -> std::collections::BTreeMap<String, serde_json::Value> {
    let mut all = held.state.clone();
    all.extend(shown);
    all
}

pub fn collect_plugin_form(
    document: &ic_view::Document,
    values: std::collections::BTreeMap<String, serde_json::Value>,
    touched: Vec<String>,
    stored: &std::collections::BTreeSet<String>,
) -> Option<PluginFormValues> {
    let state = commit_state(values, touched, stored);
    let outcome = ic_view::commit(document, &state);
    if !outcome.is_valid() {
        return None;
    }
    Some(PluginFormValues {
        record: outcome.record.clone(),
        keep_stored: outcome.keep_stored.clone(),
        name: outcome.record.get("name").cloned().unwrap_or_default(),
        values: outcome.settings,
    })
}

/// Puts a committed form back into a stored record, using only the binds the
/// document declared. Secrets the form left blank are absent here, so the caller
/// carries the stored ones over with [`carry_secrets`].
/// Either the committed form, or the binds that still have to be filled in.
pub enum FormOutcome {
    Ready(PluginFormValues),
    Incomplete(Vec<String>),
}

pub fn check_plugin_form(
    document: &ic_view::Document,
    values: std::collections::BTreeMap<String, serde_json::Value>,
    touched: Vec<String>,
    stored: &std::collections::BTreeSet<String>,
) -> FormOutcome {
    if let Some(form) = collect_plugin_form(document, values.clone(), touched.clone(), stored) {
        return FormOutcome::Ready(form);
    }
    let state = commit_state(values, touched, stored);
    let outcome = ic_view::commit(document, &state);
    let mut why = outcome.missing.clone();
    for problem in &outcome.problems {
        why.push(match problem {
            ic_view::CommitProblem::NotAnInteger { bind, got } => {
                format!("{bind}: {got:?} is not a number")
            }
            ic_view::CommitProblem::OutOfRange { bind, got } => {
                format!("{bind}: {got} is out of range")
            }
        });
    }
    FormOutcome::Incomplete(why)
}

/// One event a rendered form sends, in the shape `ic-view-session` speaks.
#[derive(serde::Deserialize)]
struct FormEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    bind: Option<String>,
    #[serde(default)]
    value: Option<serde_json::Value>,
    #[serde(default)]
    values: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    touched: Vec<String>,
}

thread_local! {
    /// The form a frontend that keeps no session of its own has open. One slot:
    /// opening another form drops the one before it, so a form the user walked
    /// away from is held no longer than until the next one opens.
    static OPEN_FORM: std::cell::RefCell<Option<(String, ic_view_session::Session)>> =
        const { std::cell::RefCell::new(None) };
}

/// The session behind the open form. The plugin is described when a form opens
/// and again only when it asks to be, exactly as where the frontend keeps the
/// session itself.
fn open_form(
    kind: &str,
    opening: bool,
    facts: &ic_view_session::HostFacts,
) -> Option<ic_view_session::Session> {
    let held = OPEN_FORM.with(|slot| slot.borrow_mut().take());
    if !opening {
        if let Some((open, session)) = held {
            if open == kind {
                return Some(session);
            }
        }
    }
    let document = ic_plugin_host::connection_document(kind)?;
    Some(ic_view_session::Session::open(
        kind, document, "null", facts,
    ))
}

/// Hands one form event to the plugin behind a kind and reports the form as it
/// now stands: the widget values arrive with the event and the answer carries
/// the document, the state and what the plugin wrote back. `redescribe` says
/// whether the plugin asked for the form to be built again.
pub fn form_event(
    kind: &str,
    incoming: &serde_json::Value,
    facts: &ic_view_session::HostFacts,
) -> Option<serde_json::Value> {
    let event: FormEvent = serde_json::from_value(incoming.clone()).ok()?;
    let mut session = open_form(kind, event.kind == "opened", facts)?;
    let surface = ic_view_session::ValueSurface::new(event.values, event.touched);
    let outcome = session.send(
        &surface,
        &ic_plugin_host::Kinds,
        facts,
        &event.kind,
        event.node.as_deref(),
        event.bind.as_deref(),
        event.value,
    );
    if outcome.redescribe {
        session.redescribe(&surface, &ic_plugin_host::Kinds, facts);
    }
    // Not even a seeded default: a secret is the frontend's to hold, never ours to send back.
    let mut shown = session.state.clone();
    for bind in plugin_secret_binds(&session.document) {
        shown.state.remove(&bind);
    }
    let answered = serde_json::json!({
        "view": kind,
        "document": session.document,
        "state": shown,
        "put": surface.written(),
        "clipboard": outcome.clipboard,
        "redescribe": outcome.redescribe,
    });
    OPEN_FORM.with(|slot| *slot.borrow_mut() = Some((kind.to_string(), session)));
    Some(answered)
}

pub fn record_from_form(form: &PluginFormValues, kind: &str, folder: Option<String>) -> Connection {
    let mut record = Connection::new(kind);
    for (bind, value) in form.record.iter().chain(form.values.iter()) {
        record.put(bind, value.clone());
    }
    if let Some(folder) = folder {
        record.folder = Some(folder);
    }
    record
}

pub const CONNECTIONS_KEY: &str = "ui.connections";

/// Where the list was kept while a record was FTP-shaped. Still read, so an
/// older version's connections open here; never written to again, so going back
/// to that version finds its own list where it left it.
pub const LEGACY_CONNECTIONS_KEY: &str = "ui.ftp_connections";

pub fn stored_connections(config: &client_config::AppConfig) -> Vec<Connection> {
    if let Some(saved) = config.get::<Vec<Connection>>(CONNECTIONS_KEY) {
        return saved;
    }
    carried_list(
        config
            .get::<Vec<Legacy>>(LEGACY_CONNECTIONS_KEY)
            .unwrap_or_default(),
    )
}

fn carried_list(old: Vec<Legacy>) -> Vec<Connection> {
    old.into_iter().map(carried_over).collect()
}

/// The one place the list is written. Everything that adds, edits, reorders or
/// deletes goes through here, so there is one key on disk and one shape in it.
pub fn save_connections(config: &client_config::AppConfig, connections: Vec<Connection>) {
    config.set(CONNECTIONS_KEY, connections);
}

/// Reads a list in either shape, for an exported file that may have been
/// written by an older version.
pub fn connections_from_json(raw: &serde_json::Value) -> Option<Vec<Connection>> {
    if let Ok(saved) = serde_json::from_value::<Vec<Connection>>(raw.clone()) {
        if saved.iter().all(|held| !held.kind.is_empty()) {
            return Some(saved);
        }
    }
    serde_json::from_value::<Vec<Legacy>>(raw.clone())
        .ok()
        .map(carried_list)
}

/// Encrypts whatever the document declared secret, or drops it when the user
/// asked not to keep passwords at all.
pub fn seal(config: &client_config::AppConfig, record: &mut Connection) {
    let kind = record.kind.clone();
    if config.get::<bool>("ui.save_passwords").unwrap_or(true) {
        map_secrets(record, &kind, |plain| {
            Some(secret_store::encrypt_secret(plain))
        });
    } else {
        map_secrets(record, &kind, |_| Some(String::new()));
    }
}

pub fn unseal(record: &mut Connection) {
    let kind = record.kind.clone();
    map_secrets(record, &kind, secret_store::decrypt_secret);
}

/// The one place a committed form becomes a stored record. Returns where it
/// landed, or why it could not be stored.
pub fn store_connection(
    config: &client_config::AppConfig,
    form: &PluginFormValues,
    kind: &str,
    editing: Option<usize>,
) -> Result<usize, String> {
    let mut stored = stored_connections(config);
    let previous = editing.and_then(|at| stored.get(at).cloned());
    let mut record = record_from_form(
        form,
        kind,
        previous.as_ref().and_then(|held| held.folder.clone()),
    );
    if let Some(previous) = previous.as_ref() {
        let mut opened = previous.clone();
        unseal(&mut opened);
        carry_secrets(&mut record, &opened, kind);
    }
    if record.name.trim().is_empty() {
        return Err("a connection needs a name".to_string());
    }
    let clashes = stored
        .iter()
        .enumerate()
        .any(|(at, held)| held.name == record.name && Some(at) != editing);
    if clashes {
        return Err(format!(
            "a connection named {:?} already exists",
            record.name
        ));
    }
    seal(config, &mut record);
    let at = match editing.filter(|at| *at < stored.len()) {
        Some(at) => {
            stored[at] = record;
            at
        }
        None => {
            stored.push(record);
            stored.len() - 1
        }
    };
    save_connections(config, stored);
    config.save();
    Ok(at)
}

pub fn translate_optional(key: &str) -> Option<String> {
    let resolved = ic_i18n::tr(key);
    (resolved != key).then_some(resolved)
}

/// The picture a kind says it has. A plugin registers its own image with
/// `register_asset` and names it `asset:<name>` in its document; a kind that
/// names a theme icon instead gets `Theme`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KindPicture {
    Svg(Vec<u8>),
    Theme(String),
    None,
}

pub fn kind_picture(kind: &str) -> KindPicture {
    let Some(document) = ic_plugin_host::connection_document(kind) else {
        return KindPicture::None;
    };
    let Some(named) = document
        .icon
        .as_ref()
        .map(|icon| icon.resolve(&translate_optional))
    else {
        return KindPicture::None;
    };
    if let Some(asset) = named.strip_prefix("asset:") {
        return match ic_plugin_host::asset(asset) {
            Some(bytes) => KindPicture::Svg(bytes),
            None => KindPicture::None,
        };
    }
    let bare = named.strip_prefix("theme:").unwrap_or(&named).trim();
    if bare.is_empty() {
        KindPicture::None
    } else {
        KindPicture::Theme(bare.to_string())
    }
}

/// The picture a kind names, spelled the way its document wrote it:
/// `asset:<plugin>/<name>` for one a plugin brought, `theme:<name>` for a theme
/// icon. A frontend that cannot read bytes — the browser — asks for it by this
/// name instead.
pub fn kind_icon_ref(kind: &str) -> Option<String> {
    let document = ic_plugin_host::connection_document(kind)?;
    let named = document
        .icon
        .as_ref()
        .map(|icon| icon.resolve(&translate_optional))?;
    (!named.trim().is_empty()).then_some(named)
}

/// What favourites and the active-drive highlight hang off. Built from the
/// kind and the record's identity, both of which the plugin declares, so a
/// kind with no host or port has a key like any other.
pub fn connection_key(record: &Connection) -> String {
    format!("conn://{}/{}", record.kind, record_identity(record))
}

/// The value of whatever field the kind named as its identity, falling back to
/// the record's name.
pub fn record_identity(record: &Connection) -> String {
    let named = ic_plugin_host::connection_document(&record.kind)
        .and_then(|document| document.identity.clone())
        .and_then(|bind| record.value(&bind))
        .unwrap_or_default();
    if named.trim().is_empty() {
        record.name.clone()
    } else {
        named
    }
}

/// Where a panel stands after mounting this record: the value of whatever field
/// the kind named as `opens_at`. The mount is the whole filesystem either way.
pub fn opening_path(record: &Connection) -> Option<String> {
    let bind = ic_plugin_host::connection_document(&record.kind)?.opens_at?;
    record.text(&bind)
}

/// The same for a mount that came from a plugin's own drive row rather than
/// from a saved record: what it handed over is read by the same name.
pub fn opening_path_in(
    kind: &str,
    settings: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let bind = ic_plugin_host::connection_document(kind)?.opens_at?;
    settings.get(&bind).cloned().filter(|held| !held.is_empty())
}

/// The one line under a connection's name, as its own kind writes it. A kind
/// with nothing to say about a record gets nothing.
pub fn kind_summary(record: &Connection) -> Option<String> {
    let document = ic_plugin_host::connection_document(&record.kind)?;
    let state = plugin_state_from_record(&document, record);
    ic_view::render_summary(document.summary.as_ref(), &state)
}

pub struct ConnectionKindEntry {
    /// The id the plugin registered, spelled as it registered it: what the host
    /// is asked with and what the plugin is addressed by. Only `label` is shown.
    pub protocol: String,
    pub label: String,
    pub document: ic_view::Document,
}

pub fn kind_table() -> Vec<ConnectionKindEntry> {
    let mut table: Vec<ConnectionKindEntry> = Vec::new();
    for id in ic_plugin_host::connection_kind_ids() {
        if table
            .iter()
            .any(|entry| entry.protocol.eq_ignore_ascii_case(&id))
        {
            continue;
        }
        let Some(document) = ic_plugin_host::connection_document(&id) else {
            continue;
        };
        let label = document
            .label
            .as_ref()
            .map(|text| text.resolve(&translate_optional))
            .unwrap_or_else(|| id.to_uppercase());
        table.push(ConnectionKindEntry {
            protocol: id,
            label,
            document,
        });
    }
    table.sort_by(|a, b| a.label.cmp(&b.label));
    table
}

/// Where a protocol sits in the offered table. An unknown one falls back to the
/// first kind offered, never to a protocol this crate names itself.
pub fn kind_index(table: &[ConnectionKindEntry], protocol: &str) -> u32 {
    table
        .iter()
        .position(|entry| entry.protocol.eq_ignore_ascii_case(protocol))
        .unwrap_or(0) as u32
}

pub fn kind_protocol(table: &[ConnectionKindEntry], index: u32) -> String {
    table
        .get(index as usize)
        .or_else(|| table.first())
        .map(|entry| entry.protocol.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{kind_table, Connection};

    /// `kind_table` asks every registered kind to describe itself, so a test that
    /// counts how often one plugin is described cannot run beside one that lists
    /// them all. These few take turns; the rest of the module still runs freely.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn alone() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn the_dialog_offers_exactly_the_kinds_a_plugin_declared() {
        let _alone = alone();
        for entry in kind_table() {
            assert!(
                ic_plugin_host::connection_document(&entry.protocol).is_some(),
                "{} is offered without a plugin behind it",
                entry.protocol
            );
        }
        assert!(
            !kind_table()
                .iter()
                .any(|entry| entry.protocol == "nosuchkind"),
            "a kind the user cannot connect with must not be offered"
        );
    }

    #[test]
    fn the_table_carries_the_id_the_plugin_registered_and_shows_it_uppercased_only_as_a_label() {
        let _alone = alone();
        declare("MixedKind", &[]);
        let table = kind_table();
        let entry = table
            .iter()
            .find(|entry| entry.protocol == "MixedKind")
            .expect("the kind is offered under the id it registered");
        assert_eq!(
            entry.label, "MIXEDKIND",
            "a kind with no label of its own is named to the user in capitals"
        );
        assert_eq!(
            super::kind_protocol(&table, super::kind_index(&table, "mixedkind")),
            "MixedKind",
            "what the dropdown hands on is the spelling the plugin is addressed by"
        );
    }

    #[test]
    fn an_unknown_protocol_falls_back_to_what_is_offered_not_to_a_named_one() {
        let _alone = alone();
        declare("alpha", &[]);
        declare("omega", &[]);
        let table = kind_table();
        let offered: Vec<&str> = table.iter().map(|entry| entry.protocol.as_str()).collect();
        assert!(offered.contains(&"alpha") && offered.contains(&"omega"));

        let at = super::kind_index(&table, "nosuchkind");
        assert_eq!(
            super::kind_protocol(&table, at),
            table[0].protocol,
            "the fallback is the first kind a plugin offered"
        );
        let past_the_end = super::kind_protocol(&table, table.len() as u32 + 5);
        assert_eq!(past_the_end, table[0].protocol);

        let empty: Vec<super::ConnectionKindEntry> = Vec::new();
        assert_eq!(super::kind_index(&empty, "alpha"), 0);
        assert!(
            super::kind_protocol(&empty, 0).is_empty(),
            "with nothing offered the host names no protocol at all"
        );
    }

    #[test]
    fn a_record_the_old_version_saved_is_read_field_for_field() {
        let legacy = serde_json::json!([{
            "name": "old",
            "folder": "work",
            "protocol": "FTP",
            "host": "ftp.example.org",
            "port": 21,
            "user": "anonymous",
            "pass": "sealed:x",
            "use_tunnel": true,
            "tunnel_port": 2222,
            "settings": { "api_token": "t-7" }
        }]);
        let carried = super::connections_from_json(&legacy).expect("an old list reads");
        let record = &carried[0];
        assert_eq!(record.name, "old");
        assert_eq!(record.folder.as_deref(), Some("work"));
        assert_eq!(
            record.kind, "ftp",
            "the kind is the plugin's id, as it registers it"
        );
        assert_eq!(record.value("host").as_deref(), Some("ftp.example.org"));
        assert_eq!(
            record.value("url").as_deref(),
            Some("ftp.example.org"),
            "WebDAV's field is called url, and which plugin wrote the record cannot be asked here"
        );
        assert_eq!(record.value("port").as_deref(), Some("21"));
        assert_eq!(record.value("user").as_deref(), Some("anonymous"));
        assert_eq!(record.value("pass").as_deref(), Some("sealed:x"));
        assert_eq!(record.value("use_tunnel").as_deref(), Some("true"));
        assert_eq!(record.value("tunnel_port").as_deref(), Some("2222"));
        assert_eq!(
            record.value("api_token").as_deref(),
            Some("t-7"),
            "what the old bag held is kept under the same name"
        );
    }

    #[test]
    fn an_empty_field_of_the_old_shape_is_not_carried_over_as_a_blank() {
        let legacy = serde_json::json!([{ "name": "bare", "protocol": "ftp" }]);
        let carried = super::connections_from_json(&legacy).expect("an old list reads");
        assert!(
            carried[0].settings.is_empty(),
            "{:?} is an empty record dressed up as a filled-in one",
            carried[0].settings
        );
    }

    #[test]
    fn the_list_an_older_version_saved_is_read_until_something_is_saved_over_it() {
        let config = client_config::AppConfig::new("ice-commander-connections-carried");
        config.forget(super::CONNECTIONS_KEY);
        config.set(
            super::LEGACY_CONNECTIONS_KEY,
            serde_json::json!([{ "name": "old", "protocol": "FTP", "host": "ftp.example.org" }]),
        );
        let carried = super::stored_connections(&config);
        assert_eq!(
            carried.len(),
            1,
            "an old list is what the application reads"
        );
        assert_eq!(carried[0].kind, "ftp");

        super::save_connections(&config, Vec::new());
        assert!(
            super::stored_connections(&config).is_empty(),
            "and once the new list is written it is the one answered from"
        );
        assert!(
            config
                .get::<Vec<serde_json::Value>>(super::LEGACY_CONNECTIONS_KEY)
                .is_some(),
            "while the old one is left where an older version would look for it"
        );
    }

    #[test]
    fn a_list_in_the_new_shape_is_read_as_it_stands() {
        let saved = serde_json::json!([{
            "name": "peers",
            "kind": "nodeinnet",
            "settings": { "device": "laptop" }
        }]);
        let read = super::connections_from_json(&saved).expect("a new list reads");
        assert_eq!(read[0].kind, "nodeinnet");
        assert_eq!(read[0].value("device").as_deref(), Some("laptop"));
    }
    /// A record the way a saved one looks: a kind, a name, and whatever the
    /// kind's own fields are called.
    fn record(kind: &str, name: &str, fields: &[(&str, &str)]) -> Connection {
        let mut built = Connection::new(kind);
        built.name = name.to_string();
        for (bind, value) in fields {
            built.put(bind, (*value).to_string());
        }
        built
    }

    fn declare(kind: &str, secrets: &[&str]) {
        let fields: Vec<serde_json::Value> = ["name", "host", "user", "pass", "api_token"]
            .iter()
            .map(|bind| {
                serde_json::json!({
                    "bind": bind,
                    "type": "text",
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
        declare_source(kind, &document);
    }

    fn declare_source(kind: &str, document: &str) {
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

    fn declare_with_icon(kind: &str, icon: &str) {
        let document = serde_json::json!({
            "schema": 1,
            "kind": kind,
            "icon": icon,
            "fields": [ { "bind": "name", "type": "text" } ],
            "form": { "t": "column", "children": [] },
        })
        .to_string();
        declare_source(kind, &document);
    }

    #[test]
    fn a_connections_key_is_its_kind_and_the_identity_the_kind_named() {
        let document = serde_json::json!({
            "schema": 1,
            "kind": "keyed",
            "identity": "name",
            "fields": [
                { "bind": "name", "type": "text", "scope": "record" },
                { "bind": "host", "type": "text" }
            ],
            "form": { "t": "column", "children": [] },
        })
        .to_string();
        declare_source("keyed", &document);
        let held = record("KEYED", "work files", &[("host", "files.example.org")]);
        assert_eq!(super::connection_key(&held), "conn://keyed/work files");

        // A kind nobody installed still gets a key, or it could never be starred again.
        let orphan = Connection {
            kind: "gone".to_string(),
            ..held.clone()
        };
        assert_eq!(super::connection_key(&orphan), "conn://gone/work files");
    }

    #[test]
    fn the_line_under_a_connection_is_the_one_its_kind_writes() {
        let document = serde_json::json!({
            "schema": 1,
            "kind": "summarised",
            "summary": { "fmt": "{user}@{host}", "fallback": { "fmt": "{host}" } },
            "fields": [
                { "bind": "host", "type": "text" },
                { "bind": "user", "type": "text" }
            ],
            "form": { "t": "column", "children": [] },
        })
        .to_string();
        declare_source("summarised", &document);
        let mut held = record(
            "summarised",
            "s",
            &[("host", "files.example.org"), ("user", "ivan")],
        );
        assert_eq!(
            super::kind_summary(&held).as_deref(),
            Some("ivan@files.example.org")
        );
        held.settings.remove("user");
        assert_eq!(
            super::kind_summary(&held).as_deref(),
            Some("files.example.org"),
            "what the kind named as its fallback"
        );
        assert_eq!(
            super::kind_summary(&Connection {
                kind: "no-such-kind-at-all".to_string(),
                ..held.clone()
            }),
            None
        );
    }

    #[test]
    fn a_kind_is_drawn_with_the_picture_its_plugin_registered() {
        let picture = b"<svg id=\"kind\"/>";
        let owner = std::ffi::CString::new("ic-picturesque").expect("an id");
        let name = std::ffi::CString::new("pict.svg").expect("a name");
        let host = ic_plugin_host::host_table();
        assert_eq!(
            (host.register_plugin_asset)(
                owner.as_ptr(),
                name.as_ptr(),
                picture.as_ptr(),
                picture.len() as u64
            ),
            ic_plugin_api::IC_OK
        );
        declare_with_icon("picturesque", "asset:ic-picturesque/pict.svg");
        assert_eq!(
            super::kind_picture("picturesque"),
            super::KindPicture::Svg(picture.to_vec())
        );

        // The same name under nobody's ownership is a different picture.
        declare_with_icon("nameless", "asset:pict.svg");
        assert_eq!(super::kind_picture("nameless"), super::KindPicture::None);
    }

    #[test]
    fn a_frontend_that_cannot_take_bytes_is_given_the_name_instead() {
        declare_with_icon("named", "asset:ic-named/pic.svg");
        assert_eq!(
            super::kind_icon_ref("named").as_deref(),
            Some("asset:ic-named/pic.svg"),
            "the browser fetches it from /api/plugin-assets by this name"
        );
        declare("wordless", &[]);
        assert_eq!(super::kind_icon_ref("wordless"), None);
        assert_eq!(super::kind_icon_ref("no-such-kind-at-all"), None);
    }

    #[test]
    fn a_kind_can_name_a_theme_icon_instead() {
        declare_with_icon("themed", "theme:folder-remote-symbolic");
        assert_eq!(
            super::kind_picture("themed"),
            super::KindPicture::Theme("folder-remote-symbolic".to_string())
        );
        declare_with_icon("bare", "folder-remote-symbolic");
        assert_eq!(
            super::kind_picture("bare"),
            super::KindPicture::Theme("folder-remote-symbolic".to_string())
        );
    }

    #[test]
    fn a_kind_that_brought_no_picture_leaves_the_application_to_draw_one() {
        declare("plain", &[]);
        assert_eq!(super::kind_picture("plain"), super::KindPicture::None);
        declare_with_icon("promising", "asset:nobody-registered-this.svg");
        assert_eq!(
            super::kind_picture("promising"),
            super::KindPicture::None,
            "an asset the host does not hold is not a picture"
        );
        assert_eq!(
            super::kind_picture("no-such-kind-at-all"),
            super::KindPicture::None
        );
    }

    #[test]
    fn a_secret_the_plugin_invented_is_mapped_just_like_a_known_one() {
        declare("vault", &["pass", "api_token"]);
        let mut held = record(
            "VAULT",
            "v",
            &[
                ("host", "vault.example.org"),
                ("user", "ivan"),
                ("pass", "hunter2"),
                ("api_token", "t-0007"),
            ],
        );
        super::map_secrets(&mut held, "vault", |plain| Some(format!("sealed:{plain}")));
        assert_eq!(held.value("pass").as_deref(), Some("sealed:hunter2"));
        assert_eq!(
            held.value("api_token").as_deref(),
            Some("sealed:t-0007"),
            "a secret the plugin invented is sealed like any other"
        );
        assert_eq!(
            held.value("user").as_deref(),
            Some("ivan"),
            "only the declared secrets are touched"
        );
    }

    /// A plugin can fill in a field nothing draws — which share was chosen —
    /// and the form has to commit it, or the save button does nothing and says
    /// nothing.
    #[test]
    fn what_the_plugin_set_is_committed_even_though_no_widget_shows_it() {
        let mut held = ic_view::State::default();
        held.set_state("peer_id", serde_json::json!("peer-1"));
        held.set_state("name", serde_json::json!("from the plugin"));
        let shown = std::collections::BTreeMap::from([
            ("name".to_string(), serde_json::json!("typed by the user")),
            ("remote_path".to_string(), serde_json::json!("/docs")),
        ]);

        let all = super::values_to_commit(&held, shown);
        assert_eq!(
            all.get("peer_id"),
            Some(&serde_json::json!("peer-1")),
            "a value only the plugin knows about still reaches the commit"
        );
        assert_eq!(
            all.get("name"),
            Some(&serde_json::json!("typed by the user")),
            "and what the user typed wins over what the plugin suggested"
        );
        assert_eq!(all.get("remote_path"), Some(&serde_json::json!("/docs")));
    }

    #[test]
    fn every_bind_a_document_declares_survives_a_write_then_read() {
        declare("roundtrip", &[]);
        let mut record = Connection::new("roundtrip");
        let binds = [
            "name",
            "folder",
            "url",
            "user",
            "pass",
            "remote_path",
            "port",
            "use_tunnel",
            "tunnel_passphrase",
            "whatever a plugin calls its own field",
        ];
        for bind in binds {
            let written = format!("value-of-{bind}");
            record.put(bind, written.clone());
            assert_eq!(
                record.value(bind).as_deref(),
                Some(written.as_str()),
                "{bind} did not read back what was written"
            );
        }
    }

    const SEEDS: &str = r#"{
        "schema": 1,
        "kind": "seeds",
        "fields": [
            { "bind": "name", "type": "text" },
            { "bind": "host", "type": "text" },
            { "bind": "port", "type": "integer", "min": 1, "max": 65535, "default": 22 }
        ],
        "form": { "t": "column", "children": [] }
    }"#;

    #[test]
    fn a_connection_being_created_seeds_nothing_so_the_kinds_own_defaults_stand() {
        declare_source("seeds", SEEDS);
        let document = ic_plugin_host::connection_document("seeds").expect("a form");

        let blank = super::plugin_state_from_record(&document, &Connection::new("seeds"));
        assert!(
            blank.state.is_empty(),
            "a record that holds nothing seeds no bind at all"
        );
        let opened = ic_view_session::seeded_state(&document, Some(&blank), &facts(), "null");
        assert_eq!(
            opened.state.get("port"),
            Some(&serde_json::json!(22)),
            "so the default the kind declared is what the plugin is told the form holds"
        );

        let held = record(
            "seeds",
            "work",
            &[("host", "h.example.org"), ("port", "2222")],
        );
        let seeded = super::plugin_state_from_record(&document, &held);
        assert_eq!(seeded.state.get("name"), Some(&serde_json::json!("work")));
        assert_eq!(
            seeded.state.get("port"),
            Some(&serde_json::json!("2222")),
            "a record that holds a value still seeds it, as text"
        );
    }

    #[test]
    fn a_blank_incoming_secret_keeps_the_one_already_stored() {
        declare("keeper", &["pass", "api_token"]);
        let stored = record(
            "KEEPER",
            "k",
            &[("pass", "kept"), ("api_token", "kept-token")],
        );
        let mut incoming = record("KEEPER", "k", &[("pass", "")]);
        super::carry_secrets(&mut incoming, &stored, "keeper");
        assert_eq!(incoming.value("pass").as_deref(), Some("kept"));
        assert_eq!(
            incoming.settings.get("api_token").map(String::as_str),
            Some("kept-token")
        );

        let mut replaced = record("KEEPER", "k", &[("pass", "fresh")]);
        super::carry_secrets(&mut replaced, &stored, "keeper");
        assert_eq!(
            replaced.value("pass").as_deref(),
            Some("fresh"),
            "a password the user actually typed is not overwritten"
        );
    }

    const LISTENS: &str = r#"{
        "schema": 1,
        "kind": "listens",
        "fields": [
            { "bind": "name", "type": "text" },
            { "bind": "host", "type": "text" },
            { "bind": "pass", "type": "text", "secret": true }
        ],
        "form": { "t": "column", "children": [
            { "t": "input", "id": "host", "bind": "host" },
            { "t": "button", "id": "check", "intent": { "do": "emit", "node": "check" } } ] }
    }"#;

    const SIGN_IN: &str = r#"{
        "schema": 1,
        "kind": "flips",
        "fields": [ { "bind": "name", "type": "text" } ],
        "form": { "t": "column", "children": [
            { "t": "button", "id": "signin", "intent": { "do": "emit", "node": "signin" } } ] }
    }"#;

    const SIGNED_IN: &str = r#"{
        "schema": 1,
        "kind": "flips",
        "fields": [ { "bind": "name", "type": "text" } ],
        "form": { "t": "column", "children": [
            { "t": "text", "id": "already", "text": "signed in" } ] }
    }"#;

    const GUARDED: &str = r#"{
        "schema": 1,
        "kind": "guarded",
        "fields": [
            { "bind": "name", "type": "text", "scope": "record", "required": true },
            { "bind": "pass", "type": "text", "scope": "settings", "secret": true,
              "required": true, "empty_as_absent": true, "empty_keeps_stored": true }
        ],
        "form": { "t": "column", "children": [
            { "t": "input", "id": "pass", "bind": "pass" } ] }
    }"#;

    /// Each test drives a kind of its own and listens through a tally of its own,
    /// handed over as `user_data`: these run in one process and in parallel, so a
    /// shared log would make every assertion depend on what else was running.
    struct Heard {
        events: std::sync::Mutex<Vec<serde_json::Value>>,
        describes: std::sync::atomic::AtomicUsize,
        signed: std::sync::atomic::AtomicBool,
    }

    impl Heard {
        const fn new() -> Heard {
            Heard {
                events: std::sync::Mutex::new(Vec::new()),
                describes: std::sync::atomic::AtomicUsize::new(0),
                signed: std::sync::atomic::AtomicBool::new(false),
            }
        }

        fn of(user_data: *mut std::ffi::c_void) -> Option<&'static Heard> {
            (!user_data.is_null()).then(|| unsafe { &*(user_data as *const Heard) })
        }

        fn events(&self) -> Vec<serde_json::Value> {
            self.events.lock().expect("the log").clone()
        }

        fn describes(&self) -> usize {
            self.describes.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    thread_local! {
        static ANSWER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn spoken(source: &str) -> ic_plugin_api::IcBytes {
        ANSWER.with(|slot| {
            *slot.borrow_mut() = source.as_bytes().to_vec();
            let held = slot.borrow();
            ic_plugin_api::IcBytes {
                data: held.as_ptr(),
                len: held.len() as u64,
            }
        })
    }

    extern "C" fn opens(
        _: *const u8,
        _: u64,
        _: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcFsHandle {
        std::ptr::null_mut()
    }

    extern "C" fn listens_describes(
        _: *const u8,
        _: u64,
        user_data: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        if let Some(heard) = Heard::of(user_data) {
            heard
                .describes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        spoken(LISTENS)
    }

    extern "C" fn listens_hears(
        event: *const u8,
        len: u64,
        user_data: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        let source = unsafe { std::slice::from_raw_parts(event, len as usize) };
        if let (Some(heard), Ok(parsed)) = (
            Heard::of(user_data),
            serde_json::from_slice::<serde_json::Value>(source),
        ) {
            heard.events.lock().expect("the log").push(parsed);
        }
        spoken(r#"{ "put": { "host": "found.example.org" }, "clipboard": "t-0007" }"#)
    }

    extern "C" fn flips_describes(
        _: *const u8,
        _: u64,
        user_data: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        let Some(heard) = Heard::of(user_data) else {
            return spoken(SIGN_IN);
        };
        heard
            .describes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        spoken(if heard.signed.load(std::sync::atomic::Ordering::Relaxed) {
            SIGNED_IN
        } else {
            SIGN_IN
        })
    }

    extern "C" fn flips_hears(
        _: *const u8,
        _: u64,
        user_data: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        if let Some(heard) = Heard::of(user_data) {
            heard
                .signed
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        spoken(r#"{ "redescribe": true }"#)
    }

    fn declare_listening(
        kind: &str,
        document: &str,
        describe: ic_plugin_api::IcViewDescribeFn,
        on_event: ic_plugin_api::IcViewEventFn,
        heard: &'static Heard,
    ) {
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: opens,
            fs: std::ptr::null(),
            describe: Some(describe),
            on_event: Some(on_event),
        };
        let host = ic_plugin_host::host_table();
        let id = std::ffi::CString::new(kind).expect("a kind id");
        assert_eq!(
            (host.register_connection_kind)(
                id.as_ptr(),
                document.as_ptr(),
                document.len() as u64,
                &table,
                heard as *const Heard as *mut std::ffi::c_void,
            ),
            ic_plugin_api::IC_OK
        );
    }

    fn facts() -> ic_view_session::HostFacts {
        ic_view_session::HostFacts::new("test", "en", &[])
    }

    fn gesture(node: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "activate",
            "node": node,
            "values": { "name": "work", "host": "h.example.org", "pass": "hunter2" },
            "touched": ["host"],
        })
    }

    fn opening() -> serde_json::Value {
        serde_json::json!({
            "type": "opened",
            "values": { "name": "work", "host": "h.example.org" },
            "touched": [],
        })
    }

    fn nodes_of(answered: &serde_json::Value) -> Vec<String> {
        let document: ic_view::Document =
            serde_json::from_value(answered["document"].clone()).expect("a document");
        let mut found = Vec::new();
        document.form.walk(&mut |node| {
            if let Some(id) = node.id.clone() {
                found.push(id);
            }
        });
        found
    }

    #[test]
    fn an_answer_reports_redescribe_only_when_the_plugin_asked_for_the_form_again() {
        static HEARD: Heard = Heard::new();
        declare_listening(
            "listens.answers",
            LISTENS,
            listens_describes,
            listens_hears,
            &HEARD,
        );
        let answered = super::form_event("listens.answers", &gesture("check"), &facts())
            .expect("the plugin answered");
        assert_eq!(
            answered["redescribe"],
            serde_json::json!(false),
            "a plugin that only wrote into the form did not ask for a new one"
        );
        assert_eq!(
            answered["put"]["host"],
            serde_json::json!("found.example.org")
        );
        assert_eq!(answered["clipboard"], serde_json::json!("t-0007"));
        assert_eq!(
            nodes_of(&answered),
            vec!["host".to_string(), "check".to_string()],
            "the form is the one the frontend already draws"
        );
    }

    #[test]
    fn a_plugin_that_asks_to_be_described_again_is_answered_with_the_form_it_now_wants() {
        static HEARD: Heard = Heard::new();
        declare_listening(
            "flips.rebuilt",
            SIGN_IN,
            flips_describes,
            flips_hears,
            &HEARD,
        );
        let answered = super::form_event("flips.rebuilt", &gesture("signin"), &facts())
            .expect("the plugin answered");
        assert_eq!(answered["redescribe"], serde_json::json!(true));
        assert_eq!(
            nodes_of(&answered),
            vec!["already".to_string()],
            "redescribe answers with the form the plugin now wants shown"
        );
    }

    #[test]
    fn the_plugin_behind_a_form_is_described_when_the_form_opens_and_not_again_at_every_event() {
        let _alone = alone();
        static HEARD: Heard = Heard::new();
        declare_listening(
            "listens.describes",
            LISTENS,
            listens_describes,
            listens_hears,
            &HEARD,
        );
        super::form_event("listens.describes", &opening(), &facts()).expect("the form opened");
        assert_eq!(
            HEARD.describes(),
            1,
            "opening a form asks the plugin for it exactly once"
        );

        super::form_event("listens.describes", &gesture("check"), &facts())
            .expect("the plugin answered");
        super::form_event("listens.describes", &gesture("check"), &facts())
            .expect("the plugin answered");
        assert_eq!(
            HEARD.describes(),
            1,
            "an event that did not ask for a new form leaves the plugin undescribed"
        );

        super::form_event("listens.describes", &opening(), &facts()).expect("the form opened");
        assert_eq!(
            HEARD.describes(),
            2,
            "the next form opened over the same kind is described afresh"
        );
    }

    #[test]
    fn a_plugin_that_asks_for_a_new_form_is_described_once_more_and_no_more_than_once() {
        let _alone = alone();
        static HEARD: Heard = Heard::new();
        declare_listening(
            "flips.counted",
            SIGN_IN,
            flips_describes,
            flips_hears,
            &HEARD,
        );
        super::form_event("flips.counted", &opening(), &facts()).expect("the form opened");
        let opened = HEARD.describes();
        super::form_event("flips.counted", &gesture("signin"), &facts())
            .expect("the plugin answered");
        assert_eq!(
            HEARD.describes(),
            opened + 1,
            "asking for the form again costs one describe, not one per event"
        );
    }

    #[test]
    fn a_secret_the_form_holds_reaches_neither_the_plugin_nor_the_answer() {
        static HEARD: Heard = Heard::new();
        declare_listening(
            "listens.secret",
            LISTENS,
            listens_describes,
            listens_hears,
            &HEARD,
        );
        let answered = super::form_event("listens.secret", &gesture("check"), &facts())
            .expect("the plugin answered");
        let heard = HEARD.events();
        let seen = heard.last().expect("the plugin was reached");
        assert_eq!(seen["values"]["host"], serde_json::json!("h.example.org"));
        assert!(
            seen["values"].get("pass").is_none(),
            "a secret never leaves the host"
        );
        assert!(
            answered["state"]["state"].get("pass").is_none(),
            "nor is it handed back to the frontend"
        );
    }

    #[test]
    fn a_form_event_for_a_kind_no_plugin_declared_is_not_answered_at_all() {
        assert!(super::form_event("nosuchkind", &gesture("check"), &facts()).is_none());
    }

    #[test]
    fn a_password_the_record_already_holds_is_not_asked_for_again_to_commit_the_form() {
        declare_source("guarded", GUARDED);
        let document = ic_plugin_host::connection_document("guarded").expect("a form");
        let typed: std::collections::BTreeMap<String, serde_json::Value> = [(
            "name".to_string(),
            serde_json::Value::String("work".to_string()),
        )]
        .into_iter()
        .collect();

        let nothing_stored = std::collections::BTreeSet::new();
        match super::check_plugin_form(&document, typed.clone(), Vec::new(), &nothing_stored) {
            super::FormOutcome::Incomplete(why) => assert!(
                why.contains(&"pass".to_string()),
                "a password nobody has ever typed is still required"
            ),
            super::FormOutcome::Ready(_) => {
                panic!("a required password nothing holds must not commit")
            }
        }

        let held = std::collections::BTreeSet::from(["pass".to_string()]);
        let super::FormOutcome::Ready(form) =
            super::check_plugin_form(&document, typed, Vec::new(), &held)
        else {
            panic!("a password the record already holds is not typed again to save");
        };
        assert!(
            form.keep_stored.contains("pass"),
            "the commit says the stored one is to be kept"
        );
        assert!(
            !form.values.contains_key("pass"),
            "and carries no password of its own"
        );
    }

    #[test]
    fn a_record_that_answered_nothing_is_written_out_as_nothing() {
        let mut plain = Connection::new("ftp");
        plain.name = "old".to_string();
        let encoded = serde_json::to_value(&plain).expect("encodes");
        assert_eq!(
            encoded,
            serde_json::json!({ "name": "old", "kind": "ftp" }),
            "an absent folder and an empty form are not written at all"
        );
    }
}
