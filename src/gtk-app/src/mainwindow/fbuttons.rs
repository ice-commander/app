use gtk::prelude::*;
use gtk::{Button, Orientation};

use crate::file_operations::{trigger_copy, trigger_move};

pub(super) fn build_fbuttons(
    window: &adw::ApplicationWindow,
    app: &adw::Application,
    config: &client_config::AppConfig,
    left_info: &crate::panel_builder::PanelInfo,
    right_info: &crate::panel_builder::PanelInfo,
    active_panel: std::rc::Rc<std::cell::Cell<super::ActivePanelSide>>,
    selector_updaters: std::rc::Rc<std::cell::RefCell<Vec<std::rc::Rc<dyn Fn()>>>>,
    global_on_connect: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn(crate::connection_manager::FtpConnection) + 'static>>>>,
    btn_f4_ref: std::rc::Rc<std::cell::RefCell<Option<Button>>>,
    btn_f7_ref: std::rc::Rc<std::cell::RefCell<Option<Button>>>,
) -> gtk::Box {
    let bottom_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .margin_start(4)
        .margin_end(4)
        .margin_bottom(4)
        .homogeneous(true)
        .build();

    let f_buttons = vec![
        (crate::i18n::tr("f_buttons.f1").to_string(), "help"),
        (crate::i18n::tr("f_buttons.f2").to_string(), "rename"),
        (crate::i18n::tr("f_buttons.f3").to_string(), "view"),
        (crate::i18n::tr("f_buttons.f4").to_string(), "edit"),
        (crate::i18n::tr("f_buttons.f5").to_string(), "copy"),
        (crate::i18n::tr("f_buttons.f6").to_string(), "move"),
        (crate::i18n::tr("f_buttons.f7").to_string(), "newfolder"),
        (crate::i18n::tr("f_buttons.f8").to_string(), "delete"),
        (crate::i18n::tr("f_buttons.f9").to_string(), "terminal"),
        (crate::i18n::tr("f_buttons.f10").to_string(), "quit"),
    ];

    for (label, action) in f_buttons {
        let btn = Button::builder().label(label).build();

        match action {
            "quit" => {
                let app_clone = app.clone();
                btn.connect_clicked(move |_| {
                    app_clone.quit();
                });
            }
            "help" => {
                let window_clone = window.clone();
                let config_help = config.clone();
                btn.connect_clicked(move |_| {
                    crate::help::show_help_dialog(&window_clone, &config_help);
                });
            }
            "view" => {
                let left_info_clone = left_info.clone();
                let right_info_clone = right_info.clone();
                let window_clone = window.clone();
                let active_panel_btn = active_panel.clone();
                btn.connect_clicked(move |_| {
                    let left_fm_clone = left_info_clone.active_router();
                    let right_fm_clone = right_info_clone.active_router();
                    let (active_fm, side) = match active_panel_btn.get() {
                        super::ActivePanelSide::Left => (Some(&left_fm_clone), "left"),
                        super::ActivePanelSide::Right => (Some(&right_fm_clone), "right"),
                        super::ActivePanelSide::None => (None, ""),
                    };
                    if let Some(active_fm) = active_fm {
                        if let Some(entry) = active_fm.selected_entries().into_iter().next() {
                            let path = entry.path();
                            crate::viewer::show_viewer(&window_clone, entry, active_fm.clone());
                            crate::api::notify_viewer_opened(side, &path, "view");
                        }
                    }
                });
            }
            "edit" => {
                let left_info_clone = left_info.clone();
                let right_info_clone = right_info.clone();
                let window_clone = window.clone();
                let btn_clone = btn.clone();
                let active_panel_btn = active_panel.clone();
                btn.connect_clicked(move |_| {
                    let left_fm_clone = left_info_clone.active_router();
                    let right_fm_clone = right_info_clone.active_router();
                    let (active_fm, side) = match active_panel_btn.get() {
                        super::ActivePanelSide::Left => (Some(&left_fm_clone), "left"),
                        super::ActivePanelSide::Right => (Some(&right_fm_clone), "right"),
                        super::ActivePanelSide::None => (None, ""),
                    };
                    if let Some(active_fm) = active_fm {
                        if btn_clone.label().as_ref().map(|s| s.as_str())
                            == Some(crate::i18n::tr("f_buttons.f4_new").as_str())
                        {
                            crate::editor::show_new_file_window(&window_clone, Some(active_fm.clone()));
                        } else if let Some(entry) = active_fm.selected_entries().into_iter().next() {
                            let path = entry.path();
                            crate::editor::show_editor(&window_clone, entry, active_fm.clone());
                            crate::api::notify_viewer_opened(side, &path, "edit");
                        }
                    }
                });
                *btn_f4_ref.borrow_mut() = Some(btn.clone());
            }
            "rename" => {
                let left_info_clone = left_info.clone();
                let right_info_clone = right_info.clone();
                let active_panel_btn = active_panel.clone();
                btn.connect_clicked(move |_| {
                    let left_fm_clone = left_info_clone.active_router();
                    let right_fm_clone = right_info_clone.active_router();
                    let active_fm = match active_panel_btn.get() {
                        super::ActivePanelSide::Left => Some(&left_fm_clone),
                        super::ActivePanelSide::Right => Some(&right_fm_clone),
                        super::ActivePanelSide::None => None,
                    };
                    if let Some(active_fm) = active_fm {
                        active_fm.start_rename();
                    }
                });
            }
            "terminal" => {
                let left_info_clone = left_info.clone();
                let right_info_clone = right_info.clone();
                let active_panel_btn = active_panel.clone();
                btn.connect_clicked(move |_| {
                    let toggle = match active_panel_btn.get() {
                        super::ActivePanelSide::Left => Some(left_info_clone.active_toggle_terminal()),
                        super::ActivePanelSide::Right => Some(right_info_clone.active_toggle_terminal()),
                        super::ActivePanelSide::None => None,
                    };
                    if let Some(toggle) = toggle {
                        toggle();
                    }
                });
            }
            "copy" => {
                let left_info_clone = left_info.clone();
                let right_info_clone = right_info.clone();
                let window_clone = window.clone();
                let active_panel_btn = active_panel.clone();
                btn.connect_clicked(move |_| {
                    let left_fm_clone = left_info_clone.active_router();
                    let right_fm_clone = right_info_clone.active_router();
                    let active_pair = match active_panel_btn.get() {
                        super::ActivePanelSide::Left => Some((&left_fm_clone, &right_fm_clone)),
                        super::ActivePanelSide::Right => Some((&right_fm_clone, &left_fm_clone)),
                        super::ActivePanelSide::None => None,
                    };
                    if let Some((active_fm, inactive_fm)) = active_pair {
                        trigger_copy(&window_clone, active_fm, inactive_fm);
                    }
                });
            }
            "move" => {
                let left_info_clone = left_info.clone();
                let right_info_clone = right_info.clone();
                let window_clone = window.clone();
                let active_panel_btn = active_panel.clone();
                btn.connect_clicked(move |_| {
                    let left_fm_clone = left_info_clone.active_router();
                    let right_fm_clone = right_info_clone.active_router();
                    let active_pair = match active_panel_btn.get() {
                        super::ActivePanelSide::Left => Some((&left_fm_clone, &right_fm_clone)),
                        super::ActivePanelSide::Right => Some((&right_fm_clone, &left_fm_clone)),
                        super::ActivePanelSide::None => None,
                    };
                    if let Some((active_fm, inactive_fm)) = active_pair {
                        trigger_move(&window_clone, active_fm, inactive_fm);
                    }
                });
            }
            "newfolder" => {
                let left_info_clone = left_info.clone();
                let right_info_clone = right_info.clone();
                let window_clone = window.clone();
                let btn_clone = btn.clone();
                let active_panel_btn = active_panel.clone();
                btn.connect_clicked(move |_| {
                    let left_fm_clone = left_info_clone.active_router();
                    let right_fm_clone = right_info_clone.active_router();
                    let active_fm = match active_panel_btn.get() {
                        super::ActivePanelSide::Left => Some(&left_fm_clone),
                        super::ActivePanelSide::Right => Some(&right_fm_clone),
                        super::ActivePanelSide::None => None,
                    };
                    if let Some(active_fm) = active_fm {
                        if btn_clone.label().as_ref().map(|s| s.as_str())
                            == Some(crate::i18n::tr("f_buttons.f7_find").as_str())
                        {
                            crate::ui::find::show_search_dialog(&window_clone, active_fm);
                        } else {
                            active_fm.request_create_dir();
                        }
                    }
                });
                *btn_f7_ref.borrow_mut() = Some(btn.clone());
            }
            "delete" => {
                let left_info_clone = left_info.clone();
                let right_info_clone = right_info.clone();
                let active_panel_btn = active_panel.clone();
                btn.connect_clicked(move |_| {
                    let left_fm_clone = left_info_clone.active_router();
                    let right_fm_clone = right_info_clone.active_router();
                    let active_fm = match active_panel_btn.get() {
                        super::ActivePanelSide::Left => Some(&left_fm_clone),
                        super::ActivePanelSide::Right => Some(&right_fm_clone),
                        super::ActivePanelSide::None => None,
                    };
                    if let Some(active_fm) = active_fm {
                        active_fm.request_delete();
                    }
                });
            }
            _ => {
                let window_clone = window.clone();
                let updaters = selector_updaters.clone();
                let config_conn = config.clone();
                let global_on_connect_clone = global_on_connect.clone();
                btn.connect_clicked(move |_| {
                    let updaters_inner = updaters.clone();
                    let on_change = std::rc::Rc::new(move || {
                        for updater in updaters_inner.borrow().iter() {
                            updater();
                        }
                    });
                    let on_connect_cb = global_on_connect_clone.borrow().clone();
                    crate::connection_manager::show_manage_ftp_dialog(
                        &window_clone,
                        on_change,
                        config_conn.clone(),
                        on_connect_cb,
                    );
                });
            }
        }

        bottom_box.append(&btn);
    }

    bottom_box
}
