use adw::prelude::*;
use ic_plugin_host::Views;
use ic_view_session::{Applying, HostFacts, Redescribed, Session, Watch};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

enum Holder {
    Window(gtk::Window),
    Embedded(gtk::Box),
}

impl Holder {
    fn show(&self, root: &gtk::Widget) {
        match self {
            Holder::Window(window) => window.set_child(Some(root)),
            Holder::Embedded(frame) => {
                while let Some(child) = frame.first_child() {
                    frame.remove(&child);
                }
                frame.append(root);
            }
        }
    }
}

/// Who answers for an open window: a view the plugin registered by name, or a
/// viewer opened on one file, which is told which window it is.
#[derive(Clone)]
enum Answers {
    Registered,
    /// The filesystem is carried with it: a window rebuilt after the plugin
    /// changed its mind — the next track, the next page — has to be drawn with
    /// the same pictures and the same player as the first time, or the cover
    /// turns into a grey line and the sound stops.
    Viewer {
        instance: u64,
        source: ic_plugin_api::IcFsSource,
    },
}

impl Answers {
    fn host(&self, view: &str) -> Box<dyn ic_view_session::ViewHost> {
        match self {
            Answers::Registered => Box::new(Views),
            Answers::Viewer { instance, .. } => Box::new(ic_plugin_host::Viewer {
                id: view.to_string(),
                instance: *instance,
            }),
        }
    }

    /// The renderer this window was built with, and must go on being built
    /// with for as long as it is open.
    fn renderer(&self, view: &str) -> ic_view_gtk::Renderer {
        match self {
            Answers::Registered => renderer(),
            Answers::Viewer { instance, source } => renderer_for(view, *instance, *source),
        }
    }
}

struct OpenView {
    view: String,
    answers: Answers,
    holder: Holder,
    built: ic_view_gtk::BuiltView,
    session: Session,
}

thread_local! {
    static OPEN: RefCell<HashMap<String, OpenView>> = RefCell::new(HashMap::new());
}

pub(crate) fn facts() -> HostFacts {
    HostFacts::new(
        ic_plugin_api::IC_HOST_GTK,
        &crate::i18n::current_lang(),
        &["copy"],
    )
}

fn renderer() -> ic_view_gtk::Renderer {
    let icons: ic_view_gtk::IconSource = Rc::new(|name: &str, size: u32| {
        let bare = name.strip_prefix("asset:")?;
        let bytes = ic_plugin_host::asset(bare)?;
        crate::plugin_host::image_from_svg_at(&bytes, size).map(|image| image.upcast())
    });
    ic_view_gtk::Renderer::new(Rc::new(crate::connection_manager::translate_optional))
        .with_icons(icons)
}

/// The same renderer, for a window opened on one file: it can also draw the
/// pictures that window names.
///
/// `file:` is read by the host, through the filesystem the viewer was let into
/// — so the same document works on a disk, on a server and inside an archive.
/// `part:` is asked of the plugin, which is how a page of a document or the
/// picture inside a camera file arrives without ever being in the document.
fn renderer_for(
    viewer: &str,
    instance: u64,
    source: ic_plugin_api::IcFsSource,
) -> ic_view_gtk::Renderer {
    let held = Carried(source);
    let pictures: ic_view_gtk::PictureSource = {
        let viewer = viewer.to_string();
        Rc::new(move |named: &str| {
            if let Some(part) = named.strip_prefix("part:") {
                return ic_plugin_host::viewer_part(&viewer, instance, part);
            }
            let path = named.strip_prefix("file:")?;
            read_through_the_host(held.0, path)
        })
    };
    let media: ic_view_gtk::MediaSource = Rc::new(move |named: &str| {
        // A player opens a file itself, so it needs a path rather than bytes.
        let path = named.strip_prefix("file:")?;
        let named = std::ffi::CString::new(path).ok()?;
        let host = ic_plugin_host::host_ref();
        let answered = (host.fs_local_path)(held.0, named.as_ptr());
        (!answered.is_null()).then(|| {
            unsafe { std::ffi::CStr::from_ptr(answered) }
                .to_string_lossy()
                .into_owned()
        })
    });
    let player: ic_view_gtk::PlayerSource = Rc::new(|at: &str, kind, autoplay| {
        // Sound is ours to play; moving pictures are left to the toolkit until
        // there is a plugin that does them better.
        (kind == ic_view::MediaKind::Audio).then(|| crate::media_widget::sound(at, autoplay))
    });
    let canvas: ic_view_gtk::CanvasSource = {
        let viewer = viewer.to_string();
        Rc::new(move |_named: &str| crate::plugin_canvas::place(&viewer, instance))
    };
    renderer()
        .with_pictures(pictures)
        .with_media(media)
        .with_player(player)
        .with_canvas(canvas)
}

