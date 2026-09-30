//! A plugin learns things on its own thread. These check that what it asks for
//! gets to the thread that is allowed to do it, and nowhere else.

use std::sync::{Arc, Mutex};

fn register_a_view(id: &str) {
    let document = serde_json::json!({
        "schema": 1, "kind": id,
        "form": { "t": "column", "children": [] },
    })
    .to_string();
    extern "C" fn describe(
        _ctx: *const u8,
        _len: u64,
        _user: *mut std::os::raw::c_void,
    ) -> ic_plugin_api::IcBytes {
        ic_plugin_api::IcBytes::EMPTY
    }
    let table = ic_plugin_api::IcViewVTable {
        struct_size: std::mem::size_of::<ic_plugin_api::IcViewVTable>() as u32,
        describe,
        on_event: None,
        closed: None,
    };
    let host = ic_plugin_host::host_table();
    let (name, title) = (
        std::ffi::CString::new(id).unwrap(),
        std::ffi::CString::new("probe").unwrap(),
    );
    assert_eq!(
        (host.register_view)(name.as_ptr(), title.as_ptr(), &table, std::ptr::null_mut()),
        ic_plugin_api::IC_OK
    );
    let _ = document;
}

/// The waker is global and these tests run in parallel, so they share one
/// collector and each looks only for its own view.
fn collected() -> &'static Mutex<Vec<ic_plugin_host::Wanted>> {
    static HELD: std::sync::OnceLock<Mutex<Vec<ic_plugin_host::Wanted>>> =
        std::sync::OnceLock::new();
    static ONCE: std::sync::Once = std::sync::Once::new();
    let held = HELD.get_or_init(|| Mutex::new(Vec::new()));
    ONCE.call_once(|| {
        ic_plugin_host::set_waker(Arc::new(|wanted| {
            collected().lock().unwrap().push(wanted);
        }));
    });
    held
}

fn asked_for(view: &str) -> Vec<ic_plugin_host::Wanted> {
    collected()
        .lock()
        .unwrap()
        .iter()
        .filter(|wanted| match wanted {
            ic_plugin_host::Wanted::Open { view: held, .. } => held == view,
            ic_plugin_host::Wanted::Invalidate { view: held } => held == view,
            ic_plugin_host::Wanted::HeaderLabel { .. } => false,
            ic_plugin_host::Wanted::FormChanged { .. } => false,
            ic_plugin_host::Wanted::Redraw { .. } => false,
            ic_plugin_host::Wanted::DrivesChanged => false,
            ic_plugin_host::Wanted::PinnedConnectionsChanged => false,
            ic_plugin_host::Wanted::FsInvalidate { .. } => false,
            ic_plugin_host::Wanted::FsChanged { .. } => false,
            ic_plugin_host::Wanted::HeaderVisible { .. } => false,
            ic_plugin_host::Wanted::HeaderIcon { .. } => false,
        })
        .cloned()
        .collect()
}

#[test]
fn a_request_from_another_thread_reaches_the_frontend() {
    register_a_view("probe.view");
    collected();

    let host = ic_plugin_host::host_table();
    let invalidate = host.view_invalidate;
    let worker = std::thread::spawn(move || {
        let id = std::ffi::CString::new("probe.view").unwrap();
        invalidate(id.as_ptr())
    });
    assert_eq!(worker.join().unwrap(), ic_plugin_api::IC_OK);

    assert_eq!(
        asked_for("probe.view"),
        vec![ic_plugin_host::Wanted::Invalidate {
            view: "probe.view".to_string()
        }],
        "the request crossed the thread instead of being dropped"
    );
}

#[test]
fn an_argument_survives_the_crossing() {
    register_a_view("probe.arg");
    collected();

    let host = ic_plugin_host::host_table();
    let open = host.open_view;
    std::thread::spawn(move || {
        let id = std::ffi::CString::new("probe.arg").unwrap();
        let argument = br#"{"peer":"abc"}"#;
        open(id.as_ptr(), argument.as_ptr(), argument.len() as u64)
    })
    .join()
    .unwrap();

    assert_eq!(
        asked_for("probe.arg"),
        vec![ic_plugin_host::Wanted::Open {
            view: "probe.arg".to_string(),
            argument: r#"{"peer":"abc"}"#.to_string(),
        }]
    );
}

#[test]
fn a_view_nobody_registered_is_refused_before_any_thread_is_woken() {
    collected();
    let host = ic_plugin_host::host_table();
    let id = std::ffi::CString::new("probe.absent").unwrap();
    assert_ne!((host.view_invalidate)(id.as_ptr()), ic_plugin_api::IC_OK);
    assert!(
        asked_for("probe.absent").is_empty(),
        "nothing was posted for a view that does not exist"
    );
}

#[test]
fn a_peer_count_from_the_plugins_own_thread_is_carried_to_the_frontend() {
    collected();

    let host = ic_plugin_host::host_table();
    let relabel = host.set_header_label;
    let worker = std::thread::spawn(move || {
        let id = std::ffi::CString::new("nodeinnet.peers").unwrap();
        let label = std::ffi::CString::new("Node In Net \u{b7} 2").unwrap();
        relabel(id.as_ptr(), label.as_ptr())
    });
    assert_eq!(worker.join().unwrap(), ic_plugin_api::IC_OK);

    let posted = collected().lock().unwrap().iter().any(|wanted| {
        matches!(
            wanted,
            ic_plugin_host::Wanted::HeaderLabel { id, label }
                if id == "nodeinnet.peers" && label == "Node In Net \u{b7} 2"
        )
    });
    assert!(
        posted,
        "the request waits for the frontend rather than touching its registry"
    );
}

#[test]
fn a_frame_ready_on_a_worker_thread_is_handed_to_the_frontend() {
    const INSTANCE: u64 = 0x00c0_ffee;
    collected();

    let invalidate = ic_plugin_host::host_table().canvas_invalidate;
    let worker = std::thread::spawn(move || invalidate(INSTANCE));
    assert_eq!(worker.join().unwrap(), ic_plugin_api::IC_OK);

    assert!(
        collected()
            .lock()
            .unwrap()
            .iter()
            .any(|wanted| *wanted == ic_plugin_host::Wanted::Redraw { instance: INSTANCE }),
        "canvas_invalidate did not reach the frontend as a redraw"
    );
}
