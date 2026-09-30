//! Loads the deployed SDK examples through the real loader and checks that each
//! one registered what its README claims.

#[test]
#[ignore = "needs the SDK deployed: run ./build.sh && ./deploy-local.sh in plugin-sdk/"]
fn the_sdk_examples_register_what_they_promise() {
    let config = client_config::AppConfig::new("ice-commander-sdk-live");
    let chosen: Vec<String> = ic_plugin_host::loader::installed()
        .iter()
        .map(|path| ic_plugin_host::loader::name_of_library(path))
        .filter(|name| name.starts_with("sdk_"))
        .collect();
    assert_eq!(chosen.len(), 8, "expected eight examples, found {chosen:?}");
    ic_plugin_host::loader::set_chosen(&config, &chosen);

    for attempt in ic_plugin_host::loader::load_for(&config) {
        assert!(attempt.loaded(), "{attempt:?}");
        let about = attempt
            .about
            .clone()
            .expect("every example says what it is");
        eprintln!("{:<22} {:<20} v{}", attempt.name, about.name, about.version);
    }

    // 1 + 2: a window each, opened from a button
    let views = ic_plugin_host::view_ids();
    assert!(
        views.iter().any(|id| id == "sdk.hello.toolbar"),
        "{views:?}"
    );
    assert!(views.iter().any(|id| id == "sdk.hello.header"), "{views:?}");
    for id in ["sdk.hello.toolbar", "sdk.hello.header"] {
        let described = ic_plugin_host::describe_view(
            id,
            &serde_json::json!({ "host": { "kind": "gtk", "locale": "en" } }),
        )
        .expect("the host accepted the document");
        assert!(!described.form.children.is_empty());
    }
    assert_eq!(
        ic_plugin_host::toolbar_entries(ic_plugin_api::IC_SIDE_RIGHT)
            .iter()
            .filter(|entry| entry.id == "sdk.hello.toolbar")
            .count(),
        1,
        "the toolbar button must be on the side the panel actually reads"
    );
    assert!(ic_plugin_host::header_entries(ic_plugin_api::IC_SIDE_LEFT)
        .iter()
        .any(|entry| entry.id == "sdk.hello.header"));

    // 3: an extension that opens like a folder
    assert!(
        fm_core::plugin_fs::handles_extension("greeting.hello"),
        "the .hello extension is not registered"
    );
    assert_eq!(
        fm_core::plugin_fs::mount_label("greeting.hello").as_deref(),
        Some("HELLO")
    );

    // 3b: an event round trip, answered by the plugin itself
    let answered = ic_plugin_host::view_event(
        "sdk.hello.events",
        &serde_json::json!({ "type": "activate", "node": "shout", "values": { "text": "hi" } }),
    )
    .expect("the host relayed the press");
    assert_eq!(answered["put"]["field"], serde_json::json!("HI"));

    // 5: a filesystem that dresses its own panel
    assert!(
        fm_core::plugin_fs::handles_extension("chores.checklist"),
        "the .checklist extension is not registered"
    );
    assert!(
        !ic_plugin_host::default_toolbar_shown(".checklist"),
        "it asked to draw its own toolbar"
    );
    let mine: Vec<String> = ic_plugin_host::fs_actions()
        .into_iter()
        .filter(|action| action.belongs_to(".checklist"))
        .map(|action| action.action_id)
        .collect();
    assert_eq!(
        mine,
        vec![
            "checklist.tick_selected".to_string(),
            "checklist.tick_all".to_string(),
            "checklist.untick_all".to_string(),
            "checklist.only_remaining".to_string(),
        ],
        "and to put four buttons of its own there"
    );
    assert!(
        ic_plugin_host::fs_actions()
            .into_iter()
            .any(|action| action.action_id == "checklist.only_remaining" && action.is_toggle()),
        "the last of them stays pressed"
    );
    // And an indicator in the header, absent until there is something to say.
    let indicator = ic_plugin_host::header_entries(ic_plugin_api::IC_SIDE_RIGHT)
        .into_iter()
        .find(|entry| entry.id == "checklist.left")
        .expect("the indicator is registered");
    assert!(
        !indicator.shown,
        "nothing is open, so it should not be on the header yet"
    );

    // 6: entries of a plugin's own beside the disks
    let shelves: Vec<String> = ic_plugin_host::plugin_drives()
        .into_iter()
        .filter(|drive| drive.kind == "sdk.shelf")
        .map(|drive| drive.key)
        .collect();
    assert_eq!(
        shelves,
        vec!["sdk.shelf.upper".to_string(), "sdk.shelf.lower".to_string()],
        "the shelves should be offered beside the real drives"
    );
    assert!(
        ic_plugin_host::connection_kind_ids().contains(&"sdk.shelf".to_string()),
        "and mounted through a kind of their own"
    );
    assert!(ic_plugin_host::connection_document("sdk.shelf").is_none());
    assert!(
        ic_plugin_host::pinned_connections()
            .iter()
            .any(|pin| pin.view == "sdk.shelves"),
        "with an entry of their own in the connections list"
    );

    // 7: a viewer of its own, offered for the extension it claimed
    assert_eq!(
        ic_plugin_host::viewer_for("notes.test").as_deref(),
        Some("hello-view"),
        "F3 on a .test file should reach the example"
    );

    // 4: a connection kind with a form
    let document = ic_plugin_host::connection_document("website").expect("the kind is registered");
    assert_eq!(document.kind, "website");
    assert!(document.field("url").expect("declared").required);
    assert!(document.field("token").expect("declared").secret);
    eprintln!(
        "connection kinds: {:?}; views: {:?}",
        ic_plugin_host::connection_kind_ids(),
        views
    );
}
