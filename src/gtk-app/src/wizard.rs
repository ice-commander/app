use adw::prelude::*;
use gtk::{Align, Box, Button, DropDown, Label, ListBox, ListBoxRow, Orientation, SelectionMode, Switch};
use std::cell::RefCell;
use std::rc::Rc;

type Retrans = Rc<RefCell<Vec<Rc<dyn Fn()>>>>;

fn reg(retrans: &Retrans, f: Rc<dyn Fn()>) {
    f();
    retrans.borrow_mut().push(f);
}

fn tr_label(retrans: &Retrans, text: impl Fn() -> String + 'static) -> Label {
    let l = Label::new(None);
    let w = l.clone();
    reg(retrans, Rc::new(move || w.set_text(&text())));
    l
}

fn tr_markup(retrans: &Retrans, markup: impl Fn() -> String + 'static) -> Label {
    let l = Label::builder().use_markup(true).build();
    let w = l.clone();
    reg(retrans, Rc::new(move || w.set_markup(&markup())));
    l
}

pub fn should_show(config: &client_config::AppConfig) -> bool {
    !config.get::<bool>("ui.setup_wizard_done").unwrap_or(false)
}

pub fn show_setup_wizard(
    parent_window: &gtk::Window,
    config: client_config::AppConfig,
    on_connections_changed: Rc<dyn Fn() + 'static>,
) {
    let wizard = gtk::Window::builder()
        .title("Welcome to Ice Commander")
        .transient_for(parent_window)
        .modal(true)
        .default_width(640)
        .default_height(720)
        .resizable(false)
        .build();

    let root = Box::builder().orientation(Orientation::Vertical).build();
    let carousel = adw::Carousel::builder().hexpand(true).vexpand(true).build();

    let retrans: Retrans = Rc::new(RefCell::new(Vec::new()));
    let original_lang = config.get::<String>("ui.language").unwrap_or_else(|| "en".to_string());

    let restart_note = Label::new(None);
    restart_note.set_wrap(true);
    restart_note.set_visible(false);
    restart_note.add_css_class("dim-label");
    {
        let n = restart_note.clone();
        reg(&retrans, Rc::new(move || {
            n.set_text(&crate::i18n::tr("wizard.restart_note"));
        }));
    }

    let on_language_selected: Rc<dyn Fn(bool)> = {
        let note = restart_note.clone();
        let retrans = retrans.clone();
        Rc::new(move |changed| {
            note.set_visible(changed);
            for f in retrans.borrow().iter() {
                f();
            }
        })
    };

    let mut pages: Vec<gtk::Widget> = Vec::new();

    let page1 = build_page_appearance(&config, &retrans, on_language_selected);
    carousel.append(&page1);
    pages.push(page1.upcast());

    let page2 = build_page_panels(&config, &retrans, on_connections_changed.clone());
    carousel.append(&page2);
    pages.push(page2.upcast());

    let pages = Rc::new(pages);

    let footer = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(14)
        .margin_start(20)
        .margin_end(20)
        .margin_top(6)
        .margin_bottom(18)
        .build();

    let dots = adw::CarouselIndicatorDots::new();
    dots.set_carousel(Some(&carousel));
    dots.set_halign(Align::Center);
    footer.append(&dots);
    footer.append(&restart_note);

    let buttons = Box::builder().orientation(Orientation::Horizontal).spacing(10).build();

    let later_btn = Button::new();
    later_btn.add_css_class("flat");
    {
        let b = later_btn.clone();
        reg(&retrans, Rc::new(move || {
            b.set_label(&crate::i18n::tr("wizard.later"));
        }));
    }

    let spacer = Box::builder().hexpand(true).build();

    let back_btn = Button::new();
    back_btn.add_css_class("flat");
    back_btn.set_visible(false);
    {
        let b = back_btn.clone();
        reg(&retrans, Rc::new(move || b.set_label(&crate::i18n::tr("wizard.back"))));
    }

    let next_btn = Button::new();
    next_btn.add_css_class("suggested-action");

    buttons.append(&later_btn);
    buttons.append(&spacer);
    buttons.append(&back_btn);
    buttons.append(&next_btn);
    footer.append(&buttons);

    root.append(&carousel);
    root.append(&footer);
    wizard.set_child(Some(&root));

    let update_nav: Rc<dyn Fn(u32)> = {
        let back_btn = back_btn.clone();
        let next_btn = next_btn.clone();
        let carousel = carousel.clone();
        Rc::new(move |index: u32| {
            let n = carousel.n_pages();
            back_btn.set_visible(index > 0);
            next_btn.set_label(&if index + 1 >= n {
                crate::i18n::tr("wizard.finish").to_string()
            } else {
                crate::i18n::tr("wizard.next").to_string()
            });
        })
    };
    update_nav(0);
    {
        let update_nav = update_nav.clone();
        let carousel = carousel.clone();
        reg(&retrans, Rc::new(move || update_nav(carousel.position().round() as u32)));
    }
    {
        let update_nav = update_nav.clone();
        carousel.connect_page_changed(move |_carousel, index| update_nav(index));
    }

    {
        let carousel = carousel.clone();
        let pages = pages.clone();
        back_btn.connect_clicked(move |_| {
            let idx = carousel.position().round() as usize;
            if idx > 0 {
                carousel.scroll_to(&pages[idx - 1], true);
            }
        });
    }

    {
        let carousel = carousel.clone();
        let pages = pages.clone();
        let config = config.clone();
        let wizard = wizard.clone();
        let original_lang = original_lang.clone();
        next_btn.connect_clicked(move |_| {
            let idx = carousel.position().round() as usize;
            if idx + 1 < pages.len() {
                carousel.scroll_to(&pages[idx + 1], true);
            } else {
                config.set("ui.setup_wizard_done", true);
                config.save();
                let lang_changed =
                    config.get::<String>("ui.language").unwrap_or_default() != original_lang;
                wizard.close();
                if lang_changed {
                    crate::utils::restart_app();
                }
            }
        });
    }

    {
        let wizard = wizard.clone();
        later_btn.connect_clicked(move |_| wizard.close());
    }

    wizard.present();
}

