#[cfg(feature = "gtk")]
use adw::prelude::*;
use std::path::Path;

pub(crate) fn show_error_dialog(title: &str, message: &str) {
    #[cfg(feature = "gtk")]
    {
        let dialog = adw::AlertDialog::builder()
            .heading(title)
            .body(message)
            .build();
        dialog.add_response("ok", &*crate::i18n::tr("common.ok"));
        let parent = gtk::gio::Application::default()
            .and_then(|app| app.downcast::<gtk::Application>().ok())
            .and_then(|app| app.active_window());
        dialog.present(parent.as_ref());
    }
    #[cfg(not(feature = "gtk"))]
    eprintln!("[error] {title}: {message}");
}

pub(crate) fn show_info_dialog(title: &str, message: &str) {
    #[cfg(feature = "gtk")]
    {
        let dialog = adw::AlertDialog::builder()
            .heading(title)
            .body(message)
            .build();
        dialog.add_response("ok", &*crate::i18n::tr("common.ok"));
        let parent = gtk::gio::Application::default()
            .and_then(|app| app.downcast::<gtk::Application>().ok())
            .and_then(|app| app.active_window());
        dialog.present(parent.as_ref());
    }
    #[cfg(not(feature = "gtk"))]
    eprintln!("[info] {title}: {message}");
}

pub(crate) fn open_downloaded_file(file_path: &Path) {
    let file_path = file_path.to_string_lossy().to_string();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open")
        .arg(&file_path)
        .spawn();

    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&file_path).spawn();

    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(&["/c", "start", "", &file_path])
        .spawn();
}
