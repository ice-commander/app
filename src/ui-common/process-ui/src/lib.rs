#![allow(deprecated)] // adw::MessageDialog — modernization tracked separately

mod i18n;

use adw::prelude::*;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub memory: u64,
    pub cpu_usage: f32,
}
use gtk::glib;
use relm4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Once;

static RESOURCES_INIT: Once = Once::new();

pub fn init_resources() {
    RESOURCES_INIT.call_once(|| {
        gtk::gio::resources_register_include!("process.gresource")
            .expect("Failed to register process-ui resources");
    });
}

fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Pid,
    Name,
    Memory,
    Cpu,
}

pub struct ProcessInit {
    pub show_toolbar: bool,
}

impl Default for ProcessInit {
    fn default() -> Self {
        Self { show_toolbar: true }
    }
}

#[derive(Debug)]
pub enum ProcessInput {
    Update(Vec<ProcessInfo>),
    Clear,
    SortBy(SortColumn),
    Select(Option<(u32, String)>),
    RequestRefresh,
}

#[derive(Debug)]
pub enum ProcessOutput {
    RequestList,
    Kill(u32),
    Back,
}

pub struct ProcessModel {
    show_toolbar: bool,
    processes: Vec<ProcessInfo>,
    sort_column: SortColumn,
    ascending: bool,
    selected: Option<(u32, String)>,
    selected_mirror: Rc<RefCell<Option<(u32, String)>>>,
    list_generation: u64,
}

impl ProcessModel {
    fn set_selected(&mut self, sel: Option<(u32, String)>) {
        *self.selected_mirror.borrow_mut() = sel.clone();
        self.selected = sel;
    }
}

pub struct ProcessWidgets {
    header: adw::HeaderBar,
    status_lbl: gtk::Label,
    btn_kill: gtk::Button,
    list_box: gtk::ListBox,
    row_selected_handler: glib::SignalHandlerId,
    rendered_generation: u64,
}

impl SimpleComponent for ProcessModel {
    type Init = ProcessInit;
    type Input = ProcessInput;
    type Output = ProcessOutput;
    type Root = gtk::Box;
    type Widgets = ProcessWidgets;

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        crate::init_resources();

        let empty_title = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .title_widget(&empty_title)
            .build();

        let status_lbl = gtk::Label::builder()
            .label(&*crate::i18n::trf("process.active_tasks_count", &[("count", &*(0).to_string())]))
            .halign(gtk::Align::Center)
            .margin_start(12)
            .margin_end(12)
            .css_classes(vec!["dim-label"])
            .build();

        let btn_back = gtk::Button::builder()
            .child(&gtk::Image::from_resource(
                "/com/process-ui/gtk/arrow-left.svg",
            ))
            .tooltip_text(&*crate::i18n::tr("process.back_to_services"))
            .build();
        btn_back.set_cursor_from_name(Some("pointer"));
        let sender_back = sender.clone();
        btn_back.connect_clicked(move |_| {
            let _ = sender_back.output(ProcessOutput::Back);
        });
        header.pack_start(&btn_back);

        header.pack_start(
            &gtk::Separator::builder()
                .orientation(gtk::Orientation::Vertical)
                .margin_top(8)
                .margin_bottom(8)
                .build(),
        );

        header.pack_end(&status_lbl);

        let refresh_btn = gtk::Button::builder()
            .child(&gtk::Image::from_resource(
                "/com/process-ui/gtk/refresh.svg",
            ))
            .tooltip_text(&*crate::i18n::tr("process.refresh_list"))
            .build();
        refresh_btn.set_cursor_from_name(Some("pointer"));
        let sender_refresh = sender.clone();
        refresh_btn.connect_clicked(move |_| {
            let _ = sender_refresh.output(ProcessOutput::RequestList);
        });
        header.pack_start(&refresh_btn);

        let kill_icon = gtk::Image::from_resource("/com/process-ui/gtk/close.svg");
        kill_icon.set_pixel_size(16);
        let btn_kill = gtk::Button::builder().child(&kill_icon).build();
        btn_kill.set_cursor_from_name(Some("pointer"));
        btn_kill.set_tooltip_text(Some(&*crate::i18n::tr("process.kill_process_tooltip")));
        btn_kill.add_css_class("destructive-action");
        btn_kill.set_sensitive(false);
        header.pack_start(&btn_kill);

        header.pack_start(&gtk::Separator::new(gtk::Orientation::Vertical));

        header.set_visible(init.show_toolbar);
        root.append(&header);

