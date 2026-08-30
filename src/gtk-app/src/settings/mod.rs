use adw::prelude::*;
use gtk::glib;
use gtk::{
    Box, Label, ListBox, ListBoxRow, Orientation, PolicyType, ScrolledWindow, SelectionMode,
    Separator, Stack, StackTransitionType,
};
use std::cell::RefCell;
use std::rc::Rc;

mod page_about;
mod page_applications;
mod page_connections;
mod page_editors;
mod page_hotkeys;
mod page_interface;
mod page_logging;
mod page_security;

pub fn show_settings_dialog(
    parent_window: &gtk::Window,
    config: client_config::AppConfig,
    on_connections_changed: Rc<dyn Fn() + 'static>,
) {
    let settings_dialog = gtk::Window::builder()
        .title(&*crate::i18n::tr("settings.window_title"))
        .transient_for(parent_window)
        .modal(true)
        .default_width(900)
        .default_height(600)
        .resizable(true)
        .build();

    let main_box = Box::builder().orientation(Orientation::Horizontal).build();

    let sidebar_box = Box::builder()
        .orientation(Orientation::Vertical)
        .width_request(180)
        .build();

    let sidebar_list = ListBox::builder()
        .selection_mode(SelectionMode::Single)
        .build();
    sidebar_list.add_css_class("navigation-sidebar");

    let sidebar_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(&sidebar_list)
        .vexpand(true)
        .build();

    sidebar_box.append(&sidebar_scroll);

    let separator = Separator::new(Orientation::Vertical);

    let stack = Stack::builder()
        .transition_type(StackTransitionType::Crossfade)
        .hexpand(true)
        .vexpand(true)
        .build();

    type PageBuilder = std::boxed::Box<dyn Fn(&Box)>;
    let builders: Rc<RefCell<Vec<Option<PageBuilder>>>> = Rc::new(RefCell::new(Vec::new()));
    let pages: Rc<RefCell<Vec<Box>>> = Rc::new(RefCell::new(Vec::new()));

    let stack_clone = stack.clone();
    let builders_select = builders.clone();
    let pages_select = pages.clone();
    sidebar_list.connect_row_selected(move |_, row| {
        let Some(row) = row else { return };
        let index = row.index() as usize;
        let builder = builders_select.borrow_mut().get_mut(index).and_then(Option::take);
        let page_box = pages_select.borrow().get(index).cloned();
        if let (Some(build), Some(page_box)) = (builder, page_box) {
            build(&page_box);
        }
        stack_clone.set_visible_child_name(&format!("page_{}", index));
    });

    let mut categories = vec![(
        "About",
        crate::i18n::tr("settings.cat_about"),
    )];
    categories.push((
        "Connections",
        crate::i18n::tr("settings.cat_connections"),
    ));
    categories.push((
        "Interface",
        crate::i18n::tr("settings.cat_interface"),
    ));
    categories.push((
        "Hot keys",
        crate::i18n::tr("settings.cat_hotkeys"),
    ));
    categories.push((
        "Editors",
        crate::i18n::tr("settings.cat_editors"),
    ));
    categories.push((
        "Applications",
        crate::i18n::tr("settings.cat_applications"),
    ));
    categories.push((
        "Security",
        crate::i18n::tr("settings.cat_security"),
    ));
    categories.push((
        "Logging",
        crate::i18n::tr("settings.cat_logging"),
    ));

    let mut first_row = None;

    for (i, (cat, display_name)) in categories.iter().enumerate() {
        let row = ListBoxRow::builder().build();
        let label = Label::builder()
            .label(display_name.as_str())
            .xalign(0.0)
            .margin_start(16)
            .margin_end(16)
            .margin_top(10)
            .margin_bottom(10)
            .build();
        row.set_child(Some(&label));
        sidebar_list.append(&row);

        if i == 0 {
            first_row = Some(row.clone());
        }

        let page_box = Box::builder()
            .orientation(Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();

        let builder: Option<PageBuilder> = match *cat {
            "About" => {
                let dialog = settings_dialog.clone();
                Some(std::boxed::Box::new(move |page_box: &Box| {
                    page_about::build(page_box, &dialog);
                }))
            }
            "Connections" => {
                let dialog = settings_dialog.clone();
                let config = config.clone();
                let on_changed = on_connections_changed.clone();
                Some(std::boxed::Box::new(move |page_box: &Box| {
                    page_connections::build(
                        page_box,
                        &dialog,
                        config.clone(),
                        on_changed.clone(),
                    );
                }))
            }
            "Interface" => {
                let config = config.clone();
                let on_changed = on_connections_changed.clone();
                Some(std::boxed::Box::new(move |page_box: &Box| {
                    page_interface::build(page_box, config.clone(), on_changed.clone());
                }))
            }
            "Hot keys" => {
                let dialog = settings_dialog.clone();
                let config = config.clone();
                let on_changed = on_connections_changed.clone();
                Some(std::boxed::Box::new(move |page_box: &Box| {
                    page_hotkeys::build(page_box, &dialog, config.clone(), on_changed.clone());
                }))
            }
            "Editors" => {
                let dialog = settings_dialog.clone();
                let config = config.clone();
                Some(std::boxed::Box::new(move |page_box: &Box| {
                    page_editors::build(page_box, &dialog, config.clone());
                }))
            }
            "Applications" => {
                let dialog = settings_dialog.clone();
                let config = config.clone();
                Some(std::boxed::Box::new(move |page_box: &Box| {
                    page_applications::build(page_box, &dialog, config.clone());
                }))
            }
            "Security" => {
                let dialog = settings_dialog.clone();
                let config = config.clone();
                Some(std::boxed::Box::new(move |page_box: &Box| {
                    page_security::build(page_box, &dialog, config.clone());
                }))
            }
            "Logging" => {
                let dialog = settings_dialog.clone();
                let config = config.clone();
                Some(std::boxed::Box::new(move |page_box: &Box| {
                    page_logging::build(page_box, &dialog, config.clone());
                }))
            }
            _ => None,
        };
        builders.borrow_mut().push(builder);
        pages.borrow_mut().push(page_box.clone());

        let page_scroll = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .child(&page_box)
            .build();
        stack.add_named(&page_scroll, Some(&format!("page_{}", i)));
    }

    if let Some(row) = first_row {
        sidebar_list.select_row(Some(&row));
    }

    main_box.append(&sidebar_box);
    main_box.append(&separator);
    main_box.append(&stack);

    settings_dialog.set_child(Some(&main_box));

    let key_controller = gtk::EventControllerKey::new();
    let dialog_esc = settings_dialog.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gtk::gdk::Key::Escape {
            dialog_esc.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    settings_dialog.add_controller(key_controller);

    settings_dialog.present();
}
