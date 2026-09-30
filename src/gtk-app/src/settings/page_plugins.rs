use adw::prelude::*;
use gtk::{Align, Box, Button, Label, Orientation};
use ic_plugin_host::catalog::{self, Offer};
use ic_plugin_host::loader;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// One row's worth of what is known about a plugin, whether it is installed,
/// switched off, or only offered by the catalogue.
struct Entry {
    library: String,
    path: Option<PathBuf>,
    about: Option<loader::About>,
    enabled: bool,
    offered: Option<Offer>,
}

fn shown(text: &str, fallback: &str) -> String {
    if text.is_empty() {
        fallback.to_string()
    } else {
        text.to_string()
    }
}

fn installed_entries(config: &client_config::AppConfig) -> Vec<Entry> {
    loader::installed_with_about()
        .into_iter()
        .map(|held| Entry {
            enabled: loader::is_enabled(config, &held.library),
            about: held.about,
            library: held.library,
            path: Some(held.path),
            offered: None,
        })
        .collect()
}

/// Folds the catalogue into the installed list: a known plugin gains its offer,
/// an unknown one becomes a row of its own.
fn merge(mut rows: Vec<Entry>, offers: Vec<Offer>) -> Vec<Entry> {
    for offer in offers {
        match rows.iter_mut().find(|row| row.library == offer.library) {
            Some(row) => row.offered = Some(offer),
            None => rows.push(Entry {
                library: offer.library.clone(),
                path: None,
                about: None,
                enabled: true,
                offered: Some(offer),
            }),
        }
    }
    rows.sort_by(|a, b| a.library.cmp(&b.library));
    rows
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

    let group = adw::PreferencesGroup::builder()
        .title(&*crate::i18n::tr("settings.plugins_group"))
        .description(&*crate::i18n::tr("settings.plugins_group_desc"))
        .build();
    page_box.append(&group);

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

    let status = Label::builder()
        .halign(Align::Start)
        .margin_top(10)
        .wrap(true)
        .visible(false)
        .build();
    status.add_css_class("dim-label");
    page_box.append(&status);

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

    let offers: Rc<RefCell<Vec<Offer>>> = Rc::new(RefCell::new(Vec::new()));
    let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));

    // Nothing a row does takes effect before a restart, so a row never has to
    // redraw itself. Only the catalogue can add rows, and that is this closure.
    let refill: Rc<dyn Fn()> = {
        let group = group.clone();
        let rows = rows.clone();
        let offers = offers.clone();
        let config = config.clone();
        let restart_box = restart_box.clone();
        let status = status.clone();
        let parent = parent.clone();
        Rc::new(move || {
            for row in rows.borrow_mut().drain(..) {
                group.remove(&row);
            }
            let listed = merge(installed_entries(&config), offers.borrow().clone());
            if listed.is_empty() {
                status.set_label(&crate::i18n::tr("settings.plugins_none"));
                status.set_visible(true);
            }
            for entry in listed {
                let row = make_row(&entry, &config, &restart_box, &status, &parent);
                group.add(&row);
                rows.borrow_mut().push(row);
            }
        })
    };
    refill();

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
    {
        let config = config.clone();
        let offers = offers.clone();
        let status = status.clone();
        let refill = refill.clone();
        check_btn.connect_clicked(move |_| {
            status.set_visible(true);
            match catalog::fetch(&config) {
                Ok(found) => {
                    let count = found.len();
                    *offers.borrow_mut() = found;
                    status.set_label(&crate::i18n::trf(
                        "settings.plugins_catalogue_read",
                        &[("count", &count.to_string())],
                    ));
                    refill();
                }
                Err(why) => {
                    offers.borrow_mut().clear();
                    status.set_label(&why.to_string());
                    refill();
                }
            }
        });
    }
    actions.append(&check_btn);
    page_box.append(&actions);
}

