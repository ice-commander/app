use adw::prelude::*;
use ic_view::{
    as_text, choice_index, is_truthy, resolve, resolve_text, visible, Chrome, Cond, Document, Fit,
    InputVariant, InputVariant as Variant, Node, NodeKind, Scroll, State, Text,
};
use gtk::glib;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

pub type Translate = Rc<dyn Fn(&str) -> Option<String>>;
pub type IconSource = Rc<dyn Fn(&str, u32) -> Option<gtk::Widget>>;
/// Where a picture named in a document comes from. A document never carries
/// pixels: it says `file:<path>` for something on the filesystem the window
/// was opened on, or `part:<name>` for something the plugin makes. Reading the
/// first and asking for the second belongs to whoever put the window up.
pub type PictureSource = Rc<dyn Fn(&str) -> Option<Vec<u8>>>;
/// A path on this machine for something a player opens itself. A file on a
/// server has to be copied first, and that too belongs to the host.
pub type MediaSource = Rc<dyn Fn(&str) -> Option<String>>;
/// The application's own player, handed the local path and the `media` node; without it the toolkit's player is used.
pub type PlayerSource = Rc<dyn Fn(&str, &Node) -> Option<gtk::Widget>>;
/// Answers with the widget a `canvas` node draws in, by the node's id.
pub type CanvasSource = Rc<dyn Fn(&str) -> Option<gtk::Widget>>;

/// A table the user can pick a row in. The rows are kept beside the widget
/// because a selection answers with the row as the plugin wrote it, not with
/// the strings the cells happen to show.
#[derive(Clone)]
struct TableView {
    header: gtk::Box,
    list: gtk::ListBox,
    widths: Rc<RefCell<Vec<gtk::SizeGroup>>>,
    rows: Rc<RefCell<Vec<Value>>>,
}

impl TableView {
    fn selected_row(&self) -> Option<Value> {
        let index = self.list.selected_row()?.index();
        usize::try_from(index)
            .ok()
            .and_then(|at| self.rows.borrow().get(at).cloned())
    }
}

/// A table whose rows hold rows. `ListBox` has no expanders, so this is the one
/// control built on `ListView`: a `TreeListModel` makes the children, and a
/// `TreeExpander` in front of each line draws the arrow and the indent.
///
/// Which rows are open is not kept here. The plugin says so in the data, and
/// the renderer puts the arrows back where the data says after every refresh —
/// the same rule as everything else on screen, and it survives a rebuild.
#[derive(Clone)]
struct TreeView {
    header: gtk::Box,
    view: gtk::ListView,
    root: gtk::gio::ListStore,
    rows: Rc<RefCell<Vec<Value>>>,
    widths: Rc<RefCell<Vec<gtk::SizeGroup>>>,
    /// Filled in by `on_expand` after the factory was already built, which is
    /// why it is a slot rather than a plain field.
    opening: Rc<RefCell<Option<Rc<dyn Fn(Value, bool)>>>>,
    /// Held while the arrows are being put back from the data, so restoring
    /// them is not reported as the user opening something.
    restoring: Rc<std::cell::Cell<bool>>,
    /// The child store built for each branch that has been opened. A refresh
    /// puts new children into the branch already on screen through this, rather
    /// than emptying the whole tree — which used to cost every row its
    /// expansion and the reader their place in the list.
    kids: Rc<RefCell<HashMap<String, gtk::gio::ListStore>>>,
}

/// One row of a tree, as the plugin wrote it.
struct Held {
    id: String,
    row: Value,
}

/// What makes a row the same row across refreshes. The plugin's `path` when it
/// has one: names repeat at every depth, so matching on a name alone would
/// confuse two branches the moment they sit under different parents.
fn identity(row: &Value, under: &str) -> String {
    match row.get("path").and_then(Value::as_str) {
        Some(path) => path.to_string(),
        None => format!(
            "{under}/{}",
            row.get("name").and_then(Value::as_str).unwrap_or_default()
        ),
    }
}

fn boxed_row(id: String, row: Value) -> glib::BoxedAnyObject {
    glib::BoxedAnyObject::new(Held { id, row })
}

fn held(item: &glib::Object) -> Option<Value> {
    let row = item.downcast_ref::<gtk::TreeListRow>()?;
    carried_in(&row.item()?)
}

fn carried_in(carried: &glib::Object) -> Option<Value> {
    let boxed = carried.downcast_ref::<glib::BoxedAnyObject>()?;
    let inside: std::cell::Ref<Held> = boxed.borrow();
    Some(inside.row.clone())
}

fn id_in(carried: &glib::Object) -> Option<String> {
    let boxed = carried.downcast_ref::<glib::BoxedAnyObject>()?;
    let inside: std::cell::Ref<Held> = boxed.borrow();
    Some(inside.id.clone())
}

/// Whether two versions of a row would draw the same cells. `children` and
/// `expanded` are deliberately left out: they move the rows below, not the row
/// itself, and rebinding for them is what threw the tree about.
fn same_cells(before: &Value, after: &Value) -> bool {
    let (Some(was), Some(now)) = (before.as_object(), after.as_object()) else {
        return before == after;
    };
    let shown = |key: &str| key != "children" && key != "expanded";
    let count = |map: &serde_json::Map<String, Value>| {
        map.keys().filter(|key| shown(key)).count()
    };
    count(was) == count(now)
        && was
            .iter()
            .filter(|(key, _)| shown(key))
            .all(|(key, value)| now.get(key) == Some(value))
}

fn kids_of(value: &Value) -> Option<Vec<Value>> {
    let listed = value.get("children")?.as_array()?;
    Some(listed.clone())
}

/// Whether a row can be opened at all: either it already holds children, or it
/// says it has some and is waiting to be asked.
fn opens(value: &Value) -> bool {
    value
        .get("expandable")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || kids_of(value).is_some_and(|kids| !kids.is_empty())
}

impl TreeView {
    fn selection(&self) -> Option<gtk::SingleSelection> {
        self.view.model()?.downcast::<gtk::SingleSelection>().ok()
    }

    fn selected_row(&self) -> Option<Value> {
        let chosen = self.selection()?.selected_item()?;
        held(&chosen)
    }
}

#[derive(Clone)]
enum Control {
    Plain,
    Entry(gtk::Entry),
    EntryRow(adw::EntryRow),
    PasswordRow(adw::PasswordEntryRow),
    Switch(gtk::Switch),
    Choice(gtk::DropDown, Rc<Vec<Value>>),
    Label(gtk::Label),
    Multiline(gtk::TextView),
    Table(TableView),
    Tree(TreeView),
    Chart(
        gtk::DrawingArea,
        Rc<RefCell<Vec<ic_view::Series>>>,
        Rc<RefCell<String>>,
    ),
    Button(gtk::Button),
    Pick(gtk::Entry, gtk::Button),
    /// The scale, and when a person last moved it themselves.
    Slider(gtk::Scale, Rc<std::cell::Cell<Option<std::time::Instant>>>),
}

struct Bound {
    node: Node,
    frame: gtk::Widget,
    control: Control,
}

struct BoundAction {
    id: String,
    button: gtk::Button,
    action: ic_view::Action,
}

pub struct BuiltView {
    pub root: gtk::Widget,
    bound: Vec<Bound>,
    by_id: HashMap<String, usize>,
    actions: Vec<BoundAction>,
    action_by_id: HashMap<String, usize>,
    translate: Translate,
    touched: Rc<RefCell<Vec<String>>>,
    secrets: std::collections::BTreeSet<String>,
    /// Held while the host writes a value into a control, so the change it causes
    /// is not reported back as something the user typed.
    writing: Rc<std::cell::Cell<bool>>,
    /// Players from the window as it was, by what they are playing, for as
    /// long as this one is being built.
    playing: Vec<(String, gtk::Widget)>,
    canvases: Vec<(String, gtk::Widget)>,
    icons: Option<IconSource>,
    keys: Vec<(String, String)>,
    key_pressed: KeyPressed,
}

type KeyPressed = Rc<RefCell<Option<Rc<dyn Fn(String)>>>>;

pub struct Renderer {
    translate: Translate,
    icons: Option<IconSource>,
    pictures: Option<PictureSource>,
    media: Option<MediaSource>,
    player: Option<PlayerSource>,
    canvas: Option<CanvasSource>,
}

impl Renderer {
    pub fn new(translate: Translate) -> Renderer {
        Renderer {
            translate,
            icons: None,
            pictures: None,
            media: None,
            player: None,
            canvas: None,
        }
    }

    pub fn with_icons(mut self, icons: IconSource) -> Renderer {
        self.icons = Some(icons);
        self
    }

    pub fn with_pictures(mut self, pictures: PictureSource) -> Renderer {
        self.pictures = Some(pictures);
        self
    }

    pub fn with_media(mut self, media: MediaSource) -> Renderer {
        self.media = Some(media);
        self
    }

    pub fn with_player(mut self, player: PlayerSource) -> Renderer {
        self.player = Some(player);
        self
    }

    pub fn with_canvas(mut self, canvas: CanvasSource) -> Renderer {
        self.canvas = Some(canvas);
        self
    }

    fn text(&self, conditional: Option<&Cond<Text>>, state: &State) -> Option<String> {
        resolve_text(conditional, state, &|key| (self.translate)(key))
    }

    pub fn build(&self, document: &Document, state: &State) -> BuiltView {
        self.build_over(document, state, None)
    }

