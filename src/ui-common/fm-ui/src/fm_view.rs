#![allow(deprecated)] // adw::MessageDialog / FileChooserNative — modernization tracked separately
use adw::prelude::*;
use chrono::TimeZone;
use relm4::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::utils::{build_path_string, update_status_bar_text};
use crate::view_factories::{create_grid_factory, setup_column_view};
use fm_core::rpc::{PathSegment, RemoteFileEntry};

pub type ThumbnailFn = Rc<dyn Fn(&str, &gtk::Picture)>;

#[derive(Clone, Debug, Default)]
pub struct SourceInfo {
    pub is_local: bool,
    pub display_name: Option<String>,
    pub fs_label: Option<String>,
    pub root_icon_svg: Option<String>,
    pub root_icon: String,
    pub connection_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BreadcrumbSegment {
    pub name: String,
    pub path: String,
    pub icon: String,
    pub icon_svg: Option<String>,
}

impl BreadcrumbSegment {
    pub fn from_segments(segments: &[PathSegment]) -> Vec<BreadcrumbSegment> {
        segments
            .iter()
            .map(|s| BreadcrumbSegment {
                name: s.name.clone(),
                path: s.path.clone(),
                icon: "/com/fm-ui/gtk/folder.svg".to_string(),
                icon_svg: None,
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct Shared {
    pub panel_id: String,
    pub config: client_config::AppConfig,
    pub current_path: Rc<RefCell<Vec<String>>>,
    pub root_path: Rc<RefCell<String>>,
    pub cached_entries: Rc<RefCell<Vec<(String, bool, u64, String, Option<u32>)>>>,
    pub editing_name: Rc<RefCell<Option<String>>>,
    pub active_widgets: Rc<RefCell<HashMap<(bool, String), (gtk::Label, gtk::Entry)>>>,
    pub selected_files: Rc<RefCell<Vec<(String, bool)>>>,
    pub select_mask_enabled: Rc<Cell<bool>>,
    pub source: Rc<RefCell<SourceInfo>>,
}

#[derive(Clone)]
struct Views {
    rendered_crumbs: std::cell::RefCell<(String, Vec<BreadcrumbSegment>)>,
    stack: gtk::Stack,
    list_view: gtk::ColumnView,
    grid_view: gtk::GridView,
    list_store: gtk::gio::ListStore,
    selection_model: gtk::MultiSelection,
    breadcrumbs_box: gtk::Box,
    source_label: gtk::Label,
    status_label: gtk::Label,
    item_progress_bars: Rc<RefCell<HashMap<String, gtk::ProgressBar>>>,
    filter_bar: gtk::Box,
    filter_entry: gtk::Entry,
    select_bar: gtk::Box,
    select_entry: gtk::Entry,
    breadcrumbs_scroller: gtk::ScrolledWindow,
    address_entry: gtk::Entry,
    address_hint: gtk::Popover,
    address_hint_label: gtk::Label,
    error_label: gtk::Label,
}

pub struct FmPanelInit {
    pub panel_id: String,
    pub show_toolbar: bool,
    pub config: client_config::AppConfig,
    pub select_mask_enabled: bool,
    pub thumbnailer: Option<ThumbnailFn>,
    pub toolbar_start_extras: Vec<gtk::Widget>,
    pub toolbar_end_extras: Vec<gtk::Widget>,
}

impl FmPanelInit {
    pub fn new(config: client_config::AppConfig) -> Self {
        Self {
            panel_id: "default".to_string(),
            show_toolbar: true,
            config,
            select_mask_enabled: false,
            thumbnailer: None,
            toolbar_start_extras: Vec::new(),
            toolbar_end_extras: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub enum FmPanelInput {
    Listing {
        path: String,
        entries: Vec<RemoteFileEntry>,
        breadcrumb: Vec<BreadcrumbSegment>,
        source: SourceInfo,
        select_name: Option<String>,
    },
    Loading,
    LoadFailed {
        message: String,
    },
    OpFailed {
        title: String,
        message: String,
    },
    EntryAdded {
        name: String,
        is_dir: bool,
    },
    EntriesRemoved {
        names: Vec<String>,
    },
    EntryRenamed {
        old: String,
        new: String,
    },
    SetItemProgress {
        name: String,
        fraction: f64,
    },
    SetHistory {
        can_back: bool,
        can_forward: bool,
    },
    SetViewMode(String),
    SetSort {
        column: String,
        descending: bool,
    },
    StartRename,
    RequestCreateDir,
    RequestDelete,
    AddAddressEndWidget(gtk::Widget),
    SetContentVisible(bool),
    CancelEditing,
    ShowFilterBar,
    ShowSelectBar,
    QuickJump(char),
    ApplyMaskSelection(String),
    ReRender,
    SelectByName(String),
    GrabFocus,
    AddressNotResolved,
    BeginAddressEdit,
    CancelAddressEdit,
    CommitAddressEdit(String),
    ViewToggled(u32),
    FilterChanged(String),
}

#[derive(Debug)]
pub enum FmPanelOutput {
    NavigateEnter(String),
    NavigateUp,
    NavigateLevel(usize),
    NavigateTyped(String),
    Refresh,
    HistoryBack,
    HistoryForward,
    Back,
    Start,
    ActivateFile {
        path: String,
    },
    Download {
        paths: Vec<String>,
    },
    UploadFiles {
        dir: String,
        files: Vec<std::path::PathBuf>,
    },
    Mkdir {
        parent: String,
        name: String,
    },
    Rename {
        old_path: String,
        new_path: String,
    },
    Delete {
        paths: Vec<String>,
        names: Vec<String>,
    },
    Duplicate {
        src: String,
        dst: String,
    },
    Chmod {
        path: String,
        mode: u32,
    },
    #[cfg(feature = "support-archives")]
    Extract {
        archive_path: String,
    },
    #[cfg(feature = "support-archives")]
    Compress {
        src: String,
        dest: String,
    },
    ViewModeChanged(String),
    StateChanged {
        path: String,
        selected: Vec<(String, bool)>,
        cursor: Option<String>,
    },
    SortChanged {
        column: String,
        descending: bool,
    },
}

fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let s: Vec<char> = name.to_lowercase().chars().collect();
    let (mut i, mut j) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while i < s.len() {
        if j < p.len() && (p[j] == '?' || p[j] == s[i]) {
            i += 1;
            j += 1;
        } else if j < p.len() && p[j] == '*' {
            star = Some(j);
            mark = i;
            j += 1;
        } else if let Some(sj) = star {
            j = sj + 1;
            mark += 1;
            i = mark;
        } else {
            return false;
        }
    }
    while j < p.len() && p[j] == '*' {
        j += 1;
    }
    j == p.len()
}

pub struct FmPanelModel {
    panel_id: String,
    show_toolbar: bool,
    config: client_config::AppConfig,
    shared: Shared,
    loading: bool,
    error_message: Option<String>,
    can_hist_back: bool,
    can_hist_forward: bool,
    breadcrumb: Vec<BreadcrumbSegment>,
    is_root: bool,
    select_name: Option<String>,
    listing_generation: u64,
    view_mode_reflect: Option<String>,
    sort_reflect: Option<(String, bool)>,
    address_row: gtk::Box,
    content_box: gtk::Box,
    views: Views,
}

pub struct FmPanelWidgets {
    header: adw::HeaderBar,
    btn_up: gtk::Button,
    btn_up_sep: gtk::Separator,
    btn_hist_back: gtk::Button,
    btn_hist_forward: gtk::Button,
    list_view: gtk::ColumnView,
    name_column: gtk::ColumnViewColumn,
    date_column: gtk::ColumnViewColumn,
    size_column: gtk::ColumnViewColumn,
    btn_view_list: gtk::ToggleButton,
    btn_view_small: gtk::ToggleButton,
    btn_view_large: gtk::ToggleButton,
    view_handlers: [gtk::glib::SignalHandlerId; 3],
    rendered_generation: u64,
}

impl FmPanelModel {
    fn config_key(&self) -> String {
        if self.panel_id == "default" {
            "ui.fm_grid_icon_size".to_string()
        } else {
            format!("ui.fm_grid_icon_size_{}", self.panel_id)
        }
    }

    fn icon_size(&self) -> u32 {
        self.config.get::<u32>(&self.config_key()).unwrap_or(80)
    }

    fn show_hidden_config_key(&self) -> String {
        let shared = self
            .config
            .get::<bool>("ui.show_hidden_files_shared")
            .unwrap_or(true);
        if shared || self.panel_id == "default" {
            "ui.show_hidden_files".to_string()
        } else {
            format!("ui.show_hidden_files_{}", self.panel_id)
        }
    }

    fn should_skip_entry(&self, name: &str) -> bool {
        let show_hidden = self
            .config
            .get::<bool>(&self.show_hidden_config_key())
            .unwrap_or(false);
        !show_hidden && cfg!(unix) && name.starts_with('.')
    }

    fn emit_state_changed(&self, sender: &ComponentSender<Self>) {
        let path = build_path_string(&self.shared.current_path.borrow());
        let selected = self.shared.selected_files.borrow().clone();
        let cursor = self.cursor_name();
        let _ = sender.output(FmPanelOutput::StateChanged {
            path,
            selected,
            cursor,
        });
    }

    fn cursor_name(&self) -> Option<String> {
        let bitset = self.views.selection_model.selection();
        if bitset.size() == 0 {
            return None;
        }
        let obj = self.views.selection_model.item(bitset.nth(0))?;
        let entry = obj.downcast::<crate::file_entry::FileEntry>().ok()?;
        Some(entry.name())
    }

    fn patch_entry_added(&self, name: &str, is_dir: bool) {
        *self.shared.editing_name.borrow_mut() = None;
        self.shared.active_widgets.borrow_mut().clear();
        let date_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.shared.cached_entries.borrow_mut().push((
            name.to_string(),
            is_dir,
            0,
            date_str.clone(),
            None,
        ));

        let store = &self.views.list_store;
        let n = store.n_items();
        let mut insert_pos = 0u32;
        for i in 0..n {
            if let Some(obj) = store.item(i) {
                if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                    if entry.is_dir() {
                        insert_pos = i + 1;
                    }
                }
            }
        }
        let mut path_parts = self.shared.current_path.borrow().clone();
        path_parts.push(name.to_string());
        let full_path = build_path_string(&path_parts);
        let entry = crate::file_entry::FileEntry::new(name, &full_path, is_dir, 0, &date_str, None);
        store.splice(insert_pos, 0, &[entry.upcast::<gtk::glib::Object>()]);
        update_status_bar_text(
            &self.views.status_label,
            &self.views.selection_model,
            store,
            &self.shared.cached_entries,
        );
    }

    fn patch_entries_removed(&self, names: &[String]) {
        *self.shared.editing_name.borrow_mut() = None;
        self.shared.active_widgets.borrow_mut().clear();
        self.shared
            .cached_entries
            .borrow_mut()
            .retain(|(name, _, _, _, _)| !names.contains(name));

        let store = &self.views.list_store;
        let mut positions = Vec::new();
        let n = store.n_items();
        for i in 0..n {
            if let Some(obj) = store.item(i) {
                if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                    if names.contains(&entry.name()) {
                        positions.push(i);
                    }
                }
            }
        }
        for pos in positions.iter().rev() {
            store.remove(*pos);
        }
        update_status_bar_text(
            &self.views.status_label,
            &self.views.selection_model,
            store,
            &self.shared.cached_entries,
        );
    }

    fn patch_entry_renamed(&self, old_name: &str, new_name: &str) {
        *self.shared.editing_name.borrow_mut() = None;
        self.shared.active_widgets.borrow_mut().clear();
        for entry in self.shared.cached_entries.borrow_mut().iter_mut() {
            if entry.0 == old_name {
                entry.0 = new_name.to_string();
                break;
            }
        }
        let mut path_parts = self.shared.current_path.borrow().clone();
        path_parts.push(new_name.to_string());
        let new_path = build_path_string(&path_parts);

        let store = &self.views.list_store;
        let n = store.n_items();
        for i in 0..n {
            if let Some(obj) = store.item(i) {
                if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                    if entry.name() == old_name {
                        let new_entry = crate::file_entry::FileEntry::new(
                            new_name,
                            &new_path,
                            entry.is_dir(),
                            entry.size(),
                            &entry.date(),
                            entry.permissions(),
                        );
                        store.splice(i, 1, &[new_entry.upcast::<gtk::glib::Object>()]);
                        break;
                    }
                }
            }
        }
    }

    fn add_ghost_upload_item(&self, name: &str, size: u64) {
        let mut full_path = self.shared.current_path.borrow().clone();
        full_path.push(name.to_string());
        let full_path_str = build_path_string(&full_path);
        let entry = crate::file_entry::FileEntry::new(
            &format!("⏳ {}", name),
            &full_path_str,
            false,
            size,
            "",
            None,
        );
        self.views.list_store.append(&entry);
    }

    fn set_item_progress(&self, filename: &str, fraction: f64) {
        if fraction < 1.0
            && !self
                .views
                .item_progress_bars
                .borrow()
                .contains_key(&format!("list_{}", filename))
        {
            self.add_ghost_upload_item(filename, 0);
        }
        for prefix in ["list_", "grid_"] {
            if let Some(bar) = self
                .views
                .item_progress_bars
                .borrow()
                .get(&format!("{}{}", prefix, filename))
            {
                bar.set_fraction(fraction);
                bar.set_visible(fraction > 0.0 && fraction < 1.0);
            }
        }
    }

    fn apply_mask_selection(&self, mask: &str) {
        let mask = mask.trim();
        if mask.is_empty() {
            return;
        }
        let sm = &self.views.selection_model;
        sm.unselect_all();
        let n = sm.n_items();
        for pos in 0..n {
            let Some(obj) = sm.item(pos) else { continue };
            let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() else {
                continue;
            };
            if entry.is_dir() {
                continue;
            }
            if glob_match(mask, &entry.name()) {
                sm.select_item(pos, false);
            }
        }
    }

    fn start_rename(&self) {
        let sm = &self.views.selection_model;
        let bitset = sm.selection();
        if bitset.size() != 1 {
            return;
        }
        let Some(obj) = sm.item(bitset.nth(0)) else {
            return;
        };
        let Ok(entry_obj) = obj.downcast::<crate::file_entry::FileEntry>() else {
            return;
        };
        let name = entry_obj.name();
        if name == ".." {
            return;
        }
        *self.shared.editing_name.borrow_mut() = Some(name.clone());
        let is_list = self
            .views
            .stack
            .visible_child_name()
            .map(|s| s.as_str() == "list-view")
            .unwrap_or(true);
        if let Some((label, entry)) = self
            .shared
            .active_widgets
            .borrow()
            .get(&(is_list, name.clone()))
        {
            label.set_visible(false);
            entry.set_visible(true);
            entry.set_text(&name);
            let entry_c = entry.clone();
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let _ = entry_c.grab_focus();
                entry_c.select_region(0, -1);
                gtk::glib::ControlFlow::Break
            });
        }
    }

    fn cancel_editing(&self) {
        let mut editing = self.shared.editing_name.borrow_mut();
        if let Some(name) = editing.clone() {
            let is_list = self
                .views
                .stack
                .visible_child_name()
                .map(|s| s.as_str() == "list-view")
                .unwrap_or(true);
            if let Some((label, entry)) = self.shared.active_widgets.borrow().get(&(is_list, name))
            {
                label.set_visible(true);
                entry.set_visible(false);
            }
            *editing = None;
        }
    }

    fn quick_jump(&self, ch: char) {
        if self.shared.editing_name.borrow().is_some() {
            return;
        }
        let needle: String = ch.to_lowercase().collect();
        let sm = &self.views.selection_model;
        let n = sm.n_items();
        if n == 0 {
            return;
        }
        let bitset = sm.selection();
        let start = if bitset.is_empty() {
            0
        } else {
            bitset.nth(0) + 1
        };
        for off in 0..n {
            let pos = (start + off) % n;
            let Some(obj) = sm.item(pos) else { continue };
            let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() else {
                continue;
            };
            let name = entry.name();
            if name == ".." {
                continue;
            }
            if name.to_lowercase().starts_with(&needle) {
                sm.select_item(pos, true);
                let is_list = self
                    .views
                    .stack
                    .visible_child_name()
                    .map(|s| s.as_str() == "list-view")
                    .unwrap_or(true);
                if is_list {
                    self.views.list_view.scroll_to(
                        pos,
                        None,
                        gtk::ListScrollFlags::FOCUS,
                        None::<gtk::ScrollInfo>,
                    );
                } else {
                    self.views.grid_view.scroll_to(
                        pos,
                        gtk::ListScrollFlags::FOCUS,
                        None::<gtk::ScrollInfo>,
                    );
                }
                return;
            }
        }
    }
}

impl SimpleComponent for FmPanelModel {
    type Init = FmPanelInit;
    type Input = FmPanelInput;
    type Output = FmPanelOutput;
    type Root = gtk::Box;
    type Widgets = FmPanelWidgets;

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        crate::init_resources();
        let config = init.config.clone();
        let panel_id = init.panel_id.clone();

