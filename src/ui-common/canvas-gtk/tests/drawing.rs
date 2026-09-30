//! What a plugin is told about the place it draws in, and in what order.
//!
//! GTK may only be used from the thread that started it, so these share one
//! `main` rather than being a test each.
//!
//! None of them waits for the compositor to ask for a frame: a window that is
//! not on screen is never asked, and a check that hangs on that would be a
//! check of the desktop rather than of this code. Frames are asked for here.

use gtk::prelude::*;
use ic_plugin_api::{IcCanvas, IcFrame};
use std::cell::RefCell;
use std::rc::Rc;

type Said = Rc<RefCell<Vec<String>>>;

/// Slots that write down what they were told instead of drawing anything, and
/// answer `ready` as the caller asked them to.
fn recording(said: &Said, accepting: bool) -> ic_canvas_gtk::Slots {
    let on_ready = said.clone();
    let on_draw = said.clone();
    let on_gone = said.clone();
    ic_canvas_gtk::Slots {
        ready: Rc::new(move |instance, canvas: &IcCanvas| {
            let sized = canvas.struct_size as usize == std::mem::size_of::<IcCanvas>();
            let found = (canvas.get_proc_address)(
                canvas.proc_ctx,
                c"glGetIntegerv".as_ptr(),
            );
            on_ready.borrow_mut().push(format!(
                "ready instance={instance} api={} sized={sized} gl={}",
                canvas.api,
                !found.is_null()
            ));
            accepting
        }),
        draw: Rc::new(move |instance, frame: &IcFrame| {
            on_draw.borrow_mut().push(format!(
                "draw instance={instance} size={}x{} scale={} sized={}",
                frame.width,
                frame.height,
                frame.scale,
                frame.struct_size as usize == std::mem::size_of::<IcFrame>()
            ));
        }),
        gone: Rc::new(move |instance| {
            on_gone
                .borrow_mut()
                .push(format!("gone instance={instance}"));
        }),
    }
}

/// A place in a window on the screen, with what it takes to ask it for a frame
/// without waiting for the desktop to ask first.
struct Put {
    window: gtk::Window,
    holder: gtk::Box,
    area: gtk::GLArea,
}

impl Put {
    fn up(instance: u64, slots: ic_canvas_gtk::Slots) -> Put {
        let place = ic_canvas_gtk::place(instance, slots);
        let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        holder.append(&place);
        let window = gtk::Window::builder()
            .default_width(160)
            .default_height(90)
            .child(&holder)
            .build();
        window.present();
        let area = place
            .downcast::<gtk::GLArea>()
            .expect("a place is a GL area");
        Put {
            window,
            holder,
            area,
        }
    }

    /// Draws the place here and now, the way the toolkit would.
    fn draw_now(&self) {
        self.holder
            .snapshot_child(&self.area, &gtk::Snapshot::new());
    }
}

