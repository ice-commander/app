use gtk::prelude::*;

use crate::file_operations::{trigger_copy, trigger_move};

pub(super) fn setup_keyboard(
    window: &adw::ApplicationWindow,
    app: &adw::Application,
    config: &client_config::AppConfig,
    left_info: &crate::panel_builder::PanelInfo,
    right_info: &crate::panel_builder::PanelInfo,
    left_panel: &gtk::Box,
    active_panel: std::rc::Rc<std::cell::Cell<super::ActivePanelSide>>,
    selector_updaters: std::rc::Rc<std::cell::RefCell<Vec<std::rc::Rc<dyn Fn()>>>>,
    shift_held: std::rc::Rc<std::cell::Cell<bool>>,
    global_on_connect: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn(crate::connection_manager::FtpConnection) + 'static>>>>,
    btn_f4_ref: std::rc::Rc<std::cell::RefCell<Option<gtk::Button>>>,
    btn_f7_ref: std::rc::Rc<std::cell::RefCell<Option<gtk::Button>>>,
    on_expand: std::rc::Rc<dyn Fn(super::ActivePanelSide)>,
    on_collapse: std::rc::Rc<dyn Fn()>,
    expanded_side: std::rc::Rc<std::cell::Cell<Option<super::ActivePanelSide>>>,
    clipboard: std::rc::Rc<fm_core::clipboard::Clipboard>,
) {
    let key_controller = gtk::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);

    let left_info_pressed = left_info.clone();
    let right_info_pressed = right_info.clone();
    let left_panel_pressed = left_panel.clone();
    let window_pressed = window.clone();
    let config_pressed = config.clone();
    let app_pressed = app.clone();
    let clip_keys = clipboard.clone();
    let active_panel_pressed = active_panel.clone();
    let selector_updaters_pressed = selector_updaters.clone();
    let shift_held_pressed = shift_held.clone();
    let global_on_connect_pressed = global_on_connect.clone();
    let btn_f4_ref_pressed = btn_f4_ref.clone();
    let btn_f7_ref_pressed = btn_f7_ref.clone();
    let on_expand_pressed = on_expand.clone();
    let on_collapse_pressed = on_collapse.clone();

    key_controller.connect_key_pressed(move |_, keyval, _, state| {
        let left_fm_pressed = left_info_pressed.active_router();
        let right_fm_pressed = right_info_pressed.active_router();
        let left_term_pressed = left_info_pressed.active_terminal();
        let right_term_pressed = right_info_pressed.active_terminal();

        if keyval != gtk::gdk::Key::Shift_L && keyval != gtk::gdk::Key::Shift_R {
            let is_shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            if !is_shift && shift_held_pressed.get() {
                shift_held_pressed.set(false);
                for updater in selector_updaters_pressed.borrow().iter() {
                    updater();
                }
            }
        }

        let get_active_panels = || match active_panel_pressed.get() {
            super::ActivePanelSide::Left => Some((&left_fm_pressed, &right_fm_pressed)),
            super::ActivePanelSide::Right => Some((&right_fm_pressed, &left_fm_pressed)),
            super::ActivePanelSide::None => None,
        };

        if keyval == gtk::gdk::Key::Escape {
            if let Some((active_fm, _)) = get_active_panels() {
                active_fm.cancel_editing();
            }
        }

        if keyval == gtk::gdk::Key::Shift_L || keyval == gtk::gdk::Key::Shift_R {
            if !shift_held_pressed.get() {
                shift_held_pressed.set(true);
                for updater in selector_updaters_pressed.borrow().iter() {
                    updater();
                }
            }
            if let Some(ref btn) = *btn_f4_ref_pressed.borrow() {
                btn.set_label(&*crate::i18n::tr("f_buttons.f4_new"));
            }
        }

        if keyval == gtk::gdk::Key::Alt_L || keyval == gtk::gdk::Key::Alt_R {
            if let Some(ref btn) = *btn_f7_ref_pressed.borrow() {
                btn.set_label(&*crate::i18n::tr("f_buttons.f7_find"));
            }
        }

        let terminal_focused = left_term_pressed.has_focus()
            || right_term_pressed.has_focus();

        if let Some(action_id) = crate::hotkey::resolve_action(&config_pressed, keyval, state) {
            match action_id.as_str() {
                "expand_terminal" | "toggle_video_fullscreen" => {
                    if expanded_side.get().is_some() {
                        on_collapse_pressed();
                        return gtk::glib::Propagation::Stop;
                    }
                    let side = if left_term_pressed.has_focus() {
                        super::ActivePanelSide::Left
                    } else if right_term_pressed.has_focus() {
                        super::ActivePanelSide::Right
                    } else {
                        active_panel_pressed.get()
                    };
                    let is_visible = match side {
                        super::ActivePanelSide::Left => left_term_pressed.container.is_visible(),
                        super::ActivePanelSide::Right => right_term_pressed.container.is_visible(),
                        super::ActivePanelSide::None => false,
                    };
                    if is_visible {
                        on_expand_pressed(side);
                    }
                    return gtk::glib::Propagation::Stop;
                }
                _ => {}
            }

            if terminal_focused {
                return gtk::glib::Propagation::Proceed;
            }

            if let Some((active_fm, _)) = get_active_panels() {
                match action_id.as_str() {
                    "manage_connections" => {
                        let updaters = selector_updaters_pressed.clone();
                        let on_change = std::rc::Rc::new(move || {
                            for updater in updaters.borrow().iter() {
                                updater();
                            }
                        });
                        let on_connect_cb = global_on_connect_pressed.borrow().clone();
                        crate::connection_manager::show_manage_ftp_dialog(
                            &window_pressed,
                            on_change,
                            config_pressed.clone(),
                            on_connect_cb,
                        );
                        return gtk::glib::Propagation::Stop;
                    }
                    "find_files" => {
                        crate::ui::find::show_search_dialog(&window_pressed, active_fm);
                        return gtk::glib::Propagation::Stop;
                    }
                    "filter_files" => {
                        active_fm.show_filter_bar();
                        return gtk::glib::Propagation::Stop;
                    }
                    "refresh" => {
                        active_fm.refresh_spawned();
                        return gtk::glib::Propagation::Stop;
                    }
                    "move_left" | "select_left" => {
                        left_fm_pressed.grab_focus();
                        return gtk::glib::Propagation::Stop;
                    }
                    "move_right" | "select_right" => {
                        right_fm_pressed.grab_focus();
                        return gtk::glib::Propagation::Stop;
                    }
                    "go_root_win" => {
                        let path = if cfg!(target_os = "windows") {
                            "C:\\".to_string()
                        } else {
                            "/".to_string()
                        };
                        active_fm.open_path(path);
                        return gtk::glib::Propagation::Stop;
                    }
                    "go_root_unix" => {
                        active_fm.open_path("/".to_string());
                        return gtk::glib::Propagation::Stop;
                    }
                    "clip_cut" | "clip_copy" | "clip_paste" => {
                        let editing = gtk::prelude::GtkWindowExt::focus(&window_pressed)
                            .map_or(false, |w| w.is::<gtk::Editable>() || w.is::<gtk::TextView>());
                        let dialog_up = adw::prelude::AdwApplicationWindowExt::visible_dialog(&window_pressed)
                            .is_some();
                        if editing || dialog_up {
                            return gtk::glib::Propagation::Proceed;
                        }
                        if action_id == "clip_paste" {
                            if clip_keys.count() == 0 {
                                return gtk::glib::Propagation::Proceed;
                            }
                            let Some(win) = active_fm.window() else {
                                return gtk::glib::Propagation::Proceed;
                            };
                            crate::clipboard_ops::paste_into(&win, &clip_keys, &active_fm, None);
                            return gtk::glib::Propagation::Stop;
                        }
                        let kind = if action_id == "clip_cut" {
                            fm_core::clipboard::ClipKind::Cut
                        } else {
                            fm_core::clipboard::ClipKind::Copy
                        };
                        if crate::clipboard_ops::take(&clip_keys, &active_fm, kind) {
                            return gtk::glib::Propagation::Stop;
                        }
                        return gtk::glib::Propagation::Proceed;
                    }
                    _ => {}
                }
            }
        }

        if keyval == gtk::gdk::Key::F9 {
            let toggle = if left_term_pressed.has_focus() {
                Some(left_info_pressed.active_toggle_terminal())
            } else if right_term_pressed.has_focus() {
                Some(right_info_pressed.active_toggle_terminal())
            } else {
                match active_panel_pressed.get() {
                    super::ActivePanelSide::Left => Some(left_info_pressed.active_toggle_terminal()),
                    super::ActivePanelSide::Right => Some(right_info_pressed.active_toggle_terminal()),
                    super::ActivePanelSide::None => None,
                }
            };
            if let Some(toggle) = toggle {
                toggle();
                return gtk::glib::Propagation::Stop;
            }
            return gtk::glib::Propagation::Proceed;
        }

        if terminal_focused {
            return gtk::glib::Propagation::Proceed;
        }

        if keyval == gtk::gdk::Key::Tab {
            let is_textview = gtk::prelude::GtkWindowExt::focus(&window_pressed)
                .map_or(false, |w| w.is::<gtk::TextView>());
            if is_textview {
                gtk::glib::Propagation::Proceed
            } else {
                if left_panel_pressed
                    .state_flags()
                    .contains(gtk::StateFlags::FOCUS_WITHIN)
                {
                    right_fm_pressed.grab_focus();
                } else {
                    left_fm_pressed.grab_focus();
                }
                gtk::glib::Propagation::Stop
            }
        } else if keyval == gtk::gdk::Key::BackSpace {
            let is_editable = gtk::prelude::GtkWindowExt::focus(&window_pressed)
                .map_or(false, |w| w.is::<gtk::Editable>() || w.is::<gtk::TextView>());
            if is_editable {
                gtk::glib::Propagation::Proceed
            } else if let Some((active_fm, _)) = get_active_panels() {
                active_fm.go_up_spawned();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        } else if keyval == gtk::gdk::Key::F1 {
            crate::help::show_help_dialog(&window_pressed, &config_pressed);
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::F2 {
            if let Some((active_fm, _)) = get_active_panels() {
                active_fm.start_rename();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        } else if keyval == gtk::gdk::Key::F3 {
            if let Some((active_fm, _)) = get_active_panels() {
                if let Some(entry) = active_fm.selected_entries().into_iter().next() {
                    let path = entry.path();
                    crate::viewer::show_viewer(&window_pressed, entry, active_fm.clone());
                    let side = match active_panel_pressed.get() {
                        super::ActivePanelSide::Left => "left",
                        super::ActivePanelSide::Right => "right",
                        super::ActivePanelSide::None => "",
                    };
                    crate::api::notify_viewer_opened(side, &path, "view");
                }
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        } else if keyval == gtk::gdk::Key::F4 {
            if let Some((active_fm, _)) = get_active_panels() {
                if state.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                    crate::editor::show_new_file_window(&window_pressed, Some(active_fm.clone()));
                } else if let Some(entry) = active_fm.selected_entries().into_iter().next() {
                    let path = entry.path();
                    crate::editor::show_editor(&window_pressed, entry, active_fm.clone());
                    let side = match active_panel_pressed.get() {
                        super::ActivePanelSide::Left => "left",
                        super::ActivePanelSide::Right => "right",
                        super::ActivePanelSide::None => "",
                    };
                    crate::api::notify_viewer_opened(side, &path, "edit");
                }
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        } else if keyval == gtk::gdk::Key::F5 {
            if let Some((active_fm, inactive_fm)) = get_active_panels() {
                trigger_copy(&window_pressed, active_fm, inactive_fm);
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        } else if keyval == gtk::gdk::Key::F6 {
            if let Some((active_fm, inactive_fm)) = get_active_panels() {
                trigger_move(&window_pressed, active_fm, inactive_fm);
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        } else if keyval == gtk::gdk::Key::F7 {
            if let Some((active_fm, _)) = get_active_panels() {
                if state.contains(gtk::gdk::ModifierType::ALT_MASK) {
                    crate::ui::find::show_search_dialog(&window_pressed, active_fm);
                } else {
                    active_fm.request_create_dir();
                }
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        } else if keyval == gtk::gdk::Key::F8
            || keyval == gtk::gdk::Key::Delete
            || keyval == gtk::gdk::Key::KP_Delete
        {
            let is_editable = gtk::prelude::GtkWindowExt::focus(&window_pressed)
                .map_or(false, |w| w.is::<gtk::Editable>() || w.is::<gtk::TextView>());
            if (keyval == gtk::gdk::Key::Delete || keyval == gtk::gdk::Key::KP_Delete) && is_editable {
                gtk::glib::Propagation::Proceed
            } else if let Some((active_fm, _)) = get_active_panels() {
                active_fm.request_delete();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        } else if keyval == gtk::gdk::Key::F10 {
            app_pressed.quit();
            gtk::glib::Propagation::Stop
        } else if (keyval == gtk::gdk::Key::r || keyval == gtk::gdk::Key::R)
            && state.contains(gtk::gdk::ModifierType::CONTROL_MASK)
        {
            if let Some((active_fm, _)) = get_active_panels() {
                active_fm.refresh_spawned();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        } else {
            gtk::glib::Propagation::Proceed
        }
    });

    let btn_f4_ref_released = btn_f4_ref.clone();
    let btn_f7_ref_released = btn_f7_ref.clone();
    let shift_held_released = shift_held.clone();
    let selector_updaters_released = selector_updaters.clone();
    key_controller.connect_key_released(move |_, keyval, _, _| {
        if keyval == gtk::gdk::Key::Shift_L || keyval == gtk::gdk::Key::Shift_R {
            if shift_held_released.get() {
                shift_held_released.set(false);
                for updater in selector_updaters_released.borrow().iter() {
                    updater();
                }
            }
            if let Some(ref btn) = *btn_f4_ref_released.borrow() {
                btn.set_label(&*crate::i18n::tr("f_buttons.f4"));
            }
        }
        if keyval == gtk::gdk::Key::Alt_L || keyval == gtk::gdk::Key::Alt_R {
            if let Some(ref btn) = *btn_f7_ref_released.borrow() {
                btn.set_label(&*crate::i18n::tr("f_buttons.f7"));
            }
        }
    });

    let window_click = gtk::GestureClick::new();
    window_click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let shift_held_click = shift_held.clone();
    let selector_updaters_click = selector_updaters.clone();
    window_click.connect_pressed(move |gesture, _, _, _| {
        let click_state = gesture.current_event_state();
        let active = click_state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
        if shift_held_click.get() != active {
            shift_held_click.set(active);
            for updater in selector_updaters_click.borrow().iter() {
                updater();
            }
        }
    });
    window.add_controller(window_click);
    window.add_controller(key_controller);
}
