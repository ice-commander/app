//! GTK may only be used from the thread that started it, and the test harness
//! runs every test on a thread of its own, so these checks share one `main`.

use adw::prelude::{Cast, WidgetExt};
use ic_view::{Document, State};
use ic_view_gtk::Renderer;
use serde_json::json;
use std::rc::Rc;

fn document() -> Document {
    serde_json::from_value(json!({
        "schema": 1,
        "kind": "example",
        "fields": [
            { "bind": "host", "type": "text" },
            { "bind": "pass", "type": "text", "secret": true },
            { "bind": "use_tunnel", "type": "bool" },
        ],
        "form": { "t": "column", "children": [
            { "t": "input", "id": "host", "bind": "host", "emit": "change" },
            { "t": "input", "id": "pass", "bind": "pass", "emit": "change" },
            { "t": "switch", "id": "use_tunnel", "bind": "use_tunnel", "emit": "change" },
        ]},
    }))
    .expect("a document")
}

fn renderer() -> Renderer {
    Renderer::new(Rc::new(|_: &str| None))
}

fn a_value_the_plugin_sets_reaches_the_control_bound_to_it_and_is_read_back_from_it() {
    let mut state = State::for_document(&document());
    let built = renderer().build(&document(), &state);
    state.set_state("host", json!("set.example.org"));
    state.set_state("use_tunnel", json!(true));
    built.refresh(&state);
    assert_eq!(built.values().get("host"), Some(&json!("set.example.org")));
    assert_eq!(built.values().get("use_tunnel"), Some(&json!(true)));
}

fn a_value_the_plugin_sets_is_not_reported_back_as_something_the_user_typed() {
    let mut state = State::for_document(&document());
    let built = renderer().build(&document(), &state);
    built.on_change("host", |_, _| {});
    built.on_change("use_tunnel", |_, _| {});
    state.set_state("host", json!("set.example.org"));
    state.set_state("use_tunnel", json!(true));
    built.refresh(&state);
    built.set_value("host", &json!("put.example.org"));
    assert!(built.touched().is_empty(), "{:?}", built.touched());
}

fn a_masked_box_is_not_filled_with_the_secret_the_record_already_stores() {
    let mut state = State::for_document(&document());
    state.set_state("pass", json!("hunter2"));
    state.stored_secrets.insert("pass".to_string());
    let built = renderer().build(&document(), &state);
    built.refresh(&state);
    assert_eq!(built.values().get("pass"), Some(&json!("")));
}

fn a_secret_the_plugin_generated_does_reach_the_box_because_no_record_holds_it() {
    let mut state = State::for_document(&document());
    let built = renderer().build(&document(), &state);
    state.set_state("pass", json!("generated"));
    built.refresh(&state);
    assert_eq!(built.values().get("pass"), Some(&json!("generated")));
}

fn a_value_the_user_typed_is_left_alone_by_a_refresh_that_carries_no_other_word() {
    let state = State::for_document(&document());
    let built = renderer().build(&document(), &state);
    built.set_value("host", &json!("typed.example.org"));
    built.refresh(&state);
    assert_eq!(
        built.values().get("host"),
        Some(&json!("typed.example.org"))
    );
}

fn a_canvas_node_is_a_widget_the_document_can_size() {
    let document: Document = serde_json::from_value(json!({
        "schema": 1,
        "kind": "example",
        "fields": [],
        "form": { "t": "column", "children": [
            { "t": "canvas", "id": "picture", "width": 320, "height": 180 },
        ]},
    }))
    .expect("a document");
    let state = State::for_document(&document);
    let built = renderer().build(&document, &state);
    let widget = built.widget("picture").expect("the canvas was built");
    assert_eq!(
        (widget.width_request(), widget.height_request()),
        (320, 180)
    );
}

fn a_canvas_asks_the_host_once_and_keeps_the_place_across_a_rebuild() {
    let document: Document = serde_json::from_value(json!({
        "schema": 1,
        "kind": "example",
        "fields": [],
        "form": { "t": "column", "children": [
            { "t": "text", "text": "above" },
            { "t": "canvas", "id": "picture", "width": 320, "height": 180 },
        ]},
    }))
    .expect("a document");
    let asked = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let counting = asked.clone();
    let renderer =
        Renderer::new(Rc::new(|_: &str| None)).with_canvas(Rc::new(move |named: &str| {
            counting.borrow_mut().push(named.to_string());
            Some(gtk::Box::new(gtk::Orientation::Vertical, 0).upcast())
        }));

    let state = State::for_document(&document);
    let built = renderer.build(&document, &state);
    assert_eq!(asked.borrow().clone(), vec!["picture".to_string()]);
    let first = built.widget("picture").expect("the canvas was built");

    let again = renderer.build_over(&document, &state, Some(&built));
    assert_eq!(
        asked.borrow().clone(),
        vec!["picture".to_string()],
        "the place was made a second time"
    );
    assert_eq!(
        again.widget("picture"),
        Some(first),
        "the rebuilt window draws somewhere else"
    );
}

