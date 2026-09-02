mod fbuttons;
pub(crate) mod header;
mod keyboard;
mod panels;

use adw::prelude::*;
use gtk::glib;
use gtk::{Box, Orientation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ActivePanelSide {
    Left,
    Right,
    None,
}

pub struct MainWindow;

impl MainWindow {
    pub fn create(
        app: &adw::Application,
        my_info: ic_model::DeviceInfo,
        config: client_config::AppConfig,
        api_channel: Option<(
            tokio::sync::mpsc::Sender<crate::api::ApiCmd>,
            tokio::sync::mpsc::Receiver<crate::api::ApiCmd>,
            crate::api::WsSessions,
            tokio::sync::broadcast::Sender<Vec<u8>>,
            tokio::sync::broadcast::Sender<Vec<u8>>,
        )>,
        network_warning: Option<(String, u16)>,
    ) {
        let clipboard = std::rc::Rc::new(fm_core::clipboard::Clipboard::new());
        let selector_updaters =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::<std::rc::Rc<dyn Fn()>>::new()));
        let shift_held = std::rc::Rc::new(std::cell::Cell::new(false));
        let btn_f4_ref: std::rc::Rc<std::cell::RefCell<Option<gtk::Button>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let btn_f7_ref: std::rc::Rc<std::cell::RefCell<Option<gtk::Button>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let global_on_connect = std::rc::Rc::new(std::cell::RefCell::new(
            None::<std::rc::Rc<dyn Fn(crate::connection_manager::FtpConnection) + 'static>>,
        ));

        let theme_index = config.get::<u32>("ui.theme_index").unwrap_or(0);
        let sm = adw::StyleManager::default();
        match theme_index {
            1 => sm.set_color_scheme(adw::ColorScheme::ForceLight),
            2 => sm.set_color_scheme(adw::ColorScheme::ForceDark),
            _ => sm.set_color_scheme(adw::ColorScheme::Default),
        }

        let width = config.get::<i32>("ui.window_width").unwrap_or(1200);
        let height = config.get::<i32>("ui.window_height").unwrap_or(800);
        let is_max = config.get::<bool>("ui.window_maximized").unwrap_or(false);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(&*crate::i18n::tr("main_window_title"))
            .default_width(width)
            .default_height(height)
            .icon_name("com.icecommander.gtkapp")
            .build();

        if is_max {
            window.maximize();
        }

        let root_vbox = Box::builder().orientation(Orientation::Vertical).build();
        window.set_content(Some(&root_vbox));

        let header::HeaderResult {
            bar,
            settings_btn,
            on_open_sysinfo,
        } = header::build_header_bar(
            &window,
            &config,
            selector_updaters.clone(),
            global_on_connect.clone(),
        );
        root_vbox.append(&bar);

        let (term_out_left, term_out_right) = match &api_channel {
            Some((_, _, _, l, r)) => (l.clone(), r.clone()),
            None => (
                tokio::sync::broadcast::channel::<Vec<u8>>(1024).0,
                tokio::sync::broadcast::channel::<Vec<u8>>(1024).0,
            ),
        };

        let panels::PanelResult {
            paned,
            left_panel,
            right_panel: _right_panel,
            left_router,
            right_router,
            active_panel,
            left_term,
            right_term,
            left_info,
            right_info,
            on_expand,
            on_collapse,
            expanded_side,
            expanded_notifier,
            ..
        } = panels::build_panels(
            &window,
            &config,
            &my_info,
            clipboard.clone(),
            shift_held.clone(),
            on_open_sysinfo.clone(),
            selector_updaters.clone(),
            width,
            term_out_left,
            term_out_right,
        );
        {
            let clip = clipboard.clone();
            let li = left_info.clone();
            let ri = right_info.clone();
            clipboard.connect_changed(std::rc::Rc::new(move |n, side, cut| {
                for r in li.all_routers().iter().chain(ri.all_routers().iter()) {
                    r.set_clipboard_state(n, r.panel_id() == side, cut);
                }
            }));
            let _ = &clip;
        }

        root_vbox.append(&paned);

        if let Some((_tx, rx, _, _, _)) = api_channel {
            {
                *left_term.on_visibility_changed.borrow_mut() =
                    Some(std::rc::Rc::new(move |open| {
                        if open {
                            crate::api::notify_terminal_opened("left");
                        } else {
                            crate::api::notify_terminal_closed("left");
                        }
                    }));
            }
            {
                *right_term.on_visibility_changed.borrow_mut() =
                    Some(std::rc::Rc::new(move |open| {
                        if open {
                            crate::api::notify_terminal_opened("right");
                        } else {
                            crate::api::notify_terminal_closed("right");
                        }
                    }));
            }
            {
                *expanded_notifier.borrow_mut() = Some(std::rc::Rc::new(move |s: Option<ActivePanelSide>| {
                    match s {
                        Some(ActivePanelSide::Left) => crate::api::notify_terminal_expanded("left", true),
                        Some(ActivePanelSide::Right) => crate::api::notify_terminal_expanded("right", true),
                        _ => crate::api::notify_terminal_expanded("", false),
                    }
                }));
            }
            let left_term_bridge = crate::api::TerminalBridge {
                open: left_info.open_terminal.clone(),
                input_tx: left_term.pty_input_tx.clone(),
            };
            let right_term_bridge = crate::api::TerminalBridge {
                open: right_info.open_terminal.clone(),
                input_tx: right_term.pty_input_tx.clone(),
            };
            let on_expand_c = on_expand.clone();
            let on_collapse_c = on_collapse.clone();
            let term_expand: std::rc::Rc<dyn Fn(crate::api::PanelSide, bool)> =
                std::rc::Rc::new(move |side, expanded| {
                    if expanded {
                        let aside = match side {
                            crate::api::PanelSide::Left => ActivePanelSide::Left,
                            crate::api::PanelSide::Right => ActivePanelSide::Right,
                        };
                        on_expand_c(aside);
                    } else {
                        on_collapse_c();
                    }
                });
            crate::api::start_api_dispatcher(rx, left_info.clone(), right_info.clone(), config.clone(), selector_updaters.clone(), left_term_bridge, right_term_bridge, term_expand);
        }

        {
            let active_panel_conn = active_panel.clone();
            let left_info_conn = left_info.clone();
            let right_info_conn = right_info.clone();
            let config_conn = config.clone();

            *global_on_connect.borrow_mut() =
                Some(std::rc::Rc::new(move |conn| {
                    let left_router_conn = left_info_conn.active_router();
                    let right_router_conn = right_info_conn.active_router();

                    let open_target = config_conn
                        .get::<String>("ui.open_connection_target")
                        .unwrap_or_else(|| "active".to_string());

                    let target_side = match open_target.as_str() {
                        "left" => ActivePanelSide::Left,
                        "right" => ActivePanelSide::Right,
                        "opposite" => match active_panel_conn.get() {
                            ActivePanelSide::Left => ActivePanelSide::Right,
                            ActivePanelSide::Right => ActivePanelSide::Left,
                            ActivePanelSide::None => ActivePanelSide::Left,
                        },
                        _ => match active_panel_conn.get() {
                            ActivePanelSide::Left => ActivePanelSide::Left,
                            ActivePanelSide::Right => ActivePanelSide::Right,
                            ActivePanelSide::None => ActivePanelSide::Left,
                        },
                    };

                    let target_router = match target_side {
                        ActivePanelSide::Left => &left_router_conn,
                        ActivePanelSide::Right => &right_router_conn,
                        ActivePanelSide::None => &left_router_conn,
                    };

                    crate::source_selector::connect_to_connection(conn, target_router);
                }));
        }

        {
            let selector_updaters_s = selector_updaters.clone();
            let window_s = window.clone();
            let left_router_s = left_router.clone();
            let right_router_s = right_router.clone();
            let left_info_s = left_info.clone();
            let right_info_s = right_info.clone();
            let config_s = config.clone();

            settings_btn.connect_clicked(move |_| {
                let updaters = selector_updaters_s.clone();
                let left_router_inner = left_router_s.clone();
                let right_router_inner = right_router_s.clone();
                let left_info_rs = left_info_s.clone();
                let right_info_rs = right_info_s.clone();
                let on_connections_changed = std::rc::Rc::new(move || {
                    for updater in updaters.borrow().iter() {
                        updater();
                    }
                    left_router_inner.refresh_spawned();
                    right_router_inner.refresh_spawned();
                    for r in left_info_rs.all_routers().iter().chain(right_info_rs.all_routers().iter()) {
                        r.re_render();
                    }
                });
                crate::settings::show_settings_dialog(
                    window_s.upcast_ref(),
                    config_s.clone(),
                    on_connections_changed,
                );
            });
        }

        {
            let window_c = window.clone();
            let left_info_c = left_info.clone();
            let right_info_c = right_info.clone();
            *left_info.drop_hook.borrow_mut() = Some(std::rc::Rc::new(move |paths| {
                crate::file_operations::trigger_external_drop(
                    &window_c,
                    paths,
                    &left_info_c.active_router(),
                    Some(right_info_c.active_router()),
                );
            }));
        }
        {
            let window_c = window.clone();
            let left_info_c = left_info.clone();
            let right_info_c = right_info.clone();
            *right_info.drop_hook.borrow_mut() = Some(std::rc::Rc::new(move |paths| {
                crate::file_operations::trigger_external_drop(
                    &window_c,
                    paths,
                    &right_info_c.active_router(),
                    Some(left_info_c.active_router()),
                );
            }));
        }

        keyboard::setup_keyboard(
            &window,
            app,
            &config,
            &left_info,
            &right_info,
            &left_panel,
            active_panel.clone(),
            selector_updaters.clone(),
            shift_held.clone(),
            global_on_connect.clone(),
            btn_f4_ref.clone(),
            btn_f7_ref.clone(),
            on_expand.clone(),
            on_collapse.clone(),
            expanded_side.clone(),
            clipboard.clone(),
        );

        let bottom_box = fbuttons::build_fbuttons(
            &window,
            app,
            &config,
            &left_info,
            &right_info,
            active_panel.clone(),
            selector_updaters.clone(),
            global_on_connect.clone(),
            btn_f4_ref.clone(),
            btn_f7_ref.clone(),
        );

        {
            let btn_f4_ref_active = btn_f4_ref.clone();
            let btn_f7_ref_active = btn_f7_ref.clone();
            window.connect_is_active_notify(move |win| {
                if !win.is_active() {
                    if let Some(ref btn) = *btn_f4_ref_active.borrow() {
                        btn.set_label("F4 Edit");
                    }
                    if let Some(ref btn) = *btn_f7_ref_active.borrow() {
                        btn.set_label("F7 New Folder");
                    }
                }
            });
        }

        root_vbox.append(&bottom_box);

        let css_provider = gtk::CssProvider::new();
        css_provider.load_from_string(
            "
            .panel-header {
                padding: 4px;
                border-bottom: 1px solid alpha(currentColor, 0.15);
                background: alpha(currentColor, 0.05);
            }
            .f-button { font-weight: bold; }
            .panel-container {
                margin: 4px;
            }
            .panel-container listview row:selected,
            .panel-container gridview child:selected {
                background-color: alpha(@accent_bg_color, 0.15);
                color: @theme_fg_color;
            }
            .panel-container:focus-within listview row:selected,
            .panel-container:focus-within gridview child:selected {
                background-color: @accent_bg_color;
                color: @accent_fg_color;
            }
            /* Tab strip: each tab is one cohesive pill = drive name + ▾ + ×. */
            .ic-tab {
                border-radius: 7px;
                padding: 2px 4px 2px 10px;
                background-color: alpha(currentColor, 0.06);
            }
            .ic-tab:hover {
                background-color: alpha(currentColor, 0.10);
            }
            .ic-tab.active-tab {
                background-color: alpha(@accent_bg_color, 0.20);
                box-shadow: inset 0 -2px 0 0 @accent_bg_color;
            }
            /* Flatten the dropdown + buttons INSIDE a tab so nothing looks like a separate
               raised control — the whole tab reads as one unit. */
            .ic-tab dropdown > button,
            .ic-tab button {
                background: none;
                background-image: none;
                border: none;
                box-shadow: none;
                min-height: 0;
                min-width: 0;
                padding: 2px 4px;
            }
            .ic-tab dropdown > button:hover,
            .ic-tab button:hover {
                background-color: alpha(currentColor, 0.12);
            }
            /* Let list rows shrink to their content — the row size is driven by the
               cell margins (fm-ui list_row_metrics), not the theme's default padding. */
            .panel-container columnview row,
            .panel-container listview row {
                min-height: 0;
            }
            .panel-container columnview cell {
                padding-top: 0;
                padding-bottom: 0;
                min-height: 0;
            }
            .status-bar {
                padding: 4px 8px;
                border-top: 1px solid alpha(currentColor, 0.15);
                background: alpha(currentColor, 0.03);
                font-size: 0.9em;
            }
            .conn-row-selected {
                background-color: alpha(@accent_bg_color, 0.15);
            }
            .star-active {
                color: #eab308;
            }
            .star-inactive {
                color: alpha(currentColor, 0.3);
            }
            .clip-badge {
                background: @accent_bg_color;
                color: @accent_fg_color;
                border-radius: 999px;
                font-size: 9px;
                font-weight: bold;
                padding: 0 4px;
                margin: 2px;
                min-width: 12px;
            }
            .destructive-hover-action {
                transition: background-color 0.15s ease, color 0.15s ease;
            }
            .destructive-hover-action:hover:not(:disabled) {
                background-color: #ffd8d8;
                color: #ffffff;
            }
            .destructive-hover-action:hover:not(:disabled) path {
                fill: #ffffff;
            }
            .help-title {
                color: @accent_color;
            }
            .registry-list-box row {
                padding: 5px 6px;
                border-bottom: 1px solid alpha(currentColor, 0.08);
            }
            .registry-list-box row:last-child {
                border-bottom: none;
            }
            /* Address bar: nav buttons and breadcrumbs each in a rounded pill.
               alpha(currentColor) auto-adapts to light/dark like the rest of the app CSS. */
            .nav-pill,
            .address-pill {
                background: alpha(currentColor, 0.08);
                border: 1px solid alpha(currentColor, 0.22);
                border-radius: 6px;
                min-height: 0;
                padding: 2px 2px;
                margin 4px 0px 8px 0px;
            }
            .nav-pill button,
            .address-pill button {
                min-height: 0;
                min-width: 0;
                padding: 5px;
                border-radius: 4px;
                background: transparent;
                box-shadow: none;
                border: none;
            }
            .nav-pill button:hover:not(:disabled),
            .address-pill button:hover {
                background: alpha(currentColor, 0.12);
            }
            /* Match label text height to the 16px icons so the breadcrumbs pill
               is no taller than the nav pill. */
            .address-pill label {
                font-size: 12px;
            }
        ",
        );
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().expect("Could not connect to a display."),
            &css_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        {
            let left_router_close = left_router.clone();
            let right_router_close = right_router.clone();
            let config_close = config.clone();

            window.connect_close_request(move |win| {
                let (w, h) = win.default_size();
                let is_max = win.is_maximized();
                config_close.set("ui.window_width", w);
                config_close.set("ui.window_height", h);
                config_close.set("ui.window_maximized", is_max);

                let process_panel_path = |router: &panel_router::PanelRouter| -> String {
                    let resource_id = router.current_resource_id();
                    if resource_id != "local_fs" && resource_id != "root_fs" {
                        return String::new();
                    }
                    let p = router.state.path.borrow();
                    p.levels()
                        .iter()
                        .rev()
                        .find(|l| l.fs.is_local())
                        .map(|l| l.relative_path.clone())
                        .unwrap_or_default()
                };

                if !left_router_close.is_showing_selector() {
                    config_close.set("ui.left_panel_path", process_panel_path(&left_router_close));
                } else {
                    config_close.set("ui.left_panel_path", "");
                }

                if !right_router_close.is_showing_selector() {
                    config_close.set("ui.right_panel_path", process_panel_path(&right_router_close));
                } else {
                    config_close.set("ui.right_panel_path", "");
                }

                config_close.save();

                glib::Propagation::Proceed
            });
        }

        let wiz_config = config.clone();
        let wiz_left_router = left_router.clone();
        let wiz_right_router = right_router.clone();
        let wiz_updaters = selector_updaters.clone();

        window.present();

        if crate::secret_store::protection() == crate::secret_store::Protection::Password
            && wiz_config
                .get::<bool>("ui.require_startup_password")
                .unwrap_or(false)
        {
            let unlock_parent = window.clone().upcast::<gtk::Window>();
            crate::master_password::prompt_unlock(
                Some(&unlock_parent),
                std::rc::Rc::new(|unlocked| {
                    if !unlocked {
                        ic_logging::warn!("master password not entered: secrets stay locked");
                    }
                }),
            );
        }

        if crate::wizard::should_show(&wiz_config) {
            let updaters = wiz_updaters.clone();
            let lfm = wiz_left_router.clone();
            let rfm = wiz_right_router.clone();
            let on_changed = std::rc::Rc::new(move || {
                for updater in updaters.borrow().iter() {
                    updater();
                }
                lfm.refresh_spawned();
                rfm.refresh_spawned();
            });
            crate::wizard::show_setup_wizard(
                window.upcast_ref(),
                wiz_config.clone(),
                on_changed,
            );
        }

        if let Some((host, port)) = network_warning {
            let warning_text = crate::i18n::tr("headless.network_warning");
            let addr_label = crate::i18n::tr("headless.network_warning_address");
            let dialog = adw::AlertDialog::builder()
                .heading(crate::i18n::tr("headless.network_warning_title").as_str())
                .body(&format!("{warning_text}\n\n{addr_label}:  {host}:{port}"))
                .body_use_markup(false)
                .build();
            dialog.add_response("ok", "OK");
            dialog.set_default_response(Some("ok"));
            dialog.present(Some(&window));
        }

        if !std::env::args().any(|a| a == "--no-check-update") {
            crate::updater::auto_check(window.upcast::<gtk::Window>());
        }
    }
}