        let selected_mirror: Rc<RefCell<Option<(u32, String)>>> = Rc::new(RefCell::new(None));
        let mirror_kill = selected_mirror.clone();
        let sender_kill = sender.clone();
        btn_kill.connect_clicked(move |btn| {
            let Some((pid, name)) = mirror_kill.borrow().clone() else {
                return;
            };
            let Some(window) = btn.root().and_downcast::<gtk::Window>() else {
                return;
            };
            let dialog = adw::MessageDialog::builder()
                .heading(&*crate::i18n::tr("process.terminate_dialog_title"))
                .body(&*crate::i18n::trf("process.terminate_dialog_body", &[("name", &*(name).to_string()), ("pid", &*(pid).to_string())]))
                .transient_for(&window)
                .modal(true)
                .build();

            dialog.add_response("cancel", &crate::i18n::tr("common_forms.cancel"));
            dialog.add_response("kill", &crate::i18n::tr("process.kill_btn"));
            dialog.set_response_appearance("kill", adw::ResponseAppearance::Destructive);

            let sender_dialog = sender_kill.clone();
            dialog.connect_response(None, move |dlg, response| {
                if response == "kill" {
                    let _ = sender_dialog.output(ProcessOutput::Kill(pid));
                }
                dlg.destroy();
            });

            dialog.show();
        });

        let content_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();

        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text(&*crate::i18n::tr("process.search_placeholder"))
            .build();
        search_entry.set_hexpand(true);
        search_entry.set_margin_bottom(12);
        content_box.append(&search_entry);

        let table_header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_start(16)
            .margin_end(40)
            .margin_top(8)
            .margin_bottom(4)
            .build();

        let column_header = |text: &str| -> gtk::Button {
            let lbl = gtk::Label::builder()
                .label(format!("<b>{text}</b>"))
                .use_markup(true)
                .build();
            let btn = gtk::Button::builder()
                .child(&lbl)
                .css_classes(vec!["flat"])
                .build();
            btn.set_cursor_from_name(Some("pointer"));
            btn
        };

        let pid_th = column_header(&crate::i18n::tr("process.pid"));
        pid_th.set_width_request(60);
        pid_th.set_halign(gtk::Align::Center);

        let name_th = column_header(&crate::i18n::tr("process.name"));
        name_th.set_hexpand(true);
        name_th.set_halign(gtk::Align::Start);

        let mem_th = column_header(&crate::i18n::tr("process.memory"));
        mem_th.set_width_request(100);
        mem_th.set_halign(gtk::Align::Center);

        let cpu_th = column_header(&crate::i18n::tr("process.cpu"));
        cpu_th.set_width_request(120);
        cpu_th.set_halign(gtk::Align::Center);

        for (btn, col) in [
            (&pid_th, SortColumn::Pid),
            (&name_th, SortColumn::Name),
            (&mem_th, SortColumn::Memory),
            (&cpu_th, SortColumn::Cpu),
        ] {
            let sender_sort = sender.clone();
            btn.connect_clicked(move |_| sender_sort.input(ProcessInput::SortBy(col)));
        }

        table_header.append(&pid_th);
        table_header.append(&name_th);
        table_header.append(&mem_th);
        table_header.append(&cpu_th);
        content_box.append(&table_header);

        let list_box = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(vec!["boxed-list", "rich-list"])
            .build();

        let search_clone = search_entry.clone();
        list_box.set_filter_func(move |row| {
            let search_text = search_clone.text().to_string().to_lowercase();
            if search_text.is_empty() {
                return true;
            }
            let widget_name = row.widget_name().to_string().to_lowercase();
            widget_name.contains(&search_text)
        });

        let list_box_filter = list_box.clone();
        search_entry.connect_search_changed(move |_| {
            list_box_filter.invalidate_filter();
        });

        let sender_select = sender.clone();
        let row_selected_handler = list_box.connect_row_selected(move |_, row_opt| {
            sender_select.input(ProcessInput::Select(row_opt.and_then(parse_row_name)));
        });

        let scrolled_window = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .margin_top(16)
            .child(&list_box)
            .build();

        content_box.append(&scrolled_window);
        root.append(&content_box);