    /// The same, keeping what can be kept from the window as it was.
    ///
    /// A window is rebuilt whenever the plugin changes its mind — a track
    /// chosen, a list opened — and a player built afresh starts the song from
    /// the beginning. What is playing is not part of the document, so the
    /// widget that holds it moves across rather than being made again.
    pub fn build_over(
        &self,
        document: &Document,
        state: &State,
        before: Option<&BuiltView>,
    ) -> BuiltView {
        let mut built = BuiltView {
            root: gtk::Box::new(gtk::Orientation::Vertical, 0).upcast(),
            bound: Vec::new(),
            by_id: HashMap::new(),
            actions: Vec::new(),
            action_by_id: HashMap::new(),
            translate: self.translate.clone(),
            touched: Rc::new(RefCell::new(Vec::new())),
            secrets: document
                .fields
                .iter()
                .filter(|field| field.secret)
                .map(|field| field.bind.clone())
                .collect(),
            writing: Rc::new(std::cell::Cell::new(false)),
            playing: playing_in(before, state, &|conditional| {
                resolve_text(conditional, state, &|key| (self.translate)(key))
            }),
            canvases: canvases_in(before),
            icons: self.icons.clone(),
            keys: document
                .keys
                .iter()
                .map(|key| (key.accel.to_lowercase(), key.node.clone()))
                .collect(),
            key_pressed: Rc::new(RefCell::new(None)),
        };
        let root = self.node(&document.form, document, state, &mut built);
        built.root = root;
        for action in &document.actions {
            let label = self
                .text(action.label.as_ref(), state)
                .unwrap_or_else(|| action.id.clone());
            let button = gtk::Button::builder().label(label).build();
            apply_role(
                button.upcast_ref::<gtk::Widget>(),
                self.text(action.role.as_ref(), state).as_deref(),
            );
            button.set_visible(visible(action.visible.as_ref(), state));
            button.set_sensitive(visible(action.sensitive.as_ref(), state));
            built
                .action_by_id
                .insert(action.id.clone(), built.actions.len());
            built.actions.push(BoundAction {
                id: action.id.clone(),
                button,
                action: action.clone(),
            });
        }
        built.refresh(state);
        bind_keys(&built);
        // Whatever was playing in the window as it was and did not move across
        // into this one is let go here. Holding it would leave a song playing
        // in a window nobody can see, with nothing left to stop it.
        built.playing.clear();
        built.canvases.clear();
        built
    }

    fn node(
        &self,
        node: &Node,
        document: &Document,
        state: &State,
        built: &mut BuiltView,
    ) -> gtk::Widget {
        let (frame, control) = match node.kind() {
            NodeKind::View | NodeKind::Column => self.container(node, document, state, built, true),
            NodeKind::Row => self.container(node, document, state, built, false),
            NodeKind::Group => self.group(node, document, state, built),
            NodeKind::Separator => (
                gtk::Separator::new(gtk::Orientation::Horizontal).upcast(),
                Control::Plain,
            ),
            NodeKind::Text => self.label(node, state),
            NodeKind::Icon => self.icon(node, state),
            NodeKind::Input => self.input(node, document, state),
            NodeKind::Switch => self.switch(node, state),
            NodeKind::Slider => self.slider(node, state),
            NodeKind::Choice => self.choice(node, state),
            NodeKind::Button => self.button(node, state),
            NodeKind::Table => self.table(node, state),
            NodeKind::Tree => self.tree(node, state),
            NodeKind::Chart => self.chart(node, state),
            NodeKind::Image => self.picture(node, state),
            NodeKind::Media => self.player(node, state, built),
            NodeKind::Canvas => self.canvas(node, built),
            NodeKind::Unknown => self.container(node, document, state, built, true),
        };

        if let Some(width) = node.width {
            frame.set_width_request(width as i32);
        }
        if let Some(height) = node.height {
            frame.set_height_request(height as i32);
        }
        if let Some(weight) = node.weight {
            frame.set_hexpand(weight > 0);
        }
        if let Some(tooltip) = self.text(node.tooltip.as_ref(), state) {
            frame.set_tooltip_text(Some(&tooltip));
        }
        if let Some(above) = node.margin_top {
            frame.set_margin_top(above as i32);
        }

        let mut stored = node.clone();
        stored.children.clear();
        if let Some(id) = &node.id {
            built.by_id.insert(id.clone(), built.bound.len());
        }
        built.bound.push(Bound {
            node: stored,
            frame: frame.clone(),
            control,
        });
        frame
    }

    fn container(
        &self,
        node: &Node,
        document: &Document,
        state: &State,
        built: &mut BuiltView,
        vertical: bool,
    ) -> (gtk::Widget, Control) {
        let orientation = if vertical {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        };
        let container = gtk::Box::new(orientation, node.spacing.unwrap_or(0) as i32);
        if let Some(padding) = node.padding {
            let inset = padding as i32;
            container.set_margin_top(inset);
            container.set_margin_bottom(inset);
            container.set_margin_start(inset);
            container.set_margin_end(inset);
        }
        for child in &node.children {
            let widget = self.node(child, document, state, built);
            // Weight is a share of the room going spare, and which way the
            // room goes depends on the box. In a column that is downwards: a
            // list that scrolls takes what is left under the player instead of
            // collapsing to one visible row.
            if vertical && child.weight.is_some_and(|weight| weight > 0) {
                widget.set_vexpand(true);
            }
            container.append(&widget);
        }
        // Any box may say it scrolls, not only the window's own: a playlist
        // inside a player is a column, and a column that quietly did not
        // scroll was a list the user could not reach the end of.
        if node.scroll != Scroll::None {
            let scroller = gtk::ScrolledWindow::builder()
                .child(&container)
                .vexpand(true)
                .hscrollbar_policy(if node.scroll == Scroll::Both {
                    gtk::PolicyType::Automatic
                } else {
                    gtk::PolicyType::Never
                })
                .build();
            return (scroller.upcast(), Control::Plain);
        }
        (container.upcast(), Control::Plain)
    }

    fn group(
        &self,
        node: &Node,
        document: &Document,
        state: &State,
        built: &mut BuiltView,
    ) -> (gtk::Widget, Control) {
        let group = adw::PreferencesGroup::builder().build();
        if let Some(title) = self.text(node.title.as_ref(), state) {
            group.set_title(&title);
        }
        if let Some(subtitle) = self.text(node.subtitle.as_ref(), state) {
            group.set_description(Some(&subtitle));
        }
        for child in &node.children {
            let widget = self.node(child, document, state, built);
            group.add(&widget);
        }
        (group.upcast(), Control::Plain)
    }

    fn label(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let label = gtk::Label::builder()
            .label(self.text(node.text.as_ref(), state).unwrap_or_default())
            .xalign(0.0)
            .wrap(node.wrap)
            .selectable(node.selectable)
            .build();
        apply_text_role(&label, self.text(node.role.as_ref(), state).as_deref());
        (label.clone().upcast(), Control::Label(label))
    }

    fn icon(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let name = self.text(node.icon.as_ref(), state).unwrap_or_default();
        let wanted = node.height.unwrap_or(24);
        if let Some(source) = &self.icons {
            if let Some(widget) = source(&name, wanted) {
                return (widget, Control::Plain);
            }
        }
        let bare = name.strip_prefix("theme:").unwrap_or(&name);
        let image = gtk::Image::from_icon_name(bare);
        if let Some(size) = node.height {
            image.set_pixel_size(size as i32);
        }
        (image.upcast(), Control::Plain)
    }

    /// A picture the document named. The bytes come from the host — the
    /// renderer decodes and places them, and says plainly when there is
    /// nothing to show rather than leaving a hole in the window.
    fn picture(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let named = self.text(node.src.as_ref(), state).unwrap_or_default();
        let bytes = self
            .pictures
            .as_ref()
            .filter(|_| !named.is_empty())
            .and_then(|source| source(&named));
        let Some(bytes) = bytes else {
            return (self.nothing_to_show(&named), Control::Plain);
        };
        let held = gtk::glib::Bytes::from_owned(bytes);
        let stream = gtk::gio::MemoryInputStream::from_bytes(&held);
        let drawn = match gtk::gdk_pixbuf::Pixbuf::from_stream(&stream, gtk::gio::Cancellable::NONE)
        {
            // A photograph carries which way up it was taken, and every
            // frontend has to honour it or half of them come out sideways.
            Ok(pixbuf) => gtk::gdk::Texture::for_pixbuf(
                &pixbuf.apply_embedded_orientation().unwrap_or(pixbuf),
            ),
            Err(_) => return (self.nothing_to_show(&named), Control::Plain),
        };
        // A document that said how tall the picture is means it: a sleeve of
        // 180 is 180 whatever the photograph behind it measures. `Picture`
        // asks for the size of the image it holds and would push a list of
        // songs off the window; `Image` is drawn at the size it is given.
        if let Some(height) = node.height {
            let sized = gtk::Image::from_paintable(Some(&drawn));
            sized.set_pixel_size(height as i32);
            return (sized.upcast(), Control::Plain);
        }
        let picture = gtk::Picture::new();
        picture.set_paintable(Some(&drawn));
        picture.set_content_fit(match node.fit {
            Fit::Cover => gtk::ContentFit::Cover,
            Fit::Actual => gtk::ContentFit::ScaleDown,
            Fit::Width => gtk::ContentFit::Fill,
            Fit::Contain | Fit::Unknown => gtk::ContentFit::Contain,
        });
        picture.set_can_shrink(node.fit != Fit::Actual);
        // A picture takes the room going spare only where the document did not
        // say how big it is. A sleeve given a height of 180 stays 180 tall, and
        // the list of songs underneath keeps the rest — otherwise one large
        // photograph pushes everything else off the window.
        picture.set_hexpand(node.width.is_none());
        picture.set_vexpand(node.height.is_none());
        if !node.zoom {
            return (picture.upcast(), Control::Plain);
        }
        let scroller = gtk::ScrolledWindow::builder()
            .child(&picture)
            .hexpand(true)
            .vexpand(true)
            .build();
        (scroller.upcast(), Control::Plain)
    }

