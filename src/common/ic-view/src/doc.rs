use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SCHEMA: u32 = 1;

pub fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let raw = Value::deserialize(deserializer)?;
    Ok(T::deserialize(raw).unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Text {
    Plain(String),
    Translated { tr: String, en: String },
    Verbatim { literal: String },
}

impl Default for Text {
    fn default() -> Self {
        Text::Plain(String::new())
    }
}

impl Text {
    pub fn resolve(&self, translate: &dyn Fn(&str) -> Option<String>) -> String {
        match self {
            Text::Plain(s) => s.clone(),
            Text::Verbatim { literal } => literal.clone(),
            Text::Translated { tr, en } => translate(tr).unwrap_or_else(|| en.clone()),
        }
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            Text::Translated { tr, .. } => Some(tr.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Case<T> {
    pub when: Pred,
    pub then: T,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Cond<T> {
    Cases {
        cases: Vec<Case<T>>,
        #[serde(rename = "else", default, skip_serializing_if = "Option::is_none")]
        otherwise: Option<Box<Cond<T>>>,
    },
    Fixed(T),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Pred {
    Always(bool),
    Op(PredOp),
}

impl Default for Pred {
    fn default() -> Self {
        Pred::Always(true)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredOp {
    Eq(Vec<Value>),
    Ne(Vec<Value>),
    All(Vec<Pred>),
    Any(Vec<Pred>),
    Not(Box<Pred>),
    Truthy(Value),
    Empty(Value),
    HasStored(String),
    Touched(String),
    OneOf { value: Value, of: Vec<Value> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    #[default]
    Text,
    Integer,
    Bool,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Settings,
    Record,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Derive {
    #[default]
    Never,
    WhileUntouched,
    CommitOnly,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnParseError {
    #[default]
    Reject,
    Default,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub bind: String,
    #[serde(rename = "type", default, deserialize_with = "lenient")]
    pub field_type: FieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub scope: Scope,
    #[serde(default, deserialize_with = "lenient")]
    pub when: Option<Pred>,
    #[serde(default, deserialize_with = "lenient")]
    pub default: Option<Cond<Value>>,
    #[serde(default, deserialize_with = "lenient")]
    pub derive: Derive,
    #[serde(default)]
    pub empty_as_absent: bool,
    #[serde(default)]
    pub empty_keeps_stored: bool,
    #[serde(default)]
    pub keep_when_inapplicable: bool,
    #[serde(default)]
    pub min: Option<i64>,
    #[serde(default)]
    pub max: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub on_parse_error: OnParseError,
}

impl Field {
    pub fn new(bind: &str) -> Self {
        Field {
            bind: bind.to_string(),
            field_type: FieldType::Text,
            required: false,
            secret: false,
            scope: Scope::Settings,
            when: None,
            default: None,
            derive: Derive::Never,
            empty_as_absent: false,
            empty_keeps_stored: false,
            keep_when_inapplicable: false,
            min: None,
            max: None,
            on_parse_error: OnParseError::Reject,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Chrome {
    #[default]
    Bare,
    Row,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputVariant {
    #[default]
    Text,
    Masked,
    MaskedReveal,
    Integer,
    Path,
    Multiline,
    /// Bytes written as hex. The control keeps to `0-9 a-f A-F` and spaces and
    /// shows them in a fixed-width face, so what the user types is already the
    /// shape the plugin parses.
    Hex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Emit {
    #[default]
    Commit,
    Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scroll {
    #[default]
    None,
    Vertical,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Where the host puts the document.
pub enum Surface {
    /// Inside whatever asked for it: a viewer in its frame, a connection page.
    #[default]
    Embedded,
    /// A window of its own, held above the one that opened it.
    Dialog,
    /// A window of its own that the user can leave and come back to.
    Window,
    /// In the panel the plugin was pressed from, in place of the file list.
    /// `{ "do": "close" }` puts the panel back to what it was showing. A host
    /// with no panel — a terminal, a browser — gives it a window instead.
    Panel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickMode {
    #[default]
    File,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Picker {
    #[serde(default, deserialize_with = "lenient")]
    pub mode: PickMode,
    #[serde(default)]
    pub title: Option<Text>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub value: Value,
    #[serde(default)]
    pub label: Option<Text>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Decode {
    #[serde(default)]
    pub case_insensitive: bool,
    #[serde(default)]
    pub unknown: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    #[serde(default)]
    pub label: Option<Text>,
    #[serde(default)]
    pub color: Option<[f64; 4]>,
    #[serde(default)]
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub key: String,
    #[serde(default)]
    pub title: Option<Text>,
    #[serde(default)]
    pub width: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Intent {
    Submit,
    Connect,
    Close,
    Revert,
    Set {
        keys: BTreeMap<String, Value>,
    },
    Pick {
        node: String,
    },
    Emit {
        node: String,
    },
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Node {
    pub t: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    #[serde(default, deserialize_with = "lenient")]
    pub title: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub subtitle: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub text: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub placeholder: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub tooltip: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub caption: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub label: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub icon: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub role: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub visible: Option<Pred>,
    #[serde(default, deserialize_with = "lenient")]
    pub sensitive: Option<Pred>,
    #[serde(default, deserialize_with = "lenient")]
    pub chrome: Chrome,
    #[serde(default, deserialize_with = "lenient")]
    pub variant: InputVariant,
    #[serde(default, deserialize_with = "lenient")]
    pub emit: Emit,
    #[serde(default, deserialize_with = "lenient")]
    pub scroll: Scroll,
    #[serde(default, deserialize_with = "lenient")]
    pub surface: Surface,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Choice>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<Column>,
    /// Where a `table` reads its rows, in `data`. The plugin replaces them with
    /// `{"set": {"data.<key>": [...]}}` and the table redraws.
    ///
    /// A table that also carries a `bind` lets the user pick a row, and the
    /// whole row object comes back under that bind in the event's `values` —
    /// so the bind has to name a declared field, as for any other input.
    /// `"emit": "change"` reports the moment the selection moves; an
    /// `{ "do": "emit" }` intent reports a row being opened. A table with
    /// neither is a readout and stays unselectable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_key: Option<String>,
    /// Where a `slider` reads its position, in `data`: the plugin's to move, like a table's
    /// rows. Without one it shows what the state holds under its `bind`, which is what a form wants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// How far a `slider` moves in one step. Absent leaves it to the toolkit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub decode: Option<Decode>,
    #[serde(default, deserialize_with = "lenient")]
    pub picker: Option<Picker>,
    #[serde(default, deserialize_with = "lenient")]
    pub intent: Option<Cond<Intent>>,
    /// Where a picture or a piece of media is, for an `image` or `media` node.
    ///
    /// Never the thing itself: a document is text, and a picture is named
    /// rather than carried. Two spellings —
    ///
    /// - `file:<path>` — something on the filesystem this window was opened
    ///   on. The application reads it, so it works the same on a disk, on a
    ///   server and inside an archive;
    /// - `part:<name>` — something the plugin makes: a page of a document, the
    ///   picture inside a camera file. The application asks for it when it
    ///   draws and keeps as few as it likes.
    #[serde(default, deserialize_with = "lenient")]
    pub src: Option<Cond<Text>>,
    /// How a picture sits in the room it is given: `contain` (the default),
    /// `cover`, `actual` or `width`.
    #[serde(default, deserialize_with = "lenient")]
    pub fit: Fit,
    /// Whether the viewer may zoom and pan it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub zoom: bool,
    /// Whether a `media` node starts playing as soon as it is drawn. A player
    /// that moves to the next track says so; one the user opened and has not
    /// pressed anything in does not.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub autoplay: bool,
    /// A key that does what pressing this node would do: `"Left"`, `"space"`,
    /// `"ctrl+Right"`.
    ///
    /// Turning the page of a document or moving to the next photograph is done
    /// with the keyboard by everyone who does it often, and a plugin cannot
    /// reach the keyboard itself. The application binds it while the window is
    /// open and only while nothing is being typed into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accel: Option<String>,
    /// What a `media` node holds: sound or moving pictures. The application
    /// owns the player; the plugin names what to play and hears what happened.
    #[serde(default, deserialize_with = "lenient")]
    pub media: MediaKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin_top: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debounce_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_ms: Option<u32>,
    #[serde(default)]
    pub selectable: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub wrap: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    View,
    Column,
    Row,
    Group,
    Separator,
    Text,
    Icon,
    Input,
    Switch,
    Choice,
    Button,
    Table,
    /// Rows that hold rows: the same columns as a `table`, with the children of
    /// a row under `children` and an expander in front of it.
    ///
    /// A row that says `"expandable": true` with no `children` yet is one the
    /// plugin fills when it is opened: expanding it reports `expand` with the
    /// row, and the plugin answers by putting the children into `data`.
    Tree,
    Chart,
    Image,
    Media,
    Canvas,
    /// A value the user can slide: a volume, a place in a film.
    ///
    /// Its span is `min`..`max` and must not move: a document of a different shape has the host
    /// build the window again. A range known only later slides over a fixed span and scales.
    Slider,
    Unknown,
}

pub const NODE_KINDS: &[&str] = &[
    "view",
    "column",
    "row",
    "group",
    "separator",
    "text",
    "icon",
    "input",
    "switch",
    "choice",
    "button",
    "table",
    "tree",
    "chart",
    "image",
    "media",
    "canvas",
    "slider",
];

impl Node {
    pub fn kind(&self) -> NodeKind {
        match self.t.as_str() {
            "view" => NodeKind::View,
            "column" => NodeKind::Column,
            "row" => NodeKind::Row,
            "group" => NodeKind::Group,
            "separator" => NodeKind::Separator,
            "text" => NodeKind::Text,
            "icon" => NodeKind::Icon,
            "input" => NodeKind::Input,
            "switch" => NodeKind::Switch,
            "choice" => NodeKind::Choice,
            "button" => NodeKind::Button,
            "table" => NodeKind::Table,
            "tree" => NodeKind::Tree,
            "chart" => NodeKind::Chart,
            "image" => NodeKind::Image,
            "media" => NodeKind::Media,
            "canvas" => NodeKind::Canvas,
            "slider" => NodeKind::Slider,
            _ => NodeKind::Unknown,
        }
    }

    pub fn takes_value(&self) -> bool {
        matches!(
            self.kind(),
            NodeKind::Input
                | NodeKind::Switch
                | NodeKind::Choice
                | NodeKind::Slider
                | NodeKind::Table
                | NodeKind::Tree
        )
    }

    pub fn walk(&self, visit: &mut dyn FnMut(&Node)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub fmt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Box<Summary>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    #[serde(default, deserialize_with = "lenient")]
    pub label: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub role: Option<Cond<Text>>,
    #[serde(default, deserialize_with = "lenient")]
    pub visible: Option<Pred>,
    #[serde(default, deserialize_with = "lenient")]
    pub sensitive: Option<Pred>,
    #[serde(default, deserialize_with = "lenient")]
    pub intent: Option<Cond<Intent>>,
}

/// How a picture sits in the room it is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    #[default]
    Contain,
    Cover,
    /// Its own size, however large the picture is.
    Actual,
    /// As wide as the room, as tall as it needs.
    Width,
    #[serde(other)]
    Unknown,
}

/// What a `media` node plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    #[default]
    Audio,
    Video,
    #[serde(other)]
    Unknown,
}

/// A key that reaches the plugin as `activate` of `node`, with nothing on screen to press.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    pub accel: String,
    pub node: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema: u32,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub label: Option<Text>,
    #[serde(default)]
    pub icon: Option<Text>,
    #[serde(default)]
    pub identity: Option<String>,
    /// Which of this kind's own fields holds the folder a panel opens at. The
    /// mount is the whole filesystem either way; this only says where to stand.
    #[serde(default)]
    pub opens_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub immutable_after_create: Vec<String>,
    #[serde(default)]
    pub summary: Option<Summary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, Value>,
    pub form: Node,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<Key>,
}

impl Document {
    pub fn parse(source: &str) -> Result<Document, String> {
        let parsed: Document = serde_json::from_str(source).map_err(|e| e.to_string())?;
        if parsed.schema == 0 || parsed.schema > SCHEMA {
            return Err(format!("unsupported schema {}", parsed.schema));
        }
        Ok(parsed)
    }

    pub fn field(&self, bind: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.bind == bind)
    }

    pub fn shows_the_same_tree_as(&self, other: &Document) -> bool {
        self.form == other.form && self.actions == other.actions && self.keys == other.keys
    }
}

#[cfg(test)]
mod pictures_and_players {
    use super::*;

    fn parsed(source: &str) -> Node {
        serde_json::from_str::<Node>(source).expect("a node the application can read")
    }

    /// A picture is named, never carried: a document is text, and the bytes of
    /// a photograph have no business being in it.
    #[test]
    fn a_picture_names_where_it_is_rather_than_holding_it() {
        let node = parsed(
            r#"{ "t": "image", "src": "file:holiday.jpg", "fit": "contain", "zoom": true }"#,
        );
        assert_eq!(node.t, "image");
        assert!(NODE_KINDS.contains(&node.t.as_str()));
        assert_eq!(node.fit, Fit::Contain);
        assert!(node.zoom);
        match node.src {
            Some(Cond::Fixed(Text::Plain(where_it_is))) => {
                assert_eq!(where_it_is, "file:holiday.jpg")
            }
            other => panic!("the source did not survive: {other:?}"),
        }
    }

    /// The other spelling: something the plugin made, which the application
    /// asks for when it draws rather than being handed.
    #[test]
    fn a_page_the_plugin_makes_is_named_the_same_way() {
        let node = parsed(r#"{ "t": "image", "src": "part:page/7", "fit": "width" }"#);
        assert_eq!(node.fit, Fit::Width);
        assert!(!node.zoom, "zooming is asked for, not assumed");
    }

    /// The word and the kind have to agree, or the validator calls a picture
    /// an unknown node and every frontend draws nothing.
    #[test]
    fn a_picture_and_a_player_are_node_types_the_application_knows() {
        assert_eq!(parsed(r#"{ "t": "image" }"#).kind(), NodeKind::Image);
        assert_eq!(parsed(r#"{ "t": "media" }"#).kind(), NodeKind::Media);
        assert!(NODE_KINDS.contains(&"image") && NODE_KINDS.contains(&"media"));
    }

    #[test]
    fn a_canvas_is_a_node_type_the_application_knows_and_names_no_source() {
        let node = parsed(r#"{ "t": "canvas", "id": "video", "weight": 1 }"#);
        assert_eq!(node.kind(), NodeKind::Canvas);
        assert!(NODE_KINDS.contains(&"canvas"));
        assert!(node.src.is_none());
        assert_eq!(node.weight, Some(1));
    }

    #[test]
    fn a_slider_takes_its_place_from_the_data_and_gives_a_value_back() {
        let shown = parsed(
            r#"{ "t": "slider", "id": "seek", "value_key": "at",
                 "min": 0, "max": 1000, "step": 1, "bind": "seek" }"#,
        );
        assert_eq!(shown.kind(), NodeKind::Slider);
        assert!(NODE_KINDS.contains(&"slider"));
        assert_eq!(shown.value_key.as_deref(), Some("at"));
        assert_eq!(shown.min, Some(0.0));
        assert_eq!(shown.max, Some(1000.0));
        assert_eq!(shown.step, Some(1.0));
        assert!(shown.takes_value(), "what is dragged is reported");

        let in_a_form = parsed(r#"{ "t": "slider", "id": "volume", "bind": "volume" }"#);
        assert!(in_a_form.value_key.is_none(), "a form's slider follows state");
        assert!(in_a_form.takes_value());
    }

    #[test]
    fn a_player_says_what_it_plays() {
        let node = parsed(r#"{ "t": "media", "media": "video", "src": "file:film.mkv" }"#);
        assert_eq!(node.t, "media");
        assert_eq!(node.media, MediaKind::Video);
        assert!(!node.autoplay, "nothing plays until it is asked to");
    }

    /// A player moving to the next track says so, and the one the user just
    /// opened does not: the difference is the whole of `autoplay`.
    /// A key is a plain string, and a node that names one still draws the same
    /// as one that does not.
    #[test]
    fn a_node_may_name_a_key_that_presses_it() {
        let node = parsed(
            r#"{ "t": "button", "id": "on", "accel": "Right",
                 "intent": { "do": "emit", "node": "on" } }"#,
        );
        assert_eq!(node.accel.as_deref(), Some("Right"));
        assert_eq!(
            parsed(r#"{ "t": "button", "id": "on" }"#).accel,
            None,
            "and most nodes name none"
        );
    }

    #[test]
    fn a_player_says_whether_it_starts_by_itself() {
        let node = parsed(
            r#"{ "t": "media", "media": "audio", "src": "file:two.mp3", "autoplay": true }"#,
        );
        assert!(node.autoplay);
    }

    /// A word neither side knows must not stop the window being drawn: an
    /// older application meets a newer plugin and shows what it can.
    #[test]
    fn a_way_of_fitting_we_do_not_know_is_not_a_broken_document() {
        let node = parsed(r#"{ "t": "image", "src": "file:a.png", "fit": "sideways" }"#);
        assert_eq!(node.fit, Fit::Unknown);
    }

    #[test]
    fn a_picture_with_nothing_said_about_it_has_sensible_answers() {
        let node = parsed(r#"{ "t": "image", "src": "file:a.png" }"#);
        assert_eq!(node.fit, Fit::Contain);
        assert_eq!(
            node.media,
            MediaKind::Audio,
            "meaningless here, and harmless"
        );
        assert!(!node.zoom);
    }
}
