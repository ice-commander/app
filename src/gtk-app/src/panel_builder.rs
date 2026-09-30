use adw::prelude::*;
use fm_core::rpc::FileSystemRpc;
use gtk::{Box, Button, Orientation, Paned, Stack};
use relm4::prelude::*;

use crate::drivestoolbar::create_drives_toolbar;
use crate::source_selector::create_source_selector;

#[derive(Clone)]
pub struct PanelInfo {
    pub panel_box: Box,
    pub tab_view: adw::TabView,
    tabs: std::rc::Rc<std::cell::RefCell<Vec<(adw::TabPage, TabInfo)>>>,
    #[allow(clippy::type_complexity)]
    add_tab_fn: std::rc::Rc<dyn Fn(Option<String>, bool)>,
    pub router: std::rc::Rc<panel_router::PanelRouter>,
    #[allow(dead_code)]
    pub registry_manager: crate::registry_panel::RegistryPanel,
    pub paned: gtk::Paned,
    pub terminal_view: crate::terminal::TerminalView,
    pub expand_btn: gtk::Button,
    pub open_terminal: std::rc::Rc<dyn Fn()>,
    pub collapse_hook: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>>,
    pub show_hidden_hook: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>>,
    #[allow(clippy::type_complexity)]
    pub drop_hook:
        std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn(Vec<std::path::PathBuf>)>>>>,
}

#[derive(Clone)]
struct TabInfo {
    id: u32,
    content: Box,
    tab_header: gtk::DropDown,
    tab_switch: Box,
    router: std::rc::Rc<panel_router::PanelRouter>,
    registry_manager: crate::registry_panel::RegistryPanel,
    paned: gtk::Paned,
    terminal_view: crate::terminal::TerminalView,
    expand_btn: gtk::Button,
    collapse_btn: gtk::Button,
    show_hidden_btn: gtk::Button,
    player_view: crate::player_ui::AudioPlayerView,
    term_btn: gtk::Button,
    plugin_actions: Vec<(Button, String)>,
    /// Buttons a plugin put on its own filesystem's toolbar, held with the
    /// extensions they belong to and the id the plugin knows them by.
    fs_actions: Vec<(gtk::Widget, Vec<String>, String)>,
    header_hide_widgets: Vec<gtk::Widget>,
    open_terminal: std::rc::Rc<dyn Fn()>,
    toggle_terminal: std::rc::Rc<dyn Fn()>,
}

#[allow(clippy::too_many_arguments)]
pub fn toolbar_icon(resource: &str) -> gtk::Image {
    gtk_fm_ui::utils::toolbar_icon(resource)
}

