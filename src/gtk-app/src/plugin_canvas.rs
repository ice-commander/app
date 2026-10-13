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
    let place = ic_canvas_gtk::place(instance, ic_canvas_gtk::Slots { ready, draw, gone });
    keep_screen_on_while_shown(&place);
    Some(place)
}

thread_local! {
    static SHOWN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Whatever is drawn here is moving pictures: the screen stays on while any of them is on it.
fn keep_screen_on_while_shown(place: &gtk::Widget) {
    use gtk::prelude::*;
    let held = std::rc::Rc::new(std::cell::RefCell::new(None::<gtk::Window>));
    let shown = held.clone();
    place.connect_realize(move |place| {
        let Some(window) = place.root().and_downcast::<gtk::Window>() else {
            return;
        };
        if SHOWN.with(|count| count.replace(count.get() + 1)) == 0 {
            crate::platform().keep_screen_on(&window, true);
        }
        *shown.borrow_mut() = Some(window);
    });
    place.connect_unrealize(move |_| {
        let Some(window) = held.borrow_mut().take() else {
            return;
        };
        if SHOWN.with(|count| {
            count.set(count.get().saturating_sub(1));
            count.get()
        }) == 0
        {
            crate::platform().keep_screen_on(&window, false);
        }
    });
}

pub(crate) fn install() {
    ic_plugin_host::set_canvas_redraw_handler(Rc::new(ic_canvas_gtk::redraw));
}

/// Said before the plugin is told its window closed: after that it has
/// forgotten the window, and the widgets are only torn down later still.
pub(crate) fn letting_go(instance: u64) {
    ic_canvas_gtk::letting_go(instance);
}