/// A source is the host's own and is only read from this thread while the
/// window is open; the closure that draws needs it to be `'static`.
#[derive(Clone, Copy)]
struct Carried(ic_plugin_api::IcFsSource);

fn read_through_the_host(source: ic_plugin_api::IcFsSource, path: &str) -> Option<Vec<u8>> {
    let named = std::ffi::CString::new(path).ok()?;
    let host = ic_plugin_host::host_ref();
    let stream = (host.fs_open)(source, named.as_ptr(), ic_plugin_api::IC_OPEN_READ);
    if stream.is_null() {
        return None;
    }
    let mut held = Vec::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = (host.fs_read)(stream, buffer.as_mut_ptr(), buffer.len() as u64);
        if read <= 0 {
            break;
        }
        held.extend_from_slice(&buffer[..read as usize]);
    }
    (host.fs_close)(stream);
    Some(held)
}

const STILL_FOR: std::time::Duration = std::time::Duration::from_secs(3);

/// The host only reports `view.pointer_idle`; what to hide is the document's own business,
/// which leaves its shape untouched — what a window with a canvas in it depends on.
struct Idle {
    slot: String,
    /// Weak: the frame owns the controllers that hold this, and a cycle would outlive the window.
    frame: gtk::glib::WeakRef<gtk::Box>,
    turn: Cell<u64>,
    away: Cell<bool>,
    at: Cell<(f64, f64)>,
    watching: Cell<bool>,
}

impl Idle {
    fn window(&self) -> Option<gtk::Window> {
        self.frame
            .upgrade()
            .and_then(|frame| frame.root())
            .and_downcast::<gtk::Window>()
    }

    fn filling_the_screen(&self) -> bool {
        self.window().is_some_and(|window| window.is_fullscreen())
    }

    fn put_away(&self, away: bool) {
        if self.away.replace(away) == away {
            return;
        }
        OPEN.with(|open| {
            let mut held = open.borrow_mut();
            let Some(OpenView { built, session, .. }) = held.get_mut(&self.slot) else {
                return;
            };
            session.state.set_view("pointer_idle", Value::Bool(away));
            built.refresh(&session.state);
        });
        if let Some(window) = self.window() {
            if away {
                window.set_cursor_from_name(Some("none"));
            } else {
                window.set_cursor(None);
            }
        }
    }

    /// Only the last arming counts, so a pointer that keeps moving never reaches the end.
    fn arm(self: &Rc<Self>) {
        let turn = self.turn.get().wrapping_add(1);
        self.turn.set(turn);
        let held = self.clone();
        gtk::glib::timeout_add_local_once(STILL_FOR, move || {
            if held.turn.get() != turn {
                return;
            }
            if !OPEN.with(|open| open.borrow().contains_key(&held.slot)) {
                return;
            }
            if held.filling_the_screen() {
                held.put_away(true);
            }
        });
    }

    fn stir(self: &Rc<Self>) {
        self.put_away(false);
        if self.filling_the_screen() {
            self.arm();
        }
    }
}

/// Full screen only: with no overlay in the language, hiding the row resizes the picture.
fn hide_while_still(slot: &str, frame: &gtk::Box) {
    let held = Rc::new(Idle {
        slot: slot.to_string(),
        frame: frame.downgrade(),
        turn: Cell::new(0),
        away: Cell::new(false),
        at: Cell::new((f64::MIN, f64::MIN)),
        watching: Cell::new(false),
    });

    let motion = gtk::EventControllerMotion::new();
    let moving = held.clone();
    motion.connect_motion(move |_, x, y| {
        let (was_x, was_y) = moving.at.replace((x, y));
        // Hiding the row resizes the picture, which makes GTK report a move nobody made.
        if (x - was_x).abs() < 2.0 && (y - was_y).abs() < 2.0 {
            return;
        }
        moving.stir();
    });
    frame.add_controller(motion);

    // Capture, ahead of the document's shortcuts: the first press has to bring the row back.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let pressed = held.clone();
    keys.connect_key_pressed(move |_, _, _, _| {
        pressed.stir();
        gtk::glib::Propagation::Proceed
    });
    frame.add_controller(keys);

    // Armed on entering full screen, not on the next move, or a still pointer keeps the row.
    let watcher = held.clone();
    frame.connect_root_notify(move |frame| {
        if watcher.watching.get() {
            return;
        }
        let Some(window) = frame.root().and_downcast::<gtk::Window>() else {
            return;
        };
        watcher.watching.set(true);
        let held = watcher.clone();
        window.connect_fullscreened_notify(move |window| {
            if window.is_fullscreen() {
                held.arm();
            } else {
                held.put_away(false);
            }
        });
    });
}