    /// Sound or moving pictures. The player opens a file itself, so the host
    /// hands over a path on this machine — copying it first if the file lives
    /// on a server.
    fn player(&self, node: &Node, state: &State, built: &mut BuiltView) -> (gtk::Widget, Control) {
        let named = self.text(node.src.as_ref(), state).unwrap_or_default();
        let local = self
            .media
            .as_ref()
            .filter(|_| !named.is_empty())
            .and_then(|source| source(&named));
        let Some(local) = local else {
            return (self.nothing_to_show(&named), Control::Plain);
        };
        // Already playing this very file: the widget moves to the new window
        // rather than being built again, so the song does not start over
        // because the plugin redrew the page around it.
        if let Some(at) = built.playing.iter().position(|(held, _)| *held == named) {
            let held = built.playing.remove(at).1;
            held.unparent();
            return (held, Control::Plain);
        }
        // Playing belongs to whoever holds this view, not to a widget builder.
        let Some(ours) = self.player.as_ref().and_then(|source| source(&local, node)) else {
            return (self.nothing_to_show(&named), Control::Plain);
        };
        (ours, Control::Plain)
    }

    fn canvas(&self, node: &Node, built: &mut BuiltView) -> (gtk::Widget, Control) {
        let named = node.id.clone().unwrap_or_default();
        // Reused, not remade: a new canvas is a new GL context, and the plugin loses what it drew.
        if !named.is_empty() {
            if let Some(at) = built.canvases.iter().position(|(held, _)| *held == named) {
                let held = built.canvases.remove(at).1;
                held.unparent();
                return (held, Control::Plain);
            }
        }
        let Some(ours) = self.canvas.as_ref().and_then(|source| source(&named)) else {
            return (
                gtk::Box::new(gtk::Orientation::Vertical, 0).upcast(),
                Control::Plain,
            );
        };
        (ours, Control::Plain)
    }

    fn nothing_to_show(&self, named: &str) -> gtk::Widget {
        let label = gtk::Label::new(Some(named));
        label.add_css_class("dim-label");
        label.set_wrap(true);
        label.upcast()
    }

    fn input(&self, node: &Node, document: &Document, state: &State) -> (gtk::Widget, Control) {
        let placeholder = self.text(node.placeholder.as_ref(), state);
        let title = self.text(node.title.as_ref(), state);
        let secret = node
            .bind
            .as_ref()
            .and_then(|bind| document.field(bind))
            .map(|field| field.secret)
            .unwrap_or(false);
        let current = node
            .bind
            .as_ref()
            .filter(|bind| !(secret && state.stored_secrets.contains(bind.as_str())))
            .map(|bind| state.text_of(bind))
            .unwrap_or_default();

        if node.chrome == Chrome::Row {
            if secret || node.variant == Variant::Masked || node.variant == Variant::MaskedReveal {
                let row = adw::PasswordEntryRow::builder()
                    .title(title.unwrap_or_default())
                    .build();
                row.set_text(&current);
                return (row.clone().upcast(), Control::PasswordRow(row));
            }
            let row = adw::EntryRow::builder()
                .title(title.unwrap_or_default())
                .build();
            row.set_text(&current);
            return (row.clone().upcast(), Control::EntryRow(row));
        }

        if node.variant == Variant::Multiline {
            let area = gtk::TextView::builder()
                .monospace(true)
                .wrap_mode(gtk::WrapMode::WordChar)
                .left_margin(6)
                .right_margin(6)
                .top_margin(6)
                .bottom_margin(6)
                .build();
            area.buffer().set_text(&current);
            area.set_editable(!node.read_only);
            area.set_cursor_visible(!node.read_only);
            let holder = gtk::ScrolledWindow::builder()
                .child(&area)
                .height_request(node.height.unwrap_or(120) as i32)
                .hexpand(true)
                .build();
            holder.add_css_class("frame");
            return (holder.upcast(), Control::Multiline(area));
        }

        let entry = gtk::Entry::builder().text(&current).build();
        if let Some(hint) = placeholder {
            entry.set_placeholder_text(Some(&hint));
        }
        match node.variant {
            Variant::Masked => entry.set_visibility(false),
            Variant::MaskedReveal => {
                entry.set_visibility(false);
                entry.set_input_purpose(gtk::InputPurpose::Password);
            }
            Variant::Integer => entry.set_input_purpose(gtk::InputPurpose::Digits),
            Variant::Hex => keep_to_hex(&entry),
            _ => {}
        }
        if secret && node.variant == InputVariant::Text {
            entry.set_visibility(false);
        }
        entry.set_editable(!node.read_only);
        if node.variant != Variant::Path {
            return (entry.clone().upcast(), Control::Entry(entry));
        }

        entry.set_hexpand(true);
        let browse = gtk::Button::from_icon_name("document-open-symbolic");
        if let Some(picker) = &node.picker {
            if let Some(hint) = picker.title.as_ref() {
                let resolved = hint.resolve(&|key| (self.translate)(key));
                browse.set_tooltip_text(Some(&resolved));
            }
        }
        let holder = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        holder.append(&entry);
        holder.append(&browse);
        (holder.upcast(), Control::Pick(entry, browse))
    }

    fn switch(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let active = node
            .bind
            .as_ref()
            .and_then(|bind| state.state.get(bind))
            .map(is_truthy)
            .unwrap_or(false);
        let switch = gtk::Switch::builder()
            .active(active)
            .valign(gtk::Align::Center)
            .build();
        if node.chrome == Chrome::Row {
            let row = adw::ActionRow::builder()
                .title(self.text(node.title.as_ref(), state).unwrap_or_default())
                .build();
            row.add_suffix(&switch);
            row.set_activatable_widget(Some(&switch));
            return (row.upcast(), Control::Switch(switch));
        }
        (switch.clone().upcast(), Control::Switch(switch))
    }

    fn slider(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let low = node.min.unwrap_or(0.0);
        let high = node.max.unwrap_or(low + 1.0);
        let (low, high) = if high > low { (low, high) } else { (low, low + 1.0) };
        let step = node.step.filter(|step| *step > 0.0).unwrap_or((high - low) / 100.0);
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, low, high, step);
        scale.set_draw_value(false);
        // No hexpand here: taking the spare room is `weight`'s to say, as for every node.
        scale.set_value(position_of(node, state).unwrap_or(low));

