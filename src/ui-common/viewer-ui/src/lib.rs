
mod content;
mod encoding;
mod hex_view;
mod i18n;
mod metadata;
mod style;

use fm_core::rpc::FileSystemRpc;
use gtk::prelude::*;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;


pub enum Needs {
    Bytes,
    LocalPath,
    Nothing,
}

pub enum Payload {
    Bytes(Vec<u8>),
    LocalPath(LocalCopy),
    Nothing,
}

pub struct LocalCopy {
    pub path: PathBuf,
    temporary: bool,
}

impl LocalCopy {
    pub fn borrowed(path: PathBuf) -> Self {
        Self { path, temporary: false }
    }
}

impl Drop for LocalCopy {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub trait ViewerObserver {
    fn opened(&self, path: &str, mode: &str, buffer: &gtk::TextBuffer);
    fn closed(&self);
}

#[derive(Clone)]
pub struct HostServices {
    pub save_hotkey: Option<String>,
    pub fast_save: bool,
    pub current_dir: Rc<dyn Fn() -> String>,
    pub on_saved: Rc<dyn Fn()>,
    pub observer: Option<Rc<dyn ViewerObserver>>,
    pub raw_thumbnail: Option<Rc<dyn Fn(&[u8]) -> Option<Vec<u8>>>>,
}

impl Default for HostServices {
    fn default() -> Self {
        Self {
            save_hotkey: Some("Ctrl+S".to_string()),
            fast_save: false,
            current_dir: Rc::new(|| "/".to_string()),
            on_saved: Rc::new(|| {}),
            observer: None,
            raw_thumbnail: None,
        }
    }
}

pub struct ViewerCtx {
    pub window: gtk::Window,
    pub stack: gtk::Stack,
    pub path: String,
    pub name: String,
    pub provider: Rc<dyn FileSystemRpc>,
    pub services: HostServices,
    pub start_in_edit_mode: bool,
}

pub trait ViewerPlugin {
    fn needs(&self) -> Needs;
    fn build(&self, ctx: &ViewerCtx, payload: Payload);
    fn window_size(&self) -> (i32, i32) {
        (800, 600)
    }
    fn configure_window(&self, _window: &gtk::Window) {}
}

pub struct TextPlugin;

impl ViewerPlugin for TextPlugin {
    fn needs(&self) -> Needs {
        Needs::Bytes
    }

    fn build(&self, ctx: &ViewerCtx, payload: Payload) {
        let Payload::Bytes(bytes) = payload else {
            return;
        };
        let window = ctx.window.clone();
        let stack = ctx.stack.clone();
        let path = ctx.path.clone();
        let name = ctx.name.clone();
        let provider = ctx.provider.clone();
        let services = ctx.services.clone();
        let start_in_edit_mode = ctx.start_in_edit_mode;

        gtk::glib::spawn_future_local(async move {
            let meta = metadata::query_file_metadata(path.clone(), Some(provider.clone())).await;
            content::build_editor_content(
                &window,
                &stack,
                Some(&path),
                &name,
                services,
                start_in_edit_mode,
                bytes,
                Some(provider),
                meta,
            );
        });
    }
}

pub struct NewFilePlugin;

impl ViewerPlugin for NewFilePlugin {
    fn needs(&self) -> Needs {
        Needs::Nothing
    }

