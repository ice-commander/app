//! Which plugin is asked to show a file, and how the host refuses one that
//! could not show anything.
//!
//! The registry is the whole of the decision: by extension and nothing else,
//! because reading the first bytes of every file on a server is a request per
//! file for a name the application already has.

use ic_plugin_api::{IcBytes, IcFsSource, IcHost, IcViewVTable, IcViewerVTable, IC_OK};
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};

thread_local! {
    /// The context the window was last asked with, so a test can see what the
    /// host put in it.
    static ASKED_WITH: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

extern "C" fn describe(ctx: *const u8, len: u64, _: *mut c_void) -> IcBytes {
    if !ctx.is_null() && len > 0 {
        let held =
            String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ctx, len as usize) })
                .into_owned();
        ASKED_WITH.with(|seen| *seen.borrow_mut() = held);
    }
    const DOCUMENT: &str = r#"{"schema":1,"fields":[],"form":{"t":"view","surface":"window",
        "children":[{"t":"image","id":"picture","src":"part:page","fit":"contain"}]}}"#;
    IcBytes {
        data: DOCUMENT.as_ptr(),
        len: DOCUMENT.len() as u64,
    }
}

extern "C" fn opens(_: u64, _: IcFsSource, _: *const c_char, _: *mut c_void) -> c_int {
    IC_OK
}

fn window() -> IcViewVTable {
    IcViewVTable {
        struct_size: std::mem::size_of::<IcViewVTable>() as u32,
        describe,
        on_event: None,
        closed: None,
    }
}

fn viewer(view: *const IcViewVTable) -> IcViewerVTable {
    IcViewerVTable {
        struct_size: std::mem::size_of::<IcViewerVTable>() as u32,
        view,
        open: opens,
        closed: None,
        content: None,
        closing: None,
        canvas_ready: None,
        canvas_draw: None,
        canvas_gone: None,
    }
}

fn register(host: &IcHost, id: &str, extensions: &str, priority: i32) -> c_int {
    let window = window();
    let table = viewer(&window);
    let (id, extensions) = (
        CString::new(id).expect("an id"),
        CString::new(extensions).expect("extensions"),
    );
    (host.register_viewer)(
        id.as_ptr(),
        extensions.as_ptr(),
        priority,
        &table,
        std::ptr::null_mut(),
    )
}

/// The registry is one per process, and these tests each put their own
/// plugins in it, so they take their turn rather than overlap.
fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn the_longest_claim_takes_the_file() {
    let _turn = one_at_a_time();
    let host = ic_plugin_host::host_table();
    assert_eq!(register(&host, "gz-view", ".gz", 0), IC_OK);
    assert_eq!(register(&host, "tarball-view", ".tar.gz,.tgz", 0), IC_OK);

    assert_eq!(
        ic_plugin_host::viewer_for("holiday.tar.gz").as_deref(),
        Some("tarball-view"),
        "a plugin claiming .gz does not take a file claimed by .tar.gz"
    );
    assert_eq!(
        ic_plugin_host::viewer_for("holiday.gz").as_deref(),
        Some("gz-view")
    );
    assert_eq!(
        ic_plugin_host::viewer_for("holiday.txt"),
        None,
        "and what nobody claims is left to the application's own viewer"
    );
}

/// Two plugins offering the same kind of file: the larger priority wins, and
/// the answer does not change from one question to the next.
#[test]
fn priority_settles_a_tie_and_the_answer_holds() {
    let _turn = one_at_a_time();
    let host = ic_plugin_host::host_table();
    assert_eq!(register(&host, "plain-pdf", ".pdf", 0), IC_OK);
    assert_eq!(register(&host, "better-pdf", ".pdf", 10), IC_OK);

    assert_eq!(
        ic_plugin_host::viewer_for("report.pdf").as_deref(),
        Some("better-pdf")
    );
    assert_eq!(
        ic_plugin_host::viewer_for("report.pdf").as_deref(),
        Some("better-pdf"),
        "asked twice, answered the same"
    );
    assert_eq!(
        ic_plugin_host::viewer_for("REPORT.PDF").as_deref(),
        Some("better-pdf"),
        "the name is read whatever its case"
    );
}

