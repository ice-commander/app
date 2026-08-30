
use std::cell::Cell;

thread_local! {
    static LOADED: Cell<bool> = const { Cell::new(false) };
}

const CSS: &str = "
.hex-row {
    font-family: monospace;
    font-size: 13px;
}
.hex-view {
    background-color: @view_bg_color;
}
.editor-toolbar button {
    padding: 4px 10px;
    min-height: 0;
}
";

pub(crate) fn ensure_loaded() {
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
