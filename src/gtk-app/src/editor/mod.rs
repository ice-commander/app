mod raw;

use adw::prelude::*;
use panel_router::PanelRouter;
use std::path::Path;
use std::rc::Rc;

pub fn show_editor(
    parent_window: &impl IsA<gtk::Window>,
    entry: gtk_fm_ui::FileEntry,
    router: Rc<PanelRouter>,
) {
    if entry.is_dir() {
        return;
    }
    let parent = parent_window.clone().upcast::<gtk::Window>();
    if let Some(template) = external_editor(&router) {
        launch_external(&parent, &router, entry.path(), template);
        return;
    }

    let file_path_str = entry.path();
    let name = entry.name();
    crate::viewer::source::confirm_large_file(parent_window, &entry.name(), entry.size(), move || {
        crate::viewer::open_in_host(&parent, file_path_str.clone(), name.clone(), router.clone(), true);
    });
}

fn external_editor(router: &Rc<PanelRouter>) -> Option<String> {
    let config = router.config();
    if config.get::<String>("ui.editor_type").as_deref() != Some("external") {
        return None;
    }
    config
        .get::<String>("ui.external_editor_path")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn launch_external(
    parent: &gtk::Window,
    router: &Rc<PanelRouter>,
    display_path: String,
    template: String,
) {
    let rel = router.state.resolve_relative(&display_path);
    let provider = router.provider();
    let router = router.clone();
    let parent = parent.clone();
    gtk::glib::spawn_future_local(async move {
        match crate::external::edit_and_write_back(provider, rel, template).await {
            Ok(written) => {
                if written {
                    let _ = router.refresh().await;
                }
            }
            Err(e) => {
                ic_logging::warn!("external editor: {e}");
                let dlg = adw::AlertDialog::builder()
                    .heading(&*crate::i18n::tr("editor.external_failed"))
                    .body(&e)
                    .build();
                dlg.add_response("ok", "OK");
                dlg.present(Some(&parent));
            }
        }
    });
}

#[allow(dead_code)] // path-string overload of `show_editor`; entry point for remote open
pub fn show_editor_for_path(
    parent_window: &impl IsA<gtk::Window>,
    file_path_str: &str,
    active_fm: Option<Rc<PanelRouter>>,
) {
    let Some(router) = active_fm else {
        return;
    };
    let name = Path::new(file_path_str)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if let Some(template) = external_editor(&router) {
        let parent = parent_window.clone().upcast::<gtk::Window>();
        launch_external(&parent, &router, file_path_str.to_string(), template);
        return;
    }
    crate::viewer::open_in_host(parent_window, file_path_str.to_string(), name, router, true);
}

pub fn show_new_file_window(
    parent_window: &impl IsA<gtk::Window>,
    active_fm: Option<Rc<PanelRouter>>,
) {
    let Some(router) = active_fm else {
        return;
    };
    crate::viewer::new_file_in_host(parent_window, router);
}

pub(crate) fn raw_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    raw::extract_raw_thumbnail_from_bytes(bytes)
}
