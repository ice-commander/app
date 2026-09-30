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
    ];
    for (name, check) in checks {
        check();
        println!("controls: {name} ... ok");
    }
}
