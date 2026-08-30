mod i18n;

pub mod sysinfo_view;

pub use sysinfo_view::{SysInfoInit, SysInfoInput, SysInfoModel, SysInfoOutput};

use std::sync::Once;

static RESOURCES_INIT: Once = Once::new();

pub fn init_resources() {
    RESOURCES_INIT.call_once(|| {
        gtk::gio::resources_register_include!("sysinfo.gresource")
            .expect("Failed to register sysinfo-ui resources");
    });
}
