//! A place in a GTK window where a plugin puts pixels of its own.
//!
//! The plugin is never handed a widget: it gets `IcCanvas` when the place is
//! ready, `IcFrame` for each frame, and nothing at all once the place is gone.
//! Everything toolkit-shaped stays on this side, which is what keeps a plugin
//! from having to link against GTK.

use gtk::prelude::*;
use ic_plugin_api::{IcCanvas, IcFrame, IC_CANVAS_GL};
use std::cell::{Cell, RefCell};
use std::os::raw::{c_char, c_void};
use std::rc::Rc;

/// Answers whether it will draw. A refusal means no frames are asked for.
pub type Ready = Rc<dyn Fn(u64, &IcCanvas) -> bool>;
pub type Draw = Rc<dyn Fn(u64, &IcFrame)>;
pub type Gone = Rc<dyn Fn(u64)>;

/// What the host does with the three moments of a place's life. Which plugin
/// answers them is the host's business, not this crate's.
#[derive(Clone)]
pub struct Slots {
    pub ready: Ready,
    pub draw: Draw,
    pub gone: Gone,
}

struct Place {
    instance: u64,
    slots: Slots,
    area: gtk::glib::WeakRef<gtk::GLArea>,
    drawing: Cell<bool>,
}

impl Place {
    fn let_go(&self) {
        if !self.drawing.replace(false) {
            return;
        }
        // What the plugin made in `ready` is freed in `gone`, so the context it
        // was made in has to be current. GTK does not promise that here.
        if let Some(area) = self.area.upgrade() {
            if area.is_realized() {
                area.make_current();
            }
        }
        (self.slots.gone)(self.instance);
    }
}

thread_local! {
    static PLACES: RefCell<Vec<Rc<Place>>> = const { RefCell::new(Vec::new()) };
}

/// Every place open on this window, taken out of the list before any of them is
/// called: a plugin asking for another frame from inside `draw` would otherwise
/// find the list still borrowed.
fn places_of(instance: u64) -> Vec<Rc<Place>> {
    PLACES.with(|list| {
        let mut list = list.borrow_mut();
        list.retain(|held| held.area.upgrade().is_some());
        list.iter()
            .filter(|held| held.instance == instance)
            .cloned()
            .collect()
    })
}