        let shared = Shared {
            panel_id: panel_id.clone(),
            config: config.clone(),
            current_path: Rc::new(RefCell::new(Vec::new())),
            root_path: Rc::new(RefCell::new("/".to_string())),
            cached_entries: Rc::new(RefCell::new(Vec::new())),
            editing_name: Rc::new(RefCell::new(None)),
            active_widgets: Rc::new(RefCell::new(HashMap::new())),
            selected_files: Rc::new(RefCell::new(Vec::new())),
            select_mask_enabled: Rc::new(Cell::new(init.select_mask_enabled)),
            source: Rc::new(RefCell::new(SourceInfo::default())),
        };

        let with_default_toolbar = init.show_toolbar;

        let stack = gtk::Stack::builder().vexpand(true).build();

        let loading_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .spacing(12)
            .build();
        let spinner = gtk::Spinner::builder()
            .width_request(40)
            .height_request(40)
            .build();
        spinner.start();
        loading_box.append(&spinner);
        loading_box.append(
            &gtk::Label::builder()
                .label(&*crate::i18n::tr("fm.loading"))
                .css_classes(vec!["dim-label"])
                .build(),
        );
        let btn_cancel_loading = gtk::Button::builder()
            .label(&*crate::i18n::tr("fm.cancel"))
            .css_classes(vec!["destructive-action"])
            .build();
        btn_cancel_loading.set_cursor_from_name(Some("pointer"));
        loading_box.append(&btn_cancel_loading);
        stack.add_named(&loading_box, Some("loading"));
        {
            let sender = sender.clone();
            let stack_weak = stack.downgrade();
            btn_cancel_loading.connect_clicked(move |_| {
                let _ = sender.output(FmPanelOutput::Back);
                if let Some(st) = stack_weak.upgrade() {
                    st.set_visible_child_name("list-view");
                }
            });
        }

