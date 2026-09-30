use ic_view::{Cond, Document, Emit, Intent, State};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub trait ViewSurface {
    fn values(&self) -> BTreeMap<String, Value>;
    fn touched(&self) -> Vec<String>;
    fn set_value(&self, node: &str, value: &Value);
    fn refresh(&self, state: &State);
}

pub trait ViewHost {
    fn describe(&self, id: &str, context: &Value) -> Option<Document>;
    fn event(&self, id: &str, event: &Value) -> Option<Value>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFacts {
    pub kind: String,
    pub locale: String,
    pub can: Vec<String>,
}

impl HostFacts {
    pub fn new(kind: &str, locale: &str, can: &[&str]) -> HostFacts {
        HostFacts {
            kind: kind.to_string(),
            locale: locale.to_string(),
            can: can.iter().map(|name| name.to_string()).collect(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({ "kind": self.kind, "locale": self.locale, "can": self.can })
    }

    pub fn seed(&self, state: &mut State) {
        state.set_host("kind", json!(self.kind));
        state.set_host("locale", json!(self.locale));
        state.set_host("can", json!(self.can));
        for name in &self.can {
            state.set_host(&format!("can_{name}"), json!(true));
        }
    }
}

pub fn context(facts: &HostFacts, argument: &str) -> Value {
    json!({ "host": facts.to_json(), "arg": parsed(argument) })
}

#[allow(clippy::too_many_arguments)]
pub fn envelope(
    view: &str,
    argument: &str,
    kind: &str,
    node: Option<&str>,
    bind: Option<&str>,
    value: Option<Value>,
    values: Value,
    facts: &HostFacts,
) -> Value {
    let mut event = Map::new();
    event.insert("v".to_string(), json!(1));
    event.insert("view".to_string(), json!(view));
    event.insert("type".to_string(), json!(kind));
    event.insert("gesture".to_string(), json!(kind == "activate"));
    event.insert("values".to_string(), values);
    event.insert("arg".to_string(), parsed(argument));
    event.insert("host".to_string(), facts.to_json());
    if let Some(node) = node {
        event.insert("node".to_string(), json!(node));
    }
    if let Some(bind) = bind {
        event.insert("bind".to_string(), json!(bind));
    }
    if let Some(value) = value {
        event.insert("value".to_string(), value);
    }
    Value::Object(event)
}

fn parsed(argument: &str) -> Value {
    serde_json::from_str(argument).unwrap_or(Value::Null)
}

pub fn seeded_state(
    document: &Document,
    carried: Option<&State>,
    facts: &HostFacts,
    argument: &str,
) -> State {
    let mut state = State::for_document(document);
    if let Some(previous) = carried {
        state.state = previous.state.clone();
        state.view = previous.view.clone();
        state.touched = previous.touched.clone();
        state.stored_secrets = previous.stored_secrets.clone();
    }
    facts.seed(&mut state);
    state.arg = parsed(argument);
    for field in &document.fields {
        if state.state.contains_key(&field.bind) {
            continue;
        }
        if let Some(fallback) = field
            .default
            .as_ref()
            .and_then(|value| ic_view::resolve(value, &state))
        {
            state.state.insert(field.bind.clone(), fallback);
        }
    }
    state
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    pub node: String,
    pub debounce_ms: u32,
    pub on_value: bool,
}

/// The bind behind every node that names one, so what a reply puts into a widget
/// is also what the state holds: a rebuild in between would otherwise lose it.
fn bound_nodes(document: &Document) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    document.form.walk(&mut |node| {
        if let (Some(id), Some(bind)) = (node.id.as_ref(), node.bind.as_ref()) {
            found.insert(id.clone(), bind.clone());
        }
    });
    found
}

pub fn watches(document: &Document) -> Vec<Watch> {
    let mut found = Vec::new();
    document.form.walk(&mut |node| {
        let Some(id) = node.id.clone() else {
            return;
        };
        if node.takes_value() && node.emit == Emit::Change {
            found.push(Watch {
                node: id.clone(),
                debounce_ms: node.debounce_ms.unwrap_or(0),
                on_value: true,
            });
        }
        let emits = matches!(
            node.intent.as_ref().and_then(|intent| match intent {
                Cond::Fixed(fixed) => Some(fixed.clone()),
                Cond::Cases { .. } => None,
            }),
            Some(Intent::Emit { .. })
        );
        if emits {
            found.push(Watch {
                node: id,
                debounce_ms: 0,
                on_value: false,
            });
        }
    });
    found
}

/// Set while a reply is being written into the surface, so the value changes it
/// causes are not echoed back to the plugin. Kept outside the session because
/// the surface calls back while the session itself is borrowed.
#[derive(Clone, Default)]
pub struct Applying(Arc<AtomicBool>);

impl Applying {
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    fn hold(&self) -> Hold {
        self.0.store(true, Ordering::SeqCst);
        Hold(self.0.clone())
    }
}

struct Hold(Arc<AtomicBool>);

impl Drop for Hold {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub clipboard: Option<String>,
    pub redescribe: bool,
    /// The plugin has done what the dialogue was opened for and wants it shut.
    /// A button can close a view by itself, but only instead of telling the
    /// plugin — this is how one that acts first gets to dismiss itself after.
    pub close: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redescribed {
    Nothing,
    InPlace,
    Rebuilt,
}

pub struct Session {
    pub id: String,
    pub document: Document,
    pub state: State,
    pub argument: String,
    applying: Applying,
    pending: BTreeMap<String, u64>,
}

impl Session {
    pub fn open(id: &str, document: Document, argument: &str, facts: &HostFacts) -> Session {
        let state = seeded_state(&document, None, facts, argument);
        Session {
            id: id.to_string(),
            document,
            state,
            argument: argument.to_string(),
            applying: Applying::default(),
            pending: BTreeMap::new(),
        }
    }

