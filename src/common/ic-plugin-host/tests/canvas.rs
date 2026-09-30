//! What reaches a plugin when the frontend has somewhere for it to draw.
//!
//! The three slots were added to the end of the viewer table, so a plugin built
//! against an older header brings none of them. Reaching past what a plugin
//! actually filled in is how a host crashes one, and that is what is checked
//! here as much as the calls themselves.

use ic_plugin_api::{
    IcBytes, IcCanvas, IcFrame, IcFsSource, IcHost, IcViewVTable, IcViewerVTable, IC_CANVAS_GL,
    IC_OK,
};
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::Mutex;

fn said() -> &'static Mutex<Vec<String>> {
    static HELD: std::sync::OnceLock<Mutex<Vec<String>>> = std::sync::OnceLock::new();
    HELD.get_or_init(|| Mutex::new(Vec::new()))
}

/// Something the plugin is given back on every call, so a test can tell a slot
/// that was reached from one that answered out of nowhere.
static MINE: u8 = 42;

fn ours(user: *mut c_void) -> bool {
    user as usize == &MINE as *const u8 as usize
}

extern "C" fn describe(_ctx: *const u8, _len: u64, _user: *mut c_void) -> IcBytes {
    const DOCUMENT: &str = r#"{"schema":1,"fields":[],"form":{"t":"view",
        "children":[{"t":"canvas","id":"picture"}]}}"#;
    IcBytes {
        data: DOCUMENT.as_ptr(),
        len: DOCUMENT.len() as u64,
    }
}

extern "C" fn opens(_: u64, _: IcFsSource, _: *const c_char, _: *mut c_void) -> c_int {
    IC_OK
}

extern "C" fn ready(instance: u64, canvas: *const IcCanvas, user: *mut c_void) -> c_int {
    let held = unsafe { *canvas };
    let found =
        (held.get_proc_address)(held.proc_ctx, c"nothing_at_all".as_ptr());
    said().lock().unwrap().push(format!(
        "ready instance={instance} api={} sized={} mine={} answered={}",
        held.api,
        held.struct_size as usize == std::mem::size_of::<IcCanvas>(),
        ours(user),
        !found.is_null()
    ));
    IC_OK
}

extern "C" fn draw(instance: u64, frame: *const IcFrame, user: *mut c_void) {
    let held = unsafe { *frame };
    said().lock().unwrap().push(format!(
        "draw instance={instance} fbo={} size={}x{} scale={} mine={}",
        held.fbo,
        held.width,
        held.height,
        held.scale,
        ours(user)
    ));
}

extern "C" fn gone(instance: u64, user: *mut c_void) {
    said()
        .lock()
        .unwrap()
        .push(format!("gone instance={instance} mine={}", ours(user)));
}

extern "C" fn nothing(_ctx: *mut c_void, _name: *const c_char) -> *mut c_void {
    std::ptr::null_mut()
}

fn window() -> IcViewVTable {
    IcViewVTable {
        struct_size: std::mem::size_of::<IcViewVTable>() as u32,
        describe,
        on_event: None,
        closed: None,
    }
}

fn register(host: &IcHost, id: &str, view: *const IcViewVTable, drawing: bool) -> c_int {
    let table = IcViewerVTable {
        struct_size: std::mem::size_of::<IcViewerVTable>() as u32,
        view,
        open: opens,
        closed: None,
        content: None,
        closing: None,
        canvas_ready: drawing.then_some(ready as ic_plugin_api::IcCanvasReadyFn),
        canvas_draw: drawing.then_some(draw as ic_plugin_api::IcCanvasDrawFn),
        canvas_gone: drawing.then_some(gone as ic_plugin_api::IcCanvasGoneFn),
    };
    let (id, extensions) = (
        CString::new(id).expect("an id"),
        CString::new(".probe").expect("extensions"),
    );
    (host.register_viewer)(
        id.as_ptr(),
        extensions.as_ptr(),
        0,
        &table,
        &MINE as *const u8 as *mut c_void,
    )
}

fn canvas() -> IcCanvas {
    IcCanvas {
        struct_size: std::mem::size_of::<IcCanvas>() as u32,
        api: IC_CANVAS_GL,
        get_proc_address: nothing,
        proc_ctx: std::ptr::null_mut(),
    }
}

fn frame() -> IcFrame {
    IcFrame {
        struct_size: std::mem::size_of::<IcFrame>() as u32,
        fbo: 3,
        width: 1920,
        height: 1080,
        scale: 2.0,
    }
}

