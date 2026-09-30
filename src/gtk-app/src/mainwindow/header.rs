use adw::prelude::*;
use gtk::{Button, Label, Orientation};

thread_local! {
    static PLUGIN_LABELS: std::cell::RefCell<std::collections::HashMap<String, Label>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    static PLUGIN_BUTTONS: std::cell::RefCell<std::collections::HashMap<String, Button>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    static PLUGIN_ICONS: std::cell::RefCell<std::collections::HashMap<String, gtk::Image>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub(super) struct HeaderResult {
    pub bar: adw::HeaderBar,
    pub settings_btn: Button,
}

pub(super) fn build_header_bar(
    window: &adw::ApplicationWindow,
    config: &client_config::AppConfig,
    selector_updaters: std::rc::Rc<std::cell::RefCell<Vec<std::rc::Rc<dyn Fn()>>>>,
    global_on_connect: std::rc::Rc<
        std::cell::RefCell<
            Option<std::rc::Rc<dyn Fn(crate::connection_manager::Connection) + 'static>>,
        >,
    >,
) -> HeaderResult {
    let header_bar = adw::HeaderBar::new();

    #[cfg(not(target_os = "linux"))]
    let logo_img = {
        let img = gtk::Image::from_resource("/com/icecommander/gtk/app-logo.svg");
        img.set_pixel_size(24);
        img.set_margin_start(8);
        img.set_margin_end(16);
        img
    };

    let settings_img = gtk::Image::from_resource("/com/icecommander/gtk/setting.svg");
    settings_img.set_pixel_size(20);
    let settings_btn = Button::builder()
        .child(&settings_img)
        .tooltip_text("Settings")
        .build();
    settings_btn.set_cursor_from_name(Some("pointer"));

    let theme_btn_img = gtk::Image::new();
    theme_btn_img.set_pixel_size(20);

    let theme_btn = Button::builder()
        .child(&theme_btn_img)
        .tooltip_text("Toggle Theme")
        .build();
    theme_btn.set_cursor_from_name(Some("pointer"));

    let theme_btn_img_notify = theme_btn_img.clone();
    adw::StyleManager::default().connect_dark_notify(move |sm| {
        if sm.is_dark() {
            theme_btn_img_notify.set_resource(Some("/com/icecommander/gtk/afternoon.svg"));
        } else {
            theme_btn_img_notify.set_resource(Some("/com/icecommander/gtk/night.svg"));
        }
    });

    let config_theme = config.clone();
    theme_btn.connect_clicked(move |_| {
        let style_mgr = adw::StyleManager::default();
        if style_mgr.is_dark() {
            style_mgr.set_color_scheme(adw::ColorScheme::ForceLight);
            config_theme.set("ui.theme_index", 1u32);
        } else {
            style_mgr.set_color_scheme(adw::ColorScheme::ForceDark);
            config_theme.set("ui.theme_index", 2u32);
        }
        config_theme.save();
    });

    if adw::StyleManager::default().is_dark() {
        theme_btn_img.set_resource(Some("/com/icecommander/gtk/afternoon.svg"));
    } else {
        theme_btn_img.set_resource(Some("/com/icecommander/gtk/night.svg"));
    }

    let conn_btn = Button::builder()
        .tooltip_text(&*crate::i18n::tr("conn_manager.title"))
        .build();
    conn_btn.set_cursor_from_name(Some("pointer"));

    let conn_btn_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    let conn_btn_icon = gtk::Image::from_resource("/com/icecommander/gtk/connect.svg");
    conn_btn_icon.set_pixel_size(20);
    let conn_btn_label = Label::new(Some("Connections"));
    let conn_badge = Label::builder().css_classes(vec!["dim-label"]).build();

    conn_btn_box.append(&conn_btn_icon);
    conn_btn_box.append(&conn_btn_label);
    conn_btn_box.append(&conn_badge);
    conn_btn.set_child(Some(&conn_btn_box));

    let window_conn = window.clone();
    let selector_updaters_conn = selector_updaters.clone();
    let config_conn = config.clone();
    let global_on_connect_conn = global_on_connect.clone();
    conn_btn.connect_clicked(move |_| {
        let updaters = selector_updaters_conn.clone();
        let on_change = std::rc::Rc::new(move || {
            for updater in updaters.borrow().iter() {
                updater();
            }
        });
        let on_connect_cb = global_on_connect_conn.borrow().clone();
        crate::connection_manager::show_manage_ftp_dialog(
            &window_conn,
            on_change,
            config_conn.clone(),
            on_connect_cb,
        );
    });

    let config_badge = config.clone();
    let update_conn_badge = {
        let conn_badge = conn_badge.clone();
        move || {
            let count = connection_form::stored_connections(&config_badge).len();
            conn_badge.set_text(&format!("({})", count));
        }
    };

    update_conn_badge();
    selector_updaters
        .borrow_mut()
        .push(std::rc::Rc::new(update_conn_badge));

    let web_args: Vec<String> = std::env::args().collect();
    let web_btn: Option<Button> = if web_args.iter().any(|a| a == "--webui") {
        let port: u16 = web_args
            .iter()
            .skip_while(|a| *a != "--port")
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(7878);
        let label_text = crate::i18n::tr("header.web_access").to_string();
        let btn = Button::builder().tooltip_text(&label_text).build();
        btn.set_cursor_from_name(Some("pointer"));
        let btn_box = gtk::Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(6)
            .build();
        let icon = gtk::Image::from_resource("/com/icecommander/gtk/cellular-network.svg");
        icon.set_pixel_size(20);
        btn_box.append(&icon);
        btn_box.append(&Label::new(Some(&label_text)));
        btn.set_child(Some(&btn_box));
        btn.connect_clicked(move |_| {
            let url = format!("http://localhost:{port}/");
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&url).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", "", &url])
                .spawn();
        });
        Some(btn)
    } else {
        None
    };

    let plugin_header_buttons = |side: u32| -> Vec<Button> {
        crate::plugin_host::header_entries(side)
            .into_iter()
            .filter_map(|entry| {
                let content = gtk::Box::builder()
                    .orientation(Orientation::Horizontal)
                    .spacing(6)
                    .build();
                // Icon, label and visibility are kept: a plugin repaints them later.
                let icon =
                    crate::plugin_host::image_from_svg(&entry.svg).unwrap_or_else(gtk::Image::new);
                icon.set_pixel_size(20);
                content.append(&icon);
                PLUGIN_ICONS.with(|held| held.borrow_mut().insert(entry.id.clone(), icon));
                let label = Label::new(Some(&entry.label));
                content.append(&label);
                label.set_visible(!entry.label.is_empty());
                PLUGIN_LABELS.with(|held| held.borrow_mut().insert(entry.id.clone(), label));
                let btn = Button::builder().child(&content).build();
                btn.set_visible(entry.shown);
                PLUGIN_BUTTONS.with(|held| held.borrow_mut().insert(entry.id.clone(), btn.clone()));
                if !entry.tooltip.is_empty() {
                    let shown = crate::connection_manager::translate_optional(&entry.tooltip)
                        .unwrap_or_else(|| entry.tooltip.clone());
                    btn.set_tooltip_text(Some(&shown));
                }
                btn.set_cursor_from_name(Some("pointer"));
                btn.connect_clicked(move |b| {
                    let Some(window) = b.root().and_downcast::<gtk::Window>() else {
                        return;
                    };
                    use gtk::glib::object::ObjectType;
                    entry.fire(window.as_ptr() as *mut std::os::raw::c_void);
                });
                Some(btn)
            })
            .collect()
    };

    #[cfg(not(target_os = "linux"))]
    header_bar.pack_start(&logo_img);
    header_bar.pack_start(&settings_btn);
    for btn in plugin_header_buttons(ic_plugin_api::IC_SIDE_LEFT) {
        header_bar.pack_start(&btn);
    }
    header_bar.pack_end(&theme_btn);
    header_bar.pack_end(&conn_btn);
    if let Some(ref web_btn) = web_btn {
        header_bar.pack_end(web_btn);
    }
    for btn in plugin_header_buttons(ic_plugin_api::IC_SIDE_RIGHT) {
        header_bar.pack_end(&btn);
    }

    ic_plugin_host::set_header_changed_handler(std::rc::Rc::new(|id: &str, text: &str| {
        PLUGIN_LABELS.with(|held| {
            if let Some(label) = held.borrow().get(id) {
                label.set_text(text);
                label.set_visible(!text.is_empty());
            }
        });
    }));

    ic_plugin_host::set_header_repainted_handler(std::rc::Rc::new(|id: &str, svg: &[u8]| {
        PLUGIN_ICONS.with(|held| {
            let Some(shown) = held.borrow().get(id).cloned() else {
                return;
            };
            if let Some(drawn) = crate::plugin_host::texture_from_svg(svg, 20) {
                shown.set_paintable(Some(&drawn));
                shown.set_pixel_size(20);
            }
        });
    }));

    ic_plugin_host::set_header_shown_handler(std::rc::Rc::new(|id: &str, shown: bool| {
        PLUGIN_BUTTONS.with(|held| {
            if let Some(btn) = held.borrow().get(id) {
                btn.set_visible(shown);
            }
        });
    }));

    HeaderResult {
        bar: header_bar,
        settings_btn,
    }
}
