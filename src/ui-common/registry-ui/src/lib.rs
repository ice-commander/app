mod i18n;

pub mod registry_view;

pub use registry_view::{RegistryInit, RegistryInput, RegistryModel, RegistryOutput};

use std::sync::Once;

static RESOURCES_INIT: Once = Once::new();

pub fn init_resources() {
    RESOURCES_INIT.call_once(|| {
        gtk::gio::resources_register_include!("registry.gresource")
            .expect("Failed to register registry-ui resources");
    });
}

pub fn build_path_string(parts: &[String]) -> String {
    if parts.is_empty() {
        return "/".to_string();
    }
    let mut path = String::new();
    for (i, p) in parts.iter().enumerate() {
        if i == 0 {
            if p.ends_with(':') {
                path = format!("{}/", p);
            } else {
                path = format!("/{}", p);
            }
        } else {
            if !path.ends_with('/') && !path.ends_with('\\') {
                path.push('/');
            }
            path.push_str(p);
        }
    }
    path
}
