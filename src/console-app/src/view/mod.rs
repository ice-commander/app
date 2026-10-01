pub(crate) mod layout;
mod paint;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ic_plugin_host::{Kinds, Views};
use ic_view::{
    as_text, choice_index, is_truthy, visible, Document, InputVariant, Intent, Node, NodeKind,
    State,
};
use ic_view_session::{HostFacts, Redescribed, Session, ViewHost, ViewSurface};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;
use serde_json::Value;

use crate::util::{byte_at, centered_rect};
use layout::{place, wanted_size, Placed};
use paint::{draw_actions, draw_placed, Surface};

#[derive(Clone, Copy, Default)]
pub(crate) struct Caret {
    pub(crate) cx: usize,
    pub(crate) cy: usize,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Spot {
    Node(String),
    Action(String),
}

#[derive(Default)]
pub(crate) struct Widgets {
    binds: BTreeMap<String, String>,
    secrets: BTreeSet<String>,
    values: RefCell<BTreeMap<String, Value>>,
    touched: RefCell<BTreeSet<String>>,
    carets: RefCell<BTreeMap<String, Caret>>,
    answered: RefCell<BTreeSet<String>>,
}

impl Widgets {
    fn for_document(document: &Document, state: &State) -> Widgets {
        let mut binds = BTreeMap::new();
        document.form.walk(&mut |node| {
            if let (Some(id), Some(bind)) = (node.id.as_ref(), node.bind.as_ref()) {
                binds.insert(id.clone(), bind.clone());
            }
        });
        let secrets: BTreeSet<String> = document
            .fields
            .iter()
            .filter(|field| field.secret)
            .map(|field| field.bind.clone())
            .collect();
        let widgets = Widgets {
            binds,
            secrets,
            ..Widgets::default()
        };
        widgets.refresh(state);
        widgets
    }

    pub(crate) fn value_of(&self, bind: &str) -> Option<Value> {
        self.values.borrow().get(bind).cloned()
    }

    pub(crate) fn caret_of(&self, node: &str) -> Caret {
        self.carets.borrow().get(node).copied().unwrap_or_default()
    }

    fn put(&self, bind: &str, value: Value) {
        self.values.borrow_mut().insert(bind.to_string(), value);
        self.touched.borrow_mut().insert(bind.to_string());
    }

    /// Takes over from the widgets of a form that was just rebuilt. The state
    /// seeds everything it knows, so only what it cannot hold is carried: a
    /// secret is kept here and nowhere else.
    fn carry(&self, previous: &Widgets) {
        let held: BTreeSet<&String> = self.binds.values().collect();
        for (bind, value) in previous.values.borrow().iter() {
            if !held.contains(bind) {
                continue;
            }
            self.values
                .borrow_mut()
                .entry(bind.clone())
                .or_insert_with(|| value.clone());
        }
        for bind in previous.touched.borrow().iter() {
            if held.contains(bind) {
                self.touched.borrow_mut().insert(bind.clone());
            }
        }
    }
}

impl ViewSurface for Widgets {
    fn values(&self) -> BTreeMap<String, Value> {
        self.values.borrow().clone()
    }

    fn touched(&self) -> Vec<String> {
        self.touched.borrow().iter().cloned().collect()
    }

    fn set_value(&self, node: &str, value: &Value) {
        let Some(bind) = self.binds.get(node).cloned() else {
            return;
        };
        self.values.borrow_mut().insert(bind.clone(), value.clone());
        self.answered.borrow_mut().insert(bind);
        self.carets.borrow_mut().remove(node);
    }

    /// Follows the state into the widgets, so what the plugin sets is what the
    /// next frame paints and what the form commits. Nothing written here is typing.
    fn refresh(&self, state: &State) {
        let answered = std::mem::take(&mut *self.answered.borrow_mut());
        let mut written: BTreeSet<String> = BTreeSet::new();
        for bind in self.binds.values() {
            // What the same answer put into a widget is newer than the state it read.
            if answered.contains(bind) {
                continue;
            }
            // The state carries the secret the record already holds; the box stays empty.
            if self.secrets.contains(bind) && state.stored_secrets.contains(bind) {
                continue;
            }
            let Some(fresh) = state.state.get(bind) else {
                continue;
            };
            let held = self.values.borrow().get(bind).cloned();
            if held.as_ref() == Some(fresh) {
                continue;
            }
            self.values.borrow_mut().insert(bind.clone(), fresh.clone());
            written.insert(bind.clone());
        }
        if written.is_empty() {
            return;
        }
        self.carets.borrow_mut().retain(|node, _| {
            !self
                .binds
                .get(node)
                .is_some_and(|bind| written.contains(bind))
        });
    }
}

struct Waiting {
    due: Instant,
    node: String,
    bind: String,
    value: Value,
    turn: u64,
}

pub(crate) enum Nested {
    Options {
        node: String,
        bind: String,
        cursor: usize,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Purpose {
    Plugin,
    Viewer {
        viewer: String,
        instance: u64,
        name: String,
    },
    Connection {
        kind: String,
        editing: Option<usize>,
    },
}

/// Both outlive the plugin's window and go only after it.
struct Held {
    source: ic_plugin_api::IcFsSource,
    _staged: fm_core::host_fs::Staged,
}

pub(crate) enum Step {
    Stay,
    Close,
    Save { connect: bool },
    Revert,
    Pick { node: String, folder: bool },
}

/// The connection documents declare no actions, so the host offers its own.
const OWN: &[(&str, &str, &str)] = &[
    ("@save", "conn_manager.save_connection", "Save"),
    ("@connect", "conn_manager.connect_btn", "Connect"),
    ("@cancel", "conn_manager.cancel_edit", "Cancel"),
];

pub(crate) struct ViewPane {
    pub(crate) session: Session,
    pub(crate) widgets: Widgets,
    pub(crate) purpose: Purpose,
    focus: usize,
    hits: Vec<(Rect, Spot)>,
    nested: Option<Nested>,
    waiting: Vec<Waiting>,
    refresh_at: Option<Instant>,
    showing: Option<Held>,
    told: bool,
}

fn node_by_id<'a>(node: &'a Node, wanted: &str) -> Option<&'a Node> {
    if node.id.as_deref() == Some(wanted) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| node_by_id(child, wanted))
}

fn node_by_accel<'a>(node: &'a Node, pressed: &str, state: &State) -> Option<&'a Node> {
    let claims = node
        .accel
        .as_deref()
        .is_some_and(|accel| accel.to_lowercase() == pressed);
    if claims && visible(node.visible.as_ref(), state) && visible(node.sensitive.as_ref(), state) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| node_by_accel(child, pressed, state))
}

/// Spelled as a document writes its `accel`, so one document serves the desktop and this.
fn pressed_as_written(key: KeyEvent) -> Option<String> {
    let name = match key.code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(letter) => letter.to_lowercase().to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "page_up".to_string(),
        KeyCode::PageDown => "page_down".to_string(),
        KeyCode::Enter => "return".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::Esc => "escape".to_string(),
        KeyCode::F(number) => format!("f{number}"),
        _ => return None,
    };
    let mut said = String::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        said.push_str("ctrl+");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        said.push_str("alt+");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        said.push_str("shift+");
    }
    said.push_str(&name);
    Some(said)
}

/// Never zero: that is what a host with nothing open answers.
fn next_instance() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn translate(key: &str) -> Option<String> {
    connection_form::translate_optional(key)
}

pub(crate) fn facts() -> HostFacts {
    HostFacts::new(
        ic_plugin_api::IC_HOST_CONSOLE,
        ic_i18n::current_lang(),
        &["copy"],
    )
}

