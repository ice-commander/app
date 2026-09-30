use adw::prelude::*;
use ic_view::{
    as_text, choice_index, is_truthy, resolve, resolve_text, visible, Chrome, Cond, Document, Fit,
    InputVariant, InputVariant as Variant, MediaKind, Node, NodeKind, Scroll, State, Text,
};
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
/// The application's own player, for when it has one. It is handed the path,
/// whether it is moving pictures, and whether to start playing; what comes
/// back is the whole transport as a widget.
///
/// Without it a `media` node falls back to the toolkit's own player, which is
/// a second decoder stack and on some desktops a slow one to start.
pub type PlayerSource = Rc<dyn Fn(&str, MediaKind, bool) -> Option<gtk::Widget>>;
/// Answers with the widget a `canvas` node draws in, by the node's id.
pub type CanvasSource = Rc<dyn Fn(&str) -> Option<gtk::Widget>>;

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
    Table(gtk::Grid),
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
}

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
        let Some(ours) = self
            .player
            .as_ref()
            .and_then(|source| source(&local, node.media, node.autoplay))
        else {
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

    fn table(&self, node: &Node, state: &State) -> (gtk::Widget, Control) {
        let grid = gtk::Grid::builder()
            .column_spacing(12)
            .row_spacing(2)
            .build();
        // Without this a weightless grid pins its rows to the top and reads as misaligned.
        if !node.weight.is_some_and(|weight| weight > 0) {
            grid.set_valign(gtk::Align::Center);
        }
        fill_table(&grid, node, state, &|key| (self.translate)(key));
        (grid.clone().upcast(), Control::Table(grid))
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

fn fill_table(
    grid: &gtk::Grid,
    node: &Node,
    state: &State,
    translate: &dyn Fn(&str) -> Option<String>,
) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
    for (column, spec) in node.columns.iter().enumerate() {
        if let Some(title) = spec.title.as_ref() {
            let heading = gtk::Label::builder()
                .label(title.resolve(translate))
                .xalign(0.0)
                .build();
            heading.add_css_class("heading");
            grid.attach(&heading, column as i32, 0, 1, 1);
        }
    }
    let rows = node
        .rows_key
        .as_ref()
        .and_then(|key| state.data.get(key))
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    for (index, row) in rows.iter().enumerate() {
        for (column, spec) in node.columns.iter().enumerate() {
            let cell = row.get(&spec.key).map(as_text).unwrap_or_default();
            let label = gtk::Label::builder()
                .label(cell)
                .xalign(0.0)
                .selectable(node.selectable)
                .build();
            grid.attach(&label, column as i32, index as i32 + 1, 1, 1);
        }
    }
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
        Control::Plain | Control::Button(_) | Control::Table(_) | Control::Chart(_, _, _) => {
            return false
        }
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
            Control::Plain
            | Control::Button(_)
            | Control::Label(_)
            | Control::Table(_)
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
            _ => false,
        }
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
                Control::Table(grid) => fill_table(grid, &bound.node, state, &|key| translate(key)),
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
        Control::Plain
        | Control::Label(_)
        | Control::Button(_)
        | Control::Table(_)
        | Control::Chart(_, _, _) => None,
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