/// The registry is one per process and these put their own plugins in it, so
/// they take their turn rather than overlap.
fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn a_plugin_that_draws_is_told_about_the_place_then_the_frames_then_the_end() {
    let _turn = one_at_a_time();
    said().lock().unwrap().clear();
    let host = ic_plugin_host::host_table();
    let window = window();
    assert_eq!(register(&host, "gl-viewer", &window, true), IC_OK);

    assert!(
        ic_plugin_host::viewer_draws("gl-viewer"),
        "a plugin with the slots filled in draws"
    );
    assert!(ic_plugin_host::viewer_canvas_ready(
        "gl-viewer",
        77,
        &canvas()
    ));
    ic_plugin_host::viewer_canvas_draw("gl-viewer", 77, &frame());
    ic_plugin_host::viewer_canvas_gone("gl-viewer", 77);

    let heard = said().lock().unwrap().clone();
    assert_eq!(
        heard,
        vec![
            "ready instance=77 api=1 sized=true mine=true answered=false".to_string(),
            "draw instance=77 fbo=3 size=1920x1080 scale=2 mine=true".to_string(),
            "gone instance=77 mine=true".to_string(),
        ]
    );
}

/// The slots sit at the end of the table and are the whole of what a plugin
/// says about drawing. One that fills in none of them is asked for nothing.
#[test]
fn a_plugin_that_brought_no_slots_is_never_reached_through_them() {
    let _turn = one_at_a_time();
    said().lock().unwrap().clear();
    let host = ic_plugin_host::host_table();
    let window = window();
    assert_eq!(register(&host, "plain-viewer", &window, false), IC_OK);

    assert!(!ic_plugin_host::viewer_draws("plain-viewer"));
    assert!(!ic_plugin_host::viewer_canvas_ready(
        "plain-viewer",
        1,
        &canvas()
    ));
    ic_plugin_host::viewer_canvas_draw("plain-viewer", 1, &frame());
    ic_plugin_host::viewer_canvas_gone("plain-viewer", 1);

    assert!(
        said().lock().unwrap().is_empty(),
        "{:?}",
        said().lock().unwrap()
    );
}

/// Nobody registered under that name at all.
#[test]
fn a_viewer_nobody_registered_draws_nothing_and_says_nothing() {
    let _turn = one_at_a_time();
    said().lock().unwrap().clear();
    assert!(!ic_plugin_host::viewer_draws("no-such-viewer"));
    assert!(!ic_plugin_host::viewer_canvas_ready(
        "no-such-viewer",
        1,
        &canvas()
    ));
    ic_plugin_host::viewer_canvas_draw("no-such-viewer", 1, &frame());
    ic_plugin_host::viewer_canvas_gone("no-such-viewer", 1);
    assert!(said().lock().unwrap().is_empty());
}

/// A plugin built before drawing existed registers a table that ends at
/// `closing`. Whatever the bytes after it happen to hold, they are not the
/// host's to read — and here they hold slots that would answer if it did.
#[test]
fn a_table_that_ends_before_the_slots_is_never_read_that_far() {
    let _turn = one_at_a_time();
    said().lock().unwrap().clear();
    let host = ic_plugin_host::host_table();
    let window = window();
    let table = IcViewerVTable {
        struct_size: std::mem::offset_of!(IcViewerVTable, canvas_ready) as u32,
        view: &window,
        open: opens,
        closed: None,
        content: None,
        closing: None,
        canvas_ready: Some(ready),
        canvas_draw: Some(draw),
        canvas_gone: Some(gone),
    };
    let (id, extensions) = (
        CString::new("older-viewer").expect("an id"),
        CString::new(".probe").expect("extensions"),
    );
    assert_eq!(
        (host.register_viewer)(
            id.as_ptr(),
            extensions.as_ptr(),
            0,
            &table,
            &MINE as *const u8 as *mut c_void,
        ),
        IC_OK
    );

    assert!(!ic_plugin_host::viewer_draws("older-viewer"));
    assert!(!ic_plugin_host::viewer_canvas_ready(
        "older-viewer",
        1,
        &canvas()
    ));
    ic_plugin_host::viewer_canvas_draw("older-viewer", 1, &frame());
    ic_plugin_host::viewer_canvas_gone("older-viewer", 1);

    assert!(
        said().lock().unwrap().is_empty(),
        "the host read past what the plugin claimed: {:?}",
        said().lock().unwrap()
    );
}