        let model = ProcessModel {
            show_toolbar: init.show_toolbar,
            processes: Vec::new(),
            sort_column: SortColumn::Name,
            ascending: true,
            selected: None,
            selected_mirror,
            list_generation: 1,
        };
        let widgets = ProcessWidgets {
            header,
            status_lbl,
            btn_kill,
            list_box,
            row_selected_handler,
            rendered_generation: 0,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            ProcessInput::Update(list) => {
                self.processes = list;
                sort_processes(&mut self.processes, self.sort_column, self.ascending);
                self.set_selected(None);
                self.list_generation += 1;
            }
            ProcessInput::Clear => {
                self.processes.clear();
                self.set_selected(None);
                self.list_generation += 1;
            }
            ProcessInput::SortBy(col) => {
                if self.sort_column == col {
                    self.ascending = !self.ascending;
                } else {
                    self.sort_column = col;
                    self.ascending = true;
                }
                sort_processes(&mut self.processes, self.sort_column, self.ascending);
                self.set_selected(None);
                self.list_generation += 1;
            }
            ProcessInput::Select(sel) => {
                self.set_selected(sel);
            }
            ProcessInput::RequestRefresh => {
                let _ = sender.output(ProcessOutput::RequestList);
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.header.set_visible(self.show_toolbar);
        widgets.status_lbl.set_label(&crate::i18n::trf("process.active_tasks_count", &[("count", &*(self.processes.len()).to_string())]));
        widgets.btn_kill.set_sensitive(self.selected.is_some());

        if widgets.rendered_generation != self.list_generation {
            widgets.rendered_generation = self.list_generation;
            rebuild_rows(self, widgets);
        }
    }
}

fn parse_row_name(row: &gtk::ListBoxRow) -> Option<(u32, String)> {
    let widget_name = row.widget_name().to_string();
    let pid = widget_name.split(':').next()?.parse::<u32>().ok()?;
    let name = widget_name
        .split(':')
        .skip(1)
        .collect::<Vec<&str>>()
        .join(":");
    Some((pid, name))
}

fn sort_processes(procs: &mut [ProcessInfo], col: SortColumn, ascending: bool) {
    procs.sort_by(|a, b| {
        let cmp = match col {
            SortColumn::Pid => a.pid.cmp(&b.pid),
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Memory => a.memory.cmp(&b.memory),
            SortColumn::Cpu => a
                .cpu_usage
                .partial_cmp(&b.cpu_usage)
                .unwrap_or(std::cmp::Ordering::Equal),
        };
        if ascending {
            cmp
        } else {
            cmp.reverse()
        }
    });
}

fn rebuild_rows(model: &ProcessModel, widgets: &mut ProcessWidgets) {
    widgets.list_box.block_signal(&widgets.row_selected_handler);
    while let Some(child) = widgets.list_box.first_child() {
        widgets.list_box.remove(&child);
    }
    for p in &model.processes {
        widgets.list_box.append(&build_row(p));
    }
    widgets.list_box.invalidate_filter();
    widgets
        .list_box
        .unblock_signal(&widgets.row_selected_handler);
}

fn build_row(p: &ProcessInfo) -> gtk::ListBoxRow {
    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();

    let pid_label = gtk::Label::builder()
        .label(p.pid.to_string())
        .width_request(60)
        .halign(gtk::Align::Center)
        .css_classes(vec!["pid-badge"])
        .build();
    let name_label = gtk::Label::builder()
        .label(&p.name)
        .hexpand(true)
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let mem_label = gtk::Label::builder()
        .label(format!(
            "<span foreground=\"#10b981\" font_features=\"tnum=1\">{}</span>",
            format_size(p.memory)
        ))
        .use_markup(true)
        .width_request(100)
        .halign(gtk::Align::End)
        .xalign(1.0)
        .build();

    let cpu_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .width_request(120)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .build();
    let cpu_bar = gtk::ProgressBar::builder()
        .fraction(if p.cpu_usage > 100.0 {
            1.0
        } else {
            p.cpu_usage as f64 / 100.0
        })
        .css_classes(vec!["cpu-bar"])
        .hexpand(true)
        .valign(gtk::Align::Center)
        .margin_end(8)
        .build();
    let cpu_lbl = gtk::Label::builder()
        .label(format!(
            "<span font_features=\"tnum=1\">{:.1}%</span>",
            p.cpu_usage
        ))
        .use_markup(true)
        .css_classes(vec!["dim-label"])
        .width_request(40)
        .halign(gtk::Align::End)
        .xalign(1.0)
        .build();
    cpu_box.append(&cpu_bar);
    cpu_box.append(&cpu_lbl);

    row_box.append(&pid_label);
    row_box.append(&name_label);
    row_box.append(&mem_label);
    row_box.append(&cpu_box);

    let row = gtk::ListBoxRow::builder()
        .child(&row_box)
        .css_classes(vec!["process-row"])
        .build();

    row.set_widget_name(&format!("{}:{}", p.pid, p.name));
    row
}