        let error_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .spacing(12)
            .margin_start(24)
            .margin_end(24)
            .build();
        let error_title = gtk::Label::new(None);
        error_title.set_markup(&format!(
            "<span size='large' weight='bold'>{}</span>",
            crate::i18n::tr("fm.load_failed")
        ));
        error_box.append(&error_title);
        let error_label = gtk::Label::builder()
            .justify(gtk::Justification::Center)
            .wrap(true)
            .max_width_chars(48)
            .css_classes(vec!["dim-label"])
            .build();
        error_box.append(&error_label);
        let error_btns = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .halign(gtk::Align::Center)
            .spacing(8)
            .build();
        let btn_error_refresh = gtk::Button::builder()
            .label(&*crate::i18n::tr("fm.refresh"))
            .css_classes(vec!["suggested-action"])
            .build();
        btn_error_refresh.set_cursor_from_name(Some("pointer"));
        let btn_error_source = gtk::Button::builder()
            .label(&*crate::i18n::tr("fm.select_other_source"))
            .build();
        btn_error_source.set_cursor_from_name(Some("pointer"));
        error_btns.append(&btn_error_refresh);
        error_btns.append(&btn_error_source);
        error_box.append(&error_btns);
        stack.add_named(&error_box, Some("error"));
        {
            let sender = sender.clone();
            btn_error_refresh.connect_clicked(move |_| {
                let _ = sender.output(FmPanelOutput::Refresh);
            });
        }
        {
            let sender = sender.clone();
            btn_error_source.connect_clicked(move |_| {
                let _ = sender.output(FmPanelOutput::Start);
            });
        }

        let breadcrumbs_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .halign(gtk::Align::Start)
            .build();
        let breadcrumbs_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .child(&breadcrumbs_box)
            .valign(gtk::Align::Center)
            .build();
        breadcrumbs_scroller.add_css_class("address-pill");
        breadcrumbs_scroller.set_hexpand(true);
        {
            let scroll_ctrl =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
            let scroller_weak = breadcrumbs_scroller.downgrade();
            scroll_ctrl.connect_scroll(move |_, dx, dy| {
                if let Some(sc) = scroller_weak.upgrade() {
                    let adj = sc.hadjustment();
                    let delta = if dx != 0.0 { dx } else { dy };
                    adj.set_value(adj.value() + delta * 40.0);
                }
                gtk::glib::Propagation::Stop
            });
            breadcrumbs_scroller.add_controller(scroll_ctrl);
        }

        let empty_title = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .title_widget(&empty_title)
            .build();

        let mk_icon_btn = |res: &str, tip: String| {
            let b = gtk::Button::builder()
                .child(&gtk::Image::from_resource(res))
                .tooltip_text(&tip)
                .build();
            b.set_cursor_from_name(Some("pointer"));
            b
        };

        let nav_pill = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        nav_pill.add_css_class("nav-pill");
        nav_pill.set_valign(gtk::Align::Center);

        let btn_hist_back = gtk::Button::builder()
            .child(&gtk::Image::from_resource("/com/fm-ui/gtk/arrow-left.svg"))
            .tooltip_text(&*crate::i18n::tr("fm.navigate_back"))
            .sensitive(false)
            .build();
        btn_hist_back.set_cursor_from_name(Some("pointer"));
        {
            let sender = sender.clone();
            btn_hist_back.connect_clicked(move |_| {
                let _ = sender.output(FmPanelOutput::HistoryBack);
            });
        }
        nav_pill.append(&btn_hist_back);

        let btn_hist_forward = gtk::Button::builder()
            .child(&gtk::Image::from_resource("/com/fm-ui/gtk/arrow-right.svg"))
            .tooltip_text(&*crate::i18n::tr("fm.navigate_forward"))
            .sensitive(false)
            .build();
        btn_hist_forward.set_cursor_from_name(Some("pointer"));
        {
            let sender = sender.clone();
            btn_hist_forward.connect_clicked(move |_| {
                let _ = sender.output(FmPanelOutput::HistoryForward);
            });
        }
        nav_pill.append(&btn_hist_forward);

        let btn_up_sep = gtk::Separator::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_top(8)
            .margin_bottom(8)
            .build();

        let btn_up = mk_icon_btn(
            "/com/fm-ui/gtk/arrow-up.svg",
            crate::i18n::tr("fm.go_up").to_string(),
        );
        {
            let sender = sender.clone();
            let shared = shared.clone();
            btn_up.connect_clicked(move |_| {
                let cur = build_path_string(&shared.current_path.borrow());
                let root = shared.root_path.borrow().clone();
                if cur == root || cur == "/" {
                    let _ = sender.output(FmPanelOutput::Back);
                } else {
                    let _ = sender.output(FmPanelOutput::NavigateUp);
                }
            });
        }
        nav_pill.append(&btn_up);

        let btn_refresh = mk_icon_btn(
            "/com/fm-ui/gtk/refresh.svg",
            crate::i18n::tr("fm.refresh").to_string(),
        );
        {
            let sender = sender.clone();
            btn_refresh.connect_clicked(move |_| {
                let _ = sender.output(FmPanelOutput::Refresh);
            });
        }
        nav_pill.append(&btn_refresh);

        let btn_create_dir = mk_icon_btn(
            "/com/fm-ui/gtk/add-folder.svg",
            crate::i18n::tr("fm.create_directory").to_string(),
        );
        {
            let sender = sender.clone();
            let shared = shared.clone();
            btn_create_dir.connect_clicked(move |btn| {
                let window = btn.root().and_downcast::<gtk::Window>();
                crate::dialogs::show_create_dir_dialog(window.as_ref(), &shared, sender.clone());
            });
        }
        if with_default_toolbar {
            header.pack_start(&btn_create_dir);
        }

        let btn_rename = mk_icon_btn(
            "/com/fm-ui/gtk/edit-pencil.svg",
            crate::i18n::tr("fm.rename_selected").to_string(),
        );
        btn_rename.set_sensitive(false);
        if with_default_toolbar {
            header.pack_start(&btn_rename);
        }

        let btn_delete = mk_icon_btn(
            "/com/fm-ui/gtk/close.svg",
            crate::i18n::tr("fm.delete_selected").to_string(),
        );
        btn_delete.set_sensitive(false);
        btn_delete.add_css_class("destructive-hover-action");
        {
            let sender = sender.clone();
            let shared = shared.clone();
            btn_delete.connect_clicked(move |btn| {
                let window = btn.root().and_downcast::<gtk::Window>();
                crate::dialogs::show_delete_dialog(window.as_ref(), &shared, sender.clone());
            });
        }
        if with_default_toolbar {
            header.pack_start(&btn_delete);
        }

        let btn_chmod = mk_icon_btn(
            "/com/fm-ui/gtk/edit-property.svg",
            "Change Permissions / Attributes (Chmod)".to_string(),
        );
        btn_chmod.set_sensitive(false);
        {
            let sender = sender.clone();
            let shared = shared.clone();
            btn_chmod.connect_clicked(move |btn| {
                let window = btn.root().and_downcast::<gtk::Window>();
                crate::dialogs::show_chmod_dialog(window.as_ref(), &shared, sender.clone());
            });
        }
        if with_default_toolbar {
            header.pack_start(&btn_chmod);
        }


