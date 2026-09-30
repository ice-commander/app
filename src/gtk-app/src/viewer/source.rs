use adw::prelude::*;

pub(crate) fn confirm_large_file(
    parent: &impl IsA<gtk::Window>,
    name: &str,
    size: u64,
    open: impl Fn() + 'static,
) {
    const WARN_AT: u64 = 64 * 1024 * 1024;
    if size <= WARN_AT {
        open();
        return;
    }

    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr("viewer.large_file"))
        .body(&crate::i18n::trf(
            "viewer.large_file_body",
            &[
                ("name", name),
                ("size", &gtk_fm_ui::utils::format_size(size)),
            ],
        ))
        .build();
    dialog.add_response("cancel", &crate::i18n::tr("editor.cancel"));
    dialog.add_response("open", &crate::i18n::tr("viewer.open_anyway"));
    dialog.set_response_appearance("open", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.connect_response(None, move |_, response| {
        if response == "open" {
            open();
        }
    });
    dialog.present(Some(parent.upcast_ref::<gtk::Window>()));
}

/// What the built-in cascade used to sort a file into. Nothing dispatches on
/// it now: a plugin claims a file by extension, or the text and hex viewer
/// takes it.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Audio,
    Video,
    Pdf,
}