    /// A form with a fixed document and no plugin behind it: nothing is ever sent,
    /// so the caller starts from the values it already has.
    pub fn over(id: &str, document: Document, carried: &State, facts: &HostFacts) -> Session {
        let state = seeded_state(&document, Some(carried), facts, "null");
        Session {
            id: id.to_string(),
            document,
            state,
            argument: "null".to_string(),
            applying: Applying::default(),
            pending: BTreeMap::new(),
        }
    }

    pub fn applying(&self) -> Applying {
        self.applying.clone()
    }

    pub fn watches(&self) -> Vec<Watch> {
        watches(&self.document)
    }

    /// Claims the next turn for a node whose change is waiting out its debounce.
    pub fn arm(&mut self, node: &str) -> u64 {
        let counter = self.pending.entry(node.to_string()).or_insert(0);
        *counter += 1;
        *counter
    }

    pub fn current(&self, node: &str, turn: u64) -> bool {
        self.pending.get(node).copied().unwrap_or(0) == turn
    }

    /// Reads the surface into the state and redraws it, returning what the
    /// plugin is allowed to see: everything but the secrets.
    pub fn snapshot(&mut self, surface: &dyn ViewSurface) -> Value {
        for bind in surface.touched() {
            self.state.touched.insert(bind);
        }
        let mut shown = Map::new();
        for (bind, value) in surface.values() {
            let secret = self
                .document
                .field(&bind)
                .map(|field| field.secret)
                .unwrap_or(false);
            if secret {
                continue;
            }
            self.state.state.insert(bind.clone(), value.clone());
            shown.insert(bind, value);
        }
        {
            let _held = self.applying.hold();
            surface.refresh(&self.state);
        }
        Value::Object(shown)
    }

    pub fn envelope(
        &self,
        kind: &str,
        node: Option<&str>,
        bind: Option<&str>,
        value: Option<Value>,
        values: Value,
        facts: &HostFacts,
    ) -> Value {
        envelope(
            &self.id,
            &self.argument,
            kind,
            node,
            bind,
            value,
            values,
            facts,
        )
    }

    pub fn send(
        &mut self,
        surface: &dyn ViewSurface,
        host: &dyn ViewHost,
        facts: &HostFacts,
        kind: &str,
        node: Option<&str>,
        bind: Option<&str>,
        value: Option<Value>,
    ) -> Outcome {
        if self.applying.is_set() {
            return Outcome::default();
        }
        let values = self.snapshot(surface);
        let event = self.envelope(kind, node, bind, value, values, facts);
        let Some(reply) = host.event(&self.id, &event) else {
            return Outcome::default();
        };
        self.apply(surface, &reply, kind == "activate")
    }