fn tick(slot: &str, every: Option<u32>) {
    let Some(every) = every.filter(|every| *every > 0) else {
        return;
    };
    let ticking = slot.to_string();
    gtk::glib::timeout_add_local(
        std::time::Duration::from_millis(u64::from(every)),
        move || {
            if OPEN.with(|open| open.borrow().contains_key(&ticking)) {
                update(&ticking);
                gtk::glib::ControlFlow::Continue
            } else {
                gtk::glib::ControlFlow::Break
            }
        },
    );
}

/// Puts a plugin-drawn window on the file the user asked to look at.
///
/// The plugin is handed the filesystem and the path, never the bytes, and
/// answers with a document this frontend draws. What comes back is the widget
/// to put in the viewer window, and the call that closes it.
pub fn embed_viewer(viewer: &str, source: ic_plugin_api::IcFsSource, path: &str) -> Option<Shown> {
    let instance = next_instance();
    if !ic_plugin_host::open_viewer(viewer, instance, source, path) {
        return None;
    }
    let facts = facts();
    let context = ic_view_session::context(&facts, "null");
    let Some(document) = ic_plugin_host::describe_viewer(viewer, instance, &context) else {
        ic_plugin_host::viewer_closed(viewer, instance);
        return None;
    };
    let session = Session::open(viewer, document, "null", &facts);
    let built = renderer_for(viewer, instance, source).build(&session.document, &session.state);
    let every = session.document.form.refresh_ms;
    let frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();
    frame.append(&built.root);

    let slot = format!("viewer:{viewer}:{instance}");
    keep_answered(
        &slot,
        viewer,
        Answers::Viewer { instance, source },
        Holder::Embedded(frame.clone()),
        built,
        session,
    );
    send(&slot, "opened", None, None, None);
    tick(&slot, every);
    hide_while_still(&slot, &frame);

    let closing = slot.clone();
    let named = viewer.to_string();
    let close = Box::new(move || {
        if OPEN
            .with(|open| open.borrow_mut().remove(&closing))
            .is_some()
        {
            // While the GL context still stands: the widgets go later than this.
            crate::plugin_canvas::letting_go(instance);
            ic_plugin_host::viewer_closed(&named, instance);
        }
    });

    // Asked before the window goes, and the plugin may say no — a document
    // half written, a page not saved. A refusal redraws the window, so what it
    // wanted to say is on the screen rather than in a dialog of its own.
    let asked = slot.clone();
    let about = viewer.to_string();
    let may_close = Box::new(move || {
        if ic_plugin_host::viewer_may_close(&about, instance) {
            return true;
        }
        update(&asked);
        false
    });

    Some(Shown {
        widget: frame.upcast(),
        close,
        may_close,
    })
}

/// A window a plugin draws, and the two things its holder must ask of it.
pub struct Shown {
    pub widget: gtk::Widget,
    /// The window has gone: tell the plugin.
    pub close: Box<dyn Fn()>,
    /// The window is about to go: may it?
    pub may_close: Box<dyn Fn() -> bool>,
}

/// One number per window, so a plugin showing two files at once can tell them
/// apart. Never zero: that is what a host with nothing open would say.
fn next_instance() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

pub fn install(parent: &gtk::Window) {
    let opener = parent.clone();
    ic_plugin_host::set_view_open_handler(Rc::new(move |id: &str, argument: &str| {
        open(&opener, id, argument);
    }));
    crate::plugin_canvas::install();
    ic_plugin_host::set_view_invalidate_handler(Rc::new(|id: &str| {
        let showing: Vec<String> = OPEN.with(|open| {
            open.borrow()
                .iter()
                .filter(|(_, held)| held.view == id)
                .map(|(slot, _)| slot.clone())
                .collect()
        });
        for slot in showing {
            update(&slot);
        }
    }));
}