fn make_row(
    entry: &Entry,
    config: &client_config::AppConfig,
    restart_box: &Box,
    status: &Label,
    parent: &gtk::Window,
) -> adw::ActionRow {
    let about = entry.about.clone().unwrap_or_default();
    let title = if about.name.is_empty() {
        entry.library.clone()
    } else {
        shown(&about.name, &entry.library)
    };
    let installed_version = about.version.clone();
    let offered_version = entry
        .offered
        .as_ref()
        .map(|offer| offer.version.clone())
        .unwrap_or_default();

    let mut subtitle = shown(&about.description, "");
    if entry.path.is_none() {
        let offered_name = entry
            .offered
            .as_ref()
            .map(|offer| offer.description.clone())
            .unwrap_or_default();
        if subtitle.is_empty() {
            subtitle = offered_name;
        }
    }
    let version_line = match (installed_version.as_str(), offered_version.as_str()) {
        ("", "") => String::new(),
        (held, "") => held.to_string(),
        ("", offered) => {
            crate::i18n::trf("settings.plugins_offered_version", &[("version", offered)])
                .to_string()
        }
        (held, offered) if catalog::is_newer(offered, held) => crate::i18n::trf(
            "settings.plugins_update_available",
            &[("installed", held), ("offered", offered)],
        )
        .to_string(),
        (held, _) => held.to_string(),
    };
    if !version_line.is_empty() {
        if subtitle.is_empty() {
            subtitle = version_line;
        } else {
            subtitle = format!("{subtitle}  \u{b7}  {version_line}");
        }
    }

    let row = adw::ActionRow::builder()
        .title(&title)
        .subtitle(&subtitle)
        .build();

    // Not installed: the only thing offered is fetching it.
    if entry.path.is_none() {
        let install = Button::builder()
            .label(&*crate::i18n::tr("settings.plugins_install"))
            .valign(Align::Center)
            .build();
        install.set_cursor_from_name(Some("pointer"));
        let offer = entry.offered.clone().unwrap_or_default();
        let status = status.clone();
        let restart_box = restart_box.clone();
        let row_for_install = row.clone();
        install.connect_clicked(move |button| match catalog::install(&offer) {
            Ok(path) => {
                status.set_visible(true);
                status.set_label(&path.display().to_string());
                restart_box.set_visible(true);
                button.set_sensitive(false);
                row_for_install.set_subtitle(&crate::i18n::tr("settings.plugins_after_restart"));
            }
            Err(why) => {
                status.set_visible(true);
                status.set_label(&why.to_string());
            }
        });
        row.add_suffix(&install);
        return row;
    }

    let switch = gtk::Switch::builder()
        .active(entry.enabled)
        .valign(Align::Center)
        .build();
    {
        let config = config.clone();
        let library = entry.library.clone();
        let restart_box = restart_box.clone();
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
        let title = title.clone();
        let parent = parent.clone();
        let status = status.clone();
        let restart_box = restart_box.clone();
        let row_for_delete = row.clone();
        let switch_for_delete = switch.clone();
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
            let status = status.clone();
            let restart_box = restart_box.clone();
            let row_for_delete = row_for_delete.clone();
            let switch_for_delete = switch_for_delete.clone();
            let button = button.clone();
            ask.connect_response(None, move |dialog, answer| {
                dialog.close();
                if answer != "delete" {
                    return;
                }
                match loader::remove(&path) {
                    Ok(()) => {
                        restart_box.set_visible(true);
                        button.set_sensitive(false);
                        switch_for_delete.set_sensitive(false);
                        row_for_delete
                            .set_subtitle(&crate::i18n::tr("settings.plugins_after_restart"));
                    }
                    Err(why) => {
                        status.set_visible(true);
                        status.set_label(&why);
                    }
                }
            });
            ask.present(Some(&parent));
        });
    }
    row.add_suffix(&delete);
    row
}

#[cfg(test)]
mod tests {
    use super::{merge, Entry};
    use ic_plugin_host::catalog::Offer;
    use std::path::PathBuf;

    fn installed(library: &str, version: &str) -> Entry {
        Entry {
            library: library.to_string(),
            path: Some(PathBuf::from(format!("/plugins/lib{library}.so"))),
            about: Some(ic_plugin_host::loader::About {
                id: format!("ic-{library}"),
                name: library.to_string(),
                version: version.to_string(),
                description: String::new(),
                abi_version: Some(ic_plugin_api::IC_ABI_VERSION),
            }),
            enabled: true,
            offered: None,
        }
    }

    fn offer(library: &str, version: &str) -> Offer {
        Offer {
            id: format!("ic-{library}"),
            library: library.to_string(),
            version: version.to_string(),
            ..Offer::default()
        }
    }

    #[test]
    fn a_catalogue_entry_lands_on_the_plugin_it_names() {
        let listed = merge(
            vec![installed("ic_ftp_fs", "0.1.0")],
            vec![offer("ic_ftp_fs", "0.2.0")],
        );
        assert_eq!(listed.len(), 1, "the same plugin is not listed twice");
        assert_eq!(
            listed[0].offered.as_ref().map(|o| o.version.as_str()),
            Some("0.2.0")
        );
        assert!(listed[0].path.is_some());
    }

    #[test]
    fn a_plugin_only_the_catalogue_knows_becomes_a_row_with_nothing_local() {
        let listed = merge(
            vec![installed("ic_ftp_fs", "0.1.0")],
            vec![offer("ic_node_in_net", "0.8.0")],
        );
        assert_eq!(listed.len(), 2);
        let fresh = listed
            .iter()
            .find(|row| row.library == "ic_node_in_net")
            .expect("listed");
        assert!(fresh.path.is_none(), "there is no file for it yet");
        assert!(fresh.about.is_none(), "and nothing to read from one");
        assert_eq!(
            listed
                .iter()
                .map(|row| row.library.as_str())
                .collect::<Vec<_>>(),
            vec!["ic_ftp_fs", "ic_node_in_net"],
            "rows are ordered by library name"
        );
    }

    #[test]
    fn an_empty_catalogue_leaves_the_installed_list_alone() {
        let listed = merge(vec![installed("ic_ftp_fs", "0.1.0")], Vec::new());
        assert_eq!(listed.len(), 1);
        assert!(listed[0].offered.is_none());
    }
}
