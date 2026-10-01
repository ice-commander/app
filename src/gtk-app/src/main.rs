#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use adw::prelude::*;
use gtk::glib;

mod api;
mod app;

pub use common::AppError;
mod clipboard_ops;
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
mod media_widget;
mod panel_builder;
mod player;
mod player_ui;
mod plugin_ask;
mod plugin_canvas;
mod plugin_host;
mod plugin_view;
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
    let config = client_config::AppConfig::new("ice-commander");
    secret_store::harden_file_permissions(&config.config_path());
    let language = config
        .get::<String>("ui.language")
        .unwrap_or_else(|| "en".to_string());
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