    fn build(&self, ctx: &ViewerCtx, _payload: Payload) {
        content::build_editor_content(
            &ctx.window,
            &ctx.stack,
            None,
            &ctx.name,
            ctx.services.clone(),
            true,
            Vec::new(),
            None,
            None,
        );
    }
}

pub fn read_blocking(path: &str) -> bool {
    const MAX: u64 = 8 * 1024 * 1024;
    std::fs::metadata(path).map(|m| m.len() <= MAX).unwrap_or(false)
}

pub async fn local_copy(
    provider: &Rc<dyn FileSystemRpc>,
    display_path: &str,
    on_progress: impl Fn(u64) + 'static,
) -> Result<LocalCopy, String> {
    if provider.is_local() {
        return Ok(LocalCopy::borrowed(PathBuf::from(display_path)));
    }

    let bytes = provider
        .read_file_opt(display_path.to_string(), Some(Box::new(on_progress)), false)
        .await
        .map_err(|e| e.to_string())?;

    let path = temp_path(display_path);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(LocalCopy { path, temporary: true })
}

fn temp_path(display_path: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let name = display_path
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("file");
    std::env::temp_dir().join(format!(
        "ice-view-{}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed),
        name
    ))
}

struct Host {
    window: gtk::Window,
    stack: gtk::Stack,
    error_label: gtk::Label,
    status: gtk::Label,
    cancelled: Rc<Cell<bool>>,
}

impl Host {
    fn build_window(
        parent: &impl IsA<gtk::Window>,
        title: &str,
        size: (i32, i32),
        observer: Option<Rc<dyn ViewerObserver>>,
    ) -> Self {
        let window = gtk::Window::builder()
            .default_width(size.0)
            .default_height(size.1)
            .modal(true)
            .transient_for(parent)
            .title(title)
            .build();

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        window.set_child(Some(&stack));

        let loading = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .spacing(12)
            .build();
        let spinner = gtk::Spinner::builder().width_request(40).height_request(40).build();
        spinner.start();
        let status = gtk::Label::new(Some(&i18n::tr("editor.loading_file")));
        let cancel = gtk::Button::with_label(&i18n::tr("editor.cancel"));
        let win_cancel = window.clone();
        cancel.connect_clicked(move |_| win_cancel.close());
        loading.append(&spinner);
        loading.append(&status);
        loading.append(&cancel);
        stack.add_named(&loading, Some("loading"));

        let error_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .spacing(12)
            .build();
        let error_label = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .max_width_chars(60)
            .build();
        let close = gtk::Button::with_label(&i18n::tr("editor.close"));
        let win_close = window.clone();
        close.connect_clicked(move |_| win_close.close());
        error_box.append(&error_label);
        error_box.append(&close);
        stack.add_named(&error_box, Some("error"));
        stack.set_visible_child_name("loading");

        let cancelled = Rc::new(Cell::new(false));
        let key_controller = gtk::EventControllerKey::new();
        let win_key = window.clone();
        key_controller.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Escape {
                win_key.close();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        window.add_controller(key_controller);

        let cancelled_close = cancelled.clone();
        window.connect_close_request(move |_| {
            cancelled_close.set(true);
            gtk::glib::Propagation::Proceed
        });
        window.connect_destroy(move |_| {
            if let Some(ref observer) = observer {
                observer.closed();
            }
        });

        Self { window, stack, error_label, status, cancelled }
    }

    fn fail(&self, message: String) {
        self.error_label.set_text(&message);
        self.stack.set_visible_child_name("error");
    }
}

pub fn open(
    parent: &impl IsA<gtk::Window>,
    plugin: Box<dyn ViewerPlugin>,
    path: String,
    name: String,
    provider: Rc<dyn FileSystemRpc>,
    services: HostServices,
    start_in_edit_mode: bool,
) {
    style::ensure_loaded();
    let host = Host::build_window(parent, &name, plugin.window_size(), services.observer.clone());
    plugin.configure_window(&host.window);
    host.window.present();

    let ctx = ViewerCtx {
        window: host.window.clone(),
        stack: host.stack.clone(),
        path: path.clone(),
        name,
        provider: provider.clone(),
        services,
        start_in_edit_mode,
    };

    gtk::glib::spawn_future_local(async move {
        let payload = match plugin.needs() {
            Needs::Nothing => Payload::Nothing,
            Needs::Bytes => {
                let blocking = read_blocking(&path);
                match provider.read_file_opt(path.clone(), None, blocking).await {
                    Ok(bytes) => Payload::Bytes(bytes),
                    Err(e) => {
                        if !host.cancelled.get() {
                            host.fail(i18n::trf(
                                "editor.failed_read",
                                &[("error", &e.to_string())],
                            ));
                        }
                        return;
                    }
                }
            }
            Needs::LocalPath => {
                let status = host.status.clone();
                match local_copy(&provider, &path, move |done| {
                    status.set_text(&i18n::trf(
                        "viewer.fetching_progress",
                        &[("done", &done.to_string()), ("total", "")],
                    ));
                })
                .await
                {
                    Ok(copy) => Payload::LocalPath(copy),
                    Err(e) => {
                        if !host.cancelled.get() {
                            host.fail(i18n::trf("editor.failed_read", &[("error", &e)]));
                        }
                        return;
                    }
                }
            }
        };

        if host.cancelled.get() {
            return;
        }
        plugin.build(&ctx, payload);
    });
}

pub fn accel_string(keyval: gtk::gdk::Key, state: gtk::gdk::ModifierType) -> String {
    use gtk::gdk::ModifierType;

    let mut parts = Vec::new();
    if state.contains(ModifierType::CONTROL_MASK) {
        parts.push("Ctrl".to_string());
    }
    if state.contains(ModifierType::ALT_MASK) {
        parts.push("Alt".to_string());
    }
    if state.contains(ModifierType::SHIFT_MASK)
        && !matches!(keyval, gtk::gdk::Key::Shift_L | gtk::gdk::Key::Shift_R)
    {
        parts.push("Shift".to_string());
    }
    match keyval.to_unicode() {
        Some(ch) if !ch.is_control() => parts.push(ch.to_uppercase().to_string()),
        _ => parts.push(keyval.name().map(|n| n.to_string()).unwrap_or_default()),
    }
    parts.join("+")
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    View,
    Edit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Image,
    Utf8,
    Ansi,
    Hex,
}
