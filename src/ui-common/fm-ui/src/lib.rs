#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod i18n;

pub mod context_menu;
pub mod dialogs;
pub mod file_entry;
pub mod fm_view;
pub mod icon_generator;
pub mod utils;
pub mod view_factories;

pub use file_entry::FileEntry;
pub use fm_core::rpc::{PathSegment, RemoteFileEntry};
pub use fm_view::{
    on_files_line, BreadcrumbSegment, FmPanelInit, FmPanelInput, FmPanelModel, FmPanelOutput,
    SourceInfo, ThumbnailFn, TOOLBAR_FILES_LINE,
};
pub use utils::build_path_string;

use std::sync::Once;

static RESOURCES_INIT: Once = Once::new();

thread_local! {
    static COMPACT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// One panel, touch first. Settled when a panel is built, asked by what must differ.
pub fn set_compact(on: bool) {
    COMPACT.with(|held| held.set(on));
}

pub fn compact() -> bool {
    COMPACT.with(|held| held.get())
}

pub fn init_resources() {
    RESOURCES_INIT.call_once(|| {
        gtk::gio::resources_register_include!("fm.gresource")
            .expect("Failed to register fm-ui resources");
    });
}
