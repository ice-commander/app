use crate::plugin_offers::{self, Listed};
use adw::prelude::*;
use gtk::{Align, Box, Button, Label, Orientation};
use ic_plugin_host::loader;
use ic_plugin_host::updates::Standing;
use std::cell::RefCell;
use std::rc::Rc;

/// What a row needs from the page around it.
#[derive(Clone)]
struct Around {
    config: client_config::AppConfig,
    parent: gtk::Window,
    restart_box: Box,
    status: Label,
}

pub(super) fn build(page_box: &Box, parent: &gtk::Window, config: client_config::AppConfig) {
    let page_title = Label::builder()
        .label(&format!(
            "<span size='x-large' weight='bold'>{}</span>",
            crate::i18n::tr("settings.cat_plugins")
        ))
        .use_markup(true)
        .halign(Align::Start)
        .margin_bottom(16)
        .build();
    page_box.append(&page_title);

    let restart_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_top(4)
        .margin_bottom(8)
        .halign(Align::Start)
        .visible(false)
        .build();
    let restart_warning = Label::builder()
        .use_markup(true)
        .halign(Align::Start)
        .build();
    restart_warning.set_markup(&format!(
        "<span foreground='orange'><b>{}</b></span>",
        crate::i18n::tr("restart_required")
    ));
    let restart_btn = Button::builder()
        .label(&*crate::i18n::tr("settings.restart_btn"))
        .valign(Align::Center)
        .build();
    restart_btn.add_css_class("suggested-action");
    restart_btn.set_cursor_from_name(Some("pointer"));
    restart_btn.connect_clicked(|_| crate::utils::restart_app());
    restart_box.append(&restart_warning);
    restart_box.append(&restart_btn);
    page_box.append(&restart_box);

    if !loader::has_chosen(&config) {
        let never = Label::builder()
            .label(&*crate::i18n::tr("settings.plugins_never_chosen"))
            .halign(Align::Start)
            .wrap(true)
            .margin_bottom(8)
            .build();
        never.add_css_class("dim-label");
        page_box.append(&never);
    }

    let installed_group = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.plugins_group"))
        .description(&*crate::i18n::tr("settings.plugins_group_desc"))
        .build();
    page_box.append(&installed_group);

    let available_group = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.plugins_available_group"))
        .description(&*crate::i18n::tr("settings.plugins_available_group_desc"))
        .margin_top(18)
        .visible(false)
        .build();
    page_box.append(&available_group);

    let status = Label::builder()
        .halign(Align::Start)
        .margin_top(10)
        .wrap(true)
        .visible(false)
        .build();
    status.add_css_class("dim-label");
    page_box.append(&status);

    let actions = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .margin_top(14)
        .halign(Align::Start)
        .build();
    let check_btn = Button::builder()
        .label(&*crate::i18n::tr("settings.plugins_check"))
        .build();
    check_btn.set_cursor_from_name(Some("pointer"));
    let update_all_btn = Button::builder()
        .label(&*crate::i18n::tr("settings.plugins_update_all"))
        .visible(false)
        .build();
    update_all_btn.add_css_class("suggested-action");
    update_all_btn.set_cursor_from_name(Some("pointer"));
    actions.append(&check_btn);
    actions.append(&update_all_btn);
    page_box.append(&actions);

    let folder = Label::builder()
        .halign(Align::Start)
        .margin_top(12)
        .wrap(true)
        .build();
    folder.add_css_class("dim-label");
    folder.set_label(&match loader::plugin_directory() {
        Some(path) => path.display().to_string(),
        None => crate::i18n::tr("settings.plugins_no_folder").to_string(),
    });
    page_box.append(&folder);

    let around = Around {
        config,
        parent: parent.clone(),
        restart_box,
        status: status.clone(),
    };
    let rows: Rc<RefCell<Vec<(adw::PreferencesGroup, adw::ActionRow)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let update_buttons: Rc<RefCell<Vec<Button>>> = Rc::new(RefCell::new(Vec::new()));

    // Nothing a row does takes effect before a restart, so a row never redraws itself: only a fresh listing does.
    let fill: Rc<dyn Fn(Vec<Listed>)> = {
        let installed_group = installed_group.clone();
        let available_group = available_group.clone();
        let update_all_btn = update_all_btn.clone();
        let rows = rows.clone();
        let update_buttons = update_buttons.clone();
        let around = around.clone();
        Rc::new(move |listed: Vec<Listed>| {
            for (group, row) in rows.borrow_mut().drain(..) {
                group.remove(&row);
            }
            update_buttons.borrow_mut().clear();
            let mut available = 0;
            for entry in listed {
                if entry.standing == Standing::Waiting {
                    around.restart_box.set_visible(true);
                }
                let (group, row) = if entry.path.is_some() {
                    let (row, update) = installed_row(entry, &around);
                    update_buttons.borrow_mut().extend(update);
                    (installed_group.clone(), row)
                } else {
                    available += 1;
                    (available_group.clone(), available_row(entry, &around))
                };
                group.add(&row);
                rows.borrow_mut().push((group, row));
            }
            available_group.set_visible(available > 0);
            update_all_btn.set_visible(!update_buttons.borrow().is_empty());
            update_all_btn.set_sensitive(true);
        })
    };
    fill(plugin_offers::listing(&Default::default()));

    let ask_site: Rc<dyn Fn()> = {
        let fill = fill.clone();
        let status = status.clone();
        let check_btn = check_btn.clone();
        Rc::new(move || {
            status.set_label(&crate::i18n::tr("settings.plugins_asking"));
            status.set_visible(true);
            check_btn.set_sensitive(false);
            let fill = fill.clone();
            let status = status.clone();
            let check_btn = check_btn.clone();
            gtk::glib::spawn_future_local(async move {
                let (listed, failed) = plugin_offers::with_offers().await;
                match failed {
                    Some(why) => status.set_label(&crate::i18n::trf(
                        "settings.plugins_unreachable",
                        &[("error", &why)],
                    )),
                    None if listed.is_empty() => {
                        status.set_label(&crate::i18n::tr("settings.plugins_none"))
                    }
                    None => status.set_visible(false),
                }
                fill(listed);
                check_btn.set_sensitive(true);
            });
        })
    };
    {
        let ask_site = ask_site.clone();
        check_btn.connect_clicked(move |_| ask_site());
    }
    update_all_btn.connect_clicked(move |button| {
        button.set_sensitive(false);
        for update in update_buttons.borrow().iter() {
            if update.is_sensitive() && update.is_visible() {
                update.emit_clicked();
            }
        }
    });
    ask_site();
}