fn spin(seconds: f64) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs_f64(seconds);
    let context = gtk::glib::MainContext::default();
    while std::time::Instant::now() < until {
        while context.iteration(false) {}
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn counted(said: &Said, what: &str) -> usize {
    said.borrow()
        .iter()
        .filter(|line| line.starts_with(what))
        .count()
}

/// The place is made, the plugin is told about it once, and only then is it
/// asked for frames. What it is handed carries a working way to find a GL
/// symbol — without one it could not draw at all.
fn a_place_is_ready_before_it_is_ever_asked_to_draw() {
    let said: Said = Rc::new(RefCell::new(Vec::new()));
    let put = Put::up(7, recording(&said, true));
    spin(0.5);
    assert_eq!(
        counted(&said, "ready"),
        1,
        "the place never came up: {:?}",
        said.borrow()
    );
    put.draw_now();

    let lines = said.borrow().clone();
    assert!(
        lines[0].starts_with("ready instance=7"),
        "the first thing said was {:?}",
        lines[0]
    );
    assert!(
        lines[0].contains("api=1") && lines[0].contains("sized=true"),
        "{:?}",
        lines[0]
    );
    assert!(
        lines[0].ends_with("gl=true"),
        "the host found no glGetIntegerv to hand over: {:?}",
        lines[0]
    );
    assert!(
        lines.len() > 1 && lines[1].starts_with("draw instance=7"),
        "the place was never drawn in: {lines:?}"
    );
    assert!(
        lines[1].contains("size=160x90") || lines[1].contains("size=320x180"),
        "a frame is sized in pixels, scale included: {:?}",
        lines[1]
    );
    assert!(lines[1].ends_with("sized=true"), "{:?}", lines[1]);
    put.window.destroy();
}

/// A plugin has a frame ready and says so from wherever it decoded it. What
/// that reaches is the one place that window draws in, and nothing else.
///
/// The place is taken off the desktop's own rhythm first, so a frame drawn
/// afterwards can only be one this asked for.
fn a_redraw_asks_for_one_more_frame() {
    let said: Said = Rc::new(RefCell::new(Vec::new()));
    let put = Put::up(8, recording(&said, true));
    let elsewhere: Said = Rc::new(RefCell::new(Vec::new()));
    let other = Put::up(108, recording(&elsewhere, true));
    spin(0.5);
    put.area.set_auto_render(false);
    other.area.set_auto_render(false);

    let (drawn, next_door) = (counted(&said, "draw"), counted(&elsewhere, "draw"));
    put.draw_now();
    assert_eq!(
        counted(&said, "draw"),
        drawn,
        "a place nobody asked to redraw drew anyway: {:?}",
        said.borrow()
    );

    ic_canvas_gtk::redraw(8);
    put.draw_now();
    assert_eq!(
        counted(&said, "draw"),
        drawn + 1,
        "a redraw did not reach the place: {:?}",
        said.borrow()
    );

    other.draw_now();
    assert_eq!(
        counted(&elsewhere, "draw"),
        next_door,
        "a redraw for one window reached another: {:?}",
        elsewhere.borrow()
    );
    put.window.destroy();
    other.window.destroy();
}

/// A plugin that refused the place is never asked for a frame, and is not told
/// the place went either — it never took it.
fn a_refused_place_is_never_drawn_in() {
    let said: Said = Rc::new(RefCell::new(Vec::new()));
    let put = Put::up(9, recording(&said, false));
    spin(0.5);
    assert_eq!(
        counted(&said, "ready"),
        1,
        "the place never came up: {:?}",
        said.borrow()
    );
    put.draw_now();
    assert_eq!(counted(&said, "draw"), 0, "{:?}", said.borrow());

    ic_canvas_gtk::letting_go(9);
    put.window.destroy();
    spin(0.2);
    assert_eq!(counted(&said, "gone"), 0, "{:?}", said.borrow());
}

/// The window is closing: the plugin is told the place has gone, once, before
/// anything tears the widgets down — and the teardown that follows does not say
/// it a second time.
fn the_place_goes_once_when_the_window_closes() {
    let said: Said = Rc::new(RefCell::new(Vec::new()));
    let put = Put::up(10, recording(&said, true));
    spin(0.5);
    put.draw_now();
    assert_eq!(counted(&said, "draw"), 1, "{:?}", said.borrow());

    ic_canvas_gtk::letting_go(10);
    assert_eq!(
        counted(&said, "gone"),
        1,
        "closing did not tell the plugin the place went: {:?}",
        said.borrow()
    );
    put.window.destroy();
    spin(0.2);
    assert_eq!(
        counted(&said, "gone"),
        1,
        "the place went twice: {:?}",
        said.borrow()
    );
}

/// The window went without anyone saying so first: unrealize is the last word,
/// and the plugin still hears it exactly once, after every frame and never
/// before one.
fn a_window_that_simply_goes_still_tells_the_plugin() {
    let said: Said = Rc::new(RefCell::new(Vec::new()));
    let put = Put::up(11, recording(&said, true));
    spin(0.5);
    put.draw_now();
    put.window.destroy();
    spin(0.5);

    let lines = said.borrow().clone();
    assert_eq!(counted(&said, "gone"), 1, "{lines:?}");
    assert_eq!(
        lines.last().map(String::as_str),
        Some("gone instance=11"),
        "{lines:?}"
    );
}

fn main() {
    if gtk::init().is_err() {
        eprintln!("drawing: no display — NOTHING was checked here");
        return;
    }
    let checks: Vec<(&str, fn())> = vec![
        (
            "a_place_is_ready_before_it_is_ever_asked_to_draw",
            a_place_is_ready_before_it_is_ever_asked_to_draw,
        ),
        (
            "a_redraw_asks_for_one_more_frame",
            a_redraw_asks_for_one_more_frame,
        ),
        (
            "a_refused_place_is_never_drawn_in",
            a_refused_place_is_never_drawn_in,
        ),
        (
            "the_place_goes_once_when_the_window_closes",
            the_place_goes_once_when_the_window_closes,
        ),
        (
            "a_window_that_simply_goes_still_tells_the_plugin",
            a_window_that_simply_goes_still_tells_the_plugin,
        ),
    ];
    let mut failed = 0;
    for (name, check) in checks {
        print!("drawing: {name} ... ");
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(check)) {
            Ok(()) => println!("ok"),
            Err(_) => {
                failed += 1;
                println!("FAILED");
            }
        }
    }
    if failed > 0 {
        eprintln!("drawing: {failed} failed");
        std::process::exit(1);
    }
}
