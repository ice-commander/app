use std::cell::Cell;

thread_local! {
    static LOADED: Cell<bool> = const { Cell::new(false) };
}

const CSS: &str = "
.pdf-page-picture {
    background-color: white;
    box-shadow: 0 4px 10px alpha(@window_fg_color, 0.25);
    border: 1px solid alpha(@window_fg_color, 0.2);
    margin: 8px 0px;
}
.pdf-scrolled-window {
    background-color: shade(@window_bg_color, 0.9);
}
.pdf-zoom-control {
    background-color: alpha(@popover_bg_color, 0.9);
    border: 1px solid alpha(@window_fg_color, 0.15);
    border-radius: 8px;
    padding: 6px;
    box-shadow: 0 4px 12px alpha(@window_fg_color, 0.3);
}
.pdf-zoom-label {
    font-weight: bold;
    font-size: 13px;
    margin: 0px 8px;
}
.video-player-window {
    background-color: @window_bg_color;
}
.video-controls-box,
.video-overlay-close-btn,
.video-overlay-title {
    background-color: alpha(@popover_bg_color, 0.9);
    border-radius: 8px;
}
.video-controls-box {
    padding: 6px;
    margin: 16px;
}
.video-overlay-title {
    border-radius: 6px;
    padding: 6px 12px;
    font-size: 14px;
    font-weight: bold;
}
";

pub(super) fn ensure_loaded() {
    if LOADED.with(|f| f.replace(true)) {
        return;
    }
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