    pub fn apply(&mut self, surface: &dyn ViewSurface, reply: &Value, gesture: bool) -> Outcome {
        {
            let _held = self.applying.hold();
            if let Some(table) = reply.get("put").and_then(|put| put.as_object()) {
                let bound = bound_nodes(&self.document);
                for (node, value) in table {
                    surface.set_value(node, value);
                    if let Some(bind) = bound.get(node) {
                        self.state.state.insert(bind.clone(), value.clone());
                    }
                }
            }
            if let Some(table) = reply.get("set").and_then(|set| set.as_object()) {
                for (key, value) in table {
                    let Some((namespace, name)) = key.split_once('.') else {
                        continue;
                    };
                    let slot = match namespace {
                        "state" => &mut self.state.state,
                        "view" => &mut self.state.view,
                        "data" => &mut self.state.data,
                        _ => continue,
                    };
                    slot.insert(name.to_string(), value.clone());
                }
                surface.refresh(&self.state);
            }
        }
        Outcome {
            clipboard: gesture
                .then(|| reply.get("clipboard").and_then(|text| text.as_str()))
                .flatten()
                .map(str::to_string),
            redescribe: reply
                .get("redescribe")
                .and_then(|again| again.as_bool())
                .unwrap_or(false),
            close: reply
                .get("close")
                .and_then(|shut| shut.as_bool())
                .unwrap_or(false),
        }
    }

    /// Asks the plugin to describe the view again. `InPlace` means the state and
    /// the surface are already up to date; `Rebuilt` means `document` and
    /// `state` are new and the caller owes a fresh surface.
    pub fn redescribe(
        &mut self,
        surface: &dyn ViewSurface,
        host: &dyn ViewHost,
        facts: &HostFacts,
    ) -> Redescribed {
        let Some(fresh) = host.describe(&self.id, &context(facts, &self.argument)) else {
            return Redescribed::Nothing;
        };
        if self.document.shows_the_same_tree_as(&fresh) {
            self.state.data = fresh.data.clone();
            self.document = fresh;
            let _held = self.applying.hold();
            surface.refresh(&self.state);
            return Redescribed::InPlace;
        }
        self.state = seeded_state(&fresh, Some(&self.state), facts, &self.argument);
        self.document = fresh;
        self.pending.clear();
        Redescribed::Rebuilt
    }
}

/// A surface with no widgets behind it: the values arrive with the request and
/// whatever the plugin writes back is collected for the caller to forward.
#[derive(Default)]
pub struct ValueSurface {
    values: BTreeMap<String, Value>,
    touched: Vec<String>,
    written: std::cell::RefCell<BTreeMap<String, Value>>,
}

impl ValueSurface {
    pub fn new(values: BTreeMap<String, Value>, touched: Vec<String>) -> ValueSurface {
        ValueSurface {
            values,
            touched,
            written: std::cell::RefCell::new(BTreeMap::new()),
        }
    }

    /// What the plugin asked to be put into the widgets, keyed by node id.
    pub fn written(&self) -> BTreeMap<String, Value> {
        self.written.borrow().clone()
    }
}

impl ViewSurface for ValueSurface {
    fn values(&self) -> BTreeMap<String, Value> {
        self.values.clone()
    }

    fn touched(&self) -> Vec<String> {
        self.touched.clone()
    }

    fn set_value(&self, node: &str, value: &Value) {
        self.written
            .borrow_mut()
            .insert(node.to_string(), value.clone());
    }

    fn refresh(&self, _state: &State) {}
}

/// The views a remote frontend has open. Lives on the thread that owns the
/// plugins; a request carries the widget values in and takes the reply out.
#[derive(Default)]
pub struct Hub {
    open: BTreeMap<String, Session>,
}

impl Hub {
    pub fn is_open(&self, id: &str) -> bool {
        self.open.contains_key(id)
    }

    /// Describes the view and tells the plugin it is on screen.
    pub fn open(
        &mut self,
        id: &str,
        argument: &str,
        host: &dyn ViewHost,
        facts: &HostFacts,
    ) -> Option<Value> {
        let document = host.describe(id, &context(facts, argument))?;
        self.open
            .insert(id.to_string(), Session::open(id, document, argument, facts));
        let opened = self.event(
            id,
            &json!({ "type": "opened", "values": {}, "touched": [] }),
            host,
            facts,
        );
        opened.or_else(|| self.snapshot(id, Value::Null, None))
    }

    fn snapshot(&self, id: &str, put: Value, clipboard: Option<String>) -> Option<Value> {
        let session = self.open.get(id)?;
        Some(json!({
            "view": id,
            "document": session.document,
            "state": session.state,
            "put": put,
            "clipboard": clipboard,
        }))
    }