pub fn build_panel(
    name: &str,
    initial_path: Option<String>,
    selector_updaters: std::rc::Rc<std::cell::RefCell<Vec<std::rc::Rc<dyn Fn()>>>>,
    clipboard: std::rc::Rc<fm_core::clipboard::Clipboard>,
    _my_device_id: String,
    shift_held: std::rc::Rc<std::cell::Cell<bool>>,
    audio_player: crate::player::AudioPlayer,
    config: client_config::AppConfig,
    term_output_tx: tokio::sync::broadcast::Sender<Vec<u8>>,
) -> PanelInfo {
    let panel_box = Box::builder().orientation(Orientation::Vertical).build();
    panel_box.add_css_class("panel-container");

    let tab_title = tab_title_for(&initial_path);

    let config_for_addtab = config.clone();

    let clipboard_tabs = clipboard.clone();
    let nav_hook: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    let collapse_hook: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    let show_hidden_hook: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    #[allow(clippy::type_complexity)]
    let drop_hook: std::rc::Rc<
        std::cell::RefCell<Option<std::rc::Rc<dyn Fn(Vec<std::path::PathBuf>)>>>,
    > = std::rc::Rc::new(std::cell::RefCell::new(None));

    let name_owned = name.to_string();
    let make_tab: std::rc::Rc<dyn Fn(Option<String>) -> TabInfo> = {
        let nav_hook = nav_hook.clone();
        let collapse_hook = collapse_hook.clone();
        let show_hidden_hook = show_hidden_hook.clone();
        let drop_hook = drop_hook.clone();
        std::rc::Rc::new(move |path| {
            build_tab(
                &name_owned,
                path,
                selector_updaters.clone(),
                clipboard_tabs.clone(),
                shift_held.clone(),
                audio_player.clone(),
                config.clone(),
                term_output_tx.clone(),
                nav_hook.clone(),
                collapse_hook.clone(),
                show_hidden_hook.clone(),
                drop_hook.clone(),
            )
        })
    };

    let next_tab_id = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let alloc_tab_id: std::rc::Rc<dyn Fn() -> u32> = {
        let n = next_tab_id.clone();
        std::rc::Rc::new(move || {
            let id = n.get();
            n.set(id + 1);
            id
        })
    };

    let mut first = make_tab(initial_path);
    first.id = alloc_tab_id();

    let tab_view = adw::TabView::builder().hexpand(true).vexpand(true).build();
    tab_view.connect_close_page(|view, page| {
        view.close_page_finish(page, view.n_pages() > 1);
        gtk::glib::Propagation::Stop
    });

    let tab_strip = Box::builder()
        .orientation(Orientation::Horizontal)
        .css_classes(vec!["panel-header"])
        .spacing(4)
        .build();
    let add_img = gtk::Image::from_resource("/com/icecommander/gtk/add.svg");
    add_img.set_pixel_size(16);
    let add_btn = Button::builder()
        .child(&add_img)
        .tooltip_text("New tab")
        .build();
    add_btn.add_css_class("flat");
    tab_strip.append(&add_btn);

    let tabs_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .hexpand(true)
        .build();
    tab_strip.append(&tabs_box);

    let close_img = gtk::Image::from_resource("/com/icecommander/gtk/close.svg");
    close_img.set_pixel_size(16);
    let close_btn = Button::builder()
        .child(&close_img)
        .tooltip_text("Close current tab")
        .build();
    close_btn.add_css_class("flat");
    close_btn.set_visible(false);
    tab_strip.append(&close_btn);
    {
        let tab_view = tab_view.clone();
        close_btn.connect_clicked(move |_| {
            if let Some(page) = tab_view.selected_page() {
                tab_view.close_page(&page);
            }
        });
    }
    {
        let close_btn = close_btn.clone();
        tab_view.connect_n_pages_notify(move |view| {
            close_btn.set_visible(view.n_pages() > 1);
        });
    }

    let tabs: std::rc::Rc<std::cell::RefCell<Vec<(adw::TabPage, TabInfo)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    #[allow(clippy::type_complexity)]
    let strip_tabs: std::rc::Rc<std::cell::RefCell<Vec<(adw::TabPage, Box)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

    let add_tab_widget: std::rc::Rc<dyn Fn(&adw::TabPage, &Box, &gtk::DropDown)> = {
        let tab_view = tab_view.clone();
        let tabs_box = tabs_box.clone();
        let strip_tabs = strip_tabs.clone();
        std::rc::Rc::new(move |page, switch_box, dropdown| {
            let tab_w = Box::builder()
                .orientation(Orientation::Horizontal)
                .spacing(2)
                .css_classes(vec!["ic-tab"])
                .build();
            tab_w.append(switch_box);
            tab_w.append(dropdown);

            {
                let tab_view = tab_view.clone();
                let page = page.clone();
                let gesture = gtk::GestureClick::new();
                gesture.connect_pressed(move |_, _, _, _| {
                    tab_view.set_selected_page(&page);
                });
                switch_box.add_controller(gesture);
            }

            tabs_box.append(&tab_w);
            strip_tabs.borrow_mut().push((page.clone(), tab_w));
        })
    };

    {
        let strip_tabs = strip_tabs.clone();
        tab_view.connect_selected_page_notify(move |view| {
            let sel = view.selected_page();
            for (page, w) in strip_tabs.borrow().iter() {
                if Some(page) == sel.as_ref() {
                    w.add_css_class("active-tab");
                } else {
                    w.remove_css_class("active-tab");
                }
            }
        });
    }

    {
        let tabs = tabs.clone();
        let strip_tabs = strip_tabs.clone();
        let tabs_box = tabs_box.clone();
        tab_view.connect_page_detached(move |_, page, _| {
            {
                let tabs_ref = tabs.borrow();
                if let Some((_, info)) = tabs_ref.iter().find(|(p, _)| p == page) {
                    info.terminal_view.stop_session();
                }
            }
            tabs.borrow_mut().retain(|(p, _)| p != page);
            let mut st = strip_tabs.borrow_mut();
            if let Some(pos) = st.iter().position(|(p, _)| p == page) {
                let (_, w) = st.remove(pos);
                tabs_box.remove(&w);
            }
        });
    }

    let add_tab_fn: std::rc::Rc<dyn Fn(Option<String>, bool)> = {
        let tab_view = tab_view.clone();
        let tabs = tabs.clone();
        let make_tab = make_tab.clone();
        let add_tab_widget = add_tab_widget.clone();
        let alloc_tab_id = alloc_tab_id.clone();
        std::rc::Rc::new(move |path, select| {
            let mut info = make_tab(path);
            info.id = alloc_tab_id();
            let page = tab_view.append(&info.content);
            add_tab_widget(&page, &info.tab_switch, &info.tab_header);
            tabs.borrow_mut().push((page.clone(), info));
            if select {
                tab_view.set_selected_page(&page);
            }
        })
    };

    {
        let page = tab_view.append(&first.content);
        page.set_title(&tab_title);
        add_tab_widget(&page, &first.tab_switch, &first.tab_header);
        tabs.borrow_mut().push((page.clone(), first.clone()));
        tab_view.set_selected_page(&page);
    }

    {
        let add_tab_fn = add_tab_fn.clone();
        let config_add = config_for_addtab.clone();
        let gesture = gtk::GestureClick::new();
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        gesture.connect_released(move |g, _, _, _| {
            let ctrl = g
                .current_event_state()
                .contains(gtk::gdk::ModifierType::CONTROL_MASK);
            let default_focus = config_add
                .get::<bool>("ui.new_tab_focus_new")
                .unwrap_or(true);
            add_tab_fn(None, default_focus ^ ctrl);
        });
        add_btn.add_controller(gesture);
    }

    panel_box.append(&tab_strip);
    panel_box.append(&tab_view);

    let panel_info = PanelInfo {
        panel_box,
        tab_view,
        tabs,
        add_tab_fn,
        router: first.router,
        registry_manager: first.registry_manager,
        paned: first.paned,
        terminal_view: first.terminal_view,
        expand_btn: first.expand_btn,
        open_terminal: first.open_terminal,
        collapse_hook,
        show_hidden_hook,
        drop_hook,
    };

    {
        let side_str = if name.to_lowercase().contains("left") {
            "left"
        } else {
            "right"
        };
        {
            let info_for_switch = panel_info.clone();
            panel_info.tab_view.connect_selected_page_notify(move |_| {
                info_for_switch.refresh_terminal_button();
                crate::api::notify_side(side_str, &info_for_switch);
            });
        }
        {
            let info_for_nav = panel_info.clone();
            let clip_nav = clipboard.clone();
            *nav_hook.borrow_mut() = Some(std::rc::Rc::new(move || {
                if clip_nav.count() > 0 {
                    let live: Vec<_> = info_for_nav
                        .all_routers()
                        .iter()
                        .flat_map(|r| {
                            r.state
                                .path
                                .borrow()
                                .levels()
                                .iter()
                                .map(|l| l.fs.clone())
                                .collect::<Vec<_>>()
                        })
                        .collect();
                    clip_nav.drop_if_unreachable(&live);
                }
                info_for_nav.refresh_terminal_button();
                crate::api::notify_side(side_str, &info_for_nav)
            }));
        }
    }

    panel_info.refresh_terminal_button();
    panel_info
}