/// Puts up a place to draw for one open window.
pub fn place(instance: u64, slots: Slots) -> gtk::Widget {
    let area = gtk::GLArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    // The picture takes the keyboard so that the window keeps a focus chain of
    // its own. Without it the only things worth focusing are the controls
    // beside the picture, and a window that puts those away would have no
    // focus at all — and then none of the document's shortcuts would be seen.
    area.set_focusable(true);
    area.connect_map(|area| {
        area.grab_focus();
    });

    let held = Rc::new(Place {
        instance,
        slots,
        area: area.downgrade(),
        drawing: Cell::new(false),
    });
    PLACES.with(|list| {
        let mut list = list.borrow_mut();
        list.retain(|held| held.area.upgrade().is_some());
        list.push(held.clone());
    });

    let opening = held.clone();
    area.connect_realize(move |area| {
        area.make_current();
        if let Some(wrong) = area.error() {
            eprintln!("canvas: no GL context for instance {instance}: {wrong}");
            return;
        }
        let canvas = IcCanvas {
            struct_size: std::mem::size_of::<IcCanvas>() as u32,
            api: IC_CANVAS_GL,
            get_proc_address: proc_address,
            proc_ctx: std::ptr::null_mut(),
        };
        opening
            .drawing
            .set((opening.slots.ready)(opening.instance, &canvas));
    });

    let drawing = held.clone();
    area.connect_render(move |area, _| {
        if drawing.drawing.get() {
            let scale = area.scale_factor();
            let frame = IcFrame {
                struct_size: std::mem::size_of::<IcFrame>() as u32,
                fbo: bound_framebuffer(),
                width: area.width() * scale,
                height: area.height() * scale,
                scale: f64::from(scale),
            };
            (drawing.slots.draw)(drawing.instance, &frame);
        }
        gtk::glib::Propagation::Proceed
    });

    let leaving = held.clone();
    area.connect_unrealize(move |_| leaving.let_go());

    // Two presses on the picture fill the screen with it, the way they do in
    // every other player. It belongs here rather than on the window: up there
    // it would also fire on the controls under the picture, and a plugin's
    // window has no way to tell the host which part of it is the picture.
    let double = gtk::GestureClick::new();
    double.set_button(gtk::gdk::BUTTON_PRIMARY);
    double.connect_pressed(move |gesture, presses, _, _| {
        if presses != 2 {
            return;
        }
        let Some(window) = gesture
            .widget()
            .and_then(|widget| widget.root())
            .and_downcast::<gtk::Window>()
        else {
            return;
        };
        if window.is_fullscreen() {
            window.unfullscreen();
        } else {
            window.fullscreen();
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    area.add_controller(double);

    area.upcast()
}

/// A frame is ready: draw it. Called on the frontend's own thread, whatever
/// thread the plugin decoded on.
pub fn redraw(instance: u64) {
    for held in places_of(instance) {
        if let Some(area) = held.area.upgrade() {
            area.queue_render();
        }
    }
}

/// The window is closing. `gone` has to reach the plugin while the context is
/// still there, and the widgets are torn down after the plugin has been told
/// the window went — so the host says so here instead of waiting for unrealize.
pub fn letting_go(instance: u64) {
    for held in places_of(instance) {
        held.let_go();
    }
}

const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;

/// What GTK is drawing into. Looked up once: the original did it per frame.
fn bound_framebuffer() -> i32 {
    static FOUND: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let found = *FOUND.get_or_init(|| {
        proc_address(
            std::ptr::null_mut(),
            c"glGetIntegerv".as_ptr(),
        ) as usize
    });
    if found == 0 {
        return 0;
    }
    let ask: unsafe extern "C" fn(u32, *mut i32) = unsafe { std::mem::transmute(found) };
    let mut bound: i32 = 0;
    unsafe { ask(GL_FRAMEBUFFER_BINDING, &mut bound) };
    bound
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
type GetProcAddress = unsafe extern "C" fn(*const c_char) -> *mut c_void;

/// Finding a GL symbol is different on every system, so the host does it and
/// the plugin is handed this one function.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
// The contract hands the plugin a safe fn pointer, and a null name is refused below.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    // EGL first: libGL.so.1 is not among this process's libraries — libepoxy
    // opens whichever stack is in use itself — so plain dlsym finds no `gl*`.
    if let Some(ask) = egl_get_proc_address() {
        let found = unsafe { ask(name) };
        if !found.is_null() {
            return found;
        }
    }
    if let Some(library) = gl_library() {
        let found = unsafe { libc::dlsym(library, name) };
        if !found.is_null() {
            return found;
        }
    }
    unsafe { libc::dlsym(libc::RTLD_DEFAULT, name) }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn egl_get_proc_address() -> Option<GetProcAddress> {
    static FOUND: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let found = *FOUND.get_or_init(|| unsafe {
        let library = libc::dlopen(
            c"libEGL.so.1".as_ptr(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL,
        );
        if library.is_null() {
            return 0;
        }
        libc::dlsym(library, c"eglGetProcAddress".as_ptr()) as usize
    });
    (found != 0).then(|| unsafe { std::mem::transmute::<usize, GetProcAddress>(found) })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn gl_library() -> Option<*mut c_void> {
    static FOUND: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let found = *FOUND.get_or_init(|| unsafe {
        libc::dlopen(
            c"libGL.so.1".as_ptr(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL,
        ) as usize
    });
    (found != 0).then_some(found as *mut c_void)
}

#[cfg(target_os = "macos")]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { libc::dlsym(libc::RTLD_DEFAULT, name) }
}

#[cfg(target_os = "windows")]
extern "system" {
    fn GetModuleHandleA(name: *const u8) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> *mut c_void;
}

#[cfg(target_os = "windows")]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let epoxy = GetModuleHandleA(b"libepoxy-0.dll\0".as_ptr());
        if epoxy != 0 {
            let ask = GetProcAddress(epoxy, b"epoxy_get_proc_address\0".as_ptr());
            if !ask.is_null() {
                type EpoxyGetProcAddress = unsafe extern "C" fn(*const c_char) -> *mut c_void;
                let ask: EpoxyGetProcAddress = std::mem::transmute(ask);
                let found = ask(name);
                if !found.is_null() {
                    return found;
                }
            }
        }
        let opengl = GetModuleHandleA(b"opengl32.dll\0".as_ptr());
        if opengl == 0 {
            return std::ptr::null_mut();
        }
        let ask = GetProcAddress(opengl, b"wglGetProcAddress\0".as_ptr());
        if !ask.is_null() {
            type WglGetProcAddress = unsafe extern "system" fn(*const c_char) -> *mut c_void;
            let ask: WglGetProcAddress = std::mem::transmute(ask);
            let found = ask(name);
            if !found.is_null() {
                return found;
            }
        }
        GetProcAddress(opengl, name as *const u8)
    }
}
