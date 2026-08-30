use adw::prelude::*;
use gtk::{Box, Orientation, Paned};
use relm4::ComponentController;

use crate::panel_builder::build_panel;

pub(super) struct PanelResult {
    pub paned: Paned,
    pub left_panel: Box,
    pub right_panel: Box,
    pub left_router: std::rc::Rc<panel_router::PanelRouter>,
    pub right_router: std::rc::Rc<panel_router::PanelRouter>,
    pub active_panel: std::rc::Rc<std::cell::Cell<super::ActivePanelSide>>,
    #[allow(dead_code)] // left/right symmetry: the terms and infos beside them are read
    pub left_vpaned: gtk::Paned,
    #[allow(dead_code)]
    pub right_vpaned: gtk::Paned,
    pub left_term: crate::terminal::TerminalView,
    pub right_term: crate::terminal::TerminalView,
    #[allow(dead_code)]
    pub left_expand: gtk::Button,
    #[allow(dead_code)]
    pub right_expand: gtk::Button,
    pub left_info: crate::panel_builder::PanelInfo,
    pub right_info: crate::panel_builder::PanelInfo,
    pub on_expand: std::rc::Rc<dyn Fn(super::ActivePanelSide)>,
    pub on_collapse: std::rc::Rc<dyn Fn()>,
    pub expanded_side: std::rc::Rc<std::cell::Cell<Option<super::ActivePanelSide>>>,
    #[allow(clippy::type_complexity)]
    pub expanded_notifier:
        std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn(Option<super::ActivePanelSide>)>>>>,
}

struct SideRefs {
    #[allow(dead_code)] // per-side record: `other_panel`, `vpaned`, `term` beside these are read
    side: super::ActivePanelSide,
    #[allow(dead_code)]
    own_panel: Box,
    other_panel: Box,
    vpaned: gtk::Paned,
    term: crate::terminal::TerminalView,
    expand_btn: gtk::Button,
    collapse_btn: gtk::Button,
    show_hidden_btn: gtk::Button,
    player_view: crate::player_ui::AudioPlayerView,
    router: std::rc::Rc<panel_router::PanelRouter>,
    header_hide_widgets: Vec<gtk::Widget>,
    #[allow(dead_code)]
    term_btn: gtk::Button,
}

fn expand_side(refs: &SideRefs) {
    refs.other_panel.set_visible(false);

    refs.router
        .fm
        .sender()
        .send(gtk_fm_ui::FmPanelInput::SetContentVisible(false))
        .ok();
    for w in &refs.header_hide_widgets {
        w.set_visible(false);
    }

    let vpaned = refs.vpaned.clone();
    gtk::glib::idle_add_local_once(move || {
        vpaned.set_position(0);
    });

    refs.term.container.set_visible(true);
    refs.player_view.container.set_visible(false);
    refs.expand_btn.set_visible(false);
    refs.show_hidden_btn.set_visible(false);
    refs.collapse_btn.set_visible(true);
}

fn collapse_side(refs: &SideRefs, saved_player_visible: bool, audio_player: &crate::player::AudioPlayer) {
    refs.other_panel.set_visible(true);
    refs.collapse_btn.set_visible(false);
    refs.router
        .fm
        .sender()
        .send(gtk_fm_ui::FmPanelInput::SetContentVisible(true))
        .ok();
    refs.show_hidden_btn.set_visible(true);

    for w in &refs.header_hide_widgets {
        w.set_visible(true);
    }
    refs.expand_btn.set_visible(refs.term.container.is_visible());

    if refs.player_view.container.is_visible() || saved_player_visible {
        refs.vpaned.set_position(refs.vpaned.height() * 3 / 5);
    } else {
        refs.vpaned.set_position(refs.vpaned.height() * 2 / 3);
    }

    if saved_player_visible && audio_player.playlist().len() > 0 {
        refs.player_view.container.set_visible(true);
    }
}