/// Registering the same id again replaces what it claimed rather than leaving
/// two of it — a plugin reloaded must not accumulate.
#[test]
fn registering_the_same_viewer_again_replaces_it() {
    let _turn = one_at_a_time();
    let host = ic_plugin_host::host_table();
    assert_eq!(register(&host, "shifting", ".probe1", 0), IC_OK);
    assert_eq!(register(&host, "shifting", ".probe2", 0), IC_OK);

    let held: Vec<_> = ic_plugin_host::viewers_offered()
        .into_iter()
        .filter(|(id, _, _)| id == "shifting")
        .collect();
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].1, vec![".probe2".to_string()]);
}

/// A viewer that could not put anything on the screen is turned away at
/// registration, where it can be said plainly — not when the user presses F3
/// and gets an empty window.
#[test]
fn a_viewer_that_could_show_nothing_is_refused() {
    let _turn = one_at_a_time();
    let host = ic_plugin_host::host_table();
    let window = window();

    let nameless = viewer(&window);
    let nothing = CString::new("").expect("an id");
    let claimed = CString::new(".refused").expect("extensions");
    assert_ne!(
        (host.register_viewer)(
            nothing.as_ptr(),
            claimed.as_ptr(),
            0,
            &nameless,
            std::ptr::null_mut()
        ),
        IC_OK,
        "a viewer with no id is nothing anyone could ask for"
    );

    assert_ne!(
        register(&host, "claims-nothing", "", 0),
        IC_OK,
        "a viewer claiming no extension would never be reached"
    );
    assert_ne!(
        register(&host, "claims-nonsense", "pdf", 0),
        IC_OK,
        "an extension without its dot is not an extension"
    );

    let windowless = viewer(std::ptr::null());
    let id = CString::new("no-window").expect("an id");
    let claimed = CString::new(".refused").expect("extensions");
    assert_ne!(
        (host.register_viewer)(
            id.as_ptr(),
            claimed.as_ptr(),
            0,
            &windowless,
            std::ptr::null_mut()
        ),
        IC_OK,
        "a viewer with no window behind it has nothing to draw"
    );

    assert_eq!(ic_plugin_host::viewer_for("x.refused"), None);
}

/// The window a viewer brings is built on the stack of `init` and gone by the
/// time anything is drawn. If the host kept the pointer instead of copying the
/// table, this is the test that would read rubbish.
#[test]
fn the_window_behind_a_viewer_still_answers_after_registration_returned() {
    let _turn = one_at_a_time();
    let host = ic_plugin_host::host_table();
    {
        let window = window();
        let table = viewer(&window);
        let id = CString::new("stack-view").expect("an id");
        let claimed = CString::new(".stackprobe").expect("extensions");
        assert_eq!(
            (host.register_viewer)(
                id.as_ptr(),
                claimed.as_ptr(),
                0,
                &table,
                std::ptr::null_mut()
            ),
            IC_OK
        );
    }
    let mut room = [0u8; 4096];
    room.fill(0xAA);
    std::hint::black_box(&room);

    let drawn = ic_plugin_host::with_viewers(|viewers| {
        let held = viewers
            .iter()
            .find(|viewer| viewer.id == "stack-view")
            .expect("registered");
        let describe = held.describe_fn().expect("it describes itself");
        let answered = describe(std::ptr::null(), 0, held.user_data as *mut c_void);
        (!answered.data.is_null()).then(|| {
            String::from_utf8_lossy(unsafe {
                std::slice::from_raw_parts(answered.data, answered.len as usize)
            })
            .into_owned()
        })
    });
    assert!(
        drawn.is_some_and(|document| document.contains("\"t\":\"image\"")),
        "the window answered with what it draws"
    );
}

