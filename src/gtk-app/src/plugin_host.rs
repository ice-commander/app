pub use ic_plugin_host::*;

// A plugin's icons are SVG text: drawn and kept by the one SVG renderer the application has.

pub fn image_from_svg(svg: &[u8]) -> Option<gtk::Image> {
    image_from_svg_at(svg, 24)
}

pub fn texture_from_svg(svg: &[u8], size: u32) -> Option<gtk::gdk::Texture> {
    ic_svg_gtk::icon(svg, size)
}

pub fn image_from_svg_at(svg: &[u8], size: u32) -> Option<gtk::Image> {
    ic_svg_gtk::icon_image(svg, size)
}

/// Loads what is installed and switched on. A plugin the user disabled is not
/// opened at all, so nothing it registers appears anywhere in this run.
pub fn load_installed_plugins(config: &client_config::AppConfig) {
    ic_plugin_host::set_host_kind(ic_plugin_api::IC_HOST_GTK);
    for attempt in ic_plugin_host::loader::load_for(config) {
        if attempt.loaded() {
            ic_logging::info!("installed plugin {} is in use", attempt.name);
        } else if attempt.went_wrong() {
            ic_logging::warn!("installed plugin {} did not load", attempt.name);
        }
    }
}

#[cfg(test)]
mod tests {
    /// The pictures plugins ship have to survive the one path that draws
    /// them. A file that does not load leaves a button with no icon at all,
    /// which looks like the button was never registered.
    #[test]
    fn the_pictures_the_shipped_plugins_bring_all_render() {
        if gtk::init().is_err() {
            eprintln!("no display: the plugin icons were not checked");
            return;
        }
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let brought = [
            "../../../plugin-torrent/src/torrent-fs/assets",
            "../../../plugin-nodeinnet/src/node-in-net-fs/assets",
        ];
        let mut looked_at = 0;
        for at in brought {
            let folder = here.join(at);
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "svg") {
                    continue;
                }
                let svg = std::fs::read(&path).expect("readable");
                assert!(
                    super::image_from_svg_at(&svg, 20).is_some(),
                    "{} does not render, so anything drawn with it is blank",
                    path.display()
                );
                looked_at += 1;
            }
        }
        assert!(looked_at > 0, "no plugin pictures were found to check");
        eprintln!("{looked_at} plugin pictures render");
    }
}