fn a_player_is_handed_the_media_node_so_its_end_can_be_reported() {
    let document: Document = serde_json::from_value(json!({
        "schema": 1,
        "kind": "example",
        "fields": [],
        "form": { "t": "column", "children": [
            { "t": "media", "id": "track", "media": "audio", "src": "file:one.mp3", "autoplay": true },
        ]},
    }))
    .expect("a document");
    let handed = Rc::new(std::cell::RefCell::new(
        Vec::<(String, Option<String>, bool)>::new(),
    ));
    let seen = handed.clone();
    let renderer = Renderer::new(Rc::new(|_: &str| None))
        .with_media(Rc::new(|named: &str| {
            named
                .strip_prefix("file:")
                .map(|path| format!("/played/{path}"))
        }))
        .with_player(Rc::new(move |at: &str, node: &ic_view::Node| {
            seen.borrow_mut()
                .push((at.to_string(), node.id.clone(), node.autoplay));
            Some(gtk::Box::new(gtk::Orientation::Vertical, 0).upcast())
        }));
    renderer.build(&document, &State::for_document(&document));
    assert_eq!(
        handed.borrow().clone(),
        vec![(
            "/played/one.mp3".to_string(),
            Some("track".to_string()),
            true
        )]
    );
}

fn a_declared_key_reaches_the_plugin_without_a_control_and_the_player_is_found_by_its_widget() {
    let document: Document = serde_json::from_value(json!({
        "schema": 1,
        "kind": "example",
        "fields": [],
        "form": { "t": "column", "children": [
            { "t": "media", "id": "track", "media": "audio", "src": "file:one.mp3" },
        ]},
        "keys": [ { "accel": "Left", "node": "back" }, { "accel": "ctrl+Right", "node": "skip" } ],
    }))
    .expect("a document");
    let handed: Rc<std::cell::RefCell<Option<gtk::Widget>>> = Rc::default();
    let giving = handed.clone();
    let renderer = renderer()
        .with_media(Rc::new(|named: &str| Some(named.to_string())))
        .with_player(Rc::new(move |_: &str, _: &ic_view::Node| {
            let widget: gtk::Widget = gtk::Box::new(gtk::Orientation::Vertical, 0).upcast();
            *giving.borrow_mut() = Some(widget.clone());
            Some(widget)
        }));
    let built = renderer.build(&document, &State::for_document(&document));
    let pressed = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let seen = pressed.clone();
    built.on_key(move |node| seen.borrow_mut().push(node));
    let keys = built.document_keys();
    let none = gtk::gdk::ModifierType::empty();
    assert!(keys.press(gtk::gdk::Key::Left, none));
    assert!(!keys.press(gtk::gdk::Key::Right, none));
    assert!(keys.press(gtk::gdk::Key::Right, gtk::gdk::ModifierType::CONTROL_MASK));
    assert_eq!(pressed.borrow().clone(), vec!["back", "skip"]);
    let player = handed.borrow().clone().expect("the player was asked for");
    assert_eq!(built.id_holding(&player).as_deref(), Some("track"));
}

fn main() {
    if adw::init().is_err() {
        eprintln!("controls: no display, nothing checked");
        return;
    }
    let checks: Vec<(&str, fn())> = vec![
        (
            "a_value_the_plugin_sets_reaches_the_control_bound_to_it_and_is_read_back_from_it",
            a_value_the_plugin_sets_reaches_the_control_bound_to_it_and_is_read_back_from_it,
        ),
        (
            "a_value_the_plugin_sets_is_not_reported_back_as_something_the_user_typed",
            a_value_the_plugin_sets_is_not_reported_back_as_something_the_user_typed,
        ),
        (
            "a_masked_box_is_not_filled_with_the_secret_the_record_already_stores",
            a_masked_box_is_not_filled_with_the_secret_the_record_already_stores,
        ),
        (
            "a_secret_the_plugin_generated_does_reach_the_box_because_no_record_holds_it",
            a_secret_the_plugin_generated_does_reach_the_box_because_no_record_holds_it,
        ),
        (
            "a_value_the_user_typed_is_left_alone_by_a_refresh_that_carries_no_other_word",
            a_value_the_user_typed_is_left_alone_by_a_refresh_that_carries_no_other_word,
        ),
        (
            "a_canvas_node_is_a_widget_the_document_can_size",
            a_canvas_node_is_a_widget_the_document_can_size,
        ),
        (
            "a_canvas_asks_the_host_once_and_keeps_the_place_across_a_rebuild",
            a_canvas_asks_the_host_once_and_keeps_the_place_across_a_rebuild,
        ),
        (
            "a_player_is_handed_the_media_node_so_its_end_can_be_reported",
            a_player_is_handed_the_media_node_so_its_end_can_be_reported,
        ),
        (
            "a_declared_key_reaches_the_plugin_without_a_control_and_the_player_is_found_by_its_widget",
            a_declared_key_reaches_the_plugin_without_a_control_and_the_player_is_found_by_its_widget,
        ),
    ];
    for (name, check) in checks {
        check();
        println!("controls: {name} ... ok");
    }
}
