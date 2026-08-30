#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use adw::prelude::*;
use gtk::glib;

mod os_processes;
mod api;
mod app;

pub use common::AppError;
mod connection_manager;
mod device_name;
mod drives;
mod drivestoolbar;
mod editor;
mod external;
mod favorites;
mod file_operations;
mod help;
mod hotkey;
mod i18n;
mod logging;
mod mainwindow;
mod master_password;
mod panel_builder;
mod player;
mod player_ui;
mod process_panel;
mod registry_panel;
mod secret_store;
mod settings;
mod source_selector;
mod terminal;
mod transfer_plan;
mod ui;
mod updater;
mod utils;
mod viewer;
mod viewer_probe;
mod wizard;

#[tokio::main]
async fn main() -> glib::ExitCode {
    #[cfg(target_os = "macos")]
    if std::env::var("ICE_COMMANDER_DEBUG_MPV").is_ok() {
        let _ = libmpv2::Mpv::new();
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(_exe_dir) = exe_path.parent() {
            #[cfg(target_os = "windows")]
            {
                let gst_plugins_dir = _exe_dir.join("lib").join("gstreamer-1.0");
                if gst_plugins_dir.exists() {
                    std::env::set_var("GST_PLUGIN_PATH", gst_plugins_dir);
                }
            }
            #[cfg(target_os = "macos")]
            {
                if let Some(contents_dir) = _exe_dir.parent() {
                    let gst_plugins_dir = contents_dir.join("Resources").join("lib").join("gstreamer-1.0");
                    if gst_plugins_dir.exists() {
                        std::env::set_var("GST_PLUGIN_PATH", gst_plugins_dir);
                    }
                }
            }
        }
    }

    let config = client_config::AppConfig::new("ice-commander");
    secret_store::harden_file_permissions(&config.config_path());
    let language = config.get::<String>("ui.language").unwrap_or_else(|| "en".to_string());
    i18n::register();
    i18n::set_lang(&language);

    gtk::gio::resources_register_include!("icecommander.gresource")
        .expect("Failed to register resources.");

    app::Application::init_resources();

    let application = adw::Application::builder()
        .application_id("com.icecommander.gtkapp")
        .build();

    let config_clone = config.clone();
    application.connect_activate(move |app| {
        if let Some(existing) = app.windows().first() {
            existing.present();
            return;
        }
        app::Application::run(app, config_clone.clone());
    });
    let gtk_args: Vec<String> = std::env::args().take(1).collect();
    application.run_with_args(&gtk_args)
}