fn setting_row(
    list: &ListBox,
    retrans: &Retrans,
    title: impl Fn() -> String + 'static,
    desc: impl Fn() -> String + 'static,
    control: &impl IsA<gtk::Widget>,
) {
    let row = ListBoxRow::new();
    let b = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(16)
        .margin_end(16)
        .build();
    let t = tr_markup(retrans, move || {
        format!("<b>{}</b>", gtk::glib::markup_escape_text(&title()))
    });
    t.set_halign(Align::Start);
    let d = tr_label(retrans, desc);
    d.set_halign(Align::Start);
    d.set_wrap(true);
    d.add_css_class("dim-label");
    control.set_halign(Align::Start);
    b.append(&t);
    b.append(&d);
    b.append(control);
    row.set_child(Some(&b));
    list.append(&row);
}

fn page_shell(
    retrans: &Retrans,
    title: impl Fn() -> String + 'static,
    subtitle: impl Fn() -> String + 'static,
) -> (gtk::ScrolledWindow, Box) {
    let content = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(10)
        .margin_start(28)
        .margin_end(28)
        .margin_top(28)
        .margin_bottom(20)
        .build();
    let heading = tr_markup(retrans, move || {
        format!(
            "<span size='xx-large' weight='bold'>{}</span>",
            gtk::glib::markup_escape_text(&title())
        )
    });
    heading.set_halign(Align::Start);
    let sub = tr_label(retrans, subtitle);
    sub.set_halign(Align::Start);
    sub.set_wrap(true);
    sub.set_margin_bottom(8);
    sub.add_css_class("dim-label");
    content.append(&heading);
    content.append(&sub);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build();
    (scroll, content)
}

