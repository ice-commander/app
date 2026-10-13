use adw::prelude::*;

use crate::file_operations::{trigger_copy, trigger_move};
use crate::panel_builder::PanelInfo;

/// A phone shows one panel: copying or moving elsewhere lends the other panel to a page of its own,
/// where the place is picked.
pub(super) fn pick(
    window: &adw::ApplicationWindow,
    source: &PanelInfo,
    target: &PanelInfo,
    moving: bool,
) {
    if source.active_router().selected_entries().is_empty() {
        return;
    }
    let text = |copying: &str, moving_key: &str| {
        crate::i18n::tr(if moving { moving_key } else { copying })
    };
    let here = gtk::Button::builder()
        .label(&*text("mobile.copy_here", "mobile.move_here"))
        .css_classes(["suggested-action"])
        .valign(gtk::Align::Center)
        .build();
    let Some(page) = crate::ui::surface::push_page(
        &text("mobile.copy_to", "mobile.move_to"),
        &target.panel_box,
        &here,
    ) else {
        return;
    };
    let page = page.downgrade();
    let (window, source, target) = (window.clone(), source.clone(), target.clone());
    here.connect_clicked(move |_| {
        if let Some(page) = page.upgrade() {
            crate::ui::surface::pop_page(&page);
        }
        let (from, to) = (source.active_router(), target.active_router());
        if moving {
            trigger_move(&window, &from, &to);
        } else {
            trigger_copy(&window, &from, &to);
        }
    });
}

/// What the phone's own toolbar buttons do; the panel that has them cannot reach the other one.
pub(super) fn install_phone_actions(
    window: &adw::ApplicationWindow,
    left: &PanelInfo,
    right: &PanelInfo,
) {
    let (window, left, right) = (window.clone(), left.clone(), right.clone());
    crate::panel_builder::set_phone_actions(std::rc::Rc::new(move |action| {
        let router = left.active_router();
        match action {
            "copy_to" => pick(&window, &left, &right, false),
            "move_to" => pick(&window, &left, &right, true),
            "view" | "edit" => {
                let Some(entry) = router.selected_entries().into_iter().next() else {
                    return;
                };
                if action == "view" {
                    crate::viewer::show_viewer(&window, entry, router.clone());
                } else {
                    crate::editor::show_editor(&window, entry, router.clone());
                }
            }
            _ => {}
        }
    }));
}