pub fn install_waker() {
    // A plugin may ask from a thread of its own; only the main loop may touch widgets.
    ic_plugin_host::set_waker(std::sync::Arc::new(|wanted| {
        gtk::glib::idle_add_once(move || ic_plugin_host::deliver(wanted.clone()));
    }));
}

fn open(parent: &gtk::Window, id: &str, argument: &str) {
    let shown = OPEN.with(
        |open| match open.borrow().get(id).map(|view| &view.holder) {
            Some(Holder::Window(window)) => Some(window.clone()),
            _ => None,
        },
    );
    if let Some(shown) = shown {
        shown.present();
        return;
    }
    let facts = facts();
    let context = ic_view_session::context(&facts, argument);
    let Some(document) = ic_plugin_host::describe_view(id, &context) else {
        return;
    };
    let session = Session::open(id, document, argument, &facts);
    let built = renderer().build(&session.document, &session.state);
    let named = ic_plugin_host::view_title(id).unwrap_or_default();
    let title = crate::connection_manager::translate_optional(&named).unwrap_or(named);
    let window = gtk::Window::builder()
        .title(title)
        .transient_for(parent)
        .modal(session.document.form.surface == ic_view::Surface::Dialog)
        .default_width(session.document.form.width.unwrap_or(700) as i32)
        .default_height(session.document.form.height.unwrap_or(600) as i32)
        .child(&built.root)
        .build();

    let escape = gtk::EventControllerKey::new();
    let closing = window.clone();
    escape.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            closing.close();
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    window.add_controller(escape);

    let closed = id.to_string();
    window.connect_close_request(move |_| {
        OPEN.with(|open| open.borrow_mut().remove(&closed));
        ic_plugin_host::view_closed(&closed, 1);
        gtk::glib::Propagation::Proceed
    });

    tick(id, session.document.form.refresh_ms);

    keep(id, id, Holder::Window(window.clone()), built, session);
    window.present();
    send(id, "opened", None, None, None);
}

pub fn embed(id: &str, argument: &str) -> Option<gtk::Widget> {
    let facts = facts();
    let context = ic_view_session::context(&facts, argument);
    let document = ic_plugin_host::describe_view(id, &context)?;
    let session = Session::open(id, document, argument, &facts);
    let built = renderer().build(&session.document, &session.state);
    let frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();
    frame.append(&built.root);
    let slot = format!("embedded:{id}");
    let leaving = slot.clone();
    let view = id.to_string();
    frame.connect_unrealize(move |going| {
        let gone = OPEN.with(|open| {
            let mut held = open.borrow_mut();
            let ours = matches!(
                held.get(&leaving).map(|view| &view.holder),
                Some(Holder::Embedded(shown)) if shown == going
            );
            if ours {
                held.remove(&leaving)
            } else {
                None
            }
        });
        if gone.is_some() {
            ic_plugin_host::view_closed(&view, 1);
        }
    });
    keep(&slot, id, Holder::Embedded(frame.clone()), built, session);
    send(&slot, "opened", None, None, None);
    Some(frame.upcast())
}

fn keep(slot: &str, view: &str, holder: Holder, built: ic_view_gtk::BuiltView, session: Session) {
    keep_answered(slot, view, Answers::Registered, holder, built, session)
}

fn keep_answered(
    slot: &str,
    view: &str,
    answers: Answers,
    holder: Holder,
    built: ic_view_gtk::BuiltView,
    session: Session,
) {
    let applying = session.applying();
    wire(slot, &built, session.watches(), &applying);
    OPEN.with(|open| {
        open.borrow_mut().insert(
            slot.to_string(),
            OpenView {
                view: view.to_string(),
                answers,
                holder,
                built,
                session,
            },
        )
    });
    OPEN.with(|open| {
        if let Some(view) = open.borrow().get(slot) {
            follow(slot, &view.built, &applying);
        }
    });
}

/// Keeps the session's state level with the widgets as they are typed in, and
/// redraws what the new state implies. Without it a `visible` or `sensitive`
/// predicate over a bind the user types into never changes, because a field
/// that does not `emit` is otherwise never heard from at all.
fn follow(id: &str, built: &ic_view_gtk::BuiltView, applying: &Applying) {
    for node in built.ids() {
        // Mirroring a node that keeps no value redraws on every twitch and fights the hand.
        if !built.keeps_a_value(node) {
            continue;
        }
        let view = id.to_string();
        let guard = applying.clone();
        built.on_change(node, move |bind, value| {
            if guard.is_set() {
                return;
            }
            OPEN.with(|open| {
                let mut held = open.borrow_mut();
                let Some(OpenView { built, session, .. }) = held.get_mut(&view) else {
                    return;
                };
                session.state.set_state(&bind, value);
                session.state.touched.insert(bind);
                built.refresh(&session.state);
            });
        });
    }
}