/// Downloads on a click, the button standing in for progress; `then` runs once all of the plugin is there.
fn fetch_on_click(
    button: &Button,
    entry: Listed,
    row: &adw::ActionRow,
    around: &Around,
    then: impl Fn() + 'static,
) {
    let label = button.label().unwrap_or_default();
    let row = row.clone();
    let around = around.clone();
    let then = Rc::new(then);
    button.connect_clicked(move |button| {
        button.set_sensitive(false);
        button.set_label(&crate::i18n::tr("settings.plugins_downloading"));
        let button = button.clone();
        let entry = entry.clone();
        let row = row.clone();
        let around = around.clone();
        let label = label.clone();
        let then = then.clone();
        gtk::glib::spawn_future_local(async move {
            match plugin_offers::fetch(&entry).await {
                Ok(()) => {
                    then();
                    button.set_visible(false);
                    row.set_subtitle(&crate::i18n::tr("settings.plugins_after_restart"));
                    around.restart_box.set_visible(true);
                }
                Err(why) => {
                    button.set_label(&label);
                    button.set_sensitive(true);
                    around.status.set_label(&crate::i18n::trf(
                        "settings.plugins_failed",
                        &[("name", &entry.name), ("error", &why)],
                    ));
                    around.status.set_visible(true);
                }
            }
        });
    });
}

/// Not installed: the only thing offered is fetching it, and it is switched on with that.
fn available_row(entry: Listed, around: &Around) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(&entry.name).build();
    if entry.standing == Standing::Waiting {
        row.set_subtitle(&crate::i18n::tr("settings.plugins_after_restart"));
        return row;
    }
    row.set_subtitle(&plugin_offers::subtitle(&[
        &entry.description,
        &entry.version,
        &plugin_offers::size_text(entry.size),
    ]));
    let install = Button::builder()
        .label(&*crate::i18n::tr("settings.plugins_install"))
        .valign(Align::Center)
        .build();
    install.set_cursor_from_name(Some("pointer"));
    let config = around.config.clone();
    let library = entry.library.clone();
    fetch_on_click(&install, entry, &row, around, move || {
        loader::set_enabled(&config, &library, true)
    });
    row.add_suffix(&install);
    row
}