        // `change-value` fires only for a person, never for `set_value` — which is how the
        // hand is told from the document. A click gesture cannot: GtkRange claims the sequence.
        let held: Rc<std::cell::Cell<Option<std::time::Instant>>> =
            Rc::new(std::cell::Cell::new(None));
        let touched = held.clone();
        scale.connect_change_value(move |_, _, _| {
            touched.set(Some(std::time::Instant::now()));
            gtk::glib::Propagation::Proceed
        });
        (scale.clone().upcast(), Control::Slider(scale, held))
    }

    fn choice(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let labels: Vec<String> = node
            .options
            .iter()
            .map(|option| match option.label.as_ref() {
                Some(text) => text.resolve(&|key| (self.translate)(key)),
                None => as_text(&option.value),
            })
            .collect();
        let borrowed: Vec<&str> = labels.iter().map(String::as_str).collect();
        let dropdown = gtk::DropDown::from_strings(&borrowed);
        let current = node
            .bind
            .as_ref()
            .and_then(|bind| state.state.get(bind))
            .cloned()
            .unwrap_or(Value::Null);
        dropdown.set_selected(select_index(node, &current));
        let control = Control::Choice(
            dropdown.clone(),
            Rc::new(node.options.iter().map(|o| o.value.clone()).collect()),
        );
        if node.chrome == Chrome::Row {
            let row = adw::ActionRow::builder()
                .title(self.text(node.title.as_ref(), state).unwrap_or_default())
                .build();
            row.add_suffix(&dropdown);
            return (row.upcast(), control);
        }
        (dropdown.upcast(), control)
    }

    fn button(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let button = match self.text(node.label.as_ref(), state) {
            Some(label) => gtk::Button::builder().label(label).build(),
            None => {
                let name = self.text(node.icon.as_ref(), state).unwrap_or_default();
                match wearing(self.icons.as_ref(), &name, node) {
                    Some(picture) => {
                        let button = gtk::Button::new();
                        button.set_child(Some(&picture));
                        button
                    }
                    None => {
                        let bare = name.strip_prefix("theme:").unwrap_or(&name);
                        gtk::Button::from_icon_name(bare)
                    }
                }
            }
        };
        let role = self.text(node.role.as_ref(), state);
        apply_role(button.upcast_ref::<gtk::Widget>(), role.as_deref());
        if matches!(role.as_deref(), Some("row") | Some("row_selected")) {
            as_a_row(&button);
        }
        if role.as_deref() == Some("row_selected") {
            row_look();
            button.add_css_class("ic-row-selected");
            bring_into_view(&button);
        }
        (button.clone().upcast(), Control::Button(button))
    }

    fn chart(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let area = gtk::DrawingArea::builder()
            .height_request(node.height.unwrap_or(150) as i32)
            .hexpand(true)
            .build();
        let key = node.series_key.clone().unwrap_or_default();
        let samples = Rc::new(RefCell::new(ic_view::series_of(state, &key)));
        let caption = Rc::new(RefCell::new(
            self.text(node.caption.as_ref(), state).unwrap_or_default(),
        ));
        let mut shape = node.clone();
        shape.children.clear();
        let painting = samples.clone();
        let written = caption.clone();
        area.set_draw_func(move |_, context, width, height| {
            paint_series(
                context,
                &painting.borrow(),
                &written.borrow(),
                &shape,
                width,
                height,
            );
        });
        (
            area.clone().upcast(),
            Control::Chart(area, samples, caption),
        )
    }

    fn tree(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();
        let root = gtk::gio::ListStore::new::<glib::BoxedAnyObject>();
        // Children are made on demand: a row that says it can open gets an
        // empty store, so the arrow is there before the plugin has filled it.
        let kids: Rc<RefCell<HashMap<String, gtk::gio::ListStore>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let branches = {
            let kids = kids.clone();
            gtk::TreeListModel::new(root.clone(), false, false, move |item| {
                let boxed = item.downcast_ref::<glib::BoxedAnyObject>()?;
                let (id, value) = {
                    let inside: std::cell::Ref<Held> = boxed.borrow();
                    (inside.id.clone(), inside.row.clone())
                };
                if !opens(&value) {
                    return None;
                }
                let store = gtk::gio::ListStore::new::<glib::BoxedAnyObject>();
                for kid in kids_of(&value).unwrap_or_default() {
                    let named = identity(&kid, &id);
                    store.append(&boxed_row(named, kid));
                }
                kids.borrow_mut().insert(id, store.clone());
                Some(store.upcast())
            })
        };
        let selection = gtk::SingleSelection::builder()
            .model(&branches)
            .autoselect(false)
            .can_unselect(true)
            .build();

        let widths: Rc<RefCell<Vec<gtk::SizeGroup>>> = Rc::new(RefCell::new(Vec::new()));
        let opening: Rc<RefCell<Option<Rc<dyn Fn(Value, bool)>>>> = Rc::new(RefCell::new(None));
        let restoring = Rc::new(std::cell::Cell::new(false));
        // The expansion handler of each bound list item, kept beside the row it
        // was connected to. Taking it off whatever `item.item()` holds at unbind
        // time is how this crashed: list items are recycled, and by then that is
        // a different row.
        let watched: Rc<RefCell<HashMap<usize, (gtk::TreeListRow, glib::SignalHandlerId)>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            item.set_child(Some(&gtk::TreeExpander::new()));
        });
        {
            let shape = node.clone();
            let widths = widths.clone();
            let told = opening.clone();
            let quiet = restoring.clone();
            let watched = watched.clone();
            factory.connect_bind(move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let Some(expander) = item.child().and_downcast::<gtk::TreeExpander>() else {
                    return;
                };
                let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
                    return;
                };
                expander.set_list_row(Some(&row));
                if let Some(value) = held(row.upcast_ref::<glib::Object>()) {
                    expander.set_child(Some(&cells_of(&value, &shape, &widths.borrow())));
                }
                let told = told.clone();
                let quiet = quiet.clone();
                let watch = row.connect_expanded_notify(move |row| {
                    if quiet.get() {
                        return;
                    }
                    let Some(say) = told.borrow().clone() else {
                        return;
                    };
                    let Some(value) = held(row.upcast_ref::<glib::Object>()) else {
                        return;
                    };
                    let said = value
                        .get("expanded")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if said != row.is_expanded() {
                        // Out of the click: the plugin answers by refilling this
                        // very model, and tearing it down inside GTK's own
                        // expansion bookkeeping is what crashed before.
                        let open = row.is_expanded();
                        glib::idle_add_local_once(move || say(value, open));
                    }
                });
                let before = watched.borrow_mut().insert(item.as_ptr() as usize, (row, watch));
                if let Some((was, old)) = before {
                    was.disconnect(old);
                }
            });
        }
        {
            let watched = watched.clone();
            factory.connect_unbind(move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let gone = watched.borrow_mut().remove(&(item.as_ptr() as usize));
                if let Some((row, watch)) = gone {
                    row.disconnect(watch);
                }
            });
        }

        let view = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .single_click_activate(false)
            .build();
        let holder = gtk::ScrolledWindow::builder()
            .child(&view)
            .hexpand(true)
            .vexpand(true)
            .build();
        let whole = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();
        whole.append(&header);
        whole.append(&holder);

        let tree = TreeView {
            header,
            view,
            root,
            rows: Rc::new(RefCell::new(Vec::new())),
            widths,
            opening,
            restoring,
            kids,
        };
        fill_tree(&tree, node, state, &|key| (self.translate)(key));
        (whole.upcast(), Control::Tree(tree))
    }

    fn table(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(if picks_rows(node) {
                gtk::SelectionMode::Single
            } else {
                gtk::SelectionMode::None
            })
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();
        root.append(&header);
        root.append(&list);
        // Without this a weightless table pins its rows to the top and reads as misaligned.
        if !node.weight.is_some_and(|weight| weight > 0) {
            root.set_valign(gtk::Align::Center);
        }
        let table = TableView {
            header,
            list,
            widths: Rc::new(RefCell::new(Vec::new())),
            rows: Rc::new(RefCell::new(Vec::new())),
        };
        fill_table(&table, node, state, &|key| (self.translate)(key));
        (root.upcast(), Control::Table(table))
    }
}

fn paint_series(
    context: &gtk::cairo::Context,
    series: &[ic_view::Series],
    caption: &str,
    node: &Node,
    width: i32,
    height: i32,
) {
    let (low, high) = ic_view::series_bounds(series, node);
    let span = high - low;
    let across = f64::from(width);
    let down = f64::from(height);

    context.set_source_rgba(0.5, 0.5, 0.5, 0.15);
    context.set_line_width(1.0);
    for line in 1..4 {
        let y = down * f64::from(line) / 4.0;
        context.move_to(0.0, y);
        context.line_to(across, y);
    }
    let _ = context.stroke();

    for drawn in series {
        if drawn.values.len() < 2 {
            continue;
        }
        let [red, green, blue, alpha] = drawn.color.unwrap_or([0.2, 0.6, 1.0, 0.8]);
        let step = across / (drawn.values.len() - 1) as f64;
        let height_of = |value: f64| down - ((value - low) / span).clamp(0.0, 1.0) * down;

        context.set_source_rgba(red, green, blue, alpha * 0.25);
        context.move_to(0.0, down);
        for (index, value) in drawn.values.iter().enumerate() {
            context.line_to(index as f64 * step, height_of(*value));
        }
        context.line_to(across, down);
        context.close_path();
        let _ = context.fill();

        context.set_source_rgba(red, green, blue, alpha);
        context.set_line_width(1.5);
        for (index, value) in drawn.values.iter().enumerate() {
            let x = index as f64 * step;
            let y = height_of(*value);
            if index == 0 {
                context.move_to(x, y);
            } else {
                context.line_to(x, y);
            }
        }
        let _ = context.stroke();
    }

    if !caption.is_empty() {
        context.set_source_rgba(0.45, 0.45, 0.45, 0.9);
        context.set_font_size(11.0);
        context.move_to(6.0, 15.0);
        let _ = context.show_text(caption);
    }
}

fn multiline_text(area: &gtk::TextView) -> String {
    let buffer = area.buffer();
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}

/// What a `hex` input is allowed to hold. Spaces are kept so bytes can be
/// grouped; everything else is dropped as it is typed or pasted.
pub fn hex_only(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_hexdigit() || *c == ' ')
        .collect()
}

fn keep_to_hex(entry: &gtk::Entry) {
    entry.add_css_class("monospace");
    // Rewriting the text inside `changed` fires it again, so the guard stops
    // the second pass rather than letting it chase its own tail.
    let fixing = Rc::new(std::cell::Cell::new(false));
    entry.clone().connect_changed(move |widget| {
        if fixing.get() {
            return;
        }
        let shown = widget.text().to_string();
        let kept = hex_only(&shown);
        if kept == shown {
            return;
        }
        let at = widget.position();
        fixing.set(true);
        widget.set_text(&kept);
        widget.set_position(at.min(kept.chars().count() as i32));
        fixing.set(false);
    });
}

/// Column headings shared by `table` and `tree`, each heading joined to the
/// cells under it by a size group so the columns line up.
/// One size group per column, shared by the heading and every cell under it, so
/// the columns line up across rows that know nothing about each other.
fn fill_headings(
    header: &gtk::Box,
    node: &Node,
    widths: &mut Vec<gtk::SizeGroup>,
    translate: &dyn Fn(&str) -> Option<String>,
) {
    if widths.len() != node.columns.len() {
        *widths = node
            .columns
            .iter()
            .map(|_| gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal))
            .collect();
    }
    while let Some(child) = header.first_child() {
        header.remove(&child);
    }
    let mut any = false;
    for (column, spec) in node.columns.iter().enumerate() {
        let heading = gtk::Label::builder()
            .label(
                spec.title
                    .as_ref()
                    .map(|title| title.resolve(translate))
                    .unwrap_or_default(),
            )
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(CELL_CHARS)
            .build();
        heading.add_css_class("heading");
        if let Some(width) = spec.width {
            heading.set_width_request(width as i32);
        }
        if let Some(group) = widths.get(column) {
            group.add_widget(&heading);
        }
        any |= spec.title.is_some();
        header.append(&heading);
    }
    header.set_visible(any);
}

