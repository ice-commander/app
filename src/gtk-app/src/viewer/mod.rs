use adw::prelude::*;

mod declared;
pub(crate) mod properties;
pub(crate) mod source;

// What the built-in viewer used to claim. Kept while the plugins that
// replace it are written; the panel still reads AUDIO_EXT for its own player.
#[allow(dead_code)]
const IMAGE_EXT: [&str; 17] = [
    "png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "ico", "nef", "cr2", "cr3", "arw", "dng",
    "raf", "orf", "rw2", "pef",
];

pub(crate) const AUDIO_EXT: [&str; 8] = ["mp3", "wav", "ogg", "oga", "flac", "m4a", "aac", "mp4a"];

pub(crate) fn extension_of(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn show_viewer(
    parent_window: &impl IsA<gtk::Window>,
    entry: gtk_fm_ui::FileEntry,
    router: std::rc::Rc<panel_router::PanelRouter>,
) {
    if entry.is_dir() {
        properties::show_directory_properties(parent_window, &entry);
        return;
    }
    let file_path_str = entry.path();

    // A plugin that claims this kind of file draws the window instead of the
    // application. By extension and nothing else: reading the first bytes of
    // everything on a server is a request per file for a name we already have.
    if let Some(viewer) = ic_plugin_host::viewer_for(&entry.name()) {
        open_with(
            parent_window,
            Box::new(declared::DeclaredPlugin::showing(viewer)),
            file_path_str.clone(),
            entry.name(),
            router.clone(),
            false,
        );
        return;
    }

    // Everything else that nobody claimed falls to the application's own text
    // and hex viewer, which stays where it is and is always the last resort.
    let parent = parent_window.clone().upcast::<gtk::Window>();
    let router_open = router.clone();
    let path_open = file_path_str.clone();
    let name_open = entry.name();
    source::confirm_large_file(parent_window, &entry.name(), entry.size(), move || {
        open_with(
            &parent,
            Box::new(gtk_viewer_ui::TextPlugin),
            path_open.clone(),
            name_open.clone(),
            router_open.clone(),
            false,
        );
    });
}

pub(crate) fn services(
    router: &std::rc::Rc<panel_router::PanelRouter>,
) -> gtk_viewer_ui::HostServices {
    let config = router.config();
    let save_hotkey = crate::hotkey::get_hotkeys(&config)
        .into_iter()
        .find(|h| h.id == "editor_save")
        .map(|h| h.keys);
    let router_saved = router.clone();
    let router_dir = router.clone();
    // Read on every press, so a key changed in Settings takes effect in open windows.
    let router_keys = router.clone();

    gtk_viewer_ui::HostServices {
        save_hotkey,
        fast_save: config.get::<bool>("ui.fast_save").unwrap_or(false),
        current_dir: std::rc::Rc::new(move || router_dir.current_path_string()),
        on_saved: std::rc::Rc::new(move || router_saved.refresh_spawned()),
        observer: Some(std::rc::Rc::new(crate::viewer_probe::Probe)),
        raw_thumbnail: Some(std::rc::Rc::new(crate::editor::raw_thumbnail)),
        fullscreen_key: std::rc::Rc::new(move |keyval, state| {
            crate::hotkey::is_bound_to(
                &router_keys.config(),
                "toggle_video_fullscreen",
                keyval,
                state,
            )
        }),
    }
}

pub(crate) fn open_with(
    parent: &impl IsA<gtk::Window>,
    plugin: Box<dyn gtk_viewer_ui::ViewerPlugin>,
    path: String,
    name: String,
    router: std::rc::Rc<panel_router::PanelRouter>,
    start_in_edit_mode: bool,
) {
    gtk_viewer_ui::open(
        parent,
        plugin,
        path,
        name,
        router.provider(),
        services(&router),
        start_in_edit_mode,
    );
}

pub(crate) fn open_in_host(
    parent: &impl IsA<gtk::Window>,
    path: String,
    name: String,
    router: std::rc::Rc<panel_router::PanelRouter>,
    start_in_edit_mode: bool,
) {
    open_with(
        parent,
        Box::new(gtk_viewer_ui::TextPlugin),
        path,
        name,
        router,
        start_in_edit_mode,
    );
}

pub(crate) fn new_file_in_host(
    parent: &impl IsA<gtk::Window>,
    router: std::rc::Rc<panel_router::PanelRouter>,
) {
    open_with(
        parent,
        Box::new(gtk_viewer_ui::NewFilePlugin),
        String::new(),
        crate::i18n::tr("editor.new_file"),
        router,
        true,
    );
}
