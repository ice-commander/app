use adw::prelude::*;
use ic_model::SysInfo;
use gtk::glib;
use relm4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

const HISTORY_LEN: usize = 60;

fn get_os_image(os_str: &str, build_type: &str, size: i32) -> gtk::Image {
    let lower_os = os_str.to_lowercase();
    let lower_build = build_type.to_lowercase();

    let resource_path =
        if lower_build.contains("exe") || lower_build.contains("msi") || lower_os.contains("win") {
            "/com/sysinfo-ui/gtk/os-win.svg"
        } else if lower_build.contains("dmg")
            || lower_build.contains("osx")
            || lower_os.contains("mac")
            || lower_os.contains("darwin")
        {
            "/com/sysinfo-ui/gtk/os-mac.svg"
        } else if lower_build.contains("apk") || lower_os.contains("android") {
            "/com/sysinfo-ui/gtk/os-android.svg"
        } else if lower_build.contains("ipa")
            || lower_build.contains("ios")
            || lower_os.contains("iphone")
        {
            "/com/sysinfo-ui/gtk/os-iphone.svg"
        } else if lower_os.contains("linux")
            || lower_build.contains("zst")
            || lower_build.contains("arch")
            || lower_build.contains("deb")
            || lower_build.contains("rpm")
        {
            "/com/sysinfo-ui/gtk/os-linux.svg"
        } else {
            "/com/sysinfo-ui/gtk/os-console.svg"
        };

    let img = gtk::Image::builder()
        .resource(resource_path)
        .pixel_size(size)
        .build();
    img.add_css_class("os-logo-image");
    img
}

fn build_graph(
    history: Rc<RefCell<Vec<f32>>>,
    rgba: (f64, f64, f64, f64),
    title: &str,
) -> (gtk::DrawingArea, gtk::Box) {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(16)
        .halign(gtk::Align::Start)
        .build();

    let title_lbl = gtk::Label::builder()
        .label(title)
        .css_classes(vec!["dim-label"])
        .halign(gtk::Align::Start)
        .build();

    let graph = gtk::DrawingArea::builder()
        .height_request(150)
        .width_request(640)
        .build();

    container.append(&title_lbl);
    container.append(&graph);

    let (r, g, b, a) = rgba;
    graph.set_draw_func(move |_, ctx, width, height| {
        let width = width as f64;
        let height = height as f64;

        ctx.set_source_rgba(0.0, 0.0, 0.0, 0.05);
        ctx.paint().unwrap();

        ctx.set_source_rgba(0.5, 0.5, 0.5, 0.15);
        ctx.set_line_width(1.0);
        let step = height / 10.0;
        for i in 0..=10 {
            let y = i as f64 * step;
            ctx.move_to(0.0, y);
            ctx.line_to(width, y);
        }
        let mut x = width;
        while x >= 0.0 {
            ctx.move_to(x, 0.0);
            ctx.line_to(x, height);
            x -= step;
        }
        ctx.stroke().unwrap();

        let history = history.borrow();
        if history.is_empty() {
            return;
        }

        ctx.set_source_rgba(r, g, b, a);
        ctx.set_line_width(2.0);
        let step_x = width / ((HISTORY_LEN - 1) as f64);
        let start = history.len().saturating_sub(HISTORY_LEN);
        let points = &history[start..];
        let offset_x = width - ((points.len() - 1) as f64 * step_x);
        for (i, &val) in points.iter().enumerate() {
            let safe = if val.is_nan() { 0.0 } else { val as f64 };
            let y = height - (height * (safe.clamp(0.0, 100.0) / 100.0));
            let px = offset_x + (i as f64 * step_x);
            if i == 0 {
                ctx.move_to(px, y);
            } else {
                ctx.line_to(px, y);
            }
        }
        ctx.stroke().unwrap();
    });

    (graph, container)
}

pub struct SysInfoInit {
    pub show_toolbar: bool,
    pub auto_update: bool,
    pub update_interval: Duration,
}

impl Default for SysInfoInit {
    fn default() -> Self {
        Self {
            show_toolbar: true,
            auto_update: false,
            update_interval: Duration::from_secs(2),
        }
    }
}

#[derive(Debug)]
pub enum SysInfoInput {
    Update(Box<SysInfo>),
    Reset,
    ToggleAutoUpdate(bool),
    RequestRefresh,
}

#[derive(Debug)]
pub enum SysInfoOutput {
    Back,
    RequestInfo,
}