impl PanelInfo {
    fn with_active<T>(&self, f: impl Fn(&TabInfo) -> T) -> T {
        let tabs = self.tabs.borrow();
        if let Some(sel) = self.tab_view.selected_page() {
            if let Some((_, info)) = tabs.iter().find(|(p, _)| *p == sel) {
                return f(info);
            }
        }
        f(&tabs[0].1)
    }

    pub fn active_router(&self) -> std::rc::Rc<panel_router::PanelRouter> {
        self.with_active(|t| t.router.clone())
    }

    pub fn all_routers(&self) -> Vec<std::rc::Rc<panel_router::PanelRouter>> {
        self.tabs
            .borrow()
            .iter()
            .map(|(_, t)| t.router.clone())
            .collect()
    }

    pub fn active_toggle_terminal(&self) -> std::rc::Rc<dyn Fn()> {
        self.with_active(|t| t.toggle_terminal.clone())
    }

    pub fn active_terminal(&self) -> crate::terminal::TerminalView {
        self.with_active(|t| t.terminal_view.clone())
    }
    pub fn active_paned(&self) -> gtk::Paned {
        self.with_active(|t| t.paned.clone())
    }
    pub fn active_expand_btn(&self) -> gtk::Button {
        self.with_active(|t| t.expand_btn.clone())
    }
    pub fn active_collapse_btn(&self) -> gtk::Button {
        self.with_active(|t| t.collapse_btn.clone())
    }
    pub fn active_show_hidden_btn(&self) -> gtk::Button {
        self.with_active(|t| t.show_hidden_btn.clone())
    }
    pub fn active_player_view(&self) -> crate::player_ui::AudioPlayerView {
        self.with_active(|t| t.player_view.clone())
    }
    pub fn refresh_terminal_button(&self) {
        let supported = self
            .active_router()
            .state
            .active_provider()
            .supports_terminal();
        self.active_term_btn().set_sensitive(supported);
        self.refresh_plugin_actions();
    }

    pub fn refresh_plugin_actions(&self) {
        let active = self.active_router().current_resource_id();
        let actions = self.with_active(|t| t.plugin_actions.clone());
        for (btn, source_id) in actions {
            btn.set_visible(*source_id == active);
        }
        let provider = self.active_router().state.active_provider();
        let scope = provider.plugin_scope().unwrap_or_default();
        // Asked of the registry, not of the filesystem: a store of files has no toolbar.
        let shown = crate::plugin_host::default_toolbar_shown(&scope);
        let _ = self
            .active_router()
            .fm
            .sender()
            .send(gtk_fm_ui::FmPanelInput::ShowDefaultToolbar(shown));
        let owned = self.with_active(|t| t.fs_actions.clone());
        for (widget, extensions, action_id) in owned {
            let mine = !scope.is_empty() && extensions.iter().any(|e| *e == scope);
            if !mine {
                widget.set_visible(false);
                continue;
            }
            let state = provider.plugin_action_state(&action_id);
            widget.set_visible(state & ic_plugin_api::IC_ACTION_SHOWN != 0);
            widget.set_sensitive(state & ic_plugin_api::IC_ACTION_ENABLED != 0);
            if let Some(toggle) = widget.downcast_ref::<gtk::ToggleButton>() {
                let on = state & ic_plugin_api::IC_ACTION_ON != 0;
                if toggle.is_active() != on {
                    toggle.set_active(on);
                }
            }
        }
    }

    pub fn active_term_btn(&self) -> gtk::Button {
        self.with_active(|t| t.term_btn.clone())
    }
    pub fn active_header_hide_widgets(&self) -> Vec<gtk::Widget> {
        self.with_active(|t| t.header_hide_widgets.clone())
    }

    pub fn add_tab(&self, path: Option<String>) {
        (self.add_tab_fn)(path, true);
    }

    pub fn read_tabs(&self) -> Vec<crate::api::ApiTab> {
        self.tabs
            .borrow()
            .iter()
            .map(|(_, t)| crate::api::ApiTab {
                id: t.id,
                title: tab_title_for_router(&t.router),
                icon: None,
            })
            .collect()
    }

    pub fn active_tab_id(&self) -> u32 {
        self.with_active(|t| t.id)
    }

    pub fn switch_tab_by_id(&self, id: u32) {
        let page = self
            .tabs
            .borrow()
            .iter()
            .find(|(_, t)| t.id == id)
            .map(|(p, _)| p.clone());
        if let Some(page) = page {
            self.tab_view.set_selected_page(&page);
        }
    }

    pub fn close_tab_by_id(&self, id: u32) {
        let page = self
            .tabs
            .borrow()
            .iter()
            .find(|(_, t)| t.id == id)
            .map(|(p, _)| p.clone());
        if let Some(page) = page {
            self.tab_view.close_page(&page);
        }
    }
}

fn tab_title_for_router(router: &panel_router::PanelRouter) -> String {
    if router.is_showing_selector() {
        return "/".to_string();
    }
    let path = router.state.path.borrow();
    path.levels()
        .last()
        .map(|l| l.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "/".to_string())
}