/// How much room a cell may ask for. One long value used to widen its column —
/// and with it the whole panel — past anything useful, so cells are cut with an
/// ellipsis instead and the full text stays in the tooltip.
const CELL_CHARS: i32 = 40;

/// A row's cells, as one line. The tree indents this; the table does not.
fn cells_of(row: &Value, node: &Node, widths: &[gtk::SizeGroup]) -> gtk::Box {
    let line = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    for (column, spec) in node.columns.iter().enumerate() {
        let cell = row.get(&spec.key).map(as_text).unwrap_or_default();
        let label = gtk::Label::builder()
            .label(&cell)
            .xalign(0.0)
            .selectable(node.selectable)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(CELL_CHARS)
            .build();
        if cell.chars().count() as i32 > CELL_CHARS {
            label.set_tooltip_text(Some(&cell));
        }
        if let Some(width) = spec.width {
            label.set_width_request(width as i32);
        }
        if let Some(group) = widths.get(column) {
            group.add_widget(&label);
        }
        line.append(&label);
    }
    line
}

fn rows_in(node: &Node, state: &State) -> Vec<Value> {
    node.rows_key
        .as_ref()
        .and_then(|key| state.data.get(key))
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

fn fill_tree(
    tree: &TreeView,
    node: &Node,
    state: &State,
    translate: &dyn Fn(&str) -> Option<String>,
) {
    {
        let mut widths = tree.widths.borrow_mut();
        fill_headings(&tree.header, node, &mut widths, translate);
    }
    // Held across the whole update: putting arrows back is the host writing,
    // not the user opening anything.
    tree.restoring.set(true);
    let rows = rows_in(node, state);
    let mut still_there = std::collections::HashSet::new();
    sync_level(&tree.root, &rows, "", &tree.kids, &mut still_there);
    tree.kids
        .borrow_mut()
        .retain(|branch, _| still_there.contains(branch));
    *tree.rows.borrow_mut() = rows;
    open_what_the_data_says(tree);
    tree.restoring.set(false);
}

/// Brings one level of the tree to what the data says, touching only what
/// actually differs.
///
/// Rebuilding instead would be two lines, and it is what this used to do: every
/// row destroyed and made again, so the arrows closed and the list jumped back
/// to the top each time a branch was opened. Rows that are still there are left
/// alone — the ones that disappeared go, the new ones are put in beside them,
/// and a branch that is already open has its children brought up to date in the
/// store it is already showing.
fn sync_level(
    store: &gtk::gio::ListStore,
    rows: &[Value],
    under: &str,
    kids: &Rc<RefCell<HashMap<String, gtk::gio::ListStore>>>,
    still_there: &mut std::collections::HashSet<String>,
) {
    let named: Vec<String> = rows.iter().map(|row| identity(row, under)).collect();
    still_there.extend(named.iter().cloned());
    let wanted: std::collections::HashSet<&str> =
        named.iter().map(String::as_str).collect();

    // Backwards, so the places of the rows not yet looked at keep their numbers.
    for at in (0..store.n_items()).rev() {
        let gone = store
            .item(at)
            .and_then(|carried| id_in(&carried))
            .is_none_or(|id| !wanted.contains(id.as_str()));
        if gone {
            store.remove(at);
        }
    }

    // What is left is in the same order as the data, so one pass settles it.
    for (at, (row, id)) in rows.iter().zip(named.iter()).enumerate() {
        let at = at as u32;
        let standing = store
            .item(at)
            .and_then(|carried| id_in(&carried).map(|had| (carried, had)));
        match standing {
            Some((carried, had)) if had == *id => reseat(&carried, store, at, id, row),
            _ => store.splice(at, 0, &[boxed_row(id.clone(), row.clone())]),
        }
        let opened = kids.borrow().get(id).cloned();
        if let Some(opened) = opened {
            let children = kids_of(row).unwrap_or_default();
            sync_level(&opened, &children, id, kids, still_there);
        }
    }
}

/// Puts the newest version of a row into the one already on screen. Quietly
/// when only its children or its arrow changed: saying so would have the list
/// drop the row and build it again, taking the branch's open state with it.
fn reseat(carried: &glib::Object, store: &gtk::gio::ListStore, at: u32, id: &str, row: &Value) {
    let Some(boxed) = carried.downcast_ref::<glib::BoxedAnyObject>() else {
        return;
    };
    let same = {
        let inside: std::cell::Ref<Held> = boxed.borrow();
        same_cells(&inside.row, row)
    };
    if same {
        let mut inside: std::cell::RefMut<Held> = boxed.borrow_mut();
        inside.row = row.clone();
    } else {
        store.splice(at, 1, &[boxed_row(id.to_string(), row.clone())]);
    }
}

/// Puts the arrows back where the plugin says they belong. Walking the rows
/// while expanding them is deliberate: opening one makes its children appear
/// further down the same list, so the count grows as we go.
fn open_what_the_data_says(tree: &TreeView) {
    let Some(selection) = tree.selection() else {
        return;
    };
    let Some(model) = selection.model() else {
        return;
    };
    let mut at = 0;
    while at < model.n_items() {
        if let Some(row) = model.item(at).and_downcast::<gtk::TreeListRow>() {
            let wants = row
                .item()
                .and_then(|carried| carried_in(&carried))
                .and_then(|value| value.get("expanded").and_then(Value::as_bool))
                .unwrap_or(false);
            if wants != row.is_expanded() {
                row.set_expanded(wants);
            }
        }
        at += 1;
    }
}

/// Whether a table answers back. A table nobody listens to stays as it was:
/// plain cells, nothing to select, no row under the pointer lighting up.
fn picks_rows(node: &Node) -> bool {
    node.bind.is_some() || matches!(node.intent.as_ref(), Some(Cond::Fixed(ic_view::Intent::Emit { .. })))
}

fn fill_table(
    table: &TableView,
    node: &Node,
    state: &State,
    translate: &dyn Fn(&str) -> Option<String>,
) {
    {
        let mut widths = table.widths.borrow_mut();
        fill_headings(&table.header, node, &mut widths, translate);
    }

    while let Some(child) = table.list.first_child() {
        table.list.remove(&child);
    }
    let rows = rows_in(node, state);
    let interactive = picks_rows(node);
    {
        let widths = table.widths.borrow();
        for row in &rows {
            table.list.append(
                &gtk::ListBoxRow::builder()
                    .child(&cells_of(row, node, &widths))
                    .activatable(interactive)
                    .selectable(interactive)
                    .build(),
            );
        }
    }
    *table.rows.borrow_mut() = rows;
}

fn select_index(node: &Node, current: &Value) -> u32 {
    choice_index(node, current).unwrap_or(0) as u32
}

/// Binds the keys the document's own nodes named.
///
/// Only while the window is open, and only when nothing is being typed into:
/// an arrow key belongs to whoever is in a text field, and a plugin that asked
/// for `Left` did not mean "instead of moving the cursor".
/// What the window as it was is playing, ready to be taken over.
fn playing_in(
    before: Option<&BuiltView>,
    _state: &State,
    said: &dyn Fn(Option<&Cond<Text>>) -> Option<String>,
) -> Vec<(String, gtk::Widget)> {
    let Some(before) = before else {
        return Vec::new();
    };
    before
        .bound
        .iter()
        .filter(|held| held.node.kind() == NodeKind::Media)
        .filter_map(|held| Some((said(held.node.src.as_ref())?, held.frame.clone())))
        .collect()
}

fn canvases_in(before: Option<&BuiltView>) -> Vec<(String, gtk::Widget)> {
    let Some(before) = before else {
        return Vec::new();
    };
    before
        .bound
        .iter()
        .filter(|held| held.node.kind() == NodeKind::Canvas)
        .filter_map(|held| Some((held.node.id.clone()?, held.frame.clone())))
        .filter(|(named, _)| !named.is_empty())
        .collect()
}

fn bind_keys(built: &BuiltView) {
    let mut bound: Vec<(String, gtk::Button)> = Vec::new();
    for held in &built.bound {
        let Some(accel) = held.node.accel.as_deref() else {
            continue;
        };
        if let Control::Button(button) = &held.control {
            bound.push((accel.to_lowercase(), button.clone()));
        }
    }
    if !built.keys.is_empty() {
        let pressing = built.document_keys();
        // Bubble, so a focused slider or list inside the view keeps its arrows.
        let controller = gtk::EventControllerKey::new();
        controller.connect_key_pressed(move |_, key, _, state| {
            if pressing.press(key, state) {
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        built.root.add_controller(controller);
    }
    if bound.is_empty() {
        return;
    }
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let root = built.root.clone();
    keys.connect_key_pressed(move |_, key, _, state| {
        if being_typed_into(&root) {
            return gtk::glib::Propagation::Proceed;
        }
        let Some(pressed) = pressed_as_written(key, state) else {
            return gtk::glib::Propagation::Proceed;
        };
        match bound.iter().find(|(named, _)| *named == pressed) {
            Some((_, button)) if button.is_sensitive() && button.is_visible() => {
                button.emit_clicked();
                gtk::glib::Propagation::Stop
            }
            _ => gtk::glib::Propagation::Proceed,
        }
    });
    built.root.add_controller(keys);
}

/// The document's `keys`, pressable from outside the view, such as from the window holding it.
#[derive(Clone)]
pub struct DocumentKeys {
    keys: Vec<(String, String)>,
    handler: KeyPressed,
    root: gtk::glib::WeakRef<gtk::Widget>,
}

impl DocumentKeys {
    /// Hands the key's node to the `on_key` handler; false when the document did not declare it.
    pub fn press(&self, key: gtk::gdk::Key, state: gtk::gdk::ModifierType) -> bool {
        if self.keys.is_empty() {
            return false;
        }
        if self
            .root
            .upgrade()
            .is_some_and(|root| being_typed_into(&root))
        {
            return false;
        }
        let Some(pressed) = pressed_as_written(key, state) else {
            return false;
        };
        let Some((_, node)) = self.keys.iter().find(|(named, _)| *named == pressed) else {
            return false;
        };
        let Some(handler) = self.handler.borrow().clone() else {
            return false;
        };
        handler(node.clone());
        true
    }
}

/// Whether the keyboard belongs to something the user is writing in.
fn being_typed_into(root: &gtk::Widget) -> bool {
    let Some(window) = root.root().and_downcast::<gtk::Window>() else {
        return false;
    };
    let Some(focused) = gtk::prelude::GtkWindowExt::focus(&window) else {
        return false;
    };
    focused.is::<gtk::Editable>() || focused.is::<gtk::TextView>()
}

/// The key as a document would write it: `left`, `space`, `ctrl+right`.
fn pressed_as_written(key: gtk::gdk::Key, state: gtk::gdk::ModifierType) -> Option<String> {
    let name = key.name()?.to_lowercase();
    let mut said = String::new();
    if state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
        said.push_str("ctrl+");
    }
    if state.contains(gtk::gdk::ModifierType::ALT_MASK) {
        said.push_str("alt+");
    }
    if state.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
        said.push_str("shift+");
    }
    said.push_str(&name);
    Some(said)
}

fn wearing(icons: Option<&IconSource>, name: &str, node: &Node) -> Option<gtk::Widget> {
    icons?(name, node.height.unwrap_or(24))
}

fn apply_role(widget: &gtk::Widget, role: Option<&str>) {
    for class in ["suggested-action", "destructive-action", "flat", "pill"] {
        widget.remove_css_class(class);
    }
    match role {
        Some("primary") => widget.add_css_class("suggested-action"),
        Some("destructive") => widget.add_css_class("destructive-action"),
        Some("flat") | Some("row") | Some("row_selected") => widget.add_css_class("flat"),
        Some("pill") => widget.add_css_class("pill"),
        _ => {}
    }
}

/// How tall a row of a list is.
///
/// Fixed, not a share of what is left: a list of songs with the window made
/// short is a list that scrolls, not one whose rows are squeezed until the
/// names cannot be read.
const ROW_HEIGHT: i32 = 38;

/// The look of a list, loaded once: a line that is the one in play is marked
/// by colour rather than by being drawn in another font, which is what a list
/// of anything does.
fn row_look() {
    static LOADED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    LOADED.get_or_init(|| {
        let Some(display) = gtk::gdk::Display::default() else {
            return;
        };
        let provider = gtk::CssProvider::new();
        provider.load_from_string(
            ".ic-row-selected {
                background-color: alpha(@accent_bg_color, 0.20);
                font-weight: bold;
            }",
        );
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
}

/// Puts the row where it can be seen when the list it is in first appears.
///
/// Opening a list of a hundred songs at the top, with the one playing far
/// below, is opening it in the wrong place.
fn bring_into_view(row: &gtk::Button) {
    row.connect_map(|row| {
        let row = row.clone();
        gtk::glib::idle_add_local_once(move || {
            let Some(scroller) = row
                .ancestor(gtk::ScrolledWindow::static_type())
                .and_downcast::<gtk::ScrolledWindow>()
            else {
                return;
            };
            let Some(child) = scroller.child() else {
                return;
            };
            let Some(at) = row
                .compute_point(&child, &gtk::graphene::Point::new(0.0, 0.0))
                .map(|point| point.y() as f64)
            else {
                return;
            };
            let along = scroller.vadjustment();
            let page = along.page_size();
            let room = (along.upper() - page).max(along.lower());
            // In the middle of what can be seen, rather than at its very top.
            let wanted = at - (page - row.height() as f64) / 2.0;
            along.set_value(wanted.clamp(along.lower(), room));
        });
    });
}

/// Makes a button read as a line in a list rather than as a button: no frame,
/// the text where the eye looks for it, and the same height whatever else
/// happens to the window.
fn as_a_row(button: &gtk::Button) {
    button.set_hexpand(true);
    button.set_halign(gtk::Align::Fill);
    button.set_valign(gtk::Align::Start);
    button.set_size_request(-1, ROW_HEIGHT);
    if let Some(label) = button.child().and_downcast::<gtk::Label>() {
        label.set_halign(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    }
}

fn apply_text_role(label: &gtk::Label, role: Option<&str>) {
    for class in [
        "title-1",
        "title-2",
        "heading",
        "body",
        "caption",
        "dim-label",
        "monospace",
        "error",
    ] {
        label.remove_css_class(class);
    }
    let class = match role {
        Some("title1") => "title-1",
        Some("title2") => "title-2",
        Some("heading") => "heading",
        Some("body") => "body",
        Some("caption") => "caption",
        Some("dim") => "dim-label",
        Some("mono") => "monospace",
        Some("error") => "error",
        _ => return,
    };
    label.add_css_class(class);
}

impl BuiltView {
    pub fn ids(&self) -> Vec<&str> {
        self.by_id.keys().map(String::as_str).collect()
    }

    pub fn widget(&self, id: &str) -> Option<gtk::Widget> {
        self.by_id
            .get(id)
            .map(|index| self.bound[*index].frame.clone())
    }

    pub fn entry(&self, id: &str) -> Option<gtk::Entry> {
        match &self.slot(id)?.control {
            Control::Entry(entry) | Control::Pick(entry, _) => Some(entry.clone()),
            _ => None,
        }
    }

    pub fn action(&self, id: &str) -> Option<gtk::Button> {
        self.action_by_id
            .get(id)
            .map(|index| self.actions[*index].button.clone())
    }

    pub fn action_ids(&self) -> Vec<&str> {
        self.actions.iter().map(|entry| entry.id.as_str()).collect()
    }

    fn slot(&self, id: &str) -> Option<&Bound> {
        self.by_id.get(id).map(|index| &self.bound[*index])
    }

    pub fn value(&self, id: &str) -> Option<Value> {
        let bound = self.slot(id)?;
        read_control(&bound.control)
    }

    pub fn values(&self) -> BTreeMap<String, Value> {
        let mut collected = BTreeMap::new();
        for bound in &self.bound {
            let Some(bind) = bound.node.bind.as_ref() else {
                continue;
            };
            if let Some(value) = read_control(&bound.control) {
                collected.insert(bind.clone(), value);
            }
        }
        collected
    }

    pub fn set_value(&self, id: &str, value: &Value) -> bool {
        let Some(bound) = self.slot(id) else {
            return false;
        };
        let _writing = self.hold();
        write_control(bound, value)
    }

    fn hold(&self) -> Writing {
        self.writing.set(true);
        Writing(self.writing.clone())
    }

    pub fn touched(&self) -> Vec<String> {
        self.touched.borrow().clone()
    }
}

struct Writing(Rc<std::cell::Cell<bool>>);

impl Drop for Writing {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// Whether the state is what fills this control. A label draws its own text and a
/// table its own rows, so neither is written from a bind.
fn position_of(node: &Node, state: &State) -> Option<f64> {
    if let Some(key) = node.value_key.as_deref() {
        return state.data.get(key).and_then(Value::as_f64);
    }
    node.bind
        .as_ref()
        .and_then(|bind| state.state.get(bind))
        .and_then(Value::as_f64)
}

/// Long enough for a debounced plugin to answer, short enough not to freeze an outside change.
const STILL_IN_HAND: std::time::Duration = std::time::Duration::from_millis(700);

/// While a person works a slider nothing writes into it, or the handle jumps from under them.
fn under_the_hand(held: &std::cell::Cell<Option<std::time::Instant>>) -> bool {
    held.get()
        .is_some_and(|when| when.elapsed() < STILL_IN_HAND)
}

fn number(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

fn takes_state(control: &Control) -> bool {
    matches!(
        control,
        Control::Entry(_)
            | Control::Pick(_, _)
            | Control::EntryRow(_)
            | Control::PasswordRow(_)
            | Control::Switch(_)
            | Control::Choice(_, _)
            | Control::Multiline(_)
    )
}

fn write_control(bound: &Bound, value: &Value) -> bool {
    match &bound.control {
        Control::Entry(entry) | Control::Pick(entry, _) => {
            let written = as_text(value);
            if entry.text() != written {
                entry.set_text(&written);
            }
        }
        Control::EntryRow(row) => {
            let written = as_text(value);
            if row.text() != written {
                row.set_text(&written);
            }
        }
        Control::PasswordRow(row) => {
            let written = as_text(value);
            if row.text() != written {
                row.set_text(&written);
            }
        }
        Control::Switch(switch) => {
            let wanted = is_truthy(value);
            if switch.is_active() != wanted {
                switch.set_active(wanted);
            }
        }
        Control::Choice(dropdown, _) => {
            let wanted = select_index(&bound.node, value);
            if dropdown.selected() != wanted {
                dropdown.set_selected(wanted);
            }
        }
        Control::Multiline(area) => {
            let written = as_text(value);
            if written != multiline_text(area) {
                area.buffer().set_text(&written);
            }
        }
        Control::Label(label) => label.set_label(&as_text(value)),
        Control::Slider(scale, _) => {
            if let Some(wanted) = value.as_f64() {
                if (scale.value() - wanted).abs() > f64::EPSILON {
                    scale.set_value(wanted);
                }
            }
        }
        Control::Table(table) => {
            let wanted = table
                .rows
                .borrow()
                .iter()
                .position(|row| row == value || row.get("key").is_some_and(|key| key == value));
            match wanted.and_then(|at| i32::try_from(at).ok()) {
                Some(at) => table.list.select_row(table.list.row_at_index(at).as_ref()),
                None => table.list.unselect_all(),
            }
        }
        Control::Tree(tree) => {
            let Some(selection) = tree.selection() else {
                return false;
            };
            // Walking the rows to find the one the host named: a tree has no
            // index into the data, only the rows it is showing right now.
            let mut at = 0;
            let mut found = false;
            while let Some(row) = selection.item(at) {
                if held(&row).as_ref() == Some(value) {
                    selection.set_selected(at);
                    found = true;
                    break;
                }
                at += 1;
            }
            if !found {
                selection.set_selected(gtk::INVALID_LIST_POSITION);
            }
        }
        Control::Plain | Control::Button(_) | Control::Chart(_, _, _) => return false,
    }
    true
}

impl BuiltView {
    /// Nothing to follow otherwise, and following it anyway redraws on every twitch of the mouse.
    pub fn keeps_a_value(&self, id: &str) -> bool {
        self.slot(id)
            .is_some_and(|bound| bound.node.bind.is_some())
    }

    pub fn on_change(&self, id: &str, handler: impl Fn(String, Value) + 'static) -> bool {
        let Some(bound) = self.slot(id) else {
            return false;
        };
        let bind = bound.node.bind.clone().unwrap_or_else(|| id.to_string());
        let touched = self.touched.clone();
        let writing = self.writing.clone();
        let note = move |key: &String| -> bool {
            if writing.get() {
                return false;
            }
            let mut seen = touched.borrow_mut();
            if !seen.contains(key) {
                seen.push(key.clone());
            }
            true
        };
        match &bound.control {
            Control::Entry(entry) | Control::Pick(entry, _) => {
                let entry = entry.clone();
                entry.clone().connect_changed(move |widget| {
                    if !note(&bind) {
                        return;
                    }
                    handler(bind.clone(), Value::String(widget.text().to_string()));
                });
            }
            Control::Multiline(area) => {
                let buffer = area.buffer();
                buffer.connect_changed(move |buffer| {
                    if !note(&bind) {
                        return;
                    }
                    let written = buffer
                        .text(&buffer.start_iter(), &buffer.end_iter(), false)
                        .to_string();
                    handler(bind.clone(), Value::String(written));
                });
            }
            Control::EntryRow(row) => {
                let row = row.clone();
                row.clone().connect_changed(move |widget| {
                    if !note(&bind) {
                        return;
                    }
                    handler(bind.clone(), Value::String(widget.text().to_string()));
                });
            }
            Control::PasswordRow(row) => {
                let row = row.clone();
                row.clone().connect_changed(move |widget| {
                    if !note(&bind) {
                        return;
                    }
                    handler(bind.clone(), Value::String(widget.text().to_string()));
                });
            }
            Control::Switch(switch) => {
                switch.clone().connect_active_notify(move |widget| {
                    if !note(&bind) {
                        return;
                    }
                    handler(bind.clone(), Value::Bool(widget.is_active()));
                });
            }
            Control::Choice(dropdown, values) => {
                let values = values.clone();
                dropdown.clone().connect_selected_notify(move |widget| {
                    if !note(&bind) {
                        return;
                    }
                    let picked = values
                        .get(widget.selected() as usize)
                        .cloned()
                        .unwrap_or(Value::Null);
                    handler(bind.clone(), picked);
                });
            }
            Control::Slider(scale, _) => {
                scale.clone().connect_value_changed(move |widget| {
                    if !note(&bind) {
                        return;
                    }
                    handler(bind.clone(), number(widget.value()));
                });
            }
            Control::Table(table) => {
                let table = table.clone();
                table.list.clone().connect_selected_rows_changed(move |_| {
                    if !note(&bind) {
                        return;
                    }
                    handler(bind.clone(), table.selected_row().unwrap_or(Value::Null));
                });
            }
            Control::Tree(tree) => {
                let Some(selection) = tree.selection() else {
                    return false;
                };
                let tree = tree.clone();
                selection.connect_selected_item_notify(move |_| {
                    if !note(&bind) {
                        return;
                    }
                    handler(bind.clone(), tree.selected_row().unwrap_or(Value::Null));
                });
            }
            Control::Plain
            | Control::Button(_)
            | Control::Label(_)
            | Control::Chart(_, _, _) => return false,
        }
        true
    }

    pub fn on_activate(&self, id: &str, handler: impl Fn(String) + 'static) -> bool {
        let Some(bound) = self.slot(id) else {
            return false;
        };
        let owned = id.to_string();
        match &bound.control {
            Control::Button(button) => {
                button
                    .clone()
                    .connect_clicked(move |_| handler(owned.clone()));
                true
            }
            Control::Pick(_, browse) => {
                browse
                    .clone()
                    .connect_clicked(move |_| handler(owned.clone()));
                true
            }
            Control::Entry(entry) => {
                entry
                    .clone()
                    .connect_activate(move |_| handler(owned.clone()));
                true
            }
            Control::Table(table) => {
                table
                    .list
                    .clone()
                    .connect_row_activated(move |_, _| handler(owned.clone()));
                true
            }
            Control::Tree(tree) => {
                tree.view
                    .clone()
                    .connect_activate(move |_, _| handler(owned.clone()));
                true
            }
            _ => false,
        }
    }

    /// A row of a tree was opened or closed. Answers false for anything that
    /// is not a tree, so the caller can offer it to every node it watches.
    pub fn on_expand(&self, id: &str, handler: impl Fn(Value, bool) + 'static) -> bool {
        let Some(bound) = self.slot(id) else {
            return false;
        };
        let Control::Tree(tree) = &bound.control else {
            return false;
        };
        *tree.opening.borrow_mut() = Some(Rc::new(handler));
        true
    }

    pub fn on_key(&self, handler: impl Fn(String) + 'static) {
        *self.key_pressed.borrow_mut() = Some(Rc::new(handler));
    }

    pub fn document_keys(&self) -> DocumentKeys {
        DocumentKeys {
            keys: self.keys.clone(),
            handler: self.key_pressed.clone(),
            root: self.root.downgrade(),
        }
    }

    /// The id of the node a widget handed out by the `PlayerSource` stands for in this build.
    pub fn id_holding(&self, widget: &gtk::Widget) -> Option<String> {
        self.bound
            .iter()
            .find(|held| held.frame == *widget)
            .and_then(|held| held.node.id.clone())
    }

    pub fn on_action(&self, id: &str, handler: impl Fn(String) + 'static) -> bool {
        let Some(index) = self.action_by_id.get(id) else {
            return false;
        };
        let owned = id.to_string();
        self.actions[*index]
            .button
            .connect_clicked(move |_| handler(owned.clone()));
        true
    }

    /// Follows the state into the controls, so what a plugin sets is what the next
    /// frame shows and what the form commits. Nothing written here is typing, and a
    /// masked box is left empty when the record behind the form already holds it.
    fn follow_state(&self, state: &State) {
        let _writing = self.hold();
        for bound in &self.bound {
            let Some(bind) = bound.node.bind.as_ref() else {
                continue;
            };
            if !takes_state(&bound.control) {
                continue;
            }
            if self.secrets.contains(bind) && state.stored_secrets.contains(bind) {
                continue;
            }
            let Some(fresh) = state.state.get(bind) else {
                continue;
            };
            write_control(bound, fresh);
        }
    }

    pub fn refresh(&self, state: &State) {
        self.follow_state(state);
        let translate = self.translate.clone();
        let text = |conditional: Option<&Cond<Text>>| -> Option<String> {
            resolve_text(conditional, state, &|key| translate(key))
        };
        for bound in &self.bound {
            bound
                .frame
                .set_visible(visible(bound.node.visible.as_ref(), state));
            bound
                .frame
                .set_sensitive(visible(bound.node.sensitive.as_ref(), state));
            if let Some(tooltip) = text(bound.node.tooltip.as_ref()) {
                bound.frame.set_tooltip_text(Some(&tooltip));
            }
            match &bound.control {
                Control::Entry(entry) | Control::Pick(entry, _) => {
                    if let Some(hint) = text(bound.node.placeholder.as_ref()) {
                        entry.set_placeholder_text(Some(&hint));
                    }
                }
                Control::EntryRow(row) => {
                    if let Some(title) = text(bound.node.title.as_ref()) {
                        row.set_title(&title);
                    }
                }
                Control::PasswordRow(row) => {
                    if let Some(title) = text(bound.node.title.as_ref()) {
                        row.set_title(&title);
                    }
                }
                Control::Label(label) => {
                    if let Some(shown) = text(bound.node.text.as_ref()) {
                        label.set_label(&shown);
                    }
                    apply_text_role(label, text(bound.node.role.as_ref()).as_deref());
                }
                Control::Button(button) => {
                    if let Some(label) = text(bound.node.label.as_ref()) {
                        button.set_label(&label);
                    } else if let Some(name) = text(bound.node.icon.as_ref()) {
                        if let Some(picture) = wearing(self.icons.as_ref(), &name, &bound.node) {
                            button.set_child(Some(&picture));
                        }
                    }
                    apply_role(
                        button.upcast_ref::<gtk::Widget>(),
                        text(bound.node.role.as_ref()).as_deref(),
                    );
                }
                Control::Table(table) => {
                    fill_table(table, &bound.node, state, &|key| translate(key))
                }
                Control::Tree(tree) => {
                    fill_tree(tree, &bound.node, state, &|key| translate(key))
                }
                Control::Chart(area, samples, caption) => {
                    if let Some(key) = bound.node.series_key.as_deref() {
                        *samples.borrow_mut() = ic_view::series_of(state, key);
                    }
                    if let Some(written) = text(bound.node.caption.as_ref()) {
                        *caption.borrow_mut() = written;
                    }
                    area.queue_draw();
                }
                Control::Slider(scale, held) => {
                    if !under_the_hand(held) {
                        // Must not look like a person moving it, or the owner hears its own answer.
                        let _writing = self.hold();
                        if let Some(at) = position_of(&bound.node, state) {
                            if (scale.value() - at).abs() > f64::EPSILON {
                                scale.set_value(at);
                            }
                        }
                    }
                }
                Control::Switch(_)
                | Control::Choice(_, _)
                | Control::Multiline(_)
                | Control::Plain => {}
            }
        }
        for entry in &self.actions {
            entry
                .button
                .set_visible(visible(entry.action.visible.as_ref(), state));
            entry
                .button
                .set_sensitive(visible(entry.action.sensitive.as_ref(), state));
            if let Some(label) = text(entry.action.label.as_ref()) {
                entry.button.set_label(&label);
            }
            apply_role(
                entry.button.upcast_ref::<gtk::Widget>(),
                text(entry.action.role.as_ref()).as_deref(),
            );
        }
    }

    pub fn intent(&self, id: &str, state: &State) -> Option<ic_view::Intent> {
        let index = self.action_by_id.get(id)?;
        resolve(self.actions[*index].action.intent.as_ref()?, state)
    }

    pub fn node_intent(&self, id: &str, state: &State) -> Option<ic_view::Intent> {
        let bound = self.slot(id)?;
        resolve(bound.node.intent.as_ref()?, state)
    }
}

fn read_control(control: &Control) -> Option<Value> {
    match control {
        Control::Entry(entry) | Control::Pick(entry, _) => {
            Some(Value::String(entry.text().to_string()))
        }
        Control::Multiline(area) => Some(Value::String(multiline_text(area))),
        Control::EntryRow(row) => Some(Value::String(row.text().to_string())),
        Control::PasswordRow(row) => Some(Value::String(row.text().to_string())),
        Control::Switch(switch) => Some(Value::Bool(switch.is_active())),
        Control::Choice(dropdown, values) => Some(
            values
                .get(dropdown.selected() as usize)
                .cloned()
                .unwrap_or(Value::Null),
        ),
        Control::Slider(scale, _) => Some(number(scale.value())),
        Control::Table(table) => table.selected_row(),
        Control::Tree(tree) => tree.selected_row(),
        Control::Plain | Control::Label(_) | Control::Button(_) | Control::Chart(_, _, _) => None,
    }
}

impl ic_view_session::ViewSurface for BuiltView {
    fn values(&self) -> BTreeMap<String, Value> {
        BuiltView::values(self)
    }

    fn touched(&self) -> Vec<String> {
        BuiltView::touched(self)
    }

    fn set_value(&self, node: &str, value: &Value) {
        BuiltView::set_value(self, node, value);
    }

    fn refresh(&self, state: &State) {
        BuiltView::refresh(self, state);
    }
}

#[cfg(test)]
mod tests {
    use super::hex_only;

    #[test]
    fn a_hex_field_keeps_digits_and_the_spaces_that_group_them() {
        assert_eq!(hex_only("00 1f ff"), "00 1f ff");
        assert_eq!(hex_only("DEADbeef"), "DEADbeef");
    }

    #[test]
    fn a_hex_field_drops_everything_that_is_not_hex() {
        assert_eq!(hex_only("0x1f"), "01f");
        assert_eq!(hex_only("zz"), "");
        assert_eq!(hex_only("00-1f"), "001f");
        assert_eq!(hex_only("ф0"), "0");
        assert_eq!(hex_only("00\n1f\t2a"), "001f2a");
    }
}

#[cfg(test)]
mod tree_tests {
    use super::*;
    use serde_json::json;

    fn branch(name: &str, children: Value) -> Value {
        json!({ "name": name, "path": name, "expandable": true, "children": children })
    }

    fn store_of(rows: &[Value]) -> (gtk::gio::ListStore, Rc<RefCell<HashMap<String, gtk::gio::ListStore>>>) {
        let store = gtk::gio::ListStore::new::<glib::BoxedAnyObject>();
        let kids = Rc::new(RefCell::new(HashMap::new()));
        let mut seen = std::collections::HashSet::new();
        sync_level(&store, rows, "", &kids, &mut seen);
        (store, kids)
    }

    fn names_in(store: &gtk::gio::ListStore) -> Vec<String> {
        (0..store.n_items())
            .filter_map(|at| store.item(at).and_then(|held| id_in(&held)))
            .collect()
    }

    fn marks(store: &gtk::gio::ListStore) -> Vec<usize> {
        (0..store.n_items())
            .filter_map(|at| store.item(at).map(|held| held.as_ptr() as usize))
            .collect()
    }

    #[test]
    fn a_level_that_did_not_change_keeps_the_very_same_rows() {
        let rows = vec![branch("HKLM", json!([])), branch("HKCU", json!([]))];
        let (store, kids) = store_of(&rows);
        let before = marks(&store);

        let mut seen = std::collections::HashSet::new();
        sync_level(&store, &rows, "", &kids, &mut seen);

        assert_eq!(marks(&store), before, "rows were rebuilt for nothing");
    }

    #[test]
    fn opening_a_branch_leaves_every_row_above_it_alone() {
        let rows = vec![branch("HKLM", json!([])), branch("HKCU", json!([]))];
        let (store, kids) = store_of(&rows);
        let before = marks(&store);

        // What the tree model does when the arrow is first worked.
        let under = gtk::gio::ListStore::new::<glib::BoxedAnyObject>();
        kids.borrow_mut().insert("HKLM".to_string(), under.clone());

        let opened = vec![
            branch("HKLM", json!([branch("SOFTWARE", json!([]))])),
            branch("HKCU", json!([])),
        ];
        let mut seen = std::collections::HashSet::new();
        sync_level(&store, &opened, "", &kids, &mut seen);

        assert_eq!(marks(&store), before, "the top level was rebuilt");
        assert_eq!(names_in(&under), vec!["SOFTWARE".to_string()]);
    }

    #[test]
    fn a_row_that_went_away_goes_and_a_new_one_arrives_in_its_place() {
        let rows = vec![
            branch("a", json!([])),
            branch("b", json!([])),
            branch("c", json!([])),
        ];
        let (store, kids) = store_of(&rows);
        let kept = store.item(0).map(|held| held.as_ptr() as usize);

        let next = vec![
            branch("a", json!([])),
            branch("b2", json!([])),
            branch("c", json!([])),
        ];
        let mut seen = std::collections::HashSet::new();
        sync_level(&store, &next, "", &kids, &mut seen);

        assert_eq!(names_in(&store), vec!["a", "b2", "c"]);
        assert_eq!(store.item(0).map(|held| held.as_ptr() as usize), kept);
    }

    #[test]
    fn a_renamed_cell_is_redrawn_while_a_new_arrow_state_is_not() {
        let rows = vec![branch("one", json!([]))];
        let (store, kids) = store_of(&rows);
        let before = marks(&store);

        let mut arrow = branch("one", json!([]));
        arrow["expanded"] = json!(true);
        let mut seen = std::collections::HashSet::new();
        sync_level(&store, &[arrow], "", &kids, &mut seen);
        assert_eq!(marks(&store), before, "an arrow must not redraw the row");

        let mut shown = branch("one", json!([]));
        shown["name"] = json!("ONE");
        let mut seen = std::collections::HashSet::new();
        sync_level(&store, &[shown], "", &kids, &mut seen);
        assert_ne!(marks(&store), before, "a changed cell must be redrawn");
    }

    #[test]
    fn branches_nobody_is_showing_any_more_are_forgotten() {
        let rows = vec![branch("HKLM", json!([]))];
        let (store, kids) = store_of(&rows);
        kids.borrow_mut().insert(
            "HKLM".to_string(),
            gtk::gio::ListStore::new::<glib::BoxedAnyObject>(),
        );

        let mut seen = std::collections::HashSet::new();
        sync_level(&store, &[branch("HKCU", json!([]))], "", &kids, &mut seen);
        kids.borrow_mut().retain(|branch, _| seen.contains(branch));

        assert!(!kids.borrow().contains_key("HKLM"));
    }

    #[test]
    fn two_branches_sharing_a_name_under_different_parents_stay_apart() {
        let kid = json!({ "name": "Run", "expandable": true, "children": [] });
        assert_ne!(identity(&kid, "HKLM"), identity(&kid, "HKCU"));
    }
}