fn build_page_appearance(
    config: &client_config::AppConfig,
    retrans: &Retrans,
    on_language_selected: Rc<dyn Fn(bool)>,
) -> gtk::ScrolledWindow {
    let (scroll, content) = page_shell(
        retrans,
        || crate::i18n::tr("wizard.welcome_title").to_string(),
        || {
            crate::i18n::tr("wizard.welcome_sub")
            .to_string()
        },
    );

    let list = ListBox::builder().selection_mode(SelectionMode::None).build();
    list.add_css_class("boxed-list");

    let languages: &'static [(&str, &str)] = &[
        ("English", "en"),
        ("Polski", "pl"),
        ("Čeština", "cs"),
        ("Slovenčina", "sk"),
        ("Deutsch", "de"),
        ("Español", "es"),
        ("Українська", "uk"),
        ("Italiano", "it"),
        ("Français", "fr"),
        ("Română", "ro"),
        ("Magyar", "hu"),
        ("Беларуская", "be"),
        ("Български", "bg"),
        ("Русский", "ru"),
        ("Српски", "sr"),
    ];
    let names: Vec<&str> = languages.iter().map(|(n, _)| *n).collect();
    let lang_dd = DropDown::from_strings(&names[..]);
    let current_lang = config.get::<String>("ui.language").unwrap_or_else(|| "en".to_string());
    let original_lang = current_lang.clone();
    let lang_idx = languages.iter().position(|(_, c)| *c == current_lang).unwrap_or(0) as u32;
    lang_dd.set_selected(lang_idx);
    let config_lang = config.clone();
    lang_dd.connect_selected_notify(move |dd| {
        let i = dd.selected() as usize;
        if i < languages.len() {
            let code = languages[i].1;
            config_lang.set("ui.language", code);
            config_lang.save();
            crate::i18n::set_lang(code);
            on_language_selected(code != original_lang.as_str());
        }
    });
    setting_row(
        &list,
        retrans,
        || crate::i18n::tr("language_select").to_string(),
        || crate::i18n::tr("settings.desc_lang").to_string(),
        &lang_dd,
    );

    let theme_idx = config.get::<u32>("ui.theme_index").unwrap_or(0);
    let is_auto = theme_idx == 0;

    let apply_theme: Rc<dyn Fn(u32)> = {
        let cfg = config.clone();
        Rc::new(move |idx: u32| {
            let sm = adw::StyleManager::default();
            match idx {
                1 => sm.set_color_scheme(adw::ColorScheme::ForceLight),
                2 => sm.set_color_scheme(adw::ColorScheme::ForceDark),
                _ => sm.set_color_scheme(adw::ColorScheme::Default),
            }
            cfg.set("ui.theme_index", idx);
            cfg.save();
        })
    };

    let make_card = |retrans: &Retrans, label: fn() -> String, resource: &str| -> gtk::ToggleButton {
        let v = Box::builder().orientation(Orientation::Vertical).spacing(6).build();
        let pic = gtk::Picture::for_resource(resource);
        pic.set_size_request(200, 125);
        pic.set_content_fit(gtk::ContentFit::Cover);
        v.append(&pic);
        v.append(&tr_label(retrans, label));
        gtk::ToggleButton::builder().child(&v).build()
    };
    let light_card = make_card(
        retrans,
        || crate::i18n::tr("settings.theme_light").to_string(),
        "/com/icecommander/gtk/ice-commander-gtk-light.jpg",
    );
    let dark_card = make_card(
        retrans,
        || crate::i18n::tr("settings.theme_dark").to_string(),
        "/com/icecommander/gtk/ice-commander-gtk-dark.jpg",
    );
    dark_card.set_group(Some(&light_card));
    if theme_idx == 2 {
        dark_card.set_active(true);
    } else {
        light_card.set_active(true);
    }
    light_card.set_sensitive(!is_auto);
    dark_card.set_sensitive(!is_auto);

    let auto_sw = Switch::builder().valign(Align::Center).active(is_auto).build();
    {
        let apply_theme = apply_theme.clone();
        let light_card = light_card.clone();
        let dark_card = dark_card.clone();
        auto_sw.connect_active_notify(move |sw| {
            let auto = sw.is_active();
            light_card.set_sensitive(!auto);
            dark_card.set_sensitive(!auto);
            if auto {
                apply_theme(0);
            } else {
                apply_theme(if dark_card.is_active() { 2 } else { 1 });
            }
        });
    }
    {
        let apply_theme = apply_theme.clone();
        let auto_sw = auto_sw.clone();
        light_card.connect_toggled(move |b| {
            if b.is_active() && !auto_sw.is_active() {
                apply_theme(1);
            }
        });
    }
    {
        let apply_theme = apply_theme.clone();
        let auto_sw = auto_sw.clone();
        dark_card.connect_toggled(move |b| {
            if b.is_active() && !auto_sw.is_active() {
                apply_theme(2);
            }
        });
    }

    setting_row(
        &list,
        retrans,
        || crate::i18n::tr("settings.theme_label").to_string(),
        || {
            crate::i18n::tr("wizard.theme_auto")
            .to_string()
        },
        &auto_sw,
    );
    let theme_box = Box::builder().orientation(Orientation::Horizontal).spacing(12).build();
    theme_box.append(&light_card);
    theme_box.append(&dark_card);
    setting_row(
        &list,
        retrans,
        || crate::i18n::tr("wizard.manual_theme").to_string(),
        || crate::i18n::tr("wizard.manual_theme_desc").to_string(),
        &theme_box,
    );

    content.append(&list);
    scroll
}