pub struct SysInfoModel {
    show_toolbar: bool,
    update_interval: Duration,
    info: Option<SysInfo>,
    auto_update: bool,
    auto_gate: Rc<Cell<bool>>,
    timer_armed: Rc<Cell<bool>>,
    cpu_history: Rc<RefCell<Vec<f32>>>,
    mem_history: Rc<RefCell<Vec<f32>>>,
}

impl SysInfoModel {
    fn arm_timer_if_needed(&self, sender: &ComponentSender<Self>) {
        if self.timer_armed.get() {
            return;
        }
        self.timer_armed.set(true);
        let gate = self.auto_gate.clone();
        let armed = self.timer_armed.clone();
        let sender = sender.clone();
        glib::timeout_add_local(self.update_interval, move || {
            if !gate.get() {
                armed.set(false);
                return glib::ControlFlow::Break;
            }
            let _ = sender.output(SysInfoOutput::RequestInfo);
            glib::ControlFlow::Continue
        });
    }
}

pub struct SysInfoWidgets {
    header: adw::HeaderBar,
    auto_update_btn: gtk::ToggleButton,
    auto_toggle_handler: glib::SignalHandlerId,
    os_icon_box: gtk::Box,
    hostname_label: gtk::Label,
    os_label: gtk::Label,
    arch_label: gtk::Label,
    cores_label: gtk::Label,
    cpu_usage_label: gtk::Label,
    uptime_label: gtk::Label,
    mem_label: gtk::Label,
    swap_label: gtk::Label,
    network_box: gtk::Box,
    graphs_container: gtk::Box,
    cpu_graph: gtk::DrawingArea,
    mem_graph: gtk::DrawingArea,
}

impl SimpleComponent for SysInfoModel {
    type Init = SysInfoInit;
    type Input = SysInfoInput;
    type Output = SysInfoOutput;
    type Root = gtk::Box;
    type Widgets = SysInfoWidgets;