fn next_refresh(document: &Document) -> Option<Instant> {
    document
        .form
        .refresh_ms
        .filter(|every| *every > 0)
        .map(|every| Instant::now() + Duration::from_millis(u64::from(every)))
}

impl ViewPane {
    pub(crate) fn open(id: &str, argument: &str) -> Option<ViewPane> {
        let facts = facts();
        let document =
            ic_plugin_host::describe_view(id, &ic_view_session::context(&facts, argument))?;
        let session = Session::open(id, document, argument, &facts);
        let widgets = Widgets::for_document(&session.document, &session.state);
        let refresh_at = next_refresh(&session.document);
        let mut pane = ViewPane {
            session,
            widgets,
            purpose: Purpose::Plugin,
            focus: 0,
            hits: Vec::new(),
            nested: None,
            waiting: Vec::new(),
            refresh_at,
            showing: None,
            told: false,
        };
        pane.deliver("opened", None, None, None);
        Some(pane)
    }

    /// The plugin is handed the filesystem and the path, never the bytes.
    pub(crate) fn viewing(
        viewer: &str,
        name: &str,
        source: ic_plugin_api::IcFsSource,
        staged: fm_core::host_fs::Staged,
    ) -> Option<ViewPane> {
        let instance = next_instance();
        if !ic_plugin_host::open_viewer(viewer, instance, source, &staged.name) {
            fm_core::host_fs::HostSource::close(source);
            return None;
        }
        let facts = facts();
        let context = ic_view_session::context(&facts, "null");
        let Some(document) = ic_plugin_host::describe_viewer(viewer, instance, &context) else {
            ic_plugin_host::viewer_closed(viewer, instance);
            fm_core::host_fs::HostSource::close(source);
            return None;
        };
        let session = Session::open(viewer, document, "null", &facts);
        let widgets = Widgets::for_document(&session.document, &session.state);
        let refresh_at = next_refresh(&session.document);
        let mut pane = ViewPane {
            session,
            widgets,
            purpose: Purpose::Viewer {
                viewer: viewer.to_string(),
                instance,
                name: name.to_string(),
            },
            focus: 0,
            hits: Vec::new(),
            nested: None,
            waiting: Vec::new(),
            refresh_at,
            showing: Some(Held {
                source,
                _staged: staged,
            }),
            told: false,
        };
        pane.deliver("opened", None, None, None);
        Some(pane)
    }

    /// The connection editor. A kind that answers events is driven like a plugin
    /// window; for one that does not, no event ever leaves the host and the form
    /// is filled in and committed on its own.
    pub(crate) fn for_connection(
        kind: &str,
        record: &connection_form::Connection,
        editing: Option<usize>,
        stored_secrets: &[String],
    ) -> Option<ViewPane> {
        let facts = facts();
        let document = ic_plugin_host::connection_document(kind)?;
        let mut carried = connection_form::plugin_state_from_record(&document, record);
        carried.set_view(
            "mode",
            Value::String(if editing.is_some() { "edit" } else { "new" }.to_string()),
        );
        for bind in stored_secrets {
            carried.stored_secrets.insert(bind.clone());
        }
        let session = Session::over(kind, document, &carried, &facts);
        let widgets = Widgets::for_document(&session.document, &session.state);
        let refresh_at = ic_plugin_host::connection_takes_events(kind)
            .then(|| next_refresh(&session.document))
            .flatten();
        let mut pane = ViewPane {
            session,
            widgets,
            purpose: Purpose::Connection {
                kind: kind.to_string(),
                editing,
            },
            focus: 0,
            hits: Vec::new(),
            nested: None,
            waiting: Vec::new(),
            refresh_at,
            showing: None,
            told: false,
        };
        pane.deliver("opened", None, None, None);
        Some(pane)
    }

    /// What the pane talks to, if anything: a window always has its plugin behind
    /// it, a connection form only when the kind offers `on_event`.
    fn plugin(&self) -> Option<Box<dyn ViewHost>> {
        match &self.purpose {
            Purpose::Plugin => Some(Box::new(Views)),
            Purpose::Viewer {
                viewer, instance, ..
            } => Some(Box::new(ic_plugin_host::Viewer {
                id: viewer.clone(),
                instance: *instance,
            })),
            Purpose::Connection { kind, .. } => ic_plugin_host::connection_takes_events(kind)
                .then(|| Box::new(Kinds) as Box<dyn ViewHost>),
        }
    }

    pub(crate) fn is_editing_kind(&self, kind: &str) -> bool {
        matches!(&self.purpose, Purpose::Connection { kind: held, .. }
            if held.eq_ignore_ascii_case(kind))
    }

    fn editing_a_connection(&self) -> bool {
        matches!(self.purpose, Purpose::Connection { .. })
    }

    /// What the form has committed so far, or the reason it cannot be saved yet.
    pub(crate) fn committed(&self) -> Result<connection_form::PluginFormValues, Vec<String>> {
        let values = connection_form::values_to_commit(&self.session.state, self.widgets.values());
        match connection_form::check_plugin_form(
            &self.session.document,
            values,
            self.widgets.touched(),
            &self.session.state.stored_secrets,
        ) {
            connection_form::FormOutcome::Ready(form) => Ok(form),
            connection_form::FormOutcome::Incomplete(why) => Err(why),
        }
    }

    fn ring(&self) -> Vec<Spot> {
        let state = &self.session.state;
        let mut ring = Vec::new();
        self.session.document.form.walk(&mut |node| {
            let Some(id) = node.id.as_ref() else {
                return;
            };
            let takes = matches!(
                node.kind(),
                NodeKind::Input | NodeKind::Switch | NodeKind::Choice | NodeKind::Button
            );
            if takes
                && !node.read_only
                && visible(node.visible.as_ref(), state)
                && visible(node.sensitive.as_ref(), state)
            {
                ring.push(Spot::Node(id.clone()));
            }
        });
        for action in &self.session.document.actions {
            if visible(action.visible.as_ref(), state) && visible(action.sensitive.as_ref(), state)
            {
                ring.push(Spot::Action(action.id.clone()));
            }
        }
        for (id, _, _) in self.own_actions() {
            ring.push(Spot::Action(id.to_string()));
        }
        ring
    }

