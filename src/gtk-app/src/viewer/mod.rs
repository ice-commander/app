use adw::prelude::*;

mod audio;
mod pdf;
mod plugins;
mod style;
pub(crate) mod properties;
pub(crate) mod source;
mod tags;
mod video;

const IMAGE_EXT: [&str; 17] = [
    "png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "ico", "nef", "cr2", "cr3", "arw", "dng",
    "raf", "orf", "rw2", "pef",
];

pub(crate) const AUDIO_EXT: [&str; 8] = ["mp3", "wav", "ogg", "oga", "flac", "m4a", "aac", "mp4a"];

pub(crate) const VIDEO_EXT: [&str; 15] = [
    "mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv", "flv", "ts", "m2ts", "mts", "mpg", "mpeg",
    "3gp", "ogv",
];

pub(crate) fn extension_of(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn load_icon(resource_path: &str) -> gtk::Image {
    let img = gtk::Image::from_resource(resource_path);
    img.set_pixel_size(30);
    img.set_width_request(30);
    img.set_height_request(30);
    img
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
    let ext = extension_of(&file_path_str);
    let kind = if AUDIO_EXT.contains(&ext.as_str()) {
        Some(source::Kind::Audio)
    } else if VIDEO_EXT.contains(&ext.as_str()) {
        Some(source::Kind::Video)
    } else if ext == "pdf" {
        Some(source::Kind::Pdf)
    } else if IMAGE_EXT.contains(&ext.as_str()) {
        None
    } else if router.provider().is_local() {
        source::sniff(&file_path_str)
    } else {
        None
    };

    if kind == Some(source::Kind::Audio) {
        open_with(
            parent_window,
            Box::new(plugins::AudioPlugin),
            file_path_str.clone(),
            entry.name(),
            router.clone(),
            false,
        );
        return;
    }

    if kind == Some(source::Kind::Video) {
        #[cfg(target_os = "linux")]
        {
            if !ic_platform::video_codecs::is_video_decoding_available() {
                video::prompt_codec_installation(parent_window, &file_path_str, &router);
                return;
            }
        }
        open_video(parent_window, &entry, &router);
        return;
    }

    if kind == Some(source::Kind::Pdf) {
        open_with(
            parent_window,
            Box::new(plugins::PdfPlugin),
            file_path_str.clone(),
            entry.name(),
            router.clone(),
            false,
        );
        return;
    }

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

fn open_video(
    parent_window: &impl IsA<gtk::Window>,
    entry: &gtk_fm_ui::FileEntry,
    router: &std::rc::Rc<panel_router::PanelRouter>,
) {
    let path = entry.path();
    let provider = router.provider();
    if provider.is_local() {
        video::show_video_player_window(parent_window, &path, router.config());
        return;
    }

    let (fetch_win, label, cancelled) =
        source::fetching_window(parent_window, &entry.name(), entry.size());
    let parent = parent_window.clone().upcast::<gtk::Window>();
    let config = router.config();
    let total = entry.size();

    gtk::glib::spawn_future_local(async move {
        let label_progress = label.clone();
        let copy = gtk_viewer_ui::local_copy(&provider, &path, move |done| {
            label_progress.set_text(&crate::i18n::trf(
                "viewer.fetching_progress",
                &[
                    ("done", &gtk_fm_ui::utils::format_size(done)),
                    ("total", &gtk_fm_ui::utils::format_size(total)),
                ],
            ));
        })
        .await;

        if cancelled.get() {
            return;
        }
        fetch_win.close();

        match copy {
            Ok(copy) => {
                let player = video::show_video_player_window(
                    &parent,
                    &copy.path.to_string_lossy(),
                    config,
                );
                let copy = std::cell::RefCell::new(Some(copy));
                player.connect_close_request(move |_| {
                    drop(copy.borrow_mut().take());
                    gtk::glib::Propagation::Proceed
                });
            }
            Err(e) => {
                let dialog = adw::AlertDialog::builder()
                    .heading(&*crate::i18n::tr("player.video_title"))
                    .body(&crate::i18n::trf("editor.failed_read", &[("error", &e)]))
                    .build();
                dialog.add_response("ok", "OK");
                dialog.present(Some(&parent));
            }
        }
    });
}

pub(crate) fn services(router: &std::rc::Rc<panel_router::PanelRouter>) -> gtk_viewer_ui::HostServices {
    let config = router.config();
    let save_hotkey = crate::hotkey::get_hotkeys(&config)
        .into_iter()
        .find(|h| h.id == "editor_save")
        .map(|h| h.keys);
    let router_saved = router.clone();
    let router_dir = router.clone();

    gtk_viewer_ui::HostServices {
        save_hotkey,
        fast_save: config.get::<bool>("ui.fast_save").unwrap_or(false),
        current_dir: std::rc::Rc::new(move || router_dir.current_path_string()),
        on_saved: std::rc::Rc::new(move || router_saved.refresh_spawned()),
        observer: Some(std::rc::Rc::new(crate::viewer_probe::Probe)),
        raw_thumbnail: Some(std::rc::Rc::new(crate::editor::raw_thumbnail)),
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