/// An installed plugin: switched on or off, deleted, and updated when the site has something else for it. Returns the
/// update button when there is one.
fn installed_row(entry: Listed, around: &Around) -> (adw::ActionRow, Option<Button>) {
    let offered_version = entry
        .offered
        .as_ref()
        .map(|offer| offer.version.clone())
        .unwrap_or_default();
    let version_line = if entry.standing == Standing::Behind && offered_version != entry.version {
        crate::i18n::trf(
            "settings.plugins_update_available",
            &[("installed", &entry.version), ("offered", &offered_version)],
        )
        .to_string()
    } else {
        entry.version.clone()
    };
    let subtitle = match entry.standing {
        Standing::Waiting => crate::i18n::tr("settings.plugins_after_restart").to_string(),
        Standing::Behind => plugin_offers::subtitle(&[
            &entry.description,
            &version_line,
            &plugin_offers::size_text(entry.size),
        ]),
        Standing::Absent | Standing::Current => {
            plugin_offers::subtitle(&[&entry.description, &version_line])
        }
    };
    let row = adw::ActionRow::builder()
        .title(&entry.name)
        .subtitle(&subtitle)
        .build();

    let update = (entry.standing == Standing::Behind).then(|| {
        let update = Button::builder()
            .label(&*crate::i18n::tr("settings.plugins_update"))
            .valign(Align::Center)
            .build();
        update.add_css_class("suggested-action");
        update.set_cursor_from_name(Some("pointer"));
        fetch_on_click(&update, entry.clone(), &row, around, || {});
        row.add_suffix(&update);
        update
    });

    let switch = gtk::Switch::builder()
        .active(loader::is_enabled(&around.config, &entry.library))
        .valign(Align::Center)
        .build();
    {
        let config = around.config.clone();
        let library = entry.library.clone();
        let restart_box = around.restart_box.clone();
        switch.connect_state_set(move |_, on| {
            if loader::is_enabled(&config, &library) != on {
                loader::set_enabled(&config, &library, on);
                restart_box.set_visible(true);
            }
            gtk::glib::Propagation::Proceed
        });
    }
    row.add_suffix(&switch);

    let delete = Button::builder()
        .icon_name("user-trash-symbolic")
        .valign(Align::Center)
        .tooltip_text(&*crate::i18n::tr("settings.plugins_delete"))
        .build();
    delete.add_css_class("flat");
    delete.set_cursor_from_name(Some("pointer"));
    {
        let path = entry.path.clone().unwrap_or_default();
        let title = entry.name.clone();
        let around = around.clone();
        let row_for_delete = row.clone();
        let switch_for_delete = switch.clone();
        let update_for_delete = update.clone();
        delete.connect_clicked(move |button| {
            let ask = adw::AlertDialog::builder()
                .heading(&*crate::i18n::tr("settings.plugins_delete"))
                .body(&crate::i18n::trf(
                    "settings.plugins_delete_body",
                    &[("name", &title)],
                ))
                .build();
            ask.add_response("cancel", &crate::i18n::tr("account.cancel"));
            ask.add_response("delete", &crate::i18n::tr("settings.plugins_delete"));
            ask.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            ask.set_default_response(Some("cancel"));
            ask.set_close_response("cancel");
            let path = path.clone();
            let around = around.clone();
            let row_for_delete = row_for_delete.clone();
            let switch_for_delete = switch_for_delete.clone();
            let update_for_delete = update_for_delete.clone();
            let button = button.clone();
            ask.connect_response(None, move |dialog, answer| {
                dialog.close();
                if answer != "delete" {
                    return;
                }
                match loader::remove(&path) {
                    Ok(()) => {
                        around.restart_box.set_visible(true);
                        button.set_sensitive(false);
                        switch_for_delete.set_sensitive(false);
                        if let Some(update) = &update_for_delete {
                            update.set_visible(false);
                        }
                        row_for_delete
                            .set_subtitle(&crate::i18n::tr("settings.plugins_after_restart"));
                    }
                    Err(why) => {
                        around.status.set_visible(true);
                        around.status.set_label(&why);
                    }
                }
            });
            ask.present(Some(&around.parent));
        });
    }
    row.add_suffix(&delete);
    (row, update)
}