/// A plugin may have two files open at once, and `describe` is one call on one
/// table. Which window is being asked about travels in the context.
#[test]
fn the_window_being_asked_about_is_named_in_the_context() {
    let _turn = one_at_a_time();
    let host = ic_plugin_host::host_table();
    assert_eq!(register(&host, "instanced", ".instanceprobe", 0), IC_OK);

    let facts = ic_view_session::HostFacts::new("gtk", "en", &[]);
    let context = ic_view_session::context(&facts, "null");
    assert!(ic_plugin_host::describe_viewer("instanced", 7, &context).is_some());
    let asked: serde_json::Value =
        serde_json::from_str(&ASKED_WITH.with(|seen| seen.borrow().clone())).expect("JSON");
    assert_eq!(asked["instance"], 7);
    assert_eq!(
        asked["host"]["kind"], "gtk",
        "and what was there stays there"
    );

    assert!(ic_plugin_host::describe_viewer("instanced", 9, &context).is_some());
    let asked: serde_json::Value =
        serde_json::from_str(&ASKED_WITH.with(|seen| seen.borrow().clone())).expect("JSON");
    assert_eq!(asked["instance"], 9);
}

/// The pixels behind a `part:` are the plugin's, and only good until it is
/// asked again — so the host copies them as it reads them.
#[test]
fn a_picture_the_plugin_makes_is_copied_out_of_its_memory() {
    extern "C" fn page(_: u64, name: *const c_char, _: *mut c_void) -> IcBytes {
        const PIXELS: &[u8] = b"<svg id=\"page\"/>";
        let asked = unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy();
        if asked != "page" {
            return IcBytes::EMPTY;
        }
        IcBytes {
            data: PIXELS.as_ptr(),
            len: PIXELS.len() as u64,
        }
    }
    let _turn = one_at_a_time();
    let host = ic_plugin_host::host_table();
    let window = window();
    let mut table = viewer(&window);
    table.content = Some(page);
    let (id, claimed) = (
        CString::new("makes-pictures").expect("an id"),
        CString::new(".partprobe").expect("extensions"),
    );
    assert_eq!(
        (host.register_viewer)(
            id.as_ptr(),
            claimed.as_ptr(),
            0,
            &table,
            std::ptr::null_mut()
        ),
        IC_OK
    );

    assert_eq!(
        ic_plugin_host::viewer_part("makes-pictures", 1, "page"),
        Some(b"<svg id=\"page\"/>".to_vec())
    );
    assert_eq!(
        ic_plugin_host::viewer_part("makes-pictures", 1, "nothing-of-the-sort"),
        None,
        "a name the plugin does not know answers nothing rather than rubbish"
    );
    assert_eq!(
        ic_plugin_host::viewer_part("no-such-viewer", 1, "page"),
        None
    );
}

/// A plugin may hold its window open — something unfinished in it — and one
/// that says nothing lets every window go.
#[test]
fn a_plugin_may_refuse_to_let_its_window_close() {
    extern "C" fn not_yet(_: u64, _: *mut c_void) -> c_int {
        ic_plugin_api::IC_ERR_IO
    }
    let _turn = one_at_a_time();
    let host = ic_plugin_host::host_table();
    let window = window();

    let mut holds_on = viewer(&window);
    holds_on.closing = Some(not_yet);
    let (id, claimed) = (
        CString::new("holds-on").expect("an id"),
        CString::new(".holdprobe").expect("extensions"),
    );
    assert_eq!(
        (host.register_viewer)(
            id.as_ptr(),
            claimed.as_ptr(),
            0,
            &holds_on,
            std::ptr::null_mut()
        ),
        IC_OK
    );
    assert!(
        !ic_plugin_host::viewer_may_close("holds-on", 1),
        "the plugin said no, so the window stays"
    );

    assert_eq!(register(&host, "lets-go", ".letgoprobe", 0), IC_OK);
    assert!(
        ic_plugin_host::viewer_may_close("lets-go", 1),
        "a plugin that says nothing about closing lets every window go"
    );
    assert!(
        ic_plugin_host::viewer_may_close("no-such-viewer", 1),
        "and so does one that is not there at all"
    );
}
