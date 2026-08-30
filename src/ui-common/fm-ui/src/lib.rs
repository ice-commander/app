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
pub use fm_view::{
    BreadcrumbSegment, FmPanelInit, FmPanelInput, FmPanelModel, FmPanelOutput, SourceInfo,
    ThumbnailFn,
};
pub use utils::build_path_string;
pub use fm_core::rpc::{PathSegment, RemoteFileEntry};

use std::sync::Once;

static RESOURCES_INIT: Once = Once::new();

pub fn init_resources() {
    RESOURCES_INIT.call_once(|| {
        gtk::gio::resources_register_include!("fm.gresource")
            .expect("Failed to register fm-ui resources");
    });
}