fn build_page_panels(
    config: &client_config::AppConfig,
    retrans: &Retrans,
    on_connections_changed: Rc<dyn Fn() + 'static>,
) -> gtk::ScrolledWindow {
    let (scroll, content) = page_shell(
        retrans,
        || crate::i18n::tr("wizard.panels_title").to_string(),
        || {
            crate::i18n::tr("wizard.panels_sub")
            .to_string()
        },
    );

    let list = ListBox::builder().selection_mode(SelectionMode::None).build();
    list.add_css_class("boxed-list");

    let target_dd = DropDown::from_strings(&[
        &*crate::i18n::tr("settings.open_target_active"),
        &*crate::i18n::tr("settings.open_target_opposite"),
        &*crate::i18n::tr("settings.open_target_left"),
        &*crate::i18n::tr("settings.open_target_right"),
    ]);
    let current_target = config
        .get::<String>("ui.open_connection_target")
        .unwrap_or_else(|| "active".to_string());
    target_dd.set_selected(match current_target.as_str() {
        "opposite" => 1,
        "left" => 2,
        "right" => 3,
        _ => 0,
    });
    let config_target = config.clone();
    let target_handler = target_dd.connect_selected_notify(move |dd| {
        let val = match dd.selected() {
            1 => "opposite",
            2 => "left",
            3 => "right",
            _ => "active",
        };
        config_target.set("ui.open_connection_target", val.to_string());
        config_target.save();
    });
    {
        let dd = target_dd.clone();
        reg(retrans, Rc::new(move || {
            let sel = dd.selected();
            let a = crate::i18n::tr("settings.open_target_active").to_string();
            let b = crate::i18n::tr("settings.open_target_opposite").to_string();
            let c = crate::i18n::tr("settings.open_target_left").to_string();
            let d = crate::i18n::tr("settings.open_target_right").to_string();
            dd.block_signal(&target_handler);
            dd.set_model(Some(&gtk::StringList::new(&[
                a.as_str(),
                b.as_str(),
                c.as_str(),
                d.as_str(),
            ])));
            dd.set_selected(sel);
            dd.unblock_signal(&target_handler);
        }));
    }
    setting_row(
        &list,
        retrans,
        || crate::i18n::tr("settings.open_target_title").to_string(),
        || crate::i18n::tr("settings.open_target_desc").to_string(),
        &target_dd,
    );

    let fav_dd = DropDown::from_strings(&[
        &*crate::i18n::tr("settings.drives_option_all"),
        &*crate::i18n::tr("settings.drives_option_fav"),
    ]);
    fav_dd.set_selected(if crate::favorites::is_favorites_only(config) { 1 } else { 0 });
    let config_fav = config.clone();
    let occ = on_connections_changed.clone();
    let fav_handler = fav_dd.connect_selected_notify(move |dd| {
        crate::favorites::set_favorites_only(&config_fav, dd.selected() == 1);
        occ();
    });
    {
        let dd = fav_dd.clone();
        reg(retrans, Rc::new(move || {
            let sel = dd.selected();
            let a = crate::i18n::tr("settings.drives_option_all").to_string();
            let b = crate::i18n::tr("settings.drives_option_fav").to_string();
            dd.block_signal(&fav_handler);
            dd.set_model(Some(&gtk::StringList::new(&[a.as_str(), b.as_str()])));
            dd.set_selected(sel);
            dd.unblock_signal(&fav_handler);
        }));
    }
    setting_row(
        &list,
        retrans,
        || crate::i18n::tr("settings.drives_toolbar").to_string(),
        || crate::i18n::tr("settings.desc_drives_toolbar").to_string(),
        &fav_dd,
    );

    let thumb_sw = Switch::builder()
        .valign(Align::Center)
        .active(config.get::<bool>("ui.show_thumbnails").unwrap_or(true))
        .build();
    let config_thumb = config.clone();
    let occ2 = on_connections_changed.clone();
    thumb_sw.connect_active_notify(move |sw| {
        config_thumb.set("ui.show_thumbnails", sw.is_active());
        config_thumb.save();
        occ2();
    });
    setting_row(
        &list,
        retrans,
        || crate::i18n::tr("settings.show_thumbnails").to_string(),
        || crate::i18n::tr("settings.desc_show_thumbnails").to_string(),
        &thumb_sw,
    );

    let rowsize_dd = DropDown::from_strings(&[
        &*crate::i18n::tr("settings.list_row_size_normal"),
        &*crate::i18n::tr("settings.list_row_size_compact"),
        &*crate::i18n::tr("settings.list_row_size_tiny"),
    ]);
    let current_rowsize = config
        .get::<String>("ui.fm_list_row_size")
        .unwrap_or_else(|| "normal".to_string());
    rowsize_dd.set_selected(match current_rowsize.as_str() {
        "compact" => 1,
        "tiny" => 2,
        _ => 0,
    });
    let config_rowsize = config.clone();
    let occ3 = on_connections_changed.clone();
    let rowsize_handler = rowsize_dd.connect_selected_notify(move |dd| {
        let val = match dd.selected() {
            1 => "compact",
            2 => "tiny",
            _ => "normal",
        };
        config_rowsize.set("ui.fm_list_row_size", val.to_string());
        config_rowsize.save();
        occ3();
    });
    {
        let dd = rowsize_dd.clone();
        reg(retrans, Rc::new(move || {
            let sel = dd.selected();
            let a = crate::i18n::tr("settings.list_row_size_normal").to_string();
            let b = crate::i18n::tr("settings.list_row_size_compact").to_string();
            let c = crate::i18n::tr("settings.list_row_size_tiny").to_string();
            dd.block_signal(&rowsize_handler);
            dd.set_model(Some(&gtk::StringList::new(&[a.as_str(), b.as_str(), c.as_str()])));
            dd.set_selected(sel);
            dd.unblock_signal(&rowsize_handler);
        }));
    }
    setting_row(
        &list,
        retrans,
        || crate::i18n::tr("settings.list_row_size_title").to_string(),
        || crate::i18n::tr("settings.list_row_size_desc").to_string(),
        &rowsize_dd,
    );

    content.append(&list);
    scroll
}