fn wire(id: &str, built: &ic_view_gtk::BuiltView, watched: Vec<Watch>, applying: &Applying) {
    for watch in watched {
        let view = id.to_string();
        let target = watch.node.clone();
        let guard = applying.clone();
        if watch.on_value {
            let debounce = watch.debounce_ms;
            built.on_change(&watch.node, move |bind, value| {
                if guard.is_set() {
                    return;
                }
                schedule(&view, &target, &bind, value, debounce);
            });
        } else {
            built.on_activate(&watch.node, move |_| {
                if guard.is_set() {
                    return;
                }
                send(&view, "activate", Some(&target), None, None);
            });
        }
    }
}

fn schedule(id: &str, node: &str, bind: &str, value: Value, debounce: u32) {
    let armed = OPEN.with(|open| {
        open.borrow_mut()
            .get_mut(id)
            .map(|view| view.session.arm(node))
    });
    let Some(turn) = armed else {
        return;
    };
    let view = id.to_string();
    let target = node.to_string();
    let bound = bind.to_string();
    let fire = move || {
        let still = OPEN.with(|open| {
            open.borrow()
                .get(&view)
                .map(|held| held.session.current(&target, turn))
                .unwrap_or(false)
        });
        if still {
            send(
                &view,
                "change",
                Some(&target),
                Some(&bound),
                Some(value.clone()),
            );
        }
    };
    if debounce == 0 {
        fire();
    } else {
        gtk::glib::timeout_add_local_once(
            std::time::Duration::from_millis(u64::from(debounce)),
            fire,
        );
    }
}

fn send(id: &str, kind: &str, node: Option<&str>, bind: Option<&str>, value: Option<Value>) {
    let facts = facts();
    let outcome = OPEN.with(|open| {
        let mut held = open.borrow_mut();
        let OpenView {
            view,
            answers,
            built,
            session,
            ..
        } = held.get_mut(id)?;
        let host = answers.host(view);
        Some(session.send(built, host.as_ref(), &facts, kind, node, bind, value))
    });
    let Some(outcome) = outcome else {
        return;
    };
    if let Some(text) = outcome.clipboard {
        if let Some(display) = gtk::gdk::Display::default() {
            display.clipboard().set_text(&text);
        }
    }
    if outcome.redescribe {
        update(id);
    }
    // Last, so the rest of the reply is applied in the same order as one that does not close.
    if outcome.close {
        let window = OPEN.with(
            |open| match open.borrow().get(id).map(|held| &held.holder) {
                Some(Holder::Window(window)) => Some(window.clone()),
                _ => None,
            },
        );
        if let Some(window) = window {
            window.close();
        }
    }
}

fn update(id: &str) {
    let facts = facts();
    let rebuilding = OPEN.with(|open| {
        let mut held = open.borrow_mut();
        let OpenView {
            view,
            answers,
            built,
            session,
            ..
        } = held.get_mut(id)?;
        let host = answers.host(view);
        if session.redescribe(built, host.as_ref(), &facts) != Redescribed::Rebuilt {
            return None;
        }
        Some((
            session.document.clone(),
            session.state.clone(),
            session.watches(),
            session.applying(),
            answers.renderer(view),
        ))
    });
    let Some((document, state, watched, applying, renderer)) = rebuilding else {
        return;
    };
    // Built over the window as it stands, so what is playing keeps playing:
    // the plugin changed the page around it, not the track.
    let built = OPEN.with(|open| {
        let held = open.borrow();
        renderer.build_over(&document, &state, held.get(id).map(|view| &view.built))
    });
    wire(id, &built, watched, &applying);
    OPEN.with(|open| {
        let mut held = open.borrow_mut();
        if let Some(view) = held.get_mut(id) {
            view.holder.show(&built.root);
            view.built = built;
        }
    });
    OPEN.with(|open| {
        if let Some(view) = open.borrow().get(id) {
            follow(id, &view.built, &applying);
        }
    });
}
