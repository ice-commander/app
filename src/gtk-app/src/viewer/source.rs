use adw::prelude::*;
use std::rc::Rc;

pub(crate) fn fetching_window(
    parent_window: &impl IsA<gtk::Window>,
    file_name: &str,
    total: u64,
) -> (gtk::Window, gtk::Label, Rc<std::cell::Cell<bool>>) {
    let window = gtk::Window::builder()
        .default_width(360)
        .modal(true)
        .transient_for(parent_window)
        .title(&format!("{} - {}", crate::i18n::tr("viewer.fetching"), file_name))
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    let spinner = gtk::Spinner::builder().width_request(32).height_request(32).build();
    spinner.start();
    content.append(&spinner);

    let label = gtk::Label::new(Some(&crate::i18n::trf(
        "viewer.fetching_progress",
        &[("done", "0"), ("total", &gtk_fm_ui::utils::format_size(total))],
    )));
    content.append(&label);

    let cancelled = Rc::new(std::cell::Cell::new(false));
    let btn = gtk::Button::with_label(&crate::i18n::tr("editor.cancel"));
    let win_btn = window.clone();
    let cancelled_btn = cancelled.clone();
    btn.connect_clicked(move |_| {
        cancelled_btn.set(true);
        win_btn.close();
    });
    content.append(&btn);

    let cancelled_close = cancelled.clone();
    window.connect_close_request(move |_| {
        cancelled_close.set(true);
        gtk::glib::Propagation::Proceed
    });

    window.set_child(Some(&content));
    window.present();
    (window, label, cancelled)
}

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
            &[("name", name), ("size", &gtk_fm_ui::utils::format_size(size))],
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Audio,
    Video,
    Pdf,
}

pub(crate) fn sniff(path: &str) -> Option<Kind> {
    use std::io::Read;

    let mut head = [0u8; 16];
    let read = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
    let head = &head[..read];

    if head.starts_with(b"%PDF-") {
        return Some(Kind::Pdf);
    }
    if head.starts_with(b"ID3")
        || head.starts_with(b"fLaC")
        || head.starts_with(b"OggS")
        || (head.len() >= 2 && head[0] == 0xFF && matches!(head[1], 0xFB | 0xF3 | 0xF2 | 0xE3))
    {
        return Some(Kind::Audio);
    }
    if head.len() >= 12 && head.starts_with(b"RIFF") {
        return match &head[8..12] {
            b"WAVE" => Some(Kind::Audio),
            b"AVI " => Some(Kind::Video),
            _ => None,
        };
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        return match &head[8..11] {
            b"M4A" | b"M4B" => Some(Kind::Audio),
            b"crx" => None,
            _ => Some(Kind::Video),
        };
    }
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) || head.starts_with(&[0x30, 0x26, 0xB2, 0x75]) {
        return Some(Kind::Video);
    }
    None
}