    fn init_root() -> Self::Root {
        gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build()
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

        let btn_back = gtk::Button::builder()
            .child(&gtk::Image::from_resource(
                "/com/sysinfo-ui/gtk/arrow-left.svg",
            ))
            .tooltip_text(&*crate::i18n::tr("sysinfo.back_to_services"))
            .build();
        btn_back.set_cursor_from_name(Some("pointer"));
        btn_back.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| {
                let _ = sender.output(SysInfoOutput::Back);
            }
        ));
        header.pack_start(&btn_back);

        header.pack_start(
            &gtk::Separator::builder()
                .orientation(gtk::Orientation::Vertical)
                .margin_top(8)
                .margin_bottom(8)
                .build(),
        );

        let auto_update_btn = gtk::ToggleButton::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text(&*crate::i18n::tr("sysinfo.enable_auto_update"))
            .build();
        auto_update_btn.set_cursor_from_name(Some("pointer"));
        let auto_toggle_handler = auto_update_btn.connect_toggled(glib::clone!(
            #[strong]
            sender,
            move |btn| {
                sender.input(SysInfoInput::ToggleAutoUpdate(btn.is_active()));
            }
        ));
        header.pack_start(&auto_update_btn);
        header.set_visible(init.show_toolbar);
        root.append(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(32)
            .margin_bottom(32)
            .margin_start(32)
            .margin_end(32)
            .build();

        let os_icon_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .halign(gtk::Align::Start)
            .margin_bottom(16)
            .build();
        content.append(&os_icon_box);

        let grid = gtk::Grid::builder()
            .row_spacing(8)
            .column_spacing(16)
            .halign(gtk::Align::Start)
            .margin_top(16)
            .build();

        let mut row = 0;
        let mut add_row = |title: &str| -> gtk::Label {
            let title_label = gtk::Label::builder()
                .label(title)
                .halign(gtk::Align::End)
                .valign(gtk::Align::Start)
                .css_classes(vec!["dim-label"])
                .build();
            let value_label = gtk::Label::builder()
                .label("...")
                .halign(gtk::Align::Start)
                .selectable(true)
                .build();
            grid.attach(&title_label, 0, row, 1, 1);
            grid.attach(&value_label, 1, row, 1, 1);
            row += 1;
            value_label
        };

        let hostname_label = add_row(&crate::i18n::tr("sysinfo.hostname"));
        let os_label = add_row(&crate::i18n::tr("sysinfo.os"));
        let arch_label = add_row(&crate::i18n::tr("sysinfo.arch"));
        let cores_label = add_row(&crate::i18n::tr("sysinfo.cores"));
        let cpu_usage_label = add_row(&crate::i18n::tr("sysinfo.cpu_usage"));
        let uptime_label = add_row(&crate::i18n::tr("sysinfo.uptime"));
        let mem_label = add_row(&crate::i18n::tr("sysinfo.memory"));
        let swap_label = add_row(&crate::i18n::tr("sysinfo.swap"));

        let net_title = gtk::Label::builder()
            .label(&*crate::i18n::tr("sysinfo.network_interfaces"))
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .css_classes(vec!["dim-label"])
            .build();
        let network_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .halign(gtk::Align::Start)
            .build();
        grid.attach(&net_title, 0, row, 1, 1);
        grid.attach(&network_box, 1, row, 1, 1);

        let cpu_history = Rc::new(RefCell::new(Vec::<f32>::new()));
        let mem_history = Rc::new(RefCell::new(Vec::<f32>::new()));
        let (cpu_graph, cpu_box) = build_graph(
            cpu_history.clone(),
            (0.2, 0.6, 1.0, 0.8),
            &crate::i18n::tr("sysinfo.cpu_history"),
        );
        let (mem_graph, mem_box) = build_graph(
            mem_history.clone(),
            (0.9, 0.3, 0.5, 0.8),
            &crate::i18n::tr("sysinfo.mem_history"),
        );

        let graphs_container = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .visible(false)
            .build();

        let performance_header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .margin_top(24)
            .margin_bottom(12)
            .build();
        performance_header.append(
            &gtk::Image::builder()
                .resource("/com/sysinfo-ui/gtk/system-task.svg")
                .pixel_size(40)
                .build(),
        );
        performance_header.append(
            &gtk::Label::builder()
                .label(&*crate::i18n::tr("sysinfo.activity_monitoring"))
                .css_classes(vec!["title-2"])
                .build(),
        );
        graphs_container.append(&performance_header);
        graphs_container.append(&cpu_box);
        graphs_container.append(&mem_box);

        let hardware_header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .margin_top(12)
            .build();
        hardware_header.append(
            &gtk::Image::builder()
                .resource("/com/sysinfo-ui/gtk/motherboard.svg")
                .pixel_size(40)
                .build(),
        );
        hardware_header.append(
            &gtk::Label::builder()
                .label(&*crate::i18n::tr("sysinfo.hardware_specs"))
                .css_classes(vec!["title-2"])
                .build(),
        );

        content.append(&hardware_header);
        content.append(&grid);
        content.append(&graphs_container);
        root.append(&content);

        let model = SysInfoModel {
            show_toolbar: init.show_toolbar,
            update_interval: init.update_interval,
            info: None,
            auto_update: false,
            auto_gate: Rc::new(Cell::new(false)),
            timer_armed: Rc::new(Cell::new(false)),
            cpu_history,
            mem_history,
        };

        let widgets = SysInfoWidgets {
            header,
            auto_update_btn,
            auto_toggle_handler,
            os_icon_box,
            hostname_label,
            os_label,
            arch_label,
            cores_label,
            cpu_usage_label,
            uptime_label,
            mem_label,
            swap_label,
            network_box,
            graphs_container,
            cpu_graph,
            mem_graph,
        };

        if init.auto_update {
            sender.input(SysInfoInput::ToggleAutoUpdate(true));
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SysInfoInput::Update(info) => {
                let mem_pct = if info.total_memory > 0 {
                    (info.used_memory as f64 / info.total_memory as f64 * 100.0) as f32
                } else {
                    0.0
                };
                push_capped(&self.cpu_history, info.cpu_usage);
                push_capped(&self.mem_history, mem_pct);
                self.info = Some(*info);
            }
            SysInfoInput::Reset => {
                self.info = None;
                self.auto_update = false;
                self.auto_gate.set(false);
                self.cpu_history.borrow_mut().clear();
                self.mem_history.borrow_mut().clear();
            }
            SysInfoInput::ToggleAutoUpdate(on) => {
                self.auto_update = on;
                self.auto_gate.set(on);
                if on {
                    let _ = sender.output(SysInfoOutput::RequestInfo);
                    self.arm_timer_if_needed(&sender);
                } else {
                    self.cpu_history.borrow_mut().clear();
                    self.mem_history.borrow_mut().clear();
                }
            }
            SysInfoInput::RequestRefresh => {
                let _ = sender.output(SysInfoOutput::RequestInfo);
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.header.set_visible(self.show_toolbar);
        widgets.graphs_container.set_visible(self.auto_update);

        widgets
            .auto_update_btn
            .block_signal(&widgets.auto_toggle_handler);
        widgets.auto_update_btn.set_active(self.auto_update);
        widgets
            .auto_update_btn
            .unblock_signal(&widgets.auto_toggle_handler);

        while let Some(child) = widgets.os_icon_box.first_child() {
            widgets.os_icon_box.remove(&child);
        }
        while let Some(child) = widgets.network_box.first_child() {
            widgets.network_box.remove(&child);
        }

        let Some(info) = self.info.as_ref() else {
            for lbl in [
                &widgets.hostname_label,
                &widgets.os_label,
                &widgets.arch_label,
                &widgets.cores_label,
                &widgets.cpu_usage_label,
                &widgets.uptime_label,
                &widgets.mem_label,
                &widgets.swap_label,
            ] {
                lbl.set_text("...");
            }
            widgets.network_box.append(
                &gtk::Label::builder()
                    .label("...")
                    .halign(gtk::Align::Start)
                    .build(),
            );
            widgets.cpu_graph.queue_draw();
            widgets.mem_graph.queue_draw();
            return;
        };

        let os_icon = get_os_image(&info.os_family, "", 64);
        os_icon.add_css_class("color-inverted");
        widgets.os_icon_box.append(&os_icon);
        widgets.os_icon_box.append(
            &gtk::Label::builder()
                .label(format!("{} {}", info.os_type, info.os_version))
                .css_classes(vec!["title-1"])
                .margin_start(16)
                .build(),
        );

        widgets.hostname_label.set_text(&info.hostname);
        widgets
            .os_label
            .set_text(&format!("{} ({})", info.os_type, info.os_version));
        widgets.arch_label.set_text(&info.cpu_arch);
        widgets.cores_label.set_text(&info.cpu_cores.to_string());
        widgets
            .cpu_usage_label
            .set_text(&format!("{:.1}%", info.cpu_usage));

        let days = info.uptime / (24 * 3600);
        let hours = (info.uptime % (24 * 3600)) / 3600;
        let minutes = (info.uptime % 3600) / 60;
        let uptime_str = if days > 0 {
            crate::i18n::trf("sysinfo.uptime_days_format", &[("days", &*(days).to_string()), ("hours", &*(hours).to_string()), ("minutes", &*(minutes).to_string())])
        } else {
            crate::i18n::trf("sysinfo.uptime_hours_format", &[("hours", &*(hours).to_string()), ("minutes", &*(minutes).to_string())])
        };
        widgets.uptime_label.set_text(&uptime_str);

        let gib = |b: u64| format!("{:.2}", b as f64 / 1024.0 / 1024.0 / 1024.0);
        let mib = |b: u64| format!("{:.2}", b as f64 / 1024.0 / 1024.0);
        widgets.mem_label.set_text(&crate::i18n::trf("sysinfo.memory_format", &[("used", &*(gib(info.used_memory)).to_string()), ("total", &*(gib(info.total_memory)).to_string())]));
        widgets.swap_label.set_text(&crate::i18n::trf("sysinfo.swap_format", &[("used", &*(mib(info.used_swap)).to_string()), ("total", &*(mib(info.total_swap)).to_string())]));

        if info.network_interfaces.is_empty() {
            widgets.network_box.append(
                &gtk::Label::builder()
                    .label("N/A")
                    .halign(gtk::Align::Start)
                    .build(),
            );
        } else {
            let net_grid = gtk::Grid::builder()
                .row_spacing(4)
                .column_spacing(24)
                .halign(gtk::Align::Start)
                .build();
            for (i, entry) in info.network_interfaces.iter().enumerate() {
                let (iface, ip) = entry.split_once(": ").unwrap_or((entry.as_str(), ""));
                let iface_lbl = gtk::Label::builder().halign(gtk::Align::Start).build();
                iface_lbl.set_markup(&format!("<b>{iface}</b>"));
                let ip_lbl = gtk::Label::builder()
                    .label(ip)
                    .halign(gtk::Align::Start)
                    .selectable(true)
                    .build();
                net_grid.attach(&iface_lbl, 0, i as i32, 1, 1);
                net_grid.attach(&ip_lbl, 1, i as i32, 1, 1);
            }
            widgets.network_box.append(&net_grid);
        }

        widgets.cpu_graph.queue_draw();
        widgets.mem_graph.queue_draw();
    }
}

fn push_capped(buf: &Rc<RefCell<Vec<f32>>>, val: f32) {
    let mut buf = buf.borrow_mut();
    buf.push(val);
    if buf.len() > HISTORY_LEN {
        buf.remove(0);
    }
}