        let view_switcher = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        view_switcher.add_css_class("linked");
        view_switcher.set_valign(gtk::Align::Center);
        let cfg_key = if panel_id == "default" {
            "ui.fm_grid_icon_size".to_string()
        } else {
            format!("ui.fm_grid_icon_size_{}", panel_id)
        };
        let current_size = config.get::<u32>(&cfg_key).unwrap_or(80) as i32;
        let btn_view_list = gtk::ToggleButton::builder()
            .child(&gtk::Image::from_resource("/com/fm-ui/gtk/list-icons.svg"))
            .tooltip_text(&*crate::i18n::tr("fm.list_view"))
            .build();
        let btn_view_small = gtk::ToggleButton::builder()
            .child(&gtk::Image::from_resource("/com/fm-ui/gtk/small-icons.svg"))
            .tooltip_text(&*crate::i18n::tr("fm.small_icons"))
            .build();
        let btn_view_large = gtk::ToggleButton::builder()
            .child(&gtk::Image::from_resource("/com/fm-ui/gtk/thumbnails.svg"))
            .tooltip_text(&*crate::i18n::tr("fm.medium_icons"))
            .build();
        for b in [&btn_view_list, &btn_view_small, &btn_view_large] {
            b.set_cursor_from_name(Some("pointer"));
        }
        btn_view_small.set_group(Some(&btn_view_list));
        btn_view_large.set_group(Some(&btn_view_list));
        match current_size {
            30 => btn_view_list.set_active(true),
            40 => btn_view_small.set_active(true),
            _ => btn_view_large.set_active(true),
        }
        view_switcher.append(&btn_view_list);
        view_switcher.append(&btn_view_small);
        view_switcher.append(&btn_view_large);
        let view_handlers = [
            {
                let sender = sender.clone();
                btn_view_list.connect_toggled(move |b| {
                    if b.is_active() {
                        sender.input(FmPanelInput::ViewToggled(30));
                    }
                })
            },
            {
                let sender = sender.clone();
                btn_view_small.connect_toggled(move |b| {
                    if b.is_active() {
                        sender.input(FmPanelInput::ViewToggled(40));
                    }
                })
            },
            {
                let sender = sender.clone();
                btn_view_large.connect_toggled(move |b| {
                    if b.is_active() {
                        sender.input(FmPanelInput::ViewToggled(80));
                    }
                })
            },
        ];
        if with_default_toolbar {
            header.pack_end(&view_switcher);
        }
        for w in &init.toolbar_start_extras {
            header.pack_start(w);
        }
        for w in &init.toolbar_end_extras {
            header.pack_end(w);
        }

        let address_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        address_row.set_margin_start(8);
        address_row.set_margin_end(8);
        address_row.set_margin_top(2);
        address_row.set_margin_bottom(2);
        address_row.append(&nav_pill);
        address_row.append(&breadcrumbs_scroller);

        let address_entry = gtk::Entry::builder().hexpand(true).visible(false).build();
        address_entry.add_css_class("address-pill");
        address_row.append(&address_entry);

        let address_hint = gtk::Popover::builder()
            .autohide(false)
            .position(gtk::PositionType::Bottom)
            .build();
        address_hint.set_parent(&address_row);
        {
            let hint = address_hint.clone();
            address_row.connect_destroy(move |_| hint.unparent());
        }
        let address_hint_label = gtk::Label::builder()
            .margin_start(8)
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        address_hint.set_child(Some(&address_hint_label));

        {
            let sender = sender.clone();
            let gesture = gtk::GestureClick::new();
            gesture.connect_pressed(move |_, n, _, _| {
                if n == 2 {
                    sender.input(FmPanelInput::BeginAddressEdit);
                }
            });
            address_row.add_controller(gesture);
        }
        {
            let sender = sender.clone();
            address_entry.connect_activate(move |e| {
                sender.input(FmPanelInput::CommitAddressEdit(e.text().to_string()));
            });
        }
        {
            let sender = sender.clone();
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _, _| {
                if keyval == gtk::gdk::Key::Escape {
                    sender.input(FmPanelInput::CancelAddressEdit);
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            address_entry.add_controller(key);
        }
        {
            let sender = sender.clone();
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(move |_| {
                sender.input(FmPanelInput::CancelAddressEdit);
            });
            address_entry.add_controller(focus);
        }

        root.append(&address_row);
        let content_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content_box.set_vexpand(true);
        root.append(&content_box);
        content_box.append(&header);
        header.set_visible(with_default_toolbar);

        let list_store = gtk::gio::ListStore::new::<crate::file_entry::FileEntry>();
        let list_view = gtk::ColumnView::builder()
            .css_classes(vec!["boxed-list"])
            .build();
        let column_view_sorter = list_view.sorter().expect("ColumnView has a sorter");
        let sort_model = gtk::SortListModel::builder()
            .model(&list_store)
            .sorter(&column_view_sorter)
            .build();
        let selection_model = gtk::MultiSelection::new(Some(sort_model));
        list_view.set_model(Some(&selection_model));

        let item_progress_bars: Rc<RefCell<HashMap<String, gtk::ProgressBar>>> =
            Rc::new(RefCell::new(HashMap::new()));

        let (name_column, date_column, size_column) = setup_column_view(
            &list_view,
            &shared,
            &sender,
            btn_delete.clone(),
            btn_refresh.clone(),
            btn_chmod.clone(),
            list_store.clone(),
            selection_model.clone(),
        );

        let sort_col_key = if panel_id == "default" {
            "ui.fm_sort_col".to_string()
        } else {
            format!("ui.fm_sort_col_{panel_id}")
        };
        let sort_dir_key = if panel_id == "default" {
            "ui.fm_sort_dir".to_string()
        } else {
            format!("ui.fm_sort_dir_{panel_id}")
        };
        let saved_desc = config.get::<bool>(&sort_dir_key).unwrap_or(false);
        let dir = if saved_desc {
            gtk::SortType::Descending
        } else {
            gtk::SortType::Ascending
        };
        let initial_col = match config.get::<String>(&sort_col_key).as_deref() {
            Some("date") => &date_column,
            Some("size") => &size_column,
            _ => &name_column,
        };
        list_view.sort_by_column(Some(initial_col), dir);
        {
            let config_sort = config.clone();
            let name_c = name_column.clone();
            let date_c = date_column.clone();
            let size_c = size_column.clone();
            let sort_col_key = sort_col_key.clone();
            let sort_dir_key = sort_dir_key.clone();
            let sorter_weak = column_view_sorter.downgrade();
            let sender_sort = sender.clone();
            column_view_sorter.connect_changed(move |_, _| {
                let Some(s) = sorter_weak.upgrade() else {
                    return;
                };
                let Ok(cv) = s.downcast::<gtk::ColumnViewSorter>() else {
                    return;
                };
                let (col_opt, order) = cv.nth_sort_column(0);
                if let Some(col) = col_opt {
                    let name = if col == name_c {
                        "name"
                    } else if col == date_c {
                        "date"
                    } else if col == size_c {
                        "size"
                    } else {
                        return;
                    };
                    let descending = order == gtk::SortType::Descending;
                    config_sort.set(&sort_col_key, name.to_string());
                    config_sort.set(&sort_dir_key, descending);
                    config_sort.save();
                    let _ = sender_sort.output(FmPanelOutput::SortChanged {
                        column: name.to_string(),
                        descending,
                    });
                }
            });
        }

        let scrolled_list = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&list_view)
            .build();
        stack.add_named(&scrolled_list, Some("list-view"));

        let grid_factory = create_grid_factory(
            &shared,
            &sender,
            btn_delete.clone(),
            btn_refresh.clone(),
            btn_chmod.clone(),
            list_store.clone(),
            selection_model.clone(),
            init.thumbnailer.clone(),
        );
        let grid_view = gtk::GridView::builder()
            .model(&selection_model)
            .factory(&grid_factory)
            .max_columns(20)
            .min_columns(1)
            .build();
        let scrolled_grid = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&grid_view)
            .build();
        stack.add_named(&scrolled_grid, Some("grid-view"));
        if config.get::<u32>(&cfg_key).unwrap_or(80) == 30 {
            stack.set_visible_child_name("list-view");
        } else {
            stack.set_visible_child_name("grid-view");
        }