    fn own_actions(&self) -> &'static [(&'static str, &'static str, &'static str)] {
        match self.purpose {
            Purpose::Plugin | Purpose::Viewer { .. } => &[],
            Purpose::Connection { .. } => OWN,
        }
    }

    fn spot(&self) -> Option<Spot> {
        let ring = self.ring();
        if ring.is_empty() {
            return None;
        }
        ring.get(self.focus % ring.len()).cloned()
    }

    fn step_focus(&mut self, by: isize) {
        let ring = self.ring();
        if ring.is_empty() {
            return;
        }
        let len = ring.len() as isize;
        let at = (self.focus % ring.len()) as isize;
        self.focus = (((at + by) % len + len) % len) as usize;
    }

    fn focused_node(&self) -> Option<&Node> {
        match self.spot()? {
            Spot::Node(id) => node_by_id(&self.session.document.form, &id),
            Spot::Action(_) => None,
        }
    }

    fn deliver(
        &mut self,
        kind: &str,
        node: Option<&str>,
        bind: Option<&str>,
        value: Option<Value>,
    ) {
        let Some(plugin) = self.plugin() else {
            return;
        };
        let facts = facts();
        let ViewPane {
            session, widgets, ..
        } = self;
        let outcome = session.send(widgets, plugin.as_ref(), &facts, kind, node, bind, value);
        if let Some(text) = outcome.clipboard {
            copy(&text);
        }
        if outcome.redescribe {
            self.redescribe();
        }
    }

    /// A plugin asking for its own window again; a connection editor that shares
    /// the id is not it.
    pub(crate) fn redescribe_now(&mut self) {
        if matches!(self.purpose, Purpose::Plugin | Purpose::Viewer { .. }) {
            self.redescribe();
        }
    }

    fn redescribe(&mut self) {
        let Some(plugin) = self.plugin() else {
            return;
        };
        let facts = facts();
        let ViewPane {
            session, widgets, ..
        } = self;
        if session.redescribe(widgets, plugin.as_ref(), &facts) == Redescribed::Rebuilt {
            let seeded = Widgets::for_document(&session.document, &session.state);
            seeded.carry(widgets);
            *widgets = seeded;
            self.focus = 0;
            self.nested = None;
            self.waiting.clear();
        }
        self.refresh_at = next_refresh(&self.session.document);
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.waiting
            .iter()
            .map(|held| held.due)
            .chain(self.refresh_at)
            .min()
    }

    pub(crate) fn on_timer(&mut self) {
        let now = Instant::now();
        let due: Vec<Waiting> = {
            let (ready, keep) = std::mem::take(&mut self.waiting)
                .into_iter()
                .partition(|held| held.due <= now);
            self.waiting = keep;
            ready
        };
        for held in due {
            if !self.session.current(&held.node, held.turn) {
                continue;
            }
            self.deliver(
                "change",
                Some(&held.node),
                Some(&held.bind),
                Some(held.value.clone()),
            );
        }
        if self.refresh_at.is_some_and(|at| at <= now) {
            self.redescribe();
        }
    }

    fn announce(&mut self, node: &str, bind: &str, value: Value, debounce: u32) {
        let turn = self.session.arm(node);
        if debounce == 0 {
            self.deliver("change", Some(node), Some(bind), Some(value));
            return;
        }
        self.waiting.push(Waiting {
            due: Instant::now() + Duration::from_millis(u64::from(debounce)),
            node: node.to_string(),
            bind: bind.to_string(),
            value,
            turn,
        });
    }

    fn edited(&mut self, id: &str, bind: &str, value: Value) {
        self.widgets.put(bind, value.clone());
        let secret = self
            .session
            .document
            .field(bind)
            .map(|field| field.secret)
            .unwrap_or(false);
        if self.editing_a_connection() || !secret {
            self.session.state.set_state(bind, value.clone());
            self.session.state.touched.insert(bind.to_string());
        }
        let Some(node) = node_by_id(&self.session.document.form, id) else {
            return;
        };
        if node.emit != ic_view::Emit::Change {
            return;
        }
        let debounce = node.debounce_ms.unwrap_or(0);
        self.announce(id, bind, value, debounce);
    }

    fn typed(&mut self, key: KeyEvent) -> bool {
        let Some(node) = self.focused_node() else {
            return false;
        };
        if node.kind() != NodeKind::Input {
            return false;
        }
        let (id, bind) = match (node.id.clone(), node.bind.clone()) {
            (Some(id), Some(bind)) => (id, bind),
            _ => return false,
        };
        let variant = node.variant;
        let multiline = variant == InputVariant::Multiline;
        let mut lines: Vec<String> = as_text(&self.widgets.value_of(&bind).unwrap_or(Value::Null))
            .split('\n')
            .map(str::to_string)
            .collect();
        let mut caret = self.widgets.caret_of(&id);
        caret.cy = caret.cy.min(lines.len().saturating_sub(1));
        caret.cx = caret.cx.min(lines[caret.cy].chars().count());
        let mut changed = true;
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if variant == InputVariant::Integer && !(c.is_ascii_digit() || c == '-') {
                    return true;
                }
                let at = byte_at(&lines[caret.cy], caret.cx);
                lines[caret.cy].insert(at, c);
                caret.cx += 1;
            }
            KeyCode::Backspace => {
                if caret.cx > 0 {
                    let line = &lines[caret.cy];
                    let (from, to) = (byte_at(line, caret.cx - 1), byte_at(line, caret.cx));
                    lines[caret.cy].replace_range(from..to, "");
                    caret.cx -= 1;
                } else if multiline && caret.cy > 0 {
                    let tail = lines.remove(caret.cy);
                    caret.cy -= 1;
                    caret.cx = lines[caret.cy].chars().count();
                    lines[caret.cy].push_str(&tail);
                } else {
                    changed = false;
                }
            }
            KeyCode::Delete => {
                let length = lines[caret.cy].chars().count();
                if caret.cx < length {
                    let line = &lines[caret.cy];
                    let (from, to) = (byte_at(line, caret.cx), byte_at(line, caret.cx + 1));
                    lines[caret.cy].replace_range(from..to, "");
                } else if multiline && caret.cy + 1 < lines.len() {
                    let tail = lines.remove(caret.cy + 1);
                    lines[caret.cy].push_str(&tail);
                } else {
                    changed = false;
                }
            }
            KeyCode::Enter if multiline => {
                let at = byte_at(&lines[caret.cy], caret.cx);
                let tail = lines[caret.cy].split_off(at);
                lines.insert(caret.cy + 1, tail);
                caret.cy += 1;
                caret.cx = 0;
            }
            KeyCode::Left => {
                changed = false;
                if caret.cx > 0 {
                    caret.cx -= 1;
                } else if multiline && caret.cy > 0 {
                    caret.cy -= 1;
                    caret.cx = lines[caret.cy].chars().count();
                }
            }
            KeyCode::Right => {
                changed = false;
                if caret.cx < lines[caret.cy].chars().count() {
                    caret.cx += 1;
                } else if multiline && caret.cy + 1 < lines.len() {
                    caret.cy += 1;
                    caret.cx = 0;
                }
            }
            KeyCode::Home => {
                changed = false;
                caret.cx = 0;
            }
            KeyCode::End => {
                changed = false;
                caret.cx = lines[caret.cy].chars().count();
            }
            KeyCode::Up if multiline && caret.cy > 0 => {
                changed = false;
                caret.cy -= 1;
                caret.cx = caret.cx.min(lines[caret.cy].chars().count());
            }
            KeyCode::Down if multiline && caret.cy + 1 < lines.len() => {
                changed = false;
                caret.cy += 1;
                caret.cx = caret.cx.min(lines[caret.cy].chars().count());
            }
            _ => return false,
        }
        self.widgets.carets.borrow_mut().insert(id.clone(), caret);
        if changed {
            self.edited(&id, &bind, Value::String(lines.join("\n")));
        }
        true
    }

    pub(crate) fn on_key(&mut self, key: KeyEvent) -> Step {
        if let Some(Nested::Options { node, bind, cursor }) = self.nested.as_mut() {
            let node = node.clone();
            let bind = bind.clone();
            let options = node_by_id(&self.session.document.form, &node)
                .map(|found| found.options.clone())
                .unwrap_or_default();
            match key.code {
                KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Down => {
                    if *cursor + 1 < options.len() {
                        *cursor += 1;
                    }
                }
                KeyCode::Home => *cursor = 0,
                KeyCode::End => *cursor = options.len().saturating_sub(1),
                KeyCode::Esc => self.nested = None,
                KeyCode::Enter => {
                    let at = *cursor;
                    self.nested = None;
                    if let Some(option) = options.get(at) {
                        self.edited(&node, &bind, option.value.clone());
                    }
                }
                _ => {}
            }
            return Step::Stay;
        }
        if self.typed(key) {
            return Step::Stay;
        }
        if let Some(step) = self.accelerated(key) {
            return step;
        }
        match key.code {
            KeyCode::Esc => return Step::Close,
            KeyCode::Tab | KeyCode::Down => self.step_focus(1),
            KeyCode::BackTab | KeyCode::Up => self.step_focus(-1),
            KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right => {
                return self.act(key.code)
            }
            _ => {}
        }
        Step::Stay
    }

    /// Reached only after whatever is being typed into has had the key.
    fn accelerated(&mut self, key: KeyEvent) -> Option<Step> {
        let pressed = pressed_as_written(key)?;
        let found = node_by_accel(&self.session.document.form, &pressed, &self.session.state).map(
            |found| {
                let resolved = found
                    .intent
                    .as_ref()
                    .and_then(|intent| ic_view::resolve(intent, &self.session.state));
                (found.id.clone(), resolved)
            },
        );
        if let Some((id, resolved)) = found {
            return Some(self.follow(resolved, id.as_deref()));
        }
        let node = self
            .session
            .document
            .keys
            .iter()
            .find(|held| held.accel.to_lowercase() == pressed)?
            .node
            .clone();
        self.deliver(ic_plugin_api::IC_EVENT_ACTIVATE, Some(&node), None, None);
        Some(Step::Stay)
    }

    fn act(&mut self, code: KeyCode) -> Step {
        let Some(spot) = self.spot() else {
            return Step::Stay;
        };
        let id = match &spot {
            Spot::Action(id) => {
                if code != KeyCode::Enter && code != KeyCode::Char(' ') {
                    return Step::Stay;
                }
                match id.as_str() {
                    "@save" => return Step::Save { connect: false },
                    "@connect" => return Step::Save { connect: true },
                    "@cancel" => return Step::Close,
                    _ => {}
                }
                let action = self
                    .session
                    .document
                    .actions
                    .iter()
                    .find(|action| action.id == *id)
                    .cloned();
                let resolved = action
                    .and_then(|action| action.intent)
                    .and_then(|intent| ic_view::resolve(&intent, &self.session.state));
                return self.follow(resolved, None);
            }
            Spot::Node(id) => id.clone(),
        };
        let Some(node) = node_by_id(&self.session.document.form, &id) else {
            return Step::Stay;
        };
        let bind = node.bind.clone();
        match node.kind() {
            NodeKind::Switch => {
                if code != KeyCode::Char(' ') && code != KeyCode::Enter {
                    return Step::Stay;
                }
                let Some(bind) = bind else {
                    return Step::Stay;
                };
                let now = !is_truthy(&self.widgets.value_of(&bind).unwrap_or(Value::Null));
                self.edited(&id, &bind, Value::Bool(now));
            }
            NodeKind::Choice => {
                let Some(bind) = bind else {
                    return Step::Stay;
                };
                let options = node.options.clone();
                if options.is_empty() {
                    return Step::Stay;
                }
                let current = self.widgets.value_of(&bind).unwrap_or(Value::Null);
                let at = choice_index(node, &current).unwrap_or(0);
                match code {
                    KeyCode::Left => {
                        let next = if at == 0 { options.len() - 1 } else { at - 1 };
                        self.edited(&id, &bind, options[next].value.clone());
                    }
                    KeyCode::Right => {
                        let next = (at + 1) % options.len();
                        self.edited(&id, &bind, options[next].value.clone());
                    }
                    _ => {
                        self.nested = Some(Nested::Options {
                            node: id,
                            bind,
                            cursor: at,
                        })
                    }
                }
            }
            NodeKind::Button => {
                if code != KeyCode::Enter && code != KeyCode::Char(' ') {
                    return Step::Stay;
                }
                let resolved = node
                    .intent
                    .as_ref()
                    .and_then(|intent| ic_view::resolve(intent, &self.session.state));
                return self.follow(resolved, Some(&id));
            }
            _ => {}
        }
        Step::Stay
    }

    fn follow(&mut self, intent: Option<Intent>, fallback: Option<&str>) -> Step {
        match intent {
            Some(Intent::Emit { node }) => {
                self.deliver("activate", Some(&node), None, None);
                Step::Stay
            }
            Some(Intent::Close) => Step::Close,
            Some(Intent::Submit) => Step::Save { connect: false },
            Some(Intent::Connect) => Step::Save { connect: true },
            Some(Intent::Revert) => Step::Revert,
            Some(Intent::Set { keys }) => {
                for (key, value) in keys {
                    let Some((namespace, name)) = key.split_once('.') else {
                        continue;
                    };
                    match namespace {
                        "state" => {
                            self.session.state.set_state(name, value.clone());
                            if let Some(node) = self.node_bound_to(name) {
                                self.widgets.set_value(&node, &value);
                            }
                        }
                        "view" => {
                            self.session.state.set_view(name, value);
                        }
                        _ => {}
                    }
                }
                Step::Stay
            }
            Some(Intent::Unknown) | None => match fallback {
                Some(id) => {
                    self.deliver("activate", Some(id), None, None);
                    Step::Stay
                }
                None => Step::Stay,
            },
            Some(Intent::Pick { node }) => {
                let folder = node_by_id(&self.session.document.form, &node)
                    .and_then(|found| found.picker.as_ref())
                    .map(|picker| picker.mode == ic_view::PickMode::Folder)
                    .unwrap_or(false);
                Step::Pick { node, folder }
            }
        }
    }

    fn node_bound_to(&self, bind: &str) -> Option<String> {
        self.widgets
            .binds
            .iter()
            .find(|(_, held)| held.as_str() == bind)
            .map(|(node, _)| node.clone())
    }

    pub(crate) fn picked(&mut self, node: &str, path: &str) {
        let Some(bind) = self.widgets.binds.get(node).cloned() else {
            return;
        };
        self.edited(node, &bind, Value::String(path.to_string()));
    }

    pub(crate) fn on_click(&mut self, col: u16, row: u16) -> Step {
        let hit = self
            .hits
            .iter()
            .find(|(rect, _)| rect.contains(ratatui::layout::Position { x: col, y: row }))
            .map(|(_, spot)| spot.clone());
        let Some(spot) = hit else {
            return Step::Stay;
        };
        let ring = self.ring();
        if let Some(at) = ring.iter().position(|current| *current == spot) {
            self.focus = at;
        }
        self.act(KeyCode::Enter)
    }

    /// A refusal redraws the pane, so what the plugin wanted to say is on the screen.
    pub(crate) fn may_close(&mut self) -> bool {
        let Purpose::Viewer {
            viewer, instance, ..
        } = &self.purpose
        else {
            return true;
        };
        if ic_plugin_host::viewer_may_close(viewer, *instance) {
            return true;
        }
        self.redescribe();
        false
    }

    /// Only a window has a view behind it: a connection editor is named after its
    /// kind, which another plugin may well have registered as a view of its own.
    ///
    /// Told here and once; the filesystem it was let into follows only after this returns,
    /// because the plugin may read while it closes.
    fn tell_the_plugin(&mut self) {
        if self.told {
            return;
        }
        self.told = true;
        match &self.purpose {
            Purpose::Plugin => ic_plugin_host::view_closed(&self.session.id, 1),
            Purpose::Viewer {
                viewer, instance, ..
            } => ic_plugin_host::viewer_closed(viewer, *instance),
            Purpose::Connection { .. } => {}
        }
    }

    pub(crate) fn closed(mut self) {
        self.tell_the_plugin();
    }

    fn title(&self) -> String {
        if let Purpose::Viewer { name, .. } = &self.purpose {
            return name.clone();
        }
        ic_plugin_host::view_title(&self.session.id)
            .and_then(|key| translate(&key).or(Some(key)))
            .unwrap_or_default()
    }

    pub(crate) fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        let tr: &layout::Tr = &translate;
        let shows_actions = !self.own_actions().is_empty()
            || self
                .session
                .document
                .actions
                .iter()
                .any(|action| visible(action.visible.as_ref(), &self.session.state));
        let (width, height) = wanted_size(
            &self.session.document,
            &self.session.state,
            tr,
            area,
            shows_actions,
        );
        let rect = centered_rect(width, height, area);
        let title = self.title();
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title} \u{2014} Tab move \u{b7} Esc close "))
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        let inner = block.inner(rect);
        f.render_widget(Clear, rect);
        f.render_widget(block, rect);

        let body = Rect {
            height: inner.height.saturating_sub(u16::from(shows_actions)),
            ..inner
        };
        let mut placed: Vec<Placed> = Vec::new();
        place(
            &self.session.document.form,
            body,
            &self.session.state,
            tr,
            &mut placed,
        );
        let focus = self.spot();
        let surface = Surface {
            state: &self.session.state,
            widgets: &self.widgets,
            focus: focus.as_ref(),
            tr,
        };
        let mut hits = Vec::new();
        for spot in &placed {
            draw_placed(f, spot, &surface);
            if let Some(id) = spot.node.id.as_ref() {
                if matches!(
                    spot.node.kind(),
                    NodeKind::Input | NodeKind::Switch | NodeKind::Choice | NodeKind::Button
                ) {
                    hits.push((spot.rect, Spot::Node(id.clone())));
                }
            }
        }
        if shows_actions {
            let bar = Rect {
                y: inner.y.saturating_add(inner.height.saturating_sub(1)),
                height: 1,
                ..inner
            };
            for (rect, id) in
                draw_actions(f, &self.session.document, self.own_actions(), bar, &surface)
            {
                hits.push((rect, Spot::Action(id)));
            }
        }
        self.hits = hits;
        if let Some(Nested::Options { node, cursor, .. }) = &self.nested {
            draw_options(f, &self.session.document, node, *cursor, rect);
        }
    }
}