    /// Hands one event to the plugin and reports the view as it now stands.
    pub fn event(
        &mut self,
        id: &str,
        incoming: &Value,
        host: &dyn ViewHost,
        facts: &HostFacts,
    ) -> Option<Value> {
        if !self.open.contains_key(id) {
            return None;
        }
        let kind = incoming
            .get("type")
            .and_then(|kind| kind.as_str())
            .unwrap_or("change")
            .to_string();
        let node = incoming
            .get("node")
            .and_then(|node| node.as_str())
            .map(str::to_string);
        let bind = incoming
            .get("bind")
            .and_then(|bind| bind.as_str())
            .map(str::to_string);
        let value = incoming.get("value").cloned();
        let values = incoming
            .get("values")
            .and_then(|values| values.as_object())
            .map(|table| {
                table
                    .iter()
                    .map(|(key, held)| (key.clone(), held.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let touched = incoming
            .get("touched")
            .and_then(|touched| touched.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|bind| bind.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let surface = ValueSurface::new(values, touched);
        let outcome = {
            let session = self.open.get_mut(id)?;
            session.send(
                &surface,
                host,
                facts,
                &kind,
                node.as_deref(),
                bind.as_deref(),
                value,
            )
        };
        if outcome.redescribe {
            let session = self.open.get_mut(id)?;
            session.redescribe(&surface, host, facts);
        }
        self.snapshot(
            id,
            serde_json::to_value(surface.written()).unwrap_or(Value::Null),
            outcome.clipboard,
        )
    }

    /// Re-asks the plugin to describe the view, for a frontend that polls.
    pub fn redescribe(
        &mut self,
        id: &str,
        host: &dyn ViewHost,
        facts: &HostFacts,
    ) -> Option<Value> {
        let surface = ValueSurface::default();
        let session = self.open.get_mut(id)?;
        session.redescribe(&surface, host, facts);
        self.snapshot(id, Value::Null, None)
    }

    pub fn close(&mut self, id: &str) -> bool {
        self.open.remove(id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Widgets {
        values: RefCell<BTreeMap<String, Value>>,
        touched: RefCell<Vec<String>>,
        redraws: RefCell<usize>,
        written: RefCell<Vec<(String, Value)>>,
    }

    impl ViewSurface for Widgets {
        fn values(&self) -> BTreeMap<String, Value> {
            self.values.borrow().clone()
        }

        fn touched(&self) -> Vec<String> {
            self.touched.borrow().clone()
        }

        fn set_value(&self, node: &str, value: &Value) {
            self.written
                .borrow_mut()
                .push((node.to_string(), value.clone()));
        }

        fn refresh(&self, _state: &State) {
            *self.redraws.borrow_mut() += 1;
        }
    }

    struct Fixed {
        document: RefCell<Option<Document>>,
        reply: Value,
        seen: RefCell<Vec<Value>>,
    }

    impl ViewHost for Fixed {
        fn describe(&self, _id: &str, _context: &Value) -> Option<Document> {
            self.document.borrow().clone()
        }

        fn event(&self, _id: &str, event: &Value) -> Option<Value> {
            self.seen.borrow_mut().push(event.clone());
            Some(self.reply.clone())
        }
    }

    fn facts() -> HostFacts {
        HostFacts::new("tui", "uk", &["copy"])
    }

    fn document() -> Document {
        serde_json::from_value(json!({
            "schema": 1,
            "kind": "example",
            "fields": [
                { "bind": "host", "type": "text", "default": "localhost" },
                { "bind": "port", "type": "integer", "default": 21 },
                { "bind": "pass", "type": "text", "secret": true },
            ],
            "form": { "t": "column", "children": [
                { "t": "input", "id": "host", "bind": "host", "emit": "change", "debounce_ms": 250 },
                { "t": "input", "id": "pass", "bind": "pass" },
                { "t": "button", "id": "test", "intent": { "do": "emit", "node": "test" } },
            ]},
        }))
        .expect("a document")
    }

    #[test]
    fn the_host_it_runs_on_is_readable_from_the_document() {
        let session = Session::open("example", document(), "null", &facts());
        assert_eq!(session.state.lookup("host.kind"), Some(&json!("tui")));
        assert_eq!(session.state.lookup("host.locale"), Some(&json!("uk")));
        assert_eq!(session.state.lookup("host.can_copy"), Some(&json!(true)));
        assert_eq!(session.state.lookup("host.can_terminal"), None);
    }

    fn a_window() -> Document {
        serde_json::from_value(json!({
            "schema": 1,
            "fields": [],
            "form": { "t": "view", "surface": "window", "children": [
                { "t": "text", "id": "which", "text": "{arg.path}" },
            ]},
        }))
        .expect("a document")
    }

    #[test]
    fn what_the_view_was_opened_with_is_readable_from_the_document() {
        let session = Session::open(
            "torrent.cleanup",
            a_window(),
            r#"{"path":"/films/album.torrent"}"#,
            &facts(),
        );
        assert_eq!(
            ic_view::substitute("{arg.path}", &session.state),
            "/films/album.torrent",
            "the dialogue drew the placeholder instead of the file it is about"
        );
    }

    #[test]
    fn the_argument_outlives_a_rebuild_of_the_document() {
        let host = Fixed {
            document: RefCell::new(Some(a_window())),
            reply: json!({}),
            seen: RefCell::new(Vec::new()),
        };
        let mut session = Session::open(
            "torrent.cleanup",
            document(),
            r#"{"path":"/films/album.torrent"}"#,
            &facts(),
        );
        let widgets = Widgets::default();
        assert_eq!(
            session.redescribe(&widgets, &host, &facts()),
            Redescribed::Rebuilt
        );
        assert_eq!(
            ic_view::substitute("{arg.path}", &session.state),
            "/films/album.torrent"
        );
    }

    #[test]
    fn a_form_with_no_argument_has_the_namespace_empty_rather_than_absent() {
        let session = Session::over("example", document(), &State::default(), &facts());
        assert_eq!(session.argument, "null");
        assert_eq!(session.state.arg, Value::Null);
        assert_eq!(session.state.lookup("arg.path"), None);
        assert_eq!(ic_view::substitute("{arg.path}", &session.state), "");
    }

    #[test]
    fn declared_defaults_are_seeded_once_and_never_overwrite_the_carried_value() {
        let session = Session::open("example", document(), "null", &facts());
        assert_eq!(session.state.state.get("host"), Some(&json!("localhost")));
        assert_eq!(session.state.state.get("port"), Some(&json!(21)));

        let mut carried = State::default();
        carried.set_state("host", json!("ftp.example.org"));
        let again = seeded_state(&document(), Some(&carried), &facts(), "null");
        assert_eq!(again.state.get("host"), Some(&json!("ftp.example.org")));
        assert_eq!(again.state.get("port"), Some(&json!(21)));
    }

    #[test]
    fn a_secret_is_kept_out_of_what_the_plugin_is_shown() {
        let mut session = Session::open("example", document(), "null", &facts());
        let widgets = Widgets::default();
        widgets
            .values
            .borrow_mut()
            .insert("host".to_string(), json!("ftp.example.org"));
        widgets
            .values
            .borrow_mut()
            .insert("pass".to_string(), json!("hunter2"));

        let shown = session.snapshot(&widgets);
        assert_eq!(shown["host"], json!("ftp.example.org"));
        assert!(
            shown.get("pass").is_none(),
            "a secret never leaves the host"
        );
        assert!(session.state.state.get("pass").is_none());
    }

    #[test]
    fn the_last_keystroke_of_a_debounced_field_wins() {
        let mut session = Session::open("example", document(), "null", &facts());
        let first = session.arm("host");
        let second = session.arm("host");
        assert!(!session.current("host", first), "the stale turn is dropped");
        assert!(session.current("host", second));
    }

    #[test]
    fn a_plugin_that_did_what_was_asked_can_shut_the_dialogue_behind_it() {
        let host = Fixed {
            document: RefCell::new(None),
            reply: json!({ "set": { "data.status": "gone" }, "close": true }),
            seen: RefCell::new(Vec::new()),
        };
        let mut session = Session::open("example", document(), "null", &facts());
        let outcome = session.send(
            &Widgets::default(),
            &host,
            &facts(),
            "activate",
            Some("wipe"),
            None,
            None,
        );
        assert!(outcome.close);
        assert_eq!(
            session.state.data.get("status"),
            Some(&json!("gone")),
            "what the reply carried still lands before the view goes"
        );
    }

    #[test]
    fn a_reply_that_says_nothing_about_closing_leaves_the_dialogue_open() {
        let host = Fixed {
            document: RefCell::new(None),
            reply: json!({ "clipboard": "copied" }),
            seen: RefCell::new(Vec::new()),
        };
        let mut session = Session::open("example", document(), "null", &facts());
        let outcome = session.send(
            &Widgets::default(),
            &host,
            &facts(),
            "activate",
            Some("test"),
            None,
            None,
        );
        assert!(!outcome.close);
    }

    #[test]
    fn a_reply_is_written_back_without_being_echoed_to_the_plugin() {
        let host = Fixed {
            document: RefCell::new(None),
            reply: json!({
                "put": { "host": "filled.example.org" },
                "set": { "data.status": "ready", "view.mode": "edit" },
                "clipboard": "copied",
                "redescribe": true,
            }),
            seen: RefCell::new(Vec::new()),
        };
        let mut session = Session::open("example", document(), "null", &facts());
        let widgets = Widgets::default();

        let outcome = session.send(
            &widgets,
            &host,
            &facts(),
            "activate",
            Some("test"),
            None,
            None,
        );
        assert_eq!(outcome.clipboard.as_deref(), Some("copied"));
        assert!(outcome.redescribe);
        assert_eq!(
            session.state.data.get("status"),
            Some(&json!("ready")),
            "set lands in the namespace it names"
        );
        assert_eq!(session.state.view.get("mode"), Some(&json!("edit")));
        assert_eq!(
            widgets.written.borrow().as_slice(),
            &[("host".to_string(), json!("filled.example.org"))]
        );

        let seen = host.seen.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0]["type"], json!("activate"));
        assert_eq!(seen[0]["gesture"], json!(true));
        assert_eq!(seen[0]["node"], json!("test"));
        assert_eq!(seen[0]["host"]["kind"], json!("tui"));
        assert!(!session.applying().is_set(), "the guard is released");
    }

    #[test]
    fn a_clipboard_offer_is_ignored_when_no_gesture_asked_for_it() {
        let host = Fixed {
            document: RefCell::new(None),
            reply: json!({ "clipboard": "copied" }),
            seen: RefCell::new(Vec::new()),
        };
        let mut session = Session::open("example", document(), "null", &facts());
        let outcome = session.send(
            &Widgets::default(),
            &host,
            &facts(),
            "change",
            Some("host"),
            Some("host"),
            Some(json!("x")),
        );
        assert_eq!(outcome.clipboard, None);
    }

    #[test]
    fn the_watch_list_separates_values_from_gestures() {
        let found = watches(&document());
        assert_eq!(
            found,
            vec![
                Watch {
                    node: "host".to_string(),
                    debounce_ms: 250,
                    on_value: true
                },
                Watch {
                    node: "test".to_string(),
                    debounce_ms: 0,
                    on_value: false
                },
            ]
        );
    }

    #[test]
    fn the_same_tree_is_refreshed_and_a_different_one_is_rebuilt() {
        let host = Fixed {
            document: RefCell::new(Some(document())),
            reply: Value::Null,
            seen: RefCell::new(Vec::new()),
        };
        let mut session = Session::open("example", document(), "null", &facts());
        session.state.set_state("host", json!("kept.example.org"));
        let widgets = Widgets::default();

        assert_eq!(
            session.redescribe(&widgets, &host, &facts()),
            Redescribed::InPlace
        );
        assert_eq!(*widgets.redraws.borrow(), 1);

        let mut grown: Value = serde_json::to_value(document()).expect("serialises");
        grown["form"]["children"]
            .as_array_mut()
            .expect("children")
            .push(json!({ "t": "text", "id": "note", "text": "extra" }));
        *host.document.borrow_mut() = Some(serde_json::from_value(grown).expect("a document"));

        assert_eq!(
            session.redescribe(&widgets, &host, &facts()),
            Redescribed::Rebuilt
        );
        assert_eq!(
            session.state.state.get("host"),
            Some(&json!("kept.example.org")),
            "what was typed survives a rebuild"
        );
        assert_eq!(
            *widgets.redraws.borrow(),
            1,
            "a rebuild does not redraw the old surface"
        );
    }

    #[test]
    fn a_hub_opens_a_view_and_reports_the_document_and_the_state() {
        let host = Fixed {
            document: RefCell::new(Some(document())),
            reply: json!({ "set": { "data.status": "ready" } }),
            seen: RefCell::new(Vec::new()),
        };
        let mut hub = Hub::default();
        let opened = hub
            .open("example", "null", &host, &facts())
            .expect("the plugin describes it");
        assert!(hub.is_open("example"));
        assert_eq!(opened["view"], json!("example"));
        assert_eq!(opened["document"]["kind"], json!("example"));
        assert_eq!(
            opened["state"]["state"]["host"],
            json!("localhost"),
            "the declared default is seeded"
        );
        assert_eq!(
            opened["state"]["host"]["kind"],
            json!("tui"),
            "the view can read which frontend it is on"
        );
        assert_eq!(
            opened["state"]["data"]["status"],
            json!("ready"),
            "what the opened event set is already visible"
        );
        assert_eq!(
            host.seen.borrow()[0]["type"],
            json!("opened"),
            "the plugin is told the view is on screen"
        );
    }

    #[test]
    fn a_hub_carries_the_values_of_the_request_and_returns_what_the_plugin_wrote() {
        let host = Fixed {
            document: RefCell::new(Some(document())),
            reply: json!({ "put": { "host": "filled.example.org" }, "clipboard": "copied" }),
            seen: RefCell::new(Vec::new()),
        };
        let mut hub = Hub::default();
        hub.open("example", "null", &host, &facts()).expect("opens");

        let answered = hub
            .event(
                "example",
                &json!({
                    "type": "activate",
                    "node": "test",
                    "values": { "host": "typed.example.org", "pass": "hunter2" },
                    "touched": ["host"],
                }),
                &host,
                &facts(),
            )
            .expect("the view is open");
        assert_eq!(answered["put"]["host"], json!("filled.example.org"));
        assert_eq!(answered["clipboard"], json!("copied"));
        assert_eq!(
            answered["state"]["state"]["port"],
            json!(21),
            "what the request carried is the state the predicates read"
        );
        assert_eq!(
            answered["state"]["state"]["host"],
            json!("filled.example.org"),
            "and a bind the reply put into is the value the plugin wrote, not the older one"
        );
        assert!(
            answered["state"]["state"].get("pass").is_none(),
            "a secret the request carried is not kept in the state"
        );

        let sent = host.seen.borrow();
        let last = sent.last().expect("an event");
        assert_eq!(last["values"]["host"], json!("typed.example.org"));
        assert!(
            last["values"].get("pass").is_none(),
            "and the plugin is never shown it"
        );
    }

    #[test]
    fn a_value_the_plugin_puts_into_a_widget_survives_the_rebuild_a_redescribe_causes() {
        let host = Fixed {
            document: RefCell::new(Some(document())),
            reply: json!({ "put": { "host": "filled.example.org" } }),
            seen: RefCell::new(Vec::new()),
        };
        let mut session = Session::open("example", document(), "null", &facts());
        let surface = ValueSurface::new(BTreeMap::new(), Vec::new());
        session.apply(&surface, &host.reply, false);
        let mut grown = document();
        grown
            .form
            .children
            .push(serde_json::from_value(json!({ "t": "label", "id": "note" })).expect("a node"));
        *host.document.borrow_mut() = Some(grown);
        assert_eq!(
            session.redescribe(&surface, &host, &facts()),
            Redescribed::Rebuilt
        );
        assert_eq!(
            session.state.state["host"],
            json!("filled.example.org"),
            "a rebuild seeds from the state, so the put has to be in it"
        );
    }

    #[test]
    fn a_hub_answers_nothing_for_a_view_that_was_never_opened() {
        let host = Fixed {
            document: RefCell::new(Some(document())),
            reply: Value::Null,
            seen: RefCell::new(Vec::new()),
        };
        let mut hub = Hub::default();
        assert!(hub
            .event("missing", &json!({ "type": "change" }), &host, &facts())
            .is_none());
        assert!(!hub.close("missing"));
        hub.open("example", "null", &host, &facts()).expect("opens");
        assert!(hub.close("example"));
        assert!(!hub.is_open("example"));
    }

    #[test]
    fn a_plugin_that_says_nothing_leaves_the_view_alone() {
        let host = Fixed {
            document: RefCell::new(None),
            reply: Value::Null,
            seen: RefCell::new(Vec::new()),
        };
        let mut session = Session::open("example", document(), "null", &facts());
        let widgets = Widgets::default();
        assert_eq!(
            session.redescribe(&widgets, &host, &facts()),
            Redescribed::Nothing
        );
        assert_eq!(*widgets.redraws.borrow(), 0);
    }
}
