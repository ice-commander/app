use adw::prelude::*;
use std::rc::Rc;

fn password_entry(placeholder_key: &str) -> gtk::PasswordEntry {
    gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .placeholder_text(&*crate::i18n::tr(placeholder_key))
        .activates_default(true)
        .build()
}

pub fn prompt_unlock(parent: Option<&gtk::Window>, on_done: Rc<dyn Fn(bool)>) {
    if crate::secret_store::is_unlocked() {
        on_done(true);
        return;
    }

    let entry = password_entry("security.master_placeholder");
    let error = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .build();
    error.add_css_class("error");

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    body.append(&entry);
    body.append(&error);

    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr("security.unlock_title"))
        .body(&*crate::i18n::tr("security.unlock_body"))
        .extra_child(&body)
        .build();
    dialog.add_response("cancel", &crate::i18n::tr("account.cancel"));
    dialog.add_response("unlock", &crate::i18n::tr("security.unlock"));
    dialog.set_response_appearance("unlock", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("unlock"));
    dialog.set_close_response("cancel");

    let parent = parent.cloned();
    let parent_present = parent.clone();
    dialog.connect_response(None, move |dlg, response| {
        if response != "unlock" {
            dlg.close();
            on_done(false);
            return;
        }
        match crate::secret_store::unlock_with_password(&entry.text()) {
            Ok(()) => {
                dlg.close();
                on_done(true);
            }
            Err(_) => {
                error.set_text(&crate::i18n::tr("security.wrong_password"));
                error.set_visible(true);
                entry.set_text("");
                entry.grab_focus();
            }
        }
    });

    dialog.present(parent_present.as_ref());
}

pub fn prompt_set(
    parent: &gtk::Window,
    config: client_config::AppConfig,
    on_done: Rc<dyn Fn(bool)>,
) {
    let first = password_entry("security.new_placeholder");
    let again = password_entry("security.repeat_placeholder");
    let error = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .build();
    error.add_css_class("error");

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    body.append(&first);
    body.append(&again);
    body.append(&error);

    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr("security.set_title"))
        .body(&*crate::i18n::tr("security.set_body"))
        .extra_child(&body)
        .build();
    dialog.add_response("cancel", &crate::i18n::tr("account.cancel"));
    dialog.add_response("set", &crate::i18n::tr("security.set"));
    dialog.set_response_appearance("set", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("set"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |dlg, response| {
        if response != "set" {
            dlg.close();
            on_done(false);
            return;
        }
        let pw = first.text().to_string();
        if pw.is_empty() {
            error.set_text(&crate::i18n::tr("security.empty_password"));
            error.set_visible(true);
            return;
        }
        if pw != again.text().as_str() {
            error.set_text(&crate::i18n::tr("security.mismatch"));
            error.set_visible(true);
            again.set_text("");
            again.grab_focus();
            return;
        }

        crate::secret_store::upgrade_legacy_secrets(&config);

        match crate::secret_store::set_master_password(Some(&pw)) {
            Ok(()) => {
                dlg.close();
                on_done(true);
            }
            Err(_) => {
                error.set_text(&crate::i18n::tr("security.keyring_failed"));
                error.set_visible(true);
            }
        }
    });

    dialog.present(Some(parent));
}

pub fn prompt_clear(parent: &gtk::Window, on_done: Rc<dyn Fn(bool)>) {
    let entry = password_entry("security.master_placeholder");
    let error = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .build();
    error.add_css_class("error");

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    body.append(&entry);
    body.append(&error);

    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr("security.clear_title"))
        .body(&*crate::i18n::tr("security.clear_body"))
        .extra_child(&body)
        .build();
    dialog.add_response("cancel", &crate::i18n::tr("account.cancel"));
    dialog.add_response("clear", &crate::i18n::tr("security.clear"));
    dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("clear"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |dlg, response| {
        if response != "clear" {
            dlg.close();
            on_done(false);
            return;
        }
        if crate::secret_store::unlock_with_password(&entry.text()).is_err() {
            error.set_text(&crate::i18n::tr("security.wrong_password"));
            error.set_visible(true);
            entry.set_text("");
            entry.grab_focus();
            return;
        }
        match crate::secret_store::set_master_password(None) {
            Ok(()) => {
                dlg.close();
                on_done(true);
            }
            Err(_) => {
                error.set_text(&crate::i18n::tr("security.keyring_failed"));
                error.set_visible(true);
            }
        }
    });

    dialog.present(Some(parent));
}