        {
            let shared_sel = shared.clone();
            let btn_delete_c = btn_delete.clone();
            let btn_rename_c = btn_rename.clone();
            let btn_chmod_c = btn_chmod.clone();
            let sender_sel = sender.clone();
            selection_model.connect_selection_changed(move |sm, _, _| {
                let mut selected = Vec::new();
                let bitset = sm.selection();
                for i in 0..bitset.size() {
                    if let Some(obj) = sm.item(bitset.nth(i as u32)) {
                        if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                            if entry.name() != ".." {
                                selected.push((entry.name(), entry.is_dir()));
                            }
                        }
                    }
                }
                let count = selected.len();
                *shared_sel.selected_files.borrow_mut() = selected.clone();
                btn_delete_c.set_sensitive(count > 0);
                btn_rename_c.set_sensitive(count == 1);
                btn_chmod_c.set_sensitive(count == 1);
                let cursor = if bitset.size() > 0 {
                    sm.item(bitset.nth(0))
                        .and_then(|o| o.downcast::<crate::file_entry::FileEntry>().ok())
                        .map(|e| e.name())
                } else {
                    None
                };
                let _ = sender_sel.output(FmPanelOutput::StateChanged {
                    path: build_path_string(&shared_sel.current_path.borrow()),
                    selected,
                    cursor,
                });
            });
        }

        {
            let sender = sender.clone();
            let shared = shared.clone();
            let sm = selection_model.clone();
            list_view.connect_activate(move |_, position| {
                if let Some(obj) = sm.item(position) {
                    if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                        handle_activate(&entry, &shared, &sender);
                    }
                }
            });
        }
        {
            let sender = sender.clone();
            let shared = shared.clone();
            let sm = selection_model.clone();
            grid_view.connect_activate(move |_, position| {
                if let Some(obj) = sm.item(position) {
                    if let Ok(entry) = obj.downcast::<crate::file_entry::FileEntry>() {
                        handle_activate(&entry, &shared, &sender);
                    }
                }
            });
        }

        for view in [
            list_view.clone().upcast::<gtk::Widget>(),
            grid_view.clone().upcast::<gtk::Widget>(),
        ] {
            let sender = sender.clone();
            let shared = shared.clone();
            let jump = gtk::EventControllerKey::new();
            jump.connect_key_pressed(move |_, keyval, _, state| {
                if state.intersects(
                    gtk::gdk::ModifierType::CONTROL_MASK
                        | gtk::gdk::ModifierType::ALT_MASK
                        | gtk::gdk::ModifierType::SUPER_MASK,
                ) {
                    return gtk::glib::Propagation::Proceed;
                }
                if let Some(ch) = keyval.to_unicode() {
                    if ch == '*' && shared.select_mask_enabled.get() {
                        sender.input(FmPanelInput::ShowSelectBar);
                        return gtk::glib::Propagation::Stop;
                    }
                    if !ch.is_control() && !ch.is_whitespace() {
                        sender.input(FmPanelInput::QuickJump(ch));
                        return gtk::glib::Propagation::Stop;
                    }
                }
                gtk::glib::Propagation::Proceed
            });
            view.add_controller(jump);
        }

        let status_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .margin_start(8)
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        status_label.add_css_class("dim-label");
        let source_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .margin_start(8)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        source_label.add_css_class("dim-label");
        let status_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .css_classes(vec!["status-bar"])
            .build();
        status_bar.append(&source_label);
        status_bar.append(&status_label);

        {
            let status_label_c = status_label.clone();
            let list_store_c = list_store.clone();
            let cached_c = shared.cached_entries.clone();
            selection_model.connect_selection_changed(move |sm, _, _| {
                update_status_bar_text(&status_label_c, sm, &list_store_c, &cached_c);
            });
        }