fn tab_title_for(initial_path: &Option<String>) -> String {
    initial_path
        .as_deref()
        .filter(|p| !p.is_empty())
        .and_then(|p| {
            std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "/".to_string())
}

pub fn selection_of(router: &panel_router::PanelRouter) -> Vec<crate::plugin_host::SelectedItem> {
    let key_column = router
        .state
        .active_provider()
        .as_any()
        .and_then(|a| a.downcast_ref::<crate::plugin_host::PluginPanelRpc>())
        .map(|p| p.key_column());
    router
        .selected_entries()
        .iter()
        .map(|e| {
            let key = match key_column {
                Some(0) | None => e.path(),
                Some(index) => e
                    .extra()
                    .get(index as usize - 1)
                    .cloned()
                    .unwrap_or_else(|| e.path()),
            };
            crate::plugin_host::SelectedItem {
                path: e.path(),
                key,
                is_dir: e.is_dir(),
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_tab(
    name: &str,
    initial_path: Option<String>,
    selector_updaters: std::rc::Rc<std::cell::RefCell<Vec<std::rc::Rc<dyn Fn()>>>>,
    clipboard: std::rc::Rc<fm_core::clipboard::Clipboard>,
    shift_held: std::rc::Rc<std::cell::Cell<bool>>,
    audio_player: crate::player::AudioPlayer,
    config: client_config::AppConfig,
    term_output_tx: tokio::sync::broadcast::Sender<Vec<u8>>,
    nav_hook: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>>,
    collapse_hook: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>>,
    show_hidden_hook: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>>,
    #[allow(clippy::type_complexity)] drop_hook: std::rc::Rc<
        std::cell::RefCell<Option<std::rc::Rc<dyn Fn(Vec<std::path::PathBuf>)>>>,
    >,
) -> TabInfo {
    let registry_manager = crate::registry_panel::RegistryPanel::new();

    let panel_id = if name.to_lowercase().contains("left") {
        "left"
    } else if name.to_lowercase().contains("right") {
        "right"
    } else {
        "default"
    };

    let local_rpc = std::rc::Rc::new(localfs::local_rpc::LocalFileSystemRpc::new(config.clone()));
    #[cfg(target_os = "windows")]
    let base_rpc = std::rc::Rc::new(localfs::drives_root_rpc::DrivesRootRpc::new())
        as std::rc::Rc<dyn FileSystemRpc>;
    #[cfg(not(target_os = "windows"))]
    let base_rpc = local_rpc.clone() as std::rc::Rc<dyn FileSystemRpc>;

    #[cfg(target_os = "windows")]
    let reg_btn = {
        let reg_btn = Button::builder()
            .child(&toolbar_icon("/com/icecommander/gtk/registry.svg"))
            .tooltip_text("Open Registry Editor")
            .build();
        reg_btn.set_cursor_from_name(Some("pointer"));
        reg_btn
    };

    let term_btn = Button::builder()
        .child(&toolbar_icon("/com/icecommander/gtk/unix-console.svg"))
        .tooltip_text("Open Terminal")
        .build();
    term_btn.set_cursor_from_name(Some("pointer"));

    let expand_btn = Button::builder()
        .child(&toolbar_icon("/com/icecommander/gtk/expand.svg"))
        .tooltip_text("Expand Terminal")
        .build();
    expand_btn.set_cursor_from_name(Some("pointer"));
    expand_btn.set_visible(false);

    let collapse_btn = Button::builder()
        .child(&toolbar_icon("/com/icecommander/gtk/collapse.svg"))
        .tooltip_text("Collapse Terminal")
        .build();
    collapse_btn.set_cursor_from_name(Some("pointer"));
    collapse_btn.add_css_class("flat");
    collapse_btn.set_visible(false);
    {
        let collapse_hook = collapse_hook.clone();
        collapse_btn.connect_clicked(move |_| {
            if let Some(f) = collapse_hook.borrow().as_ref() {
                f();
            }
        });
    }

    let search_btn = Button::builder()
        .child(&toolbar_icon("/com/icecommander/gtk/search.svg"))
        .tooltip_text("Search and Filter Files")
        .build();
    search_btn.set_cursor_from_name(Some("pointer"));

    let vsep = || {
        gtk::Separator::builder()
            .orientation(Orientation::Vertical)
            .margin_top(8)
            .margin_bottom(8)
            .build()
    };
    let term_sep = vsep();
    let search_sep = vsep();
    #[cfg(target_os = "windows")]
    let reg_sep = vsep();

    let shown = |key: &str| crate::settings::page_toolbar::shown(&config, key);

    let mut toolbar_end_extras: Vec<gtk::Widget> = Vec::new();
    let mut plugin_buttons: Vec<(Button, u32)> = Vec::new();
    let mut plugin_clicks: Vec<(Button, crate::plugin_host::ToolbarEntry)> = Vec::new();
    let mut plugin_actions: Vec<(Button, String)> = Vec::new();
    let mut toolbar_start_extras: Vec<gtk::Widget> = Vec::new();
    let mut plugin_action_clicks: Vec<(Button, crate::plugin_host::PanelAction)> = Vec::new();
    let mut fs_actions: Vec<(gtk::Widget, Vec<String>, String)> = Vec::new();
    let mut fs_action_clicks: Vec<(gtk::Widget, crate::plugin_host::FsAction)> = Vec::new();
    #[allow(unused_mut)]
    let mut header_hide_widgets: Vec<gtk::Widget> = Vec::new();
    #[cfg(target_os = "windows")]
    if shown("ui.toolbar.registry") {
        toolbar_end_extras.push(reg_sep.clone().upcast());
        toolbar_end_extras.push(reg_btn.clone().upcast());
        header_hide_widgets.push(reg_sep.clone().upcast());
        header_hide_widgets.push(reg_btn.clone().upcast());
    }
    if shown("ui.toolbar.terminal") {
        toolbar_end_extras.push(term_sep.clone().upcast());
        toolbar_end_extras.push(expand_btn.clone().upcast());
        toolbar_end_extras.push(term_btn.clone().upcast());
        header_hide_widgets.push(term_sep.clone().upcast());
    }
    if shown("ui.toolbar.search") {
        toolbar_end_extras.push(search_sep.clone().upcast());
        toolbar_end_extras.push(search_btn.clone().upcast());
        header_hide_widgets.push(search_sep.clone().upcast());
        header_hide_widgets.push(search_btn.clone().upcast());
    }
    for entry in crate::plugin_host::toolbar_entries(ic_plugin_api::IC_SIDE_RIGHT) {
        if !shown(&format!("ui.toolbar.{}", entry.id)) {
            continue;
        }
        let Some(icon) = crate::plugin_host::image_from_svg(&entry.svg) else {
            continue;
        };
        let btn = Button::builder()
            .child(&icon)
            .tooltip_text(&entry.tooltip)
            .build();
        btn.set_cursor_from_name(Some("pointer"));
        plugin_clicks.push((btn.clone(), entry.clone()));
        let sep = vsep();
        toolbar_end_extras.push(sep.clone().upcast());
        toolbar_end_extras.push(btn.clone().upcast());
        header_hide_widgets.push(sep.upcast());
        header_hide_widgets.push(btn.clone().upcast());
        btn.set_sensitive(ic_plugin_api::enabled_for(entry.enable_flags, 0, 0));
        plugin_buttons.push((btn, entry.enable_flags));
    }

    for action in crate::plugin_host::panel_actions() {
        let Some(icon) = crate::plugin_host::image_from_svg(&action.svg) else {
            continue;
        };
        let btn = Button::builder()
            .child(&icon)
            .tooltip_text(&action.tooltip)
            .visible(false)
            .build();
        btn.set_cursor_from_name(Some("pointer"));
        toolbar_start_extras.push(btn.clone().upcast());
        plugin_actions.push((btn.clone(), action.source_id.clone()));
        plugin_action_clicks.push((btn, action));
    }

    for action in crate::plugin_host::fs_actions() {
        let Some(icon) = crate::plugin_host::image_from_svg(&action.svg) else {
            continue;
        };
        // A key with no phrase behind it is its own text.
        let tooltip = crate::connection_manager::translate_optional(&action.tooltip)
            .unwrap_or_else(|| action.tooltip.clone());
        // Fixed at registration: the toolbar is built before any mount is open to ask.
        let btn: gtk::Widget = if action.is_toggle() {
            gtk::ToggleButton::builder()
                .child(&icon)
                .tooltip_text(&tooltip)
                .visible(false)
                .build()
                .upcast()
        } else {
            Button::builder()
                .child(&icon)
                .tooltip_text(&tooltip)
                .visible(false)
                .build()
                .upcast()
        };
        btn.set_cursor_from_name(Some("pointer"));
        toolbar_start_extras.push(btn.clone());
        fs_actions.push((
            btn.clone(),
            action.extensions.clone(),
            action.action_id.clone(),
        ));
        fs_action_clicks.push((btn, action));
    }

    let (out_tx, out_rx) = relm4::channel::<gtk_fm_ui::FmPanelOutput>();
    let init = gtk_fm_ui::FmPanelInit {
        panel_id: panel_id.to_string(),
        show_toolbar: true,
        config: config.clone(),
        select_mask_enabled: true,
        thumbnailer: {
            let rpc = local_rpc.clone();
            Some(std::rc::Rc::new(move |path: &str, pic: &gtk::Picture| {
                rpc.thumbnail_into(path, pic);
            }))
        },
        toolbar_start_extras,
        toolbar_end_extras,
        on_selection_changed: {
            let buttons = plugin_buttons.clone();
            Some(std::rc::Rc::new(move |files: u32, dirs: u32| {
                for (btn, flags) in &buttons {
                    btn.set_sensitive(ic_plugin_api::enabled_for(*flags, files, dirs));
                }
            }))
        },
    };
    let fm = gtk_fm_ui::FmPanelModel::builder()
        .launch(init)
        .forward(&out_tx, |o| o);
    let panel_sender = fm.sender().clone();

    let router = panel_router::PanelRouter::new(
        fm,
        base_rpc,
        local_rpc.clone(),
        "/".to_string(),
        panel_id,
        config.clone(),
    );

    let acting: crate::mainwindow::selection::PanelSelection = {
        let router = router.clone();
        std::rc::Rc::new(move || selection_of(&router))
    };

    for (btn, action) in plugin_action_clicks {
        let acting = acting.clone();
        btn.connect_clicked(move |b| {
            let Some(window) = b.root().and_downcast::<gtk::Window>() else {
                return;
            };
            use gtk::glib::object::ObjectType;
            let window = window.as_ptr() as *mut std::os::raw::c_void;
            crate::mainwindow::selection::while_acting_for(acting.clone(), || action.fire(window));
        });
    }

    for (widget, action) in fs_action_clicks {
        let router_for_action = router.clone();
        let acting = acting.clone();
        let fire = move |b: &gtk::Widget| {
            let Some(window) = b.root().and_downcast::<gtk::Window>() else {
                return;
            };
            use gtk::glib::object::ObjectType;
            // The mount the panel stands in, not whatever the plugin opened last.
            let mount = router_for_action
                .state
                .active_provider()
                .plugin_mount()
                .unwrap_or(0) as *mut std::os::raw::c_void;
            let window = window.as_ptr() as *mut std::os::raw::c_void;
            crate::mainwindow::selection::while_acting_for(acting.clone(), || {
                action.fire(mount, window)
            });
        };
        match widget.downcast_ref::<gtk::ToggleButton>() {
            // A refresh presses it too, so only the way in counts as a choice.
            Some(toggle) => {
                toggle.connect_toggled(move |t| {
                    if t.is_active() {
                        fire(t.clone().upcast_ref());
                    }
                });
            }
            None => {
                if let Some(button) = widget.downcast_ref::<Button>() {
                    button.connect_clicked(move |b| fire(b.clone().upcast_ref()));
                }
            }
        }
    }

    for (btn, entry) in plugin_clicks {
        let target = router.clone();
        let acting = acting.clone();
        btn.connect_clicked(move |b| {
            let Some(window) = b.root().and_downcast::<gtk::Window>() else {
                return;
            };
            let opener = target.clone();
            crate::plugin_host::set_open_handler(std::rc::Rc::new(move |id: &str| {
                let Some(source) = crate::plugin_host::panel_source(id) else {
                    return;
                };
                let previous = opener.state.active_provider();
                let previous_id = opener.current_resource_id();
                let restore = opener.clone();
                crate::plugin_host::set_close_handler(std::rc::Rc::new(move || {
                    restore.set_active_provider(previous.clone(), previous_id.clone());
                }));
                let provider = std::rc::Rc::new(crate::plugin_host::PluginPanelRpc::new(source));
                opener.set_active_provider(provider, id.to_string());
            }));
            use gtk::glib::object::ObjectType;
            let window = window.as_ptr() as *mut std::os::raw::c_void;
            crate::mainwindow::selection::while_acting_for(acting.clone(), || entry.fire(window));
        });
    }

    let show_hidden_btn = {
        let key = router.show_hidden_config_key();
        let on = config.get::<bool>(&key).unwrap_or(false);
        let img = toolbar_icon(if on {
            "/com/icecommander/gtk/hidden.svg"
        } else {
            "/com/icecommander/gtk/non-hidden.svg"
        });
        let btn = Button::builder()
            .child(&img)
            .css_classes(["flat"])
            .tooltip_text(&*crate::i18n::tr("toolbar.toggle_hidden"))
            .build();
        btn.set_cursor_from_name(Some("pointer"));
        let router_t = router.clone();
        let config_t = config.clone();
        let hook = show_hidden_hook.clone();
        btn.connect_clicked(move |_| {
            let key = router_t.show_hidden_config_key();
            let cur = config_t.get::<bool>(&key).unwrap_or(false);
            let _ = config_t.set(&key, &!cur);
            let _ = config_t.save();
            if let Some(h) = hook.borrow().as_ref() {
                h();
            } else {
                router_t.re_render();
            }
        });
        btn
    };
    panel_sender
        .send(gtk_fm_ui::FmPanelInput::AddAddressEndWidget(
            show_hidden_btn.clone().upcast(),
        ))
        .ok();
    panel_sender
        .send(gtk_fm_ui::FmPanelInput::AddAddressEndWidget(
            collapse_btn.clone().upcast(),
        ))
        .ok();

    let stack = Stack::builder()
        .hexpand(true)
        .vexpand(true)
        .hhomogeneous(false)
        .vhomogeneous(false)
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();

    let (tab_switch, tab_header) = create_drives_toolbar(
        router.clone(),
        stack.clone(),
        selector_updaters.clone(),
        shift_held.clone(),
        config.clone(),
        nav_hook,
    );

    stack.add_named(&registry_manager.container, Some("registry"));
    let on_open_registry = {
        let registry_manager = registry_manager.clone();
        let stack = stack.clone();
        std::rc::Rc::new(move || {
            let previous_page = stack
                .visible_child_name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "selector".to_string());
            let stack_back = stack.clone();
            registry_manager.set_back_callback(move || {
                stack_back.set_visible_child_name(&previous_page);
            });
            registry_manager.open_local();
            stack.set_visible_child_name("registry");
        })
    };

    #[cfg(target_os = "windows")]
    {
        let on_open_registry_clone = on_open_registry.clone();
        reg_btn.connect_clicked(move |_| {
            on_open_registry_clone();
        });
    }

    let on_open_panel_source: std::rc::Rc<dyn Fn(&str)> = {
        let stack = stack.clone();
        let router_for_source = router.clone();
        std::rc::Rc::new(move |id: &str| {
            let Some(source) = crate::plugin_host::panel_source(id) else {
                return;
            };
            let previous = router_for_source.state.active_provider();
            let previous_id = router_for_source.current_resource_id();
            let restore = router_for_source.clone();
            crate::plugin_host::set_close_handler(std::rc::Rc::new(move || {
                restore.set_active_provider(previous.clone(), previous_id.clone());
            }));
            let provider = std::rc::Rc::new(crate::plugin_host::PluginPanelRpc::new(source));
            router_for_source.set_active_provider(provider, id.to_string());
            stack.set_visible_child_name("filemanager");
        })
    };

    {
        let router_search = router.clone();
        search_btn.connect_clicked(move |btn| {
            if let Some(w) = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
                crate::ui::find::show_search_dialog(&w, &router_search);
            }
        });
    }

    let selector_box = create_source_selector(
        config.clone(),
        router.clone(),
        stack.clone(),
        selector_updaters,
        on_open_registry,
        on_open_panel_source,
    );
    stack.add_named(&selector_box, Some("selector"));

    stack.add_named(router.fm.widget(), Some("filemanager"));

    {
        let stack_fn = stack.clone();
        router.set_show_selector_fn(move |show| {
            stack_fn.set_visible_child_name(if show { "selector" } else { "filemanager" });
        });
    }

    {
        let router_sync = router.clone();
        stack.connect_notify_local(Some("visible-child"), move |stack, _| {
            let is_selector = stack.visible_child_name().as_deref() != Some("filemanager");
            router_sync.sync_selector_state(is_selector);
        });
    }

    let bottom = build_bottom_pane(
        &config,
        term_output_tx,
        &audio_player,
        &router,
        &stack,
        &expand_btn,
    );
    let terminal_view = bottom.terminal_view.clone();
    let player_view = bottom.player_view.clone();
    let paned = bottom.paned.clone();
    let ensure_term_open = bottom.open_terminal.clone();
    let toggle_term_rc = bottom.toggle_terminal.clone();
    {
        let toggle = toggle_term_rc.clone();
        term_btn.connect_clicked(move |_| toggle());
    }
    {
        let router = router.clone();
        let player_view = player_view.clone();
        let audio_player = audio_player.clone();
        let drop_hook = drop_hook.clone();
        let config = config.clone();
        let clipboard_out = clipboard.clone();
        gtk::glib::spawn_future_local(async move {
            use gtk_fm_ui::{FmPanelInput, FmPanelOutput};
            while let Some(out) = out_rx.recv().await {
                let Some(host) = router.handle_output(out).await else {
                    continue;
                };
                match host {
                    FmPanelOutput::Back | FmPanelOutput::Start => {
                        router.clear_history();
                        router.switch_to_selector(true);
                    }
                    FmPanelOutput::ActivateFile { path } => {
                        let name = std::path::Path::new(&path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string();
                        let association = crate::external::association_for(&config, &name);
                        let hand_over = association.is_some()
                            || crate::external::fallback(&config)
                                == crate::external::Fallback::System;

                        if hand_over {
                            let rel = router.state.resolve_relative(&path);
                            let provider = router.provider();
                            let fm = router.fm.sender().clone();
                            gtk::glib::spawn_future_local(async move {
                                if let Err(e) =
                                    crate::external::open_detached(provider, rel, association).await
                                {
                                    ic_logging::warn!("open externally: {e}");
                                    let _ = fm.send(FmPanelInput::OpFailed {
                                        title: crate::i18n::tr("assoc.open_failed").to_string(),
                                        message: e,
                                    });
                                }
                            });
                        } else if crate::viewer::AUDIO_EXT
                            .contains(&crate::viewer::extension_of(&path).as_str())
                        {
                            let (dir, entries) = {
                                let nav = router.state.path.borrow();
                                (nav.absolute_path(), nav.active().entries())
                            };
                            let mut playlist: Vec<(String, String)> = Vec::new();
                            let mut active_idx = None;
                            for e in entries.iter() {
                                if crate::viewer::AUDIO_EXT
                                    .contains(&crate::viewer::extension_of(&e.name).as_str())
                                {
                                    let full = if dir == "/" {
                                        format!("/{}", e.name)
                                    } else {
                                        format!("{}/{}", dir, e.name)
                                    };
                                    if full == path {
                                        active_idx = Some(playlist.len());
                                    }
                                    playlist.push((e.name.clone(), full));
                                }
                            }
                            audio_player.set_playlist(playlist, active_idx);
                            if let Some(idx) = active_idx {
                                player_view.play_track_at(idx);
                            }
                        } else if let Some(win) =
                            router.fm.widget().root().and_downcast::<gtk::Window>()
                        {
                            let name = std::path::Path::new(&path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("")
                                .to_string();
                            let size = {
                                let nav = router.state.path.borrow();
                                nav.active()
                                    .entries()
                                    .iter()
                                    .find(|e| e.name == name)
                                    .map(|e| e.size)
                                    .unwrap_or(0)
                            };
                            let entry =
                                gtk_fm_ui::FileEntry::new(&name, &path, false, size, "", None);
                            crate::viewer::show_viewer(&win, entry, router.clone());
                        }
                    }
                    FmPanelOutput::Cut | FmPanelOutput::Copy => {
                        let kind = if matches!(host, FmPanelOutput::Cut) {
                            fm_core::clipboard::ClipKind::Cut
                        } else {
                            fm_core::clipboard::ClipKind::Copy
                        };
                        crate::clipboard_ops::take(&clipboard_out, &router, kind);
                    }
                    FmPanelOutput::ClipboardClear => clipboard_out.clear(),
                    FmPanelOutput::Paste => {
                        if let Some(win) = router.window() {
                            crate::clipboard_ops::paste_into(&win, &clipboard_out, &router, None);
                        }
                    }
                    FmPanelOutput::PasteInto(name) => {
                        if let Some(win) = router.window() {
                            crate::clipboard_ops::paste_into(
                                &win,
                                &clipboard_out,
                                &router,
                                Some(name),
                            );
                        }
                    }
                    FmPanelOutput::Extract { archive_path } => {
                        let rel = router.state.resolve_relative(&archive_path);
                        let leaf = rel
                            .rsplit(['/', '\\'])
                            .next()
                            .unwrap_or_default()
                            .to_string();
                        if let Some(plugin) = fm_core::plugin_fs::filesystem_for(&leaf) {
                            let archive_fs: std::rc::Rc<dyn fm_core::rpc::FileSystemRpc> =
                                std::rc::Rc::new(fm_core::plugin_fs::PluginFsRpc::new(
                                    plugin,
                                    rel,
                                    router.state.active_provider(),
                                ));
                            match archive_fs.list_dir("/".to_string()).await {
                                Ok(entries) => {
                                    let items: Vec<(String, bool, u64, Option<u32>)> = entries
                                        .iter()
                                        .filter(|e| e.name != "..")
                                        .map(|e| (e.name.clone(), e.is_dir, e.size, e.permissions))
                                        .collect();
                                    if let Some(win) = router.window().and_then(|w| {
                                        use gtk::prelude::Cast;
                                        w.downcast::<adw::ApplicationWindow>().ok()
                                    }) {
                                        crate::file_operations::show_transfer_dialog(
                                            &win,
                                            crate::file_operations::TransferSource {
                                                items,
                                                parent: "/".to_string(),
                                                provider: archive_fs,
                                                refresh: Some(router.clone()),
                                                on_done: None,
                                                dest_names: std::collections::HashMap::new(),
                                            },
                                            &router,
                                            false,
                                            None,
                                        );
                                    }
                                }
                                Err(e) => {
                                    let _ = router.fm.sender().send(FmPanelInput::OpFailed {
                                        title: crate::i18n::tr("fm.extract_failed").to_string(),
                                        message: e.to_string(),
                                    });
                                }
                            }
                        }
                    }
                    FmPanelOutput::Download { paths } => {
                        let provider = router.state.active_provider();
                        for p in paths {
                            let rel = router.state.resolve_relative(&p);
                            provider.request_file_download(rel, uuid::Uuid::new_v4());
                        }
                    }
                    FmPanelOutput::UploadFiles { files, .. } => {
                        if let Some(f) = drop_hook.borrow().as_ref() {
                            f(files);
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    if let Some(ref path) = initial_path {
        if !path.is_empty() && std::path::Path::new(path).exists() {
            stack.set_visible_child_name("filemanager");
            router.open_path(path.clone());
        } else {
            stack.set_visible_child_name("selector");
        }
    } else {
        stack.set_visible_child_name("selector");
    }

    let content = Box::builder().orientation(Orientation::Vertical).build();
    content.append(&paned);

    TabInfo {
        id: 0,
        content,
        plugin_actions,
        fs_actions,
        tab_header,
        tab_switch,
        router,
        registry_manager,
        paned,
        terminal_view,
        expand_btn,
        collapse_btn,
        show_hidden_btn,
        player_view,
        term_btn,
        header_hide_widgets,
        open_terminal: ensure_term_open,
        toggle_terminal: toggle_term_rc,
    }
}

struct BottomPane {
    terminal_view: crate::terminal::TerminalView,
    player_view: crate::player_ui::AudioPlayerView,
    paned: Paned,
    open_terminal: std::rc::Rc<dyn Fn()>,
    toggle_terminal: std::rc::Rc<dyn Fn()>,
}

fn build_bottom_pane(
    config: &client_config::AppConfig,
    term_output_tx: tokio::sync::broadcast::Sender<Vec<u8>>,
    audio_player: &crate::player::AudioPlayer,
    router: &std::rc::Rc<panel_router::PanelRouter>,
    stack: &Stack,
    expand_btn: &Button,
) -> BottomPane {
    let terminal_view = crate::terminal::TerminalView::new(config.clone(), term_output_tx);
    terminal_view.container.set_visible(false);

    let player_view = crate::player_ui::AudioPlayerView::new(audio_player.clone(), router.clone());
    player_view.container.set_visible(false);

    let bottom_box = Box::builder().orientation(Orientation::Vertical).build();
    bottom_box.append(&player_view.container);
    bottom_box.append(&terminal_view.container);
    bottom_box.set_visible(false);

    let update_vis = {
        let bottom_box = bottom_box.clone();
        let term_container = terminal_view.container.clone();
        let player_container = player_view.container.clone();
        move || {
            let visible = term_container.is_visible() || player_container.is_visible();
            bottom_box.set_visible(visible);
        }
    };
    {
        let update_vis = update_vis.clone();
        terminal_view
            .container
            .connect_notify_local(Some("visible"), move |_, _| update_vis());
    }
    {
        let update_vis = update_vis.clone();
        player_view
            .container
            .connect_notify_local(Some("visible"), move |_, _| update_vis());
    }

    let paned = Paned::builder()
        .orientation(Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();
    paned.set_shrink_start_child(false);
    paned.set_shrink_end_child(false);
    paned.set_start_child(Some(stack));
    paned.set_end_child(Some(&bottom_box));

    {
        let bottom_box = bottom_box.clone();
        let player_view_c = player_view.clone();
        let terminal_view_c = terminal_view.clone();
        let paned_c = paned.clone();
        *player_view.on_show.borrow_mut() = Some(std::boxed::Box::new(move || {
            bottom_box.set_visible(true);
            player_view_c.container.set_visible(true);
            if !terminal_view_c.container.is_visible() {
                let (_, natural_size) = player_view_c.container.preferred_size();
                let height = paned_c.height();
                if height > natural_size.height() {
                    paned_c.set_position(height - natural_size.height());
                } else {
                    paned_c.set_position(height * 5 / 6);
                }
            } else {
                paned_c.set_position(paned_c.height() * 3 / 5);
            }
        }));
    }
    {
        let bottom_box = bottom_box.clone();
        let player_view_c = player_view.clone();
        let terminal_view_c = terminal_view.clone();
        let paned_c = paned.clone();
        *player_view.on_hide.borrow_mut() = Some(std::boxed::Box::new(move || {
            player_view_c.container.set_visible(false);
            if !terminal_view_c.container.is_visible() {
                bottom_box.set_visible(false);
            } else {
                paned_c.set_position(paned_c.height() * 2 / 3);
            }
        }));
    }

    let open_terminal: std::rc::Rc<dyn Fn() + 'static> = {
        let expand_btn = expand_btn.clone();
        let bottom_box = bottom_box.clone();
        let term_view = terminal_view.clone();
        let player_view = player_view.clone();
        let paned = paned.clone();
        let router = router.clone();
        std::rc::Rc::new(move || {
            if term_view.container.is_visible() {
                return;
            }
            if !router.state.active_provider().supports_terminal() {
                return;
            }
            expand_btn.set_visible(true);
            bottom_box.set_visible(true);
            term_view.container.set_visible(true);
            if player_view.container.is_visible() {
                paned.set_position(paned.height() * 3 / 5);
            } else {
                paned.set_position(paned.height() * 2 / 3);
            }
            let cwd = {
                let mut path = router.current_path_string();
                if let Some(entry) = router.selected_entries().first() {
                    if entry.is_dir() && entry.name() != ".." {
                        path = entry.path();
                    }
                }
                path
            };
            let provider = router.state.active_provider();
            // Asking, not opening: a trial open started a second shell on the far side.
            if provider.supports_terminal() {
                let opener = provider.clone();
                let where_from = cwd.clone();
                term_view.start_session_with(std::rc::Rc::new(move || {
                    opener
                        .open_shell(&where_from, 24, 80)
                        .ok_or_else(|| "the connection could not carry a shell".to_string())
                }));
            } else if let Some(cmd_args) = provider.get_ssh_connection_command(&cwd) {
                term_view.start_command_session(cmd_args, None);
            } else {
                term_view.start_local_session(Some(cwd));
            }
            term_view.notify_visibility(true);
        })
    };

    let toggle_terminal: std::rc::Rc<dyn Fn() + 'static> = {
        let expand_btn = expand_btn.clone();
        let bottom_box = bottom_box.clone();
        let term_view = terminal_view.clone();
        let player_view = player_view.clone();
        let paned = paned.clone();
        let open_terminal = open_terminal.clone();
        std::rc::Rc::new(move || {
            if term_view.container.is_visible() {
                expand_btn.set_visible(false);
                term_view.container.set_visible(false);
                term_view.stop_session();
                term_view.notify_visibility(false);
                if !player_view.container.is_visible() {
                    bottom_box.set_visible(false);
                } else {
                    let (_, natural_size) = player_view.container.preferred_size();
                    let height = paned.height();
                    if height > natural_size.height() {
                        paned.set_position(height - natural_size.height());
                    } else {
                        paned.set_position(height * 5 / 6);
                    }
                }
            } else {
                open_terminal();
            }
        })
    };

    {
        let toggle = toggle_terminal.clone();
        let term_container = terminal_view.container.clone();
        let session_gen = terminal_view.session_gen.clone();
        terminal_view.set_session_ended_callback(move || {
            let gen_at_end = session_gen.get();
            let toggle = toggle.clone();
            let term_container = term_container.clone();
            let session_gen = session_gen.clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
                if term_container.is_visible() && session_gen.get() == gen_at_end {
                    toggle();
                }
            });
        });
    }

    BottomPane {
        terminal_view,
        player_view,
        paned,
        open_terminal,
        toggle_terminal,
    }
}
