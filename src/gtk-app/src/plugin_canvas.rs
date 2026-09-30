//! Ties the places a plugin draws in to the plugin that draws in them.

use std::rc::Rc;

/// A place to draw for one open viewer window, or nothing at all when the
/// plugin brought no drawing slots — a GL context nobody paints in is a
/// context wasted, and an empty black rectangle on the screen.
pub(crate) fn place(viewer: &str, instance: u64) -> Option<gtk::Widget> {
    if !ic_plugin_host::viewer_draws(viewer) {
        return None;
    }
    let ready = {
        let viewer = viewer.to_string();
        Rc::new(move |instance, canvas: &ic_plugin_api::IcCanvas| {
            ic_plugin_host::viewer_canvas_ready(&viewer, instance, canvas)
        })
    };
    let draw = {
        let viewer = viewer.to_string();
        Rc::new(move |instance, frame: &ic_plugin_api::IcFrame| {
            ic_plugin_host::viewer_canvas_draw(&viewer, instance, frame)
        })
    };
    let gone = {
        let viewer = viewer.to_string();
        Rc::new(move |instance| ic_plugin_host::viewer_canvas_gone(&viewer, instance))
    };
    Some(ic_canvas_gtk::place(
        instance,
        ic_canvas_gtk::Slots { ready, draw, gone },
    ))
}

pub(crate) fn install() {
    ic_plugin_host::set_canvas_redraw_handler(Rc::new(ic_canvas_gtk::redraw));
}

/// Said before the plugin is told its window closed: after that it has
/// forgotten the window, and the widgets are only torn down later still.
pub(crate) fn letting_go(instance: u64) {
    ic_canvas_gtk::letting_go(instance);
}
