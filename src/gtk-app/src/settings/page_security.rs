use adw::prelude::*;
use gtk::{Align, Label};
use std::rc::Rc;

pub(super) fn build(page_box: &gtk::Box, parent: &gtk::Window, config: client_config::AppConfig) {
    let page_title = Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_security")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .margin_bottom(16)
        .build();
    page_box.append(&page_title);

    let group = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.security_group"))
        .description(&*crate::i18n::tr("settings.security_group_desc"))
        .build();

    let save_row = adw::SwitchRow::builder()
        .title(&*crate::i18n::tr("settings.save_passwords"))
        .subtitle(&*crate::i18n::tr("settings.desc_save_passwords"))
        .active(crate::secret_store::saving_enabled(&config))
        .build();

    let master_on = crate::secret_store::protection() == crate::secret_store::Protection::Password;
    let master_row = adw::SwitchRow::builder()
        .title(&*crate::i18n::tr("settings.master_password"))
        .subtitle(&*crate::i18n::tr("settings.desc_master_password"))
        .active(master_on)
        .sensitive(save_row.is_active())
        .build();

    let startup_row = adw::SwitchRow::builder()
        .title(&*crate::i18n::tr("settings.startup_password"))
        .subtitle(&*crate::i18n::tr("settings.desc_startup_password"))
        .active(
            config
                .get::<bool>("ui.require_startup_password")
                .unwrap_or(false),
        )
        .sensitive(master_on)
        .build();

    {
        let config = config.clone();
        let parent = parent.clone();
        let master_row = master_row.clone();
        save_row.connect_active_notify(move |row| {
            let on = row.is_active();
            master_row.set_sensitive(on);
            config.set("ui.save_passwords", on);
            config.save();
            if !on {
                crate::secret_store::forget_stored_secrets(&config);
                let dlg = adw::AlertDialog::builder()
                    .heading(&*crate::i18n::tr("settings.save_passwords"))
                    .body(&*crate::i18n::tr("security.secrets_erased"))
                    .build();
                dlg.add_response("ok", "OK");
                dlg.present(Some(&parent));
            }
        });
    }

    {
        let config = config.clone();
        let parent = parent.clone();
        let startup_row = startup_row.clone();
        let reverting = Rc::new(std::cell::Cell::new(false));
        master_row.connect_active_notify(move |row| {
            if reverting.get() {
                return;
            }
            let wants_on = row.is_active();
            let row_c = row.clone();
            let startup_row = startup_row.clone();
            let reverting_c = reverting.clone();
            let config_c = config.clone();
            let on_done: Rc<dyn Fn(bool)> = Rc::new(move |ok: bool| {
                if ok {
                    startup_row.set_sensitive(wants_on);
                    if !wants_on {
                        startup_row.set_active(false);
                        config_c.set("ui.require_startup_password", false);
                        config_c.save();
                    }
                    return;
                }
                reverting_c.set(true);
                row_c.set_active(!wants_on);
                reverting_c.set(false);
            });

            if wants_on {
                crate::master_password::prompt_set(&parent, config.clone(), on_done);
            } else {
                crate::master_password::prompt_clear(&parent, on_done);
            }
        });
    }

    {
        let config = config.clone();
        startup_row.connect_active_notify(move |row| {
            config.set("ui.require_startup_password", row.is_active());
            config.save();
        });
    }

    group.add(&save_row);
    group.add(&master_row);
    group.add(&startup_row);
    page_box.append(&group);
}