impl Drop for ViewPane {
    fn drop(&mut self) {
        self.tell_the_plugin();
        if let Some(held) = self.showing.take() {
            fm_core::host_fs::HostSource::close(held.source);
        }
    }
}

fn draw_options(f: &mut Frame, document: &Document, node: &str, cursor: usize, over: Rect) {
    let Some(found) = node_by_id(&document.form, node) else {
        return;
    };
    let labels: Vec<String> = found
        .options
        .iter()
        .map(|option| {
            option
                .label
                .as_ref()
                .map(|text| text.resolve(&translate))
                .unwrap_or_else(|| as_text(&option.value))
        })
        .collect();
    let width = labels
        .iter()
        .map(|label| label.chars().count() as u16)
        .max()
        .unwrap_or(10)
        + 4;
    let height = labels.len() as u16 + 2;
    let rect = centered_rect(width.max(16), height.min(over.height.max(3)), over);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    let items: Vec<ratatui::widgets::ListItem> = labels
        .into_iter()
        .map(|label| ratatui::widgets::ListItem::new(label))
        .collect();
    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(cursor));
    let list = ratatui::widgets::List::new(items).highlight_style(
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    f.render_stateful_widget(list, inner, &mut state);
}

/// OSC 52: hands the text to whatever terminal we are running inside.
fn copy(text: &str) {
    use base64::Engine;
    use std::io::Write;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut out = std::io::stdout();
    let _ = write!(out, "\u{1b}]52;c;{encoded}\u{7}");
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn declare(kind: &str) {
        let document = serde_json::json!({
            "schema": 1,
            "kind": kind,
            "identity": "name",
            "fields": [
                { "bind": "name", "type": "text", "scope": "record", "required": true },
                { "bind": "host", "type": "text", "required": true },
                { "bind": "auth_type", "type": "text", "default": "password" },
                { "bind": "pass", "type": "text", "secret": true,
                  "when": { "ne": ["state.auth_type", "key"] } },
                { "bind": "key_path", "type": "path",
                  "when": { "eq": ["state.auth_type", "key"] } },
            ],
            "form": { "t": "column", "children": [
                { "t": "input", "id": "name", "bind": "name", "title": "Name" },
                { "t": "input", "id": "host", "bind": "host", "title": "Host" },
                { "t": "choice", "id": "auth", "bind": "auth_type", "title": "Auth",
                  "options": [{ "value": "password" }, { "value": "key" }] },
                { "t": "input", "id": "pass", "bind": "pass", "variant": "masked",
                  "visible": { "ne": ["state.auth_type", "key"] } },
                { "t": "input", "id": "key_path", "bind": "key_path",
                  "visible": { "eq": ["state.auth_type", "key"] } },
            ]},
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

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    fn type_text(pane: &mut ViewPane, text: &str) {
        for letter in text.chars() {
            pane.on_key(press(KeyCode::Char(letter)));
        }
    }

    fn ids(pane: &ViewPane) -> Vec<String> {
        pane.ring()
            .into_iter()
            .map(|spot| match spot {
                Spot::Node(id) => id,
                Spot::Action(id) => id,
            })
            .collect()
    }

    fn open_form(kind: &str) -> ViewPane {
        ViewPane::for_connection(kind, &connection_form::Connection::new(kind), None, &[])
            .expect("the kind is declared")
    }

    fn open(kind: &str) -> ViewPane {
        declare(kind);
        open_form(kind)
    }

    #[test]
    fn the_host_offers_its_own_buttons_because_the_document_declares_none() {
        let pane = open("tuiown");
        assert!(pane.session.document.actions.is_empty());
        let ring = ids(&pane);
        assert_eq!(
            &ring[ring.len() - 3..],
            &[
                "@save".to_string(),
                "@connect".to_string(),
                "@cancel".to_string()
            ]
        );
    }

    #[test]
    fn choosing_key_authentication_swaps_the_password_row_for_the_key_row() {
        let mut pane = open("tuiswap");
        assert!(ids(&pane).contains(&"pass".to_string()));
        assert!(!ids(&pane).contains(&"key_path".to_string()));

        // the third control is the auth choice; step onto it and pick the next option
        pane.focus = 2;
        assert!(matches!(pane.spot(), Some(Spot::Node(ref id)) if id == "auth"));
        pane.on_key(press(KeyCode::Right));

        assert_eq!(
            pane.session.state.state.get("auth_type"),
            Some(&Value::String("key".to_string())),
            "the change is mirrored into the state the predicates read"
        );
        assert!(
            ids(&pane).contains(&"key_path".to_string()),
            "the field the document made conditional must appear"
        );
        assert!(!ids(&pane).contains(&"pass".to_string()));
    }

    #[test]
    fn what_is_typed_is_what_gets_committed() {
        let mut pane = open("tuitype");
        pane.focus = 0;
        type_text(&mut pane, "work");
        pane.on_key(press(KeyCode::Tab));
        type_text(&mut pane, "h.example.org");

        let form = pane.committed().expect("both required fields are filled");
        assert_eq!(form.name, "work");
        assert_eq!(
            form.values.get("host").map(String::as_str),
            Some("h.example.org")
        );
        assert_eq!(
            form.record.get("name").map(String::as_str),
            Some("work"),
            "a record-scoped field lands in the record, not the settings"
        );
    }

    #[test]
    fn an_incomplete_form_says_what_is_still_missing() {
        let mut pane = open("tuimissing");
        pane.focus = 0;
        type_text(&mut pane, "work");
        let why = pane.committed().expect_err("host is required");
        assert_eq!(why, vec!["host".to_string()]);
    }

    #[test]
    fn a_secret_never_leaves_the_widgets_for_the_plugin_to_see() {
        let mut pane = open("tuisecret");
        pane.focus = 3;
        assert!(matches!(pane.spot(), Some(Spot::Node(ref id)) if id == "pass"));
        type_text(&mut pane, "hunter2");
        let shown = pane.session.snapshot(&pane.widgets);
        assert!(
            shown.get("pass").is_none(),
            "a snapshot for the plugin carries no secret"
        );
        assert_eq!(
            pane.widgets.value_of("pass"),
            Some(Value::String("hunter2".to_string())),
            "but the host still holds it for the commit"
        );
    }

    #[test]
    fn escape_closes_and_the_cancel_button_closes() {
        let mut pane = open("tuiclose");
        assert!(matches!(pane.on_key(press(KeyCode::Esc)), Step::Close));
        let last = ids(&pane).len() - 1;
        pane.focus = last;
        assert!(matches!(pane.on_key(press(KeyCode::Enter)), Step::Close));
    }

    #[test]
    fn the_save_buttons_ask_for_a_save_and_a_save_with_a_connect() {
        let mut pane = open("tuisave");
        let ring = ids(&pane);
        pane.focus = ring.len() - 3;
        assert!(matches!(
            pane.on_key(press(KeyCode::Enter)),
            Step::Save { connect: false }
        ));
        pane.focus = ring.len() - 2;
        assert!(matches!(
            pane.on_key(press(KeyCode::Enter)),
            Step::Save { connect: true }
        ));
    }

    const LISTENS: &str = r#"{
        "schema": 1,
        "kind": "listens",
        "identity": "name",
        "fields": [
            { "bind": "name", "type": "text", "scope": "record", "required": true },
            { "bind": "token", "type": "text" }
        ],
        "form": { "t": "column", "children": [
            { "t": "input", "id": "name", "bind": "name", "title": "Name", "emit": "change" },
            { "t": "input", "id": "token", "bind": "token", "title": "Token" },
            { "t": "button", "id": "login", "title": "Log in",
              "intent": { "do": "emit", "node": "login" } } ] }
    }"#;

    const SIGNED_IN: &str = r#"{
        "schema": 1,
        "kind": "listens",
        "identity": "name",
        "fields": [ { "bind": "name", "type": "text", "scope": "record", "required": true } ],
        "form": { "t": "column", "children": [
            { "t": "text", "id": "signed", "text": "one account is enough" } ] }
    }"#;

    const SECRETS: &str = r#"{
        "schema": 1,
        "kind": "secrets",
        "identity": "name",
        "fields": [
            { "bind": "name", "type": "text", "scope": "record", "required": true },
            { "bind": "label", "type": "text", "when": { "touched": "label" } },
            { "bind": "pass", "type": "text", "secret": true }
        ],
        "form": { "t": "column", "children": [
            { "t": "input", "id": "name", "bind": "name", "title": "Name" },
            { "t": "input", "id": "label", "bind": "label", "title": "Label" },
            { "t": "input", "id": "pass", "bind": "pass", "title": "Password", "variant": "masked" },
            { "t": "button", "id": "again", "title": "Again",
              "intent": { "do": "emit", "node": "again" } } ] }
    }"#;

    const SECRETS_AGAIN: &str = r#"{
        "schema": 1,
        "kind": "secrets",
        "identity": "name",
        "fields": [
            { "bind": "name", "type": "text", "scope": "record", "required": true },
            { "bind": "label", "type": "text", "when": { "touched": "label" } },
            { "bind": "pass", "type": "text", "secret": true }
        ],
        "form": { "t": "column", "children": [
            { "t": "text", "id": "note", "text": "one more step" },
            { "t": "input", "id": "name", "bind": "name", "title": "Name" },
            { "t": "input", "id": "label", "bind": "label", "title": "Label" },
            { "t": "input", "id": "pass", "bind": "pass", "title": "Password", "variant": "masked" },
            { "t": "button", "id": "again", "title": "Again",
              "intent": { "do": "emit", "node": "again" } } ] }
    }"#;

    struct Script {
        answer: &'static str,
        before: &'static str,
        after: &'static str,
        seen: std::sync::Mutex<Vec<Value>>,
        describes: std::sync::atomic::AtomicUsize,
        acted: std::sync::atomic::AtomicBool,
    }

    thread_local! {
        static SPOKEN: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
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

    extern "C" fn kind_open(
        _: *const u8,
        _: u64,
        _: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcFsHandle {
        std::ptr::null_mut()
    }

    extern "C" fn kind_describes(
        _: *const u8,
        _: u64,
        user_data: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        let script = unsafe { &*(user_data as *const Script) };
        script
            .describes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        spoken(if script.acted.load(std::sync::atomic::Ordering::SeqCst) {
            script.after
        } else {
            script.before
        })
    }

    extern "C" fn kind_hears(
        event: *const u8,
        len: u64,
        user_data: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        let script = unsafe { &*(user_data as *const Script) };
        let raw = unsafe { std::slice::from_raw_parts(event, len as usize) };
        let heard: Value = serde_json::from_slice(raw).expect("the host sends json");
        if heard["type"] == Value::String("activate".to_string()) {
            script
                .acted
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        script.seen.lock().expect("the script").push(heard);
        spoken(script.answer)
    }

    /// Registers a kind served by a plugin that describes its own form and, when
    /// `hears`, answers its events with `answer`.
    fn declare_plugin(kind: &str, answer: &'static str, hears: bool) -> &'static Script {
        declare_showing(kind, answer, hears, LISTENS, SIGNED_IN)
    }

    /// The same, for a plugin that describes `before` until a gesture reaches it
    /// and `after` from then on.
    fn declare_showing(
        kind: &str,
        answer: &'static str,
        hears: bool,
        before: &'static str,
        after: &'static str,
    ) -> &'static Script {
        let script: &'static Script = Box::leak(Box::new(Script {
            answer,
            before,
            after,
            seen: std::sync::Mutex::new(Vec::new()),
            describes: std::sync::atomic::AtomicUsize::new(0),
            acted: std::sync::atomic::AtomicBool::new(false),
        }));
        let table = ic_plugin_api::IcConnectionVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcConnectionVTable>() as u32,
            open: kind_open,
            fs: std::ptr::null(),
            describe: Some(kind_describes),
            on_event: hears.then_some(kind_hears as ic_plugin_api::IcViewEventFn),
        };
        let host = ic_plugin_host::host_table();
        let id = std::ffi::CString::new(kind).expect("a kind id");
        assert_eq!(
            (host.register_connection_kind)(
                id.as_ptr(),
                before.as_ptr(),
                before.len() as u64,
                &table,
                script as *const Script as *mut std::ffi::c_void,
            ),
            ic_plugin_api::IC_OK
        );
        script
    }

    extern "C" fn window_describes(
        _: *const u8,
        _: u64,
        _: *mut std::ffi::c_void,
    ) -> ic_plugin_api::IcBytes {
        spoken(LISTENS)
    }

    extern "C" fn window_closed(_: u64, user_data: *mut std::ffi::c_void) {
        let closes = unsafe { &*(user_data as *const std::sync::atomic::AtomicUsize) };
        closes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Registers a plugin window under `id` and counts the times it is told it
    /// was closed.
    fn declare_window(id: &str) -> &'static std::sync::atomic::AtomicUsize {
        let closes: &'static std::sync::atomic::AtomicUsize =
            Box::leak(Box::new(std::sync::atomic::AtomicUsize::new(0)));
        let table = ic_plugin_api::IcViewVTable {
            struct_size: std::mem::size_of::<ic_plugin_api::IcViewVTable>() as u32,
            describe: window_describes,
            on_event: None,
            closed: Some(window_closed),
        };
        let host = ic_plugin_host::host_table();
        let named = std::ffi::CString::new(id).expect("a view id");
        let title = std::ffi::CString::new("Window").expect("a title");
        assert_eq!(
            (host.register_view)(
                named.as_ptr(),
                title.as_ptr(),
                &table,
                closes as *const std::sync::atomic::AtomicUsize as *mut std::ffi::c_void,
            ),
            ic_plugin_api::IC_OK
        );
        closes
    }

    fn heard(script: &Script) -> Vec<Value> {
        script.seen.lock().expect("the script").clone()
    }

    #[test]
    fn a_kind_that_listens_is_told_its_form_is_on_screen() {
        let script = declare_plugin("tuiopened", "null", true);
        let pane = open_form("tuiopened");
        assert!(ids(&pane).contains(&"login".to_string()));

        let seen = heard(script);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0]["type"], serde_json::json!("opened"));
        assert_eq!(seen[0]["view"], serde_json::json!("tuiopened"));
        assert_eq!(
            seen[0]["host"]["kind"],
            serde_json::json!(ic_plugin_api::IC_HOST_CONSOLE),
            "the same word init was given, so a plugin can branch on one name"
        );
    }

    #[test]
    fn a_button_on_a_connection_form_reaches_the_plugin_and_its_answer_is_applied() {
        let script = declare_plugin(
            "tuibutton",
            r#"{ "put": { "token": "issued" }, "set": { "data.account": "ready" } }"#,
            true,
        );
        let mut pane = open_form("tuibutton");
        pane.focus = 2;
        assert!(matches!(pane.spot(), Some(Spot::Node(ref id)) if id == "login"));
        assert!(matches!(pane.on_key(press(KeyCode::Enter)), Step::Stay));

        let seen = heard(script);
        assert_eq!(seen.len(), 2, "the opened event, then the press");
        assert_eq!(seen[1]["type"], serde_json::json!("activate"));
        assert_eq!(seen[1]["node"], serde_json::json!("login"));
        assert_eq!(
            pane.widgets.value_of("token"),
            Some(Value::String("issued".to_string())),
            "put is keyed by node id and lands in the widget behind it"
        );
        assert_eq!(
            pane.session.state.data.get("account"),
            Some(&serde_json::json!("ready"))
        );
    }

    #[test]
    fn a_field_that_asks_for_its_changes_sends_each_one_with_what_the_form_holds() {
        let script = declare_plugin("tuichange", "null", true);
        let mut pane = open_form("tuichange");
        pane.focus = 0;
        type_text(&mut pane, "work");

        let seen = heard(script);
        assert_eq!(
            seen.len(),
            5,
            "the opened event and one change per keystroke"
        );
        let last = seen.last().expect("a change");
        assert_eq!(last["type"], serde_json::json!("change"));
        assert_eq!(last["bind"], serde_json::json!("name"));
        assert_eq!(last["value"], serde_json::json!("work"));
        assert_eq!(last["values"]["name"], serde_json::json!("work"));
    }

    #[test]
    fn a_plugin_that_asks_to_be_described_again_gets_a_form_rebuilt_around_what_was_typed() {
        let script = declare_plugin("tuiredescribe", r#"{ "redescribe": true }"#, true);
        let mut pane = open_form("tuiredescribe");
        pane.focus = 0;
        type_text(&mut pane, "work");
        assert!(
            ids(&pane).contains(&"login".to_string()),
            "the same tree is refreshed in place while the plugin describes it the same way"
        );

        pane.focus = 2;
        pane.on_key(press(KeyCode::Enter));
        assert!(
            !ids(&pane).contains(&"login".to_string()),
            "the form the plugin now describes takes the place of the old one"
        );
        assert_eq!(
            pane.session.state.state.get("name"),
            Some(&serde_json::json!("work")),
            "what was typed survives the rebuild"
        );
        assert!(script.describes.load(std::sync::atomic::Ordering::SeqCst) > 1);
    }

    #[test]
    fn a_kind_that_takes_no_events_is_filled_in_without_the_plugin_hearing_anything() {
        let script = declare_plugin("tuideaf", "null", false);
        let mut pane = open_form("tuideaf");
        assert_eq!(
            script.describes.load(std::sync::atomic::Ordering::SeqCst),
            1
        );

        pane.focus = 0;
        type_text(&mut pane, "work");
        pane.focus = 2;
        assert!(matches!(pane.on_key(press(KeyCode::Enter)), Step::Stay));

        assert!(heard(script).is_empty());
        assert_eq!(
            script.describes.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "nothing asked for the form again"
        );
        assert_eq!(
            pane.committed().expect("the name is all it needs").name,
            "work"
        );
    }

    #[test]
    fn leaving_a_connection_editor_tells_no_window_of_the_same_name_that_it_closed() {
        declare_plugin("tuisameid", "null", true);
        let closes = declare_window("tuisameid");
        let editor = open_form("tuisameid");
        editor.closed();
        assert_eq!(
            closes.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the editor is named after the kind, not after the window the plugin has open"
        );

        let window = ViewPane::open("tuisameid", "null").expect("the view describes itself");
        window.closed();
        assert_eq!(
            closes.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "a window that closes still reaches the plugin"
        );
    }

    #[test]
    fn a_form_rebuilt_on_redescribe_keeps_the_secret_and_the_edits_the_state_cannot_seed_back() {
        declare_showing(
            "tuicarry",
            r#"{ "redescribe": true }"#,
            true,
            SECRETS,
            SECRETS_AGAIN,
        );
        let mut pane = open_form("tuicarry");
        pane.focus = 0;
        type_text(&mut pane, "work");
        pane.focus = 1;
        assert!(matches!(pane.spot(), Some(Spot::Node(ref id)) if id == "label"));
        type_text(&mut pane, "mine");
        pane.focus = 2;
        assert!(matches!(pane.spot(), Some(Spot::Node(ref id)) if id == "pass"));
        type_text(&mut pane, "hunter2");

        pane.focus = 3;
        assert!(matches!(pane.spot(), Some(Spot::Node(ref id)) if id == "again"));
        pane.on_key(press(KeyCode::Enter));
        assert!(
            node_by_id(&pane.session.document.form, "note").is_some(),
            "the plugin describes another tree, so the form is rebuilt"
        );

        assert_eq!(
            pane.widgets.value_of("pass"),
            Some(Value::String("hunter2".to_string())),
            "a secret is held by the widgets alone, so the rebuild has to carry it over"
        );
        let form = pane.committed().expect("the form is complete");
        assert_eq!(form.values.get("pass").map(String::as_str), Some("hunter2"));
        assert_eq!(
            form.values.get("label").map(String::as_str),
            Some("mine"),
            "a field the document commits only once it is touched is still touched after the rebuild"
        );
    }

    #[test]
    fn a_value_the_plugin_sets_reaches_the_widget_bound_to_it_and_is_what_gets_committed() {
        declare_plugin("tuisets", r#"{ "set": { "state.token": "issued" } }"#, true);
        let mut pane = open_form("tuisets");
        pane.focus = 0;
        type_text(&mut pane, "work");
        pane.focus = 2;
        assert!(matches!(pane.spot(), Some(Spot::Node(ref id)) if id == "login"));
        pane.on_key(press(KeyCode::Enter));

        assert_eq!(
            pane.widgets.value_of("token"),
            Some(Value::String("issued".to_string())),
            "a set on a bound node is what the box shows"
        );
        let form = pane.committed().expect("the name is all it needs");
        assert_eq!(
            form.values.get("token").map(String::as_str),
            Some("issued"),
            "and what the form commits"
        );
    }

    #[test]
    fn a_value_the_plugin_sets_is_not_taken_back_by_the_widget_at_the_next_event() {
        let script = declare_plugin(
            "tuikeeps",
            r#"{ "set": { "state.token": "issued" } }"#,
            true,
        );
        let mut pane = open_form("tuikeeps");
        pane.focus = 1;
        assert!(matches!(pane.spot(), Some(Spot::Node(ref id)) if id == "token"));
        type_text(&mut pane, "typed");
        pane.focus = 2;
        pane.on_key(press(KeyCode::Enter));

        pane.focus = 0;
        type_text(&mut pane, "w");
        let seen = heard(script);
        assert_eq!(
            seen.last().expect("a change")["values"]["token"],
            serde_json::json!("issued"),
            "the form the plugin is shown carries what it set, not the value it replaced"
        );
        assert_eq!(
            pane.widgets.value_of("token"),
            Some(Value::String("issued".to_string()))
        );
    }

    #[test]
    fn a_value_the_plugin_puts_is_not_undone_by_the_state_the_same_answer_sets() {
        declare_plugin(
            "tuiputset",
            r#"{ "put": { "token": "issued" }, "set": { "state.name": "given" } }"#,
            true,
        );
        let mut pane = open_form("tuiputset");
        pane.focus = 1;
        type_text(&mut pane, "typed");
        pane.focus = 2;
        pane.on_key(press(KeyCode::Enter));

        assert_eq!(
            pane.widgets.value_of("token"),
            Some(Value::String("issued".to_string())),
            "the node the answer put into keeps what it was given"
        );
        assert_eq!(
            pane.widgets.value_of("name"),
            Some(Value::String("given".to_string()))
        );
    }

    #[test]
    fn a_value_the_plugin_sets_is_not_reported_back_as_something_the_user_typed() {
        declare_plugin(
            "tuiuntyped",
            r#"{ "set": { "state.token": "issued" } }"#,
            true,
        );
        let mut pane = open_form("tuiuntyped");
        pane.focus = 2;
        pane.on_key(press(KeyCode::Enter));

        assert_eq!(
            pane.widgets.value_of("token"),
            Some(Value::String("issued".to_string()))
        );
        assert!(
            !pane.widgets.touched().contains(&"token".to_string()),
            "writing into a widget is not an edit the user made"
        );
        assert!(!pane.session.state.touched.contains("token"));
    }

    struct Here;

    #[async_trait::async_trait(?Send)]
    impl fm_core::rpc::FileSystemRpc for Here {
        fn is_local(&self) -> bool {
            true
        }
    }

    const SHOWS: &str = r#"{
        "schema": 1,
        "fields": [],
        "form": { "t": "view", "surface": "window", "children": [
            { "t": "text", "id": "head", "text": "the first line" },
            { "t": "button", "id": "next", "title": "Next", "accel": "Right",
              "intent": { "do": "emit", "node": "next" } } ] },
        "keys": [ { "accel": "Left", "node": "back" } ]
    }"#;

    struct Watched {
        opened: std::sync::Mutex<Vec<String>>,
        seen: std::sync::Mutex<Vec<Value>>,
        asked: std::sync::atomic::AtomicUsize,
        closes: std::sync::atomic::AtomicUsize,
        refuses: std::sync::atomic::AtomicBool,
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
        spoken(SHOWS)
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
        watched
            .asked
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
            asked: std::sync::atomic::AtomicUsize::new(0),
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

    #[tokio::test]
    async fn a_file_a_plugin_claims_is_drawn_by_it_and_asked_before_the_window_goes() {
        let watched = declare_viewer("tuiviewer", ".tuiview");
        assert_eq!(
            ic_plugin_host::viewer_for("notes.tuiview").as_deref(),
            Some("tuiviewer")
        );

        let root = std::env::temp_dir().join("ice-commander-console-viewer-test");
        std::fs::create_dir_all(&root).expect("a folder");
        let at = root.join("notes.tuiview");
        std::fs::write(&at, b"the first line\n").expect("a file");

        let provider: std::rc::Rc<dyn fm_core::rpc::FileSystemRpc> = std::rc::Rc::new(Here);
        let staged = fm_core::host_fs::stage(&provider, &at.to_string_lossy())
            .await
            .expect("the file is staged");
        let source =
            fm_core::host_fs::HostSource::rooted_at(&staged.root, &provider.fs_id(), "").open();
        let mut pane = ViewPane::viewing("tuiviewer", "notes.tuiview", source, staged)
            .expect("the plugin shows it");

        assert_eq!(
            watched.opened.lock().expect("the viewer").as_slice(),
            ["notes.tuiview".to_string()],
            "the plugin is given the name on the filesystem it was let into, not the bytes"
        );
        assert_eq!(pane.title(), "notes.tuiview");
        let opened = watched.seen.lock().expect("the viewer")[0].clone();
        assert_eq!(opened["type"], serde_json::json!("opened"));
        assert_eq!(
            opened["host"]["kind"],
            serde_json::json!(ic_plugin_api::IC_HOST_CONSOLE),
            "the same word init was given"
        );

        assert!(matches!(pane.on_key(press(KeyCode::Right)), Step::Stay));
        let pressed = watched
            .seen
            .lock()
            .expect("the viewer")
            .last()
            .cloned()
            .expect("an event");
        assert_eq!(pressed["type"], serde_json::json!("activate"));
        assert_eq!(
            pressed["node"],
            serde_json::json!("next"),
            "the key the document named does what pressing the button would"
        );

        assert!(matches!(pane.on_key(press(KeyCode::Left)), Step::Stay));
        let keyed = watched
            .seen
            .lock()
            .expect("the viewer")
            .last()
            .cloned()
            .expect("an event");
        assert_eq!(keyed["type"], serde_json::json!("activate"));
        assert_eq!(
            keyed["node"],
            serde_json::json!("back"),
            "a key with no button on screen still reaches the plugin"
        );

        watched
            .refuses
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(!pane.may_close(), "a plugin that says no keeps the window");
        assert_eq!(watched.closes.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(pane.may_close());
        pane.closed();
        assert_eq!(watched.asked.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(watched.closes.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn a_viewer_pane_that_is_replaced_still_tells_the_plugin() {
        let watched = declare_viewer("tuidropped", ".tuidropped");
        let root = std::env::temp_dir().join(format!("ic-tui-dropped-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a directory of our own");
        let at = root.join("notes.tuidropped");
        std::fs::write(&at, b"a line").expect("a file");
        let staged = futures::executor::block_on(fm_core::host_fs::stage(
            &(std::rc::Rc::new(localfs::local_rpc::LocalFileSystemRpc::new(
                client_config::AppConfig::new("ice-commander-test"),
            )) as std::rc::Rc<dyn fm_core::rpc::FileSystemRpc>),
            &at.to_string_lossy(),
        ))
        .expect("the file stands somewhere");
        let source = fm_core::host_fs::HostSource::rooted_at(&staged.root, "local", "").open();
        let pane = ViewPane::viewing("tuidropped", "notes.tuidropped", source, staged)
            .expect("the viewer opens it");

        drop(pane);
        assert_eq!(
            watched.closes.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "dropped rather than closed, and the plugin still hears it"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_masked_box_is_not_filled_with_the_secret_the_record_already_stores() {
        declare_showing(
            "tuistored",
            r#"{ "set": { "state.label": "seen" } }"#,
            true,
            SECRETS,
            SECRETS,
        );
        let pane = ViewPane::for_connection(
            "tuistored",
            &{
                let mut held = connection_form::Connection::new("tuistored");
                held.put("pass", "hunter2".to_string());
                held
            },
            Some(0),
            &["pass".to_string()],
        )
        .expect("the kind is declared");

        assert_eq!(
            pane.widgets.value_of("label"),
            Some(Value::String("seen".to_string())),
            "the answer to the opened event reached the widget behind the bind"
        );
        assert!(
            pane.widgets.value_of("pass").is_none(),
            "the box the user types a secret into stays empty"
        );
        assert_eq!(
            pane.session.state.state.get("pass"),
            Some(&serde_json::json!("hunter2")),
            "while the state still carries the stored secret the commit falls back on"
        );
    }
}