        let filter_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_start(8)
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(4)
            .visible(false)
            .build();
        let filter_icon = gtk::Image::from_icon_name("system-search-symbolic");
        filter_icon.set_margin_start(4);
        let filter_entry = gtk::Entry::builder()
            .placeholder_text(&*crate::i18n::tr("fm.filter_placeholder"))
            .hexpand(true)
            .build();
        let btn_close_filter = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text(&*crate::i18n::tr("fm.close_filter_tooltip"))
            .css_classes(vec!["flat"])
            .build();
        btn_close_filter.set_cursor_from_name(Some("pointer"));
        filter_bar.append(&filter_icon);
        filter_bar.append(&filter_entry);
        filter_bar.append(&btn_close_filter);
        {
            let sender = sender.clone();
            filter_entry.connect_changed(move |e| {
                sender.input(FmPanelInput::FilterChanged(e.text().to_string()));
            });
        }
        {
            let list_view_c = list_view.clone();
            let grid_view_c = grid_view.clone();
            let config_c = config.clone();
            let key_c = cfg_key.clone();
            filter_entry.connect_activate(move |_| {
                if config_c.get::<u32>(&key_c).unwrap_or(80) == 30 {
                    list_view_c.grab_focus();
                } else {
                    grid_view_c.grab_focus();
                }
            });
        }
        {
            let filter_bar_c = filter_bar.clone();
            let filter_entry_c = filter_entry.clone();
            let sender = sender.clone();
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _, _| {
                if keyval == gtk::gdk::Key::Escape {
                    filter_bar_c.set_visible(false);
                    filter_entry_c.set_text("");
                    sender.input(FmPanelInput::FilterChanged(String::new()));
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            filter_entry.add_controller(key);
        }
        {
            let filter_bar_c = filter_bar.clone();
            let filter_entry_c = filter_entry.clone();
            let sender = sender.clone();
            btn_close_filter.connect_clicked(move |_| {
                filter_bar_c.set_visible(false);
                filter_entry_c.set_text("");
                sender.input(FmPanelInput::FilterChanged(String::new()));
            });
        }

        let select_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_start(8)
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(4)
            .visible(false)
            .build();
        let select_icon = gtk::Image::from_icon_name("edit-select-all-symbolic");
        select_icon.set_margin_start(4);
        let select_entry = gtk::Entry::builder()
            .placeholder_text(&*crate::i18n::tr("fm.select_mask_placeholder"))
            .hexpand(true)
            .build();
        let btn_close_select = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text(&*crate::i18n::tr("fm.close_filter_tooltip"))
            .css_classes(vec!["flat"])
            .build();
        btn_close_select.set_cursor_from_name(Some("pointer"));
        select_bar.append(&select_icon);
        select_bar.append(&select_entry);
        select_bar.append(&btn_close_select);
        {
            let sender = sender.clone();
            let select_bar_c = select_bar.clone();
            select_entry.connect_activate(move |e| {
                sender.input(FmPanelInput::ApplyMaskSelection(e.text().to_string()));
                select_bar_c.set_visible(false);
            });
        }
        {
            let select_bar_c = select_bar.clone();
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _, _| {
                if keyval == gtk::gdk::Key::Escape {
                    select_bar_c.set_visible(false);
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            select_entry.add_controller(key);
        }
        {
            let select_bar_c = select_bar.clone();
            btn_close_select.connect_clicked(move |_| {
                select_bar_c.set_visible(false);
            });
        }

        content_box.append(&stack);
        content_box.append(&filter_bar);
        content_box.append(&select_bar);
        content_box.append(&status_bar);

        {
            let drop_target = gtk::DropTarget::new(
                gtk::gdk::FileList::static_type(),
                gtk::gdk::DragAction::COPY,
            );
            let sender = sender.clone();
            let shared = shared.clone();
            drop_target.connect_drop(move |_, value, _, _| {
                if let Ok(file_list) = value.get::<gtk::gdk::FileList>() {
                    let files: Vec<std::path::PathBuf> = file_list
                        .files()
                        .into_iter()
                        .filter_map(|f| f.path())
                        .collect();
                    if files.is_empty() {
                        return false;
                    }
                    let dir = build_path_string(&shared.current_path.borrow());
                    let _ = sender.output(FmPanelOutput::UploadFiles { dir, files });
                    return true;
                }
                false
            });
            root.add_controller(drop_target);
        }

        {
            let css = gtk::CssProvider::new();
            css.load_from_string(
                ".dialog-drive-box { padding: 6px 12px; background-color: alpha(currentColor, 0.05); border-radius: 8px; border: 1px solid alpha(currentColor, 0.1); } .dialog-drive-label { font-weight: bold; }",
            );
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &css,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
        }

        {
            let sender = sender.clone();
            let shared = shared.clone();
            let sm = selection_model.clone();
            btn_rename.connect_clicked(move |btn| {
                let window = btn.root().and_downcast::<gtk::Window>();
                crate::dialogs::show_rename_dialog(window.as_ref(), &shared, &sm, sender.clone());
            });
        }

        let views = Views {
            rendered_crumbs: std::cell::RefCell::new((String::new(), Vec::new())),
            stack,
            list_view: list_view.clone(),
            grid_view,
            list_store,
            selection_model,
            breadcrumbs_box,
            source_label,
            status_label,
            item_progress_bars,
            filter_bar: filter_bar.clone(),
            filter_entry: filter_entry.clone(),
            select_bar: select_bar.clone(),
            select_entry: select_entry.clone(),
            breadcrumbs_scroller,
            address_entry,
            address_hint,
            address_hint_label,
            error_label,
        };

        let model = FmPanelModel {
            panel_id,
            show_toolbar: with_default_toolbar,
            config,
            shared,
            loading: false,
            error_message: None,
            can_hist_back: false,
            can_hist_forward: false,
            breadcrumb: Vec::new(),
            is_root: true,
            select_name: None,
            listing_generation: 0,
            view_mode_reflect: None,
            sort_reflect: None,
            address_row: address_row.clone(),
            content_box: content_box.clone(),
            views,
        };

        let widgets = FmPanelWidgets {
            header,
            btn_up,
            btn_up_sep,
            btn_hist_back,
            btn_hist_forward,
            list_view,
            name_column,
            date_column,
            size_column,
            btn_view_list,
            btn_view_small,
            btn_view_large,
            view_handlers,
            rendered_generation: 0,
        };

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            FmPanelInput::AddAddressEndWidget(widget) => {
                self.address_row.append(&widget);
            }
            FmPanelInput::SetContentVisible(visible) => {
                self.content_box.set_visible(visible);
            }
            FmPanelInput::Listing {
                path,
                entries,
                breadcrumb,
                source,
                select_name,
            } => {
                *self.shared.editing_name.borrow_mut() = None;
                let parts: Vec<String> = path
                    .split(['/', '\\'])
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();

                let mut exited = None;
                {
                    let old = self.shared.current_path.borrow();
                    if old.len() > parts.len() && old.starts_with(&parts) {
                        if let Some(seg) = old.get(parts.len()) {
                            exited = Some(seg.clone());
                        }
                    }
                }
                self.select_name = select_name.or(exited);

                let display: Vec<(String, bool, u64, String, Option<u32>)> = entries
                    .iter()
                    .map(|e| {
                        let date = match chrono::Local.timestamp_opt(e.modified as i64, 0) {
                            chrono::LocalResult::Single(dt) => {
                                dt.format("%Y-%m-%d %H:%M:%S").to_string()
                            }
                            _ => "N/A".to_string(),
                        };
                        (e.name.clone(), e.is_dir, e.size, date, e.permissions)
                    })
                    .collect();

                *self.shared.current_path.borrow_mut() = parts.clone();
                *self.shared.cached_entries.borrow_mut() = display;
                let fs_label = source.fs_label.clone();
                *self.shared.source.borrow_mut() = source;
                self.breadcrumb = breadcrumb;
                self.views
                    .source_label
                    .set_text(&fs_label.map(|l| format!("{l},")).unwrap_or_default());

                let root_str = self.shared.root_path.borrow().clone();
                let root_parts: Vec<String> = root_str
                    .split(['/', '\\'])
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                self.is_root = if parts.starts_with(&root_parts) {
                    parts.len() <= root_parts.len()
                } else {
                    parts.is_empty()
                };

                self.error_message = None;
                self.loading = false;
                self.listing_generation += 1;
                self.emit_state_changed(&sender);
            }
            FmPanelInput::Loading => {
                *self.shared.editing_name.borrow_mut() = None;
                self.views.list_store.remove_all();
                self.shared.cached_entries.borrow_mut().clear();
                self.error_message = None;
                self.loading = true;
            }
            FmPanelInput::LoadFailed { message } => {
                self.loading = false;
                self.error_message = Some(message);
            }
            FmPanelInput::OpFailed { title, message } => {
                let root = self.views.stack.root().and_downcast::<gtk::Window>();
                crate::dialogs::present_error(root.as_ref(), &title, &message);
            }
            FmPanelInput::EntryAdded { name, is_dir } => self.patch_entry_added(&name, is_dir),
            FmPanelInput::EntriesRemoved { names } => self.patch_entries_removed(&names),
            FmPanelInput::EntryRenamed { old, new } => self.patch_entry_renamed(&old, &new),
            FmPanelInput::SetItemProgress { name, fraction } => {
                self.set_item_progress(&name, fraction)
            }
            FmPanelInput::SetHistory {
                can_back,
                can_forward,
            } => {
                self.can_hist_back = can_back;
                self.can_hist_forward = can_forward;
            }
            FmPanelInput::SetViewMode(mode) => {
                let size = match mode.as_str() {
                    "list" => 30u32,
                    "small" => 40,
                    _ => 80,
                };
                self.config.set(&self.config_key(), size);
                self.config.save();
                self.view_mode_reflect = Some(mode);
                self.listing_generation += 1;
            }
            FmPanelInput::SetSort { column, descending } => {
                self.sort_reflect = Some((column, descending));
            }
            FmPanelInput::StartRename => self.start_rename(),
            FmPanelInput::RequestCreateDir => {
                let window = self.views.list_view.root().and_downcast::<gtk::Window>();
                crate::dialogs::show_create_dir_dialog(window.as_ref(), &self.shared, sender.clone());
            }
            FmPanelInput::RequestDelete => {
                let window = self.views.list_view.root().and_downcast::<gtk::Window>();
                crate::dialogs::show_delete_dialog(window.as_ref(), &self.shared, sender.clone());
            }
            FmPanelInput::CancelEditing => self.cancel_editing(),
            FmPanelInput::ShowFilterBar => {
                self.views.filter_bar.set_visible(true);
                self.views.filter_entry.grab_focus();
            }
            FmPanelInput::ShowSelectBar => {
                self.views.select_bar.set_visible(true);
                self.views.select_entry.set_text("*.*");
                self.views.select_entry.grab_focus();
            }
            FmPanelInput::QuickJump(ch) => self.quick_jump(ch),
            FmPanelInput::ApplyMaskSelection(mask) => self.apply_mask_selection(&mask),
            FmPanelInput::ReRender => {
                self.listing_generation += 1;
            }
            FmPanelInput::SelectByName(name) => {
                self.select_name = Some(name);
                self.listing_generation += 1;
            }
            FmPanelInput::GrabFocus => {
                let is_list = self.config.get::<u32>(&self.config_key()).unwrap_or(80) == 30;
                if is_list {
                    self.views.list_view.grab_focus();
                } else {
                    self.views.grid_view.grab_focus();
                }
            }
            FmPanelInput::BeginAddressEdit => {
                if !WidgetExt::is_visible(&self.views.address_entry) {
                    let path = build_path_string(&self.shared.current_path.borrow());
                    self.views.address_entry.set_text(&path);
                    self.views.breadcrumbs_scroller.set_visible(false);
                    self.views.address_entry.set_visible(true);
                    let entry = self.views.address_entry.clone();
                    gtk::glib::timeout_add_local_once(
                        std::time::Duration::from_millis(50),
                        move || {
                            entry.grab_focus();
                            entry.set_position(-1); // cursor at end — edit by append/trim
                        },
                    );
                }
            }
            FmPanelInput::CancelAddressEdit => {
                if WidgetExt::is_visible(&self.views.address_entry) {
                    self.views.address_entry.set_visible(false);
                    self.views.breadcrumbs_scroller.set_visible(true);
                    sender.input(FmPanelInput::GrabFocus);
                }
            }
            FmPanelInput::CommitAddressEdit(text) => {
                sender.input(FmPanelInput::CancelAddressEdit);
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() {
                    let _ = sender.output(FmPanelOutput::NavigateTyped(trimmed));
                }
            }
            FmPanelInput::AddressNotResolved => {
                self.views.address_hint_label.set_text(&crate::i18n::tr("fm.address_not_found"));
                self.views.address_hint.popup();
                let hint = self.views.address_hint.clone();
                gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                    hint.popdown();
                });
            }
            FmPanelInput::ViewToggled(size) => {
                self.config.set(&self.config_key(), size);
                self.config.save();
                self.listing_generation += 1;
                let mode = match size {
                    30 => "list",
                    40 => "small",
                    _ => "large",
                };
                let _ = sender.output(FmPanelOutput::ViewModeChanged(mode.to_string()));
            }
            FmPanelInput::FilterChanged(query) => {
                filter_entries(self, &query);
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        widgets.header.set_visible(self.show_toolbar);
        widgets.btn_hist_back.set_sensitive(self.can_hist_back);
        widgets
            .btn_hist_forward
            .set_sensitive(self.can_hist_forward);
        widgets.btn_up.set_sensitive(!self.is_root);
        widgets.btn_up_sep.set_visible(true);

        if let Some(msg) = &self.error_message {
            self.views.error_label.set_text(msg);
            self.views.stack.set_visible_child_name("error");
        } else if self.loading {
            self.views.stack.set_visible_child_name("loading");
        }

        if let Some(mode) = &self.view_mode_reflect {
            let (btn, handler) = match mode.as_str() {
                "list" => (&widgets.btn_view_list, &widgets.view_handlers[0]),
                "small" => (&widgets.btn_view_small, &widgets.view_handlers[1]),
                _ => (&widgets.btn_view_large, &widgets.view_handlers[2]),
            };
            btn.block_signal(handler);
            btn.set_active(true);
            btn.unblock_signal(handler);
        }

        if let Some((column, descending)) = &self.sort_reflect {
            let col = match column.as_str() {
                "date" => &widgets.date_column,
                "size" => &widgets.size_column,
                _ => &widgets.name_column,
            };
            let dir = if *descending {
                gtk::SortType::Descending
            } else {
                gtk::SortType::Ascending
            };
            widgets.list_view.sort_by_column(Some(col), dir);
        }

        if widgets.rendered_generation != self.listing_generation {
            widgets.rendered_generation = self.listing_generation;
            render_listing(self, &sender);
        }
    }
}

fn render_breadcrumbs(model: &FmPanelModel, sender: &ComponentSender<FmPanelModel>) {
    let bc = &model.views.breadcrumbs_box;
    let segs = &model.breadcrumb;

    let source_key = {
        let s = model.shared.source.borrow();
        format!(
            "{}|{}|{}",
            s.display_name.clone().unwrap_or_default(),
            s.root_icon,
            s.root_icon_svg.clone().unwrap_or_default()
        )
    };

    let (prev_key, prev_segs) = model.views.rendered_crumbs.borrow().clone();
    let source_changed = prev_key != source_key;

    let keep = if source_changed {
        0
    } else {
        let common = segs
            .iter()
            .zip(prev_segs.iter())
            .take_while(|(a, b)| a == b)
            .count();
        common
            .min(segs.len().saturating_sub(1))
            .min(prev_segs.len().saturating_sub(1))
    };

    if source_changed {
        while let Some(child) = bc.first_child() {
            bc.remove(&child);
        }
        append_root_chip(model, sender, bc);
    } else {
        let mut child = bc.last_child();
        let mut have = prev_segs.len();
        while have > keep {
            if let Some(c) = child {
                child = c.prev_sibling();
                bc.remove(&c);
                have -= 1;
            } else {
                break;
            }
        }
    }

    for (i, segment) in segs.iter().enumerate().skip(keep) {
        bc.append(&build_crumb(segment, i, segs.len(), sender));
    }

    *model.views.rendered_crumbs.borrow_mut() = (source_key, segs.clone());
}

fn append_root_chip(
    model: &FmPanelModel,
    sender: &ComponentSender<FmPanelModel>,
    bc: &gtk::Box,
) {
    let flag_img = gtk::Image::from_resource("/com/fm-ui/gtk/start.svg");
    flag_img.set_pixel_size(16);
    let flag_btn = gtk::Button::builder()
        .child(&flag_img)
        .css_classes(vec!["flat"])
        .tooltip_text(&*crate::i18n::tr("fm.go_to_start"))
        .build();
    flag_btn.set_cursor_from_name(Some("pointer"));
    {
        let sender = sender.clone();
        flag_btn.connect_clicked(move |_| {
            let _ = sender.output(FmPanelOutput::Start);
        });
    }
    bc.append(&flag_btn);

    let source = model.shared.source.borrow();
    let root_icon = if source.root_icon.is_empty() {
        "/com/fm-ui/gtk/home.svg".to_string()
    } else {
        source.root_icon.clone()
    };
    let root_img = match source
        .root_icon_svg
        .clone()
        .and_then(|svg| crate::view_factories::generated_svg_paintable(svg, 16))
    {
        Some(p) => gtk::Image::from_paintable(Some(&p)),
        None => gtk::Image::from_resource(&root_icon),
    };
    root_img.set_pixel_size(16);
    let root_child: gtk::Widget = if let Some(name) = source.display_name.clone() {
        let chip = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .build();
        chip.append(&root_img);
        chip.append(
            &gtk::Label::builder()
                .label(&name)
                .margin_start(2)
                .margin_end(2)
                .build(),
        );
        chip.upcast()
    } else {
        root_img.upcast()
    };
    drop(source);

    let root_btn = gtk::Button::builder()
        .child(&root_child)
        .css_classes(vec!["flat"])
        .tooltip_text(&*crate::i18n::tr("fm.root_tooltip"))
        .build();
    root_btn.set_cursor_from_name(Some("pointer"));
    {
        let sender = sender.clone();
        root_btn.connect_clicked(move |_| {
            let _ = sender.output(FmPanelOutput::NavigateLevel(0));
        });
    }
    bc.append(&root_btn);
}

fn build_crumb(
    segment: &BreadcrumbSegment,
    index: usize,
    total: usize,
    sender: &ComponentSender<FmPanelModel>,
) -> gtk::Widget {
    let holder = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .build();
    holder.append(
        &gtk::Label::builder()
            .label("/")
            .css_classes(vec!["dim-label"])
            .build(),
    );

    let seg_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .build();
    let img = match segment
        .icon_svg
        .clone()
        .and_then(|svg| crate::view_factories::generated_svg_paintable(svg, 16))
    {
        Some(p) => gtk::Image::from_paintable(Some(&p)),
        None => gtk::Image::from_resource(&segment.icon),
    };
    img.set_pixel_size(16);
    seg_box.append(&img);
    seg_box.append(
        &gtk::Label::builder()
            .label(&segment.name)
            .margin_start(2)
            .margin_end(6)
            .build(),
    );

    if index == total - 1 {
        seg_box.set_margin_start(9);
        seg_box.set_margin_end(9);
        holder.append(&seg_box);
    } else {
        let btn = gtk::Button::builder()
            .child(&seg_box)
            .css_classes(vec!["flat"])
            .build();
        btn.set_cursor_from_name(Some("pointer"));
        let level_idx = index + 1;
        let sender = sender.clone();
        btn.connect_clicked(move |_| {
            let _ = sender.output(FmPanelOutput::NavigateLevel(level_idx));
        });
        holder.append(&btn);
    }
    holder.upcast()
}

fn handle_activate(
    entry: &crate::file_entry::FileEntry,
    shared: &Shared,
    sender: &ComponentSender<FmPanelModel>,
) {
    if entry.name() == ".." {
        let cur = build_path_string(&shared.current_path.borrow());
        let root = shared.root_path.borrow().clone();
        if cur == root || cur == "/" {
            let _ = sender.output(FmPanelOutput::Back);
        } else {
            let _ = sender.output(FmPanelOutput::NavigateUp);
        }
        return;
    }
    let lower = entry.name().to_lowercase();
    let is_archive = lower.ends_with(".zip")
        || lower.ends_with(".tar")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar.bz2")
        || lower.ends_with(".tbz2")
        || lower.ends_with(".tbz");
    if entry.is_dir() || is_archive {
        let _ = sender.output(FmPanelOutput::NavigateEnter(entry.name()));
    } else {
        let mut target = shared.current_path.borrow().clone();
        target.push(entry.name());
        let _ = sender.output(FmPanelOutput::ActivateFile {
            path: build_path_string(&target),
        });
    }
}

fn filter_entries(model: &FmPanelModel, query: &str) {
    let store = &model.views.list_store;
    store.remove_all();
    let q = query.to_lowercase();
    let cached = model.shared.cached_entries.borrow();
    let current = model.shared.current_path.borrow();
    let mut new_items = Vec::new();
    for (name, is_dir, size, date, perms) in cached.iter() {
        if model.should_skip_entry(name) {
            continue;
        }
        if query.is_empty() || name.to_lowercase().contains(&q) {
            let mut full = current.clone();
            full.push(name.clone());
            let entry = crate::file_entry::FileEntry::new(
                name,
                &build_path_string(&full),
                *is_dir,
                *size,
                date,
                *perms,
            );
            new_items.push(entry.upcast::<gtk::glib::Object>());
        }
    }
    store.splice(0, 0, &new_items);
    update_status_bar_text(
        &model.views.status_label,
        &model.views.selection_model,
        store,
        &model.shared.cached_entries,
    );
}

fn render_listing(model: &FmPanelModel, sender: &ComponentSender<FmPanelModel>) {
    let parts = model.shared.current_path.borrow().clone();
    let is_root = model.is_root;
    let icon_size = model.icon_size();

    if icon_size == 30 {
        model.views.stack.set_visible_child_name("list-view");
    } else {
        model.views.stack.set_visible_child_name("grid-view");
    }

    render_breadcrumbs(model, sender);

    let store = &model.views.list_store;
    store.remove_all();
    model.views.item_progress_bars.borrow_mut().clear();

    let mut entries: Vec<(String, bool, u64, String, Option<u32>)> = model
        .shared
        .cached_entries
        .borrow()
        .iter()
        .filter(|(name, _, _, _, _)| !model.should_skip_entry(name))
        .cloned()
        .collect();
    if !is_root && icon_size == 30 {
        entries.insert(0, ("..".to_string(), true, 0, String::new(), None));
    }

    let sm = &model.views.selection_model;
    let list_view = &model.views.list_view;
    let grid_view = &model.views.grid_view;
    let target_select = model.select_name.clone();

    let build_item = |name: &str, is_dir: bool, size: u64, date: &str, perms: Option<u32>| {
        let mut full = parts.clone();
        if name == ".." {
            if !full.is_empty() {
                full.pop();
            }
        } else {
            full.push(name.to_string());
        }
        crate::file_entry::FileEntry::new(
            name,
            &build_path_string(&full),
            is_dir,
            size,
            date,
            perms,
        )
        .upcast::<gtk::glib::Object>()
    };

    if entries.len() <= 300 {
        let items: Vec<gtk::glib::Object> = entries
            .iter()
            .map(|(n, d, s, dt, p)| build_item(n, *d, *s, dt, *p))
            .collect();
        store.splice(0, 0, &items);

        let mut select_pos = 0;
        if let Some(ref want) = target_select {
            let n = sm.n_items();
            for i in 0..n {
                if let Some(obj) = sm.item(i) {
                    if let Ok(e) = obj.downcast::<crate::file_entry::FileEntry>() {
                        if &e.name() == want {
                            select_pos = i;
                            break;
                        }
                    }
                }
            }
        }
        sm.select_item(select_pos, true);

        let focus_in_editor = list_view
            .root()
            .and_then(|r| gtk::prelude::RootExt::focus(&r))
            .map(|w| w.is::<gtk::Text>() || w.is::<gtk::Entry>())
            .unwrap_or(false);

        scroll_and_focus(
            icon_size,
            select_pos,
            target_select.is_some(),
            focus_in_editor,
            list_view,
            grid_view,
        );
        update_status_bar_text(
            &model.views.status_label,
            sm,
            store,
            &model.shared.cached_entries,
        );
    } else {
        let generation = model.listing_generation;
        let gen_cell = Rc::new(Cell::new(generation));
        let _ = &gen_cell;
        let sm = sm.clone();
        let store = store.clone();
        let list_view = list_view.clone();
        let grid_view = grid_view.clone();
        let status_label = model.views.status_label.clone();
        let cached = model.shared.cached_entries.clone();
        let parts_c = parts.clone();
        let idx = Rc::new(Cell::new(0usize));
        let selected_once = Rc::new(Cell::new(false));
        gtk::glib::idle_add_local_full(gtk::glib::Priority::default(), move || {
            let start = idx.get();
            if start >= entries.len() {
                if let Some(ref want) = target_select {
                    let n = sm.n_items();
                    for i in 0..n {
                        if let Some(obj) = sm.item(i) {
                            if let Ok(e) = obj.downcast::<crate::file_entry::FileEntry>() {
                                if &e.name() == want {
                                    sm.select_item(i, true);
                                    if icon_size == 30 {
                                        list_view.scroll_to(
                                            i,
                                            None,
                                            gtk::ListScrollFlags::FOCUS,
                                            None::<gtk::ScrollInfo>,
                                        );
                                    } else {
                                        grid_view.scroll_to(
                                            i,
                                            gtk::ListScrollFlags::FOCUS,
                                            None::<gtk::ScrollInfo>,
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                update_status_bar_text(&status_label, &sm, &store, &cached);
                return gtk::glib::ControlFlow::Break;
            }
            let end = std::cmp::min(start + 100, entries.len());
            let mut items = Vec::with_capacity(end - start);
            for (name, is_dir, size, date, perms) in &entries[start..end] {
                let mut full = parts_c.clone();
                if name == ".." {
                    if !full.is_empty() {
                        full.pop();
                    }
                } else {
                    full.push(name.clone());
                }
                items.push(
                    crate::file_entry::FileEntry::new(
                        name,
                        &build_path_string(&full),
                        *is_dir,
                        *size,
                        date,
                        *perms,
                    )
                    .upcast::<gtk::glib::Object>(),
                );
            }
            store.splice(start as u32, 0, &items);
            if start == 0 && !selected_once.get() {
                sm.select_item(0, true);
                selected_once.set(true);
                if icon_size == 30 {
                    list_view.grab_focus();
                } else {
                    grid_view.grab_focus();
                }
            }
            idx.set(end);
            gtk::glib::ControlFlow::Continue
        });
    }
}

fn scroll_and_focus(
    icon_size: u32,
    pos: u32,
    has_target: bool,
    focus_in_editor: bool,
    list_view: &gtk::ColumnView,
    grid_view: &gtk::GridView,
) {
    if icon_size == 30 {
        if has_target {
            if let Some(adj) = list_view.vadjustment() {
                adj.set_value(adj.upper());
            }
        }
        list_view.scroll_to(
            pos,
            None,
            gtk::ListScrollFlags::FOCUS,
            None::<gtk::ScrollInfo>,
        );
        if !focus_in_editor {
            list_view.grab_focus();
        }
        if has_target {
            let lv = list_view.clone();
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                if let Some(adj) = lv.vadjustment() {
                    let target = (adj.value() - adj.page_size() / 2.0)
                        .clamp(0.0, adj.upper() - adj.page_size());
                    adj.set_value(target);
                }
                gtk::glib::ControlFlow::Break
            });
        }
    } else {
        if has_target {
            if let Some(adj) = grid_view.vadjustment() {
                adj.set_value(adj.upper());
            }
        }
        grid_view.scroll_to(pos, gtk::ListScrollFlags::FOCUS, None::<gtk::ScrollInfo>);
        if !focus_in_editor {
            grid_view.grab_focus();
        }
        if has_target {
            let gv = grid_view.clone();
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                if let Some(adj) = gv.vadjustment() {
                    let target = (adj.value() - adj.page_size() / 2.0)
                        .clamp(0.0, adj.upper() - adj.page_size());
                    adj.set_value(target);
                }
                gtk::glib::ControlFlow::Break
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_without_wildcards_must_match_the_whole_name() {
        assert!(glob_match("readme.md", "readme.md"));
        assert!(!glob_match("readme", "readme.md"));
        assert!(!glob_match("readme.md", "readme"));
    }

    #[test]
    fn matching_ignores_case_on_both_sides() {
        assert!(glob_match("*.TXT", "notes.txt"));
        assert!(glob_match("*.txt", "NOTES.TXT"));
        assert!(glob_match("ReAdMe*", "readme.md"));
    }

    #[test]
    fn a_lone_star_matches_every_name_including_the_empty_one() {
        assert!(glob_match("*", "anything.tar.gz"));
        assert!(glob_match("*", ""));
        assert!(glob_match("**", "abc"));
    }

    #[test]
    fn an_empty_pattern_matches_only_an_empty_name() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "a"));
    }

    #[test]
    fn question_mark_matches_exactly_one_character() {
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(!glob_match("a?c", "abbc"));
        assert!(!glob_match("?", ""));
    }

    #[test]
    fn a_trailing_star_may_match_nothing() {
        assert!(glob_match("abc*", "abc"));
        assert!(glob_match("abc***", "abc"));
        assert!(glob_match("abc*", "abcdef"));
    }

    #[test]
    fn several_stars_match_in_order() {
        assert!(glob_match("*a*b*", "xxaybz"));
        assert!(!glob_match("*a*b*", "xxbyaz"));
        assert!(glob_match("v*.tar.*", "v1.2.tar.gz"));
    }

    #[test]
    fn a_star_backtracks_to_find_a_later_match() {
        assert!(glob_match("*ab", "aab"));
        assert!(!glob_match("*ab", "aba"));
        assert!(glob_match("*ab*ab", "abab"));
    }

    #[test]
    fn matching_is_character_based_and_survives_unicode() {
        assert!(glob_match("*отчёт*", "мой отчёт.pdf"));
        assert!(glob_match("?.mp3", "🎧.mp3"));
        assert!(!glob_match("?.mp3", "ab.mp3"));
    }

    #[test]
    fn breadcrumbs_from_an_empty_path_are_empty() {
        assert!(BreadcrumbSegment::from_segments(&[]).is_empty());
    }

    #[test]
    fn breadcrumbs_keep_segment_order_and_get_the_folder_icon() {
        let segments = vec![
            PathSegment {
                name: "home".to_string(),
                path: "/home".to_string(),
            },
            PathSegment {
                name: "ice".to_string(),
                path: "/home/ice".to_string(),
            },
        ];
        let crumbs = BreadcrumbSegment::from_segments(&segments);
        assert_eq!(crumbs.len(), 2);
        assert_eq!(crumbs[0].name, "home");
        assert_eq!(crumbs[0].path, "/home");
        assert_eq!(crumbs[1].name, "ice");
        assert_eq!(crumbs[1].path, "/home/ice");
        assert!(crumbs.iter().all(|c| c.icon == "/com/fm-ui/gtk/folder.svg"));
        assert!(crumbs.iter().all(|c| c.icon_svg.is_none()));
    }
}