pub(super) fn build_panels(
    window: &adw::ApplicationWindow,
    config: &client_config::AppConfig,
    my_info: &ic_model::DeviceInfo,
    shift_held: std::rc::Rc<std::cell::Cell<bool>>,
    on_open_sysinfo: std::rc::Rc<dyn Fn()>,
    selector_updaters: std::rc::Rc<std::cell::RefCell<Vec<std::rc::Rc<dyn Fn()>>>>,
    initial_width: i32,
    term_out_left: tokio::sync::broadcast::Sender<Vec<u8>>,
    term_out_right: tokio::sync::broadcast::Sender<Vec<u8>>,
) -> PanelResult {
    let paned = Paned::builder()
        .orientation(Orientation::Horizontal)
        .hexpand(true)
        .vexpand(true)
        .position(initial_width / 2)
        .build();

    let paned_center = paned.clone();
    let double_click = gtk::GestureClick::new();
    double_click.connect_pressed(move |g, n_press, x, _y| {
        if n_press == 2 {
            let pos = paned_center.position();
            if x >= (pos - 10) as f64 && x <= (pos + 20) as f64 {
                let w = paned_center.width();
                paned_center.set_position(w / 2);
                g.set_state(gtk::EventSequenceState::Claimed);
            }
        }
    });
    paned.add_controller(double_click);

    let left_path = config.get::<String>("ui.left_panel_path");
    let right_path = config.get::<String>("ui.right_panel_path");

    let audio_player = crate::player::AudioPlayer::new();

    let left_info = build_panel(
        "Left Panel",
        left_path,
        selector_updaters.clone(),
        my_info.id.clone(),
        shift_held.clone(),
        audio_player.clone(),
        on_open_sysinfo.clone(),
        config.clone(),
        term_out_left,
    );
    let left_panel = left_info.panel_box.clone();
    let left_router = left_info.router.clone();
    let left_vpaned = left_info.paned.clone();
    let left_term = left_info.terminal_view.clone();
    let left_expand = left_info.expand_btn.clone();

    left_panel.set_size_request(400, -1);
    paned.set_start_child(Some(&left_panel));

    let right_info = build_panel(
        "Right Panel",
        right_path,
        selector_updaters.clone(),
        my_info.id.clone(),
        shift_held.clone(),
        audio_player.clone(),
        on_open_sysinfo.clone(),
        config.clone(),
        term_out_right,
    );
    let right_panel = right_info.panel_box.clone();
    let right_router = right_info.router.clone();
    let right_vpaned = right_info.paned.clone();
    let right_term = right_info.terminal_view.clone();
    let right_expand = right_info.expand_btn.clone();

    right_panel.set_size_request(400, -1);
    paned.set_end_child(Some(&right_panel));

    paned.set_shrink_start_child(false);
    paned.set_shrink_end_child(false);

    left_panel.add_css_class("active-panel");
    right_panel.add_css_class("inactive-panel");

    let active_panel = std::rc::Rc::new(std::cell::Cell::new(super::ActivePanelSide::Left));

    let set_active_panel = {
        let active_panel = active_panel.clone();
        let left_panel = left_panel.clone();
        let right_panel = right_panel.clone();
        move |side: super::ActivePanelSide| {
            active_panel.set(side);
            match side {
                super::ActivePanelSide::Left => {
                    left_panel.add_css_class("active-panel");
                    left_panel.remove_css_class("inactive-panel");
                    right_panel.add_css_class("inactive-panel");
                    right_panel.remove_css_class("active-panel");
                }
                super::ActivePanelSide::Right => {
                    right_panel.add_css_class("active-panel");
                    right_panel.remove_css_class("inactive-panel");
                    left_panel.add_css_class("inactive-panel");
                    left_panel.remove_css_class("active-panel");
                }
                super::ActivePanelSide::None => {
                    left_panel.remove_css_class("active-panel");
                    left_panel.add_css_class("inactive-panel");
                    right_panel.remove_css_class("active-panel");
                    right_panel.add_css_class("inactive-panel");
                }
            }
        }
    };

    let left_panel_focus = left_panel.clone();
    let right_panel_focus = right_panel.clone();
    window.connect_focus_widget_notify(move |win| {
        if let Some(w) = gtk::prelude::GtkWindowExt::focus(win) {
            if w.is_ancestor(&left_panel_focus) || w == left_panel_focus {
                set_active_panel(super::ActivePanelSide::Left);
            } else if w.is_ancestor(&right_panel_focus) || w == right_panel_focus {
                set_active_panel(super::ActivePanelSide::Right);
            }
        }
    });

    let expanded_side = std::rc::Rc::new(std::cell::Cell::new(None::<super::ActivePanelSide>));
    #[allow(clippy::type_complexity)]
    let expanded_notifier: std::rc::Rc<
        std::cell::RefCell<Option<std::rc::Rc<dyn Fn(Option<super::ActivePanelSide>)>>>,
    > = std::rc::Rc::new(std::cell::RefCell::new(None));
    let saved_paned_position = std::rc::Rc::new(std::cell::Cell::new(initial_width / 2));
    let saved_player_visible_left = std::rc::Rc::new(std::cell::Cell::new(false));
    let saved_player_visible_right = std::rc::Rc::new(std::cell::Cell::new(false));

    let make_left_refs = {
        let left_panel = left_panel.clone();
        let right_panel = right_panel.clone();
        let left_info = left_info.clone();
        move || SideRefs {
            side: super::ActivePanelSide::Left,
            own_panel: left_panel.clone(),
            other_panel: right_panel.clone(),
            vpaned: left_info.active_paned(),
            term: left_info.active_terminal(),
            expand_btn: left_info.active_expand_btn(),
            collapse_btn: left_info.active_collapse_btn(),
            show_hidden_btn: left_info.active_show_hidden_btn(),
            player_view: left_info.active_player_view(),
            router: left_info.active_router(),
            header_hide_widgets: left_info.active_header_hide_widgets(),
            term_btn: left_info.active_term_btn(),
        }
    };

    let make_right_refs = {
        let left_panel = left_panel.clone();
        let right_panel = right_panel.clone();
        let right_info = right_info.clone();
        move || SideRefs {
            side: super::ActivePanelSide::Right,
            own_panel: right_panel.clone(),
            other_panel: left_panel.clone(),
            vpaned: right_info.active_paned(),
            term: right_info.active_terminal(),
            expand_btn: right_info.active_expand_btn(),
            collapse_btn: right_info.active_collapse_btn(),
            show_hidden_btn: right_info.active_show_hidden_btn(),
            player_view: right_info.active_player_view(),
            router: right_info.active_router(),
            header_hide_widgets: right_info.active_header_hide_widgets(),
            term_btn: right_info.active_term_btn(),
        }
    };

    let collapse_fn = {
        let expanded_side = expanded_side.clone();
        let saved_paned_position = saved_paned_position.clone();
        let saved_player_visible_left = saved_player_visible_left.clone();
        let saved_player_visible_right = saved_player_visible_right.clone();
        let paned = paned.clone();
        let audio_player = audio_player.clone();
        let make_left_refs = make_left_refs.clone();
        let make_right_refs = make_right_refs.clone();
        let expanded_notifier = expanded_notifier.clone();

        move || {
            let Some(side) = expanded_side.get() else {
                return;
            };
            let (refs, saved_player_visible) = match side {
                super::ActivePanelSide::Left => (make_left_refs(), saved_player_visible_left.get()),
                super::ActivePanelSide::Right => (make_right_refs(), saved_player_visible_right.get()),
                super::ActivePanelSide::None => return,
            };
            collapse_side(&refs, saved_player_visible, &audio_player);
            paned.set_position(saved_paned_position.get());
            expanded_side.set(None);
            if let Some(cb) = expanded_notifier.borrow().as_ref() {
                cb(None);
            }
        }
    };

    let collapse_fn_rc = std::rc::Rc::new(collapse_fn);

    let collapse_action: std::rc::Rc<dyn Fn()> = collapse_fn_rc.clone();
    *left_info.collapse_hook.borrow_mut() = Some(collapse_action.clone());
    *right_info.collapse_hook.borrow_mut() = Some(collapse_action);

    let sync_hidden: std::rc::Rc<dyn Fn()> = {
        let left_info = left_info.clone();
        let right_info = right_info.clone();
        let config = config.clone();
        std::rc::Rc::new(move || {
            for info in [&left_info, &right_info] {
                let router = info.active_router();
                router.re_render();
                let on = config
                    .get::<bool>(&router.show_hidden_config_key())
                    .unwrap_or(false);
                let img = gtk::Image::from_resource(if on {
                    "/com/icecommander/gtk/hidden.svg"
                } else {
                    "/com/icecommander/gtk/non-hidden.svg"
                });
                img.set_pixel_size(16);
                info.active_show_hidden_btn().set_child(Some(&img));
            }
        })
    };
    *left_info.show_hidden_hook.borrow_mut() = Some(sync_hidden.clone());
    *right_info.show_hidden_hook.borrow_mut() = Some(sync_hidden);

    let expand_fn = {
        let expanded_side = expanded_side.clone();
        let saved_paned_position = saved_paned_position.clone();
        let saved_player_visible_left = saved_player_visible_left.clone();
        let saved_player_visible_right = saved_player_visible_right.clone();
        let paned = paned.clone();
        let collapse_fn_rc = collapse_fn_rc.clone();
        let make_left_refs = make_left_refs.clone();
        let make_right_refs = make_right_refs.clone();
        let expanded_notifier = expanded_notifier.clone();

        move |side: super::ActivePanelSide| {
            if side == super::ActivePanelSide::None {
                return;
            }
            if expanded_side.get() == Some(side) {
                return;
            }

            if expanded_side.get().is_none() {
                saved_paned_position.set(paned.position());
            }

            let refs = match side {
                super::ActivePanelSide::Left => {
                    let r = make_left_refs();
                    saved_player_visible_left.set(r.player_view.container.is_visible());
                    r
                }
                super::ActivePanelSide::Right => {
                    let r = make_right_refs();
                    saved_player_visible_right.set(r.player_view.container.is_visible());
                    r
                }
                super::ActivePanelSide::None => return,
            };

            if expanded_side.get().is_some() {
                collapse_fn_rc();
            }

            expand_side(&refs);
            expanded_side.set(Some(side));
            if let Some(cb) = expanded_notifier.borrow().as_ref() {
                cb(Some(side));
            }
        }
    };

    let expand_fn_rc = std::rc::Rc::new(expand_fn);

    let expand_fn_click_left = expand_fn_rc.clone();
    let collapse_fn_click_left = collapse_fn_rc.clone();
    let expanded_side_click_left = expanded_side.clone();
    left_expand.connect_clicked(move |_| {
        if expanded_side_click_left.get() == Some(super::ActivePanelSide::Left) {
            collapse_fn_click_left();
        } else {
            expand_fn_click_left(super::ActivePanelSide::Left);
        }
    });

    let expand_fn_click_right = expand_fn_rc.clone();
    let collapse_fn_click_right = collapse_fn_rc.clone();
    let expanded_side_click_right = expanded_side.clone();
    right_expand.connect_clicked(move |_| {
        if expanded_side_click_right.get() == Some(super::ActivePanelSide::Right) {
            collapse_fn_click_right();
        } else {
            expand_fn_click_right(super::ActivePanelSide::Right);
        }
    });

    let collapse_fn_left = collapse_fn_rc.clone();
    let expanded_side_left = expanded_side.clone();
    left_term.container.connect_visible_notify(move |v| {
        if !v.is_visible() && expanded_side_left.get() == Some(super::ActivePanelSide::Left) {
            collapse_fn_left();
        }
    });

    let collapse_fn_right = collapse_fn_rc.clone();
    let expanded_side_right = expanded_side.clone();
    right_term.container.connect_visible_notify(move |v| {
        if !v.is_visible() && expanded_side_right.get() == Some(super::ActivePanelSide::Right) {
            collapse_fn_right();
        }
    });

    PanelResult {
        paned,
        left_panel,
        right_panel,
        left_router,
        right_router,
        active_panel,
        left_vpaned,
        right_vpaned,
        left_term,
        right_term,
        left_expand,
        right_expand,
        left_info,
        right_info,
        on_expand: expand_fn_rc,
        on_collapse: collapse_fn_rc,
        expanded_side,
        expanded_notifier,
    }
}
