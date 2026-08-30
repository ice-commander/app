use adw::prelude::*;
use gtk::{Button, Label, Orientation};

pub(super) struct HeaderResult {
    pub bar: adw::HeaderBar,
    pub settings_btn: Button,
    pub on_open_sysinfo: std::rc::Rc<dyn Fn()>,
}

pub(super) fn build_header_bar(
    window: &adw::ApplicationWindow,
    config: &client_config::AppConfig,
    selector_updaters: std::rc::Rc<std::cell::RefCell<Vec<std::rc::Rc<dyn Fn()>>>>,
    global_on_connect: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn(crate::connection_manager::FtpConnection) + 'static>>>>,
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

    let sysinfo_img = gtk::Image::from_resource("/com/icecommander/gtk/sysinfo.svg");
    sysinfo_img.set_pixel_size(20);
    let sysinfo_btn = Button::builder()
        .child(&sysinfo_img)
        .tooltip_text("System Information")
        .build();
    sysinfo_btn.set_cursor_from_name(Some("pointer"));

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

    let window_sysinfo = window.clone();
    let on_open_sysinfo: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
        use gtk_sysinfo_ui::{SysInfoInit, SysInfoInput, SysInfoModel, SysInfoOutput};
        use relm4::prelude::*;

        let sysinfo_dialog = gtk::Window::builder()
            .title("System Information")
            .transient_for(&window_sysinfo)
            .modal(true)
            .default_width(700)
            .default_height(600)
            .resizable(false)
            .build();

        let (out_tx, out_rx) = relm4::channel::<SysInfoOutput>();
        let sysinfo = SysInfoModel::builder()
            .launch(SysInfoInit {
                show_toolbar: false,
                auto_update: true,
                update_interval: std::time::Duration::from_secs(1),
            })
            .forward(&out_tx, |o| o);
        sysinfo_dialog.set_child(Some(sysinfo.widget()));

        let _ = sysinfo
            .sender()
            .send(SysInfoInput::Update(Box::new(
                ic_platform::system_info::get_system_info(),
            )));

        let sysinfo = std::rc::Rc::new(sysinfo);
        {
            let sysinfo = sysinfo.clone();
            let dialog = sysinfo_dialog.clone();
            gtk::glib::spawn_future_local(async move {
                while let Some(out) = out_rx.recv().await {
                    match out {
                        SysInfoOutput::RequestInfo => {
                            let _ = sysinfo.sender().send(SysInfoInput::Update(Box::new(
                                ic_platform::system_info::get_system_info(),
                            )));
                        }
                        SysInfoOutput::Back => dialog.close(),
                    }
                }
            });
        }

        let sysinfo_keepalive = sysinfo.clone();
        sysinfo_dialog.connect_close_request(move |_| {
            let _ = &sysinfo_keepalive;
            gtk::glib::Propagation::Proceed
        });

        sysinfo_dialog.present();
    });

    let on_open_sysinfo_btn = on_open_sysinfo.clone();
    sysinfo_btn.connect_clicked(move |_| {
        on_open_sysinfo_btn();
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
        .tooltip_text("FTP/SFTP Connections")
        .build();
    conn_btn.set_cursor_from_name(Some("pointer"));

    let conn_btn_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    let conn_btn_icon = gtk::Image::from_resource("/com/icecommander/gtk/ftp.svg");
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
            let count = config_badge
                .get::<Vec<serde_json::Value>>("ui.ftp_connections")
                .map(|v| v.len())
                .unwrap_or(0);
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
            let _ = std::process::Command::new("cmd").args(["/c", "start", "", &url]).spawn();
        });
        Some(btn)
    } else {
        None
    };

    #[cfg(not(target_os = "linux"))]
    header_bar.pack_start(&logo_img);
    header_bar.pack_start(&settings_btn);
    header_bar.pack_start(&sysinfo_btn);
    header_bar.pack_end(&theme_btn);
    header_bar.pack_end(&conn_btn);
    if let Some(ref web_btn) = web_btn {
        header_bar.pack_end(web_btn);
    }

    HeaderResult {
        bar: header_bar,
        settings_btn,
        on_open_sysinfo,
    }
}
