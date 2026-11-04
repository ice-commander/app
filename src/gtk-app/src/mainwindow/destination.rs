use crate::panel_builder::PanelInfo;

pub(super) fn install_phone_actions(window: &adw::ApplicationWindow, panel: &PanelInfo) {
    let (window, panel) = (window.clone(), panel.clone());
    crate::panel_builder::set_phone_actions(std::rc::Rc::new(move |action| {
        let router = panel.active_router();
        match action {
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
