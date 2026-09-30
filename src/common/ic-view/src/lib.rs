pub mod commit;
pub mod doc;
pub mod expr;
pub mod validate;

pub use commit::{applicable, commit, immutable_now, render_summary, Commit, CommitProblem};
pub use doc::{
    Action, Case, Choice, Chrome, Column, Cond, Decode, Derive, Document, Emit, Field, FieldType,
    Fit, InputVariant, Intent, MediaKind, Node, NodeKind, OnParseError, PickMode, Picker, Pred,
    PredOp, Scope, Scroll, Series, Summary, Surface, Text, NODE_KINDS, SCHEMA,
};
pub use expr::{
    as_text, choice_index, display, evaluate, fill_template, is_truthy, resolve, resolve_text,
    series_bounds, series_of, substitute, substitute_with, values_equal, visible, State,
};
pub use validate::{accept, errors, translation_keys, validate, validate_source, Issue, Severity};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with_form(fields: serde_json::Value, children: serde_json::Value) -> Document {
        let source = json!({
            "schema": 1,
            "kind": "example",
            "fields": fields,
            "form": { "t": "column", "children": children }
        })
        .to_string();
        Document::parse(&source).expect("parses")
    }

    #[test]
    fn a_predicate_that_reads_data_where_the_user_types_is_flagged() {
        let document = with_form(
            json!([{ "bind": "login", "type": "text" }]),
            json!([
                { "t": "input", "id": "login", "bind": "login" },
                { "t": "button", "id": "go", "label": "Go",
                  "sensitive": { "not": { "empty": "data.login" } },
                  "intent": { "do": "submit" } }
            ]),
        );
        let said: Vec<String> = validate(&document)
            .into_iter()
            .map(|issue| issue.message)
            .collect();
        assert!(
            said.iter().any(|message| message.contains("data.login")),
            "{said:?}"
        );
    }

    #[test]
    fn the_same_predicate_against_state_is_not_flagged() {
        let document = with_form(
            json!([{ "bind": "login", "type": "text" }]),
            json!([
                { "t": "input", "id": "login", "bind": "login" },
                { "t": "button", "id": "go", "label": "Go",
                  "sensitive": { "not": { "empty": "state.login" } },
                  "intent": { "do": "submit" } }
            ]),
        );
        assert!(validate(&document).is_empty(), "{:?}", validate(&document));
    }

    #[test]
    fn reading_data_for_something_nobody_types_into_is_left_alone() {
        let document = with_form(
            json!([]),
            json!([
                { "t": "text", "id": "note", "text": "{data.status}",
                  "visible": { "not": { "empty": "data.status" } } }
            ]),
        );
        assert!(validate(&document).is_empty());
    }

    const REFERENCE: &str = r#"{
      "schema": 1,
      "kind": "example",
      "label": { "tr": "conn_manager.kind_webdav", "en": "WebDAV" },
      "icon": "theme:folder-remote-symbolic",
      "identity": "name",
      "immutable_after_create": ["kind"],
      "summary": { "fmt": "{url}", "fallback": { "fmt": "{host}" } },
      "fields": [
        { "bind": "name", "type": "text", "scope": "record", "required": true },
        { "bind": "folder", "type": "text", "scope": "record", "empty_as_absent": true },
        { "bind": "kind", "type": "text" },
        { "bind": "url", "type": "text", "required": true, "empty_as_absent": true },
        { "bind": "user", "type": "text", "empty_as_absent": true },
        { "bind": "pass", "type": "text", "secret": true, "empty_as_absent": true, "empty_keeps_stored": true },
        { "bind": "remote_path", "type": "path", "empty_as_absent": true },
        { "bind": "port", "type": "integer", "min": 1, "max": 65535,
          "when": false, "keep_when_inapplicable": true,
          "default": 443, "derive": "commit_only", "on_parse_error": "default" }
      ],
      "form": {
        "t": "view",
        "surface": "embedded",
        "scroll": "vertical",
        "spacing": 8,
        "sensitive": { "ne": ["view.mode", "view"] },
        "children": [
          { "t": "input", "id": "name", "bind": "name", "chrome": "bare",
            "placeholder": { "tr": "conn_manager.name_placeholder", "en": "Name" } },
          { "t": "input", "id": "url", "bind": "url", "chrome": "bare",
            "placeholder": { "tr": "conn_manager.webdav_url_placeholder", "en": "https://dav.example.org/remote.php/dav" } },
          { "t": "row", "id": "credentials", "spacing": 8, "children": [
            { "t": "input", "id": "user", "bind": "user", "chrome": "bare", "weight": 1,
              "placeholder": { "tr": "conn_manager.user", "en": "User" } },
            { "t": "input", "id": "pass", "bind": "pass", "chrome": "bare", "weight": 1,
              "variant": "masked",
              "placeholder": { "cases": [
                  { "when": { "has_stored": "pass" },
                    "then": { "tr": "conn_manager.password_stored", "en": "Password stored, leave blank to keep" } } ],
                "else": { "tr": "conn_manager.password", "en": "Password" } } } ] },
          { "t": "input", "id": "remote_path", "bind": "remote_path", "chrome": "bare",
            "placeholder": { "tr": "conn_manager.remote_path", "en": "Start folder" } }
        ]
      },
      "actions": [
        { "id": "connect",
          "label": { "tr": "conn_manager.connect", "en": "Connect" },
          "visible": { "all": [ { "truthy": "host.can_connect" }, { "eq": ["view.mode", "view"] } ] },
          "role": "normal",
          "intent": { "do": "connect" } },
        { "id": "primary",
          "label": { "cases": [
              { "when": { "eq": ["view.mode", "new"] },
                "then": { "tr": "conn_manager.add_connection", "en": "Add connection" } },
              { "when": { "eq": ["view.mode", "view"] },
                "then": { "tr": "conn_manager.edit_btn", "en": "Edit" } } ],
            "else": { "tr": "conn_manager.save_connection", "en": "Save connection" } },
          "role": { "cases": [
              { "when": { "all": [ { "eq": ["view.mode", "view"] }, { "truthy": "host.can_connect" } ] },
                "then": "normal" } ],
            "else": "primary" },
          "intent": { "cases": [
              { "when": { "eq": ["view.mode", "view"] },
                "then": { "do": "set", "keys": { "view.mode": "edit" } } } ],
            "else": { "do": "submit" } } },
        { "id": "cancel",
          "label": { "tr": "common.cancel", "en": "Cancel" },
          "visible": { "ne": ["view.mode", "view"] },
          "intent": { "cases": [
              { "when": { "eq": ["view.mode", "edit"] }, "then": { "do": "revert" } } ],
            "else": { "do": "close" } } }
      ]
    }"#;

    fn no_translation(_key: &str) -> Option<String> {
        None
    }

    fn reference() -> Document {
        Document::parse(REFERENCE).expect("the reference document parses")
    }

    fn saved_record() -> State {
        let mut state = State::default();
        state
            .set_state("name", json!("cloud"))
            .set_state("kind", json!("example"))
            .set_state("url", json!("https://dav.example.org/remote.php/dav"))
            .set_state("user", json!("ivan"))
            .set_state("pass", json!(""))
            .set_state("remote_path", json!("/Documents"))
            .set_view("mode", json!("view"))
            .set_host("can_connect", json!(true));
        state.stored_secrets.insert("pass".to_string());
        state
    }

    #[test]
    fn the_reference_document_has_no_errors_and_no_dropped_properties() {
        let (document, issues) = validate_source(REFERENCE).expect("parses");
        assert_eq!(document.kind, "example");
        let blocking: Vec<&Issue> = issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
            .collect();
        assert!(blocking.is_empty(), "errors: {blocking:?}");
        let dropped: Vec<&Issue> = issues
            .iter()
            .filter(|issue| issue.message.contains("could not be understood"))
            .collect();
        assert!(dropped.is_empty(), "dropped: {dropped:?}");
    }

    #[test]
    fn a_saved_record_summarises_as_its_url() {
        let document = reference();
        let rendered = render_summary(document.summary.as_ref(), &saved_record());
        assert_eq!(
            rendered.as_deref(),
            Some("https://dav.example.org/remote.php/dav")
        );
    }

    #[test]
    fn a_legacy_record_without_a_url_summarises_from_the_host_key() {
        let document = reference();
        let mut state = State::default();
        state.set_state("host", json!("https://old.example.org"));
        let rendered = render_summary(document.summary.as_ref(), &state);
        assert_eq!(rendered.as_deref(), Some("https://old.example.org"));
    }

    #[test]
    fn a_record_with_neither_url_nor_host_has_no_summary() {
        let document = reference();
        assert_eq!(
            render_summary(document.summary.as_ref(), &State::default()),
            None
        );
    }

    #[test]
    fn a_blank_password_keeps_the_stored_secret_instead_of_erasing_it() {
        let outcome = commit(&reference(), &saved_record());
        assert!(outcome.keep_stored.contains("pass"));
        assert!(!outcome.settings.contains_key("pass"));
        assert!(outcome.is_valid(), "{outcome:?}");
    }

    #[test]
    fn a_blank_password_with_nothing_stored_is_simply_absent() {
        let mut state = saved_record();
        state.stored_secrets.clear();
        let outcome = commit(&reference(), &state);
        assert!(outcome.keep_stored.is_empty());
        assert!(!outcome.settings.contains_key("pass"));
    }

    #[test]
    fn a_typed_password_is_written_and_does_not_keep_the_stored_one() {
        let mut state = saved_record();
        state.set_state("pass", json!("hunter2"));
        let outcome = commit(&reference(), &state);
        assert_eq!(
            outcome.settings.get("pass").map(String::as_str),
            Some("hunter2")
        );
        assert!(outcome.keep_stored.is_empty());
    }

    #[test]
    fn the_hidden_port_is_still_persisted_with_its_default() {
        let outcome = commit(&reference(), &saved_record());
        assert_eq!(
            outcome.settings.get("port").map(String::as_str),
            Some("443")
        );
    }

    #[test]
    fn record_scoped_fields_are_kept_apart_from_settings() {
        let mut state = saved_record();
        state.set_state("folder", json!("work/servers"));
        let outcome = commit(&reference(), &state);
        assert_eq!(
            outcome.record.get("name").map(String::as_str),
            Some("cloud")
        );
        assert_eq!(
            outcome.record.get("folder").map(String::as_str),
            Some("work/servers")
        );
        assert!(!outcome.settings.contains_key("name"));
    }

    #[test]
    fn a_blank_required_url_is_reported_missing() {
        let mut state = saved_record();
        state.set_state("url", json!(""));
        let outcome = commit(&reference(), &state);
        assert_eq!(outcome.missing, vec!["url".to_string()]);
        assert!(!outcome.is_valid());
    }

    #[test]
    fn the_stored_password_placeholder_is_chosen_only_when_a_secret_exists() {
        let document = reference();
        let mut found = None;
        document.form.walk(&mut |node| {
            if node.id.as_deref() == Some("pass") {
                found = Some(node.clone());
            }
        });
        let node = found.expect("the password input is in the form");
        let stored = resolve_text(node.placeholder.as_ref(), &saved_record(), &no_translation);
        assert_eq!(
            stored.as_deref(),
            Some("Password stored, leave blank to keep")
        );
        let mut fresh = saved_record();
        fresh.stored_secrets.clear();
        let blank = resolve_text(node.placeholder.as_ref(), &fresh, &no_translation);
        assert_eq!(blank.as_deref(), Some("Password"));
    }

    #[test]
    fn a_translation_catalogue_wins_over_the_bundled_fallback() {
        let document = reference();
        let mut found = None;
        document.form.walk(&mut |node| {
            if node.id.as_deref() == Some("user") {
                found = Some(node.clone());
            }
        });
        let node = found.expect("the user input is in the form");
        let translated = resolve_text(node.placeholder.as_ref(), &saved_record(), &|key| {
            if key == "conn_manager.user" {
                Some("Пользователь".to_string())
            } else {
                None
            }
        });
        assert_eq!(translated.as_deref(), Some("Пользователь"));
    }

    #[test]
    fn the_primary_action_follows_the_view_edit_new_mode_machine() {
        let document = reference();
        let primary = document
            .actions
            .iter()
            .find(|action| action.id == "primary")
            .expect("primary action");
        let mut state = saved_record();
        let label = |state: &State| resolve_text(primary.label.as_ref(), state, &no_translation);
        assert_eq!(label(&state).as_deref(), Some("Edit"));
        state.set_view("mode", json!("edit"));
        assert_eq!(label(&state).as_deref(), Some("Save connection"));
        state.set_view("mode", json!("new"));
        assert_eq!(label(&state).as_deref(), Some("Add connection"));
    }

    #[test]
    fn editing_turns_the_primary_action_into_a_submit_and_viewing_into_a_mode_switch() {
        let document = reference();
        let primary = document
            .actions
            .iter()
            .find(|action| action.id == "primary")
            .expect("primary action");
        let viewing = resolve(primary.intent.as_ref().unwrap(), &saved_record());
        match viewing {
            Some(Intent::Set { keys }) => {
                assert_eq!(keys.get("view.mode"), Some(&json!("edit")));
            }
            other => panic!("expected a mode switch, got {other:?}"),
        }
        let mut state = saved_record();
        state.set_view("mode", json!("edit"));
        assert_eq!(
            resolve(primary.intent.as_ref().unwrap(), &state),
            Some(Intent::Submit)
        );
    }

    #[test]
    fn connect_is_offered_only_while_viewing_a_record_on_a_host_that_can_connect() {
        let document = reference();
        let connect = document
            .actions
            .iter()
            .find(|action| action.id == "connect")
            .expect("connect action");
        assert!(visible(connect.visible.as_ref(), &saved_record()));
        let mut editing = saved_record();
        editing.set_view("mode", json!("edit"));
        assert!(!visible(connect.visible.as_ref(), &editing));
        let mut console = saved_record();
        console.set_host("can_connect", json!(false));
        assert!(!visible(connect.visible.as_ref(), &console));
    }

    #[test]
    fn the_whole_form_goes_insensitive_while_viewing() {
        let document = reference();
        assert!(!visible(document.form.sensitive.as_ref(), &saved_record()));
        let mut editing = saved_record();
        editing.set_view("mode", json!("edit"));
        assert!(visible(document.form.sensitive.as_ref(), &editing));
    }

    #[test]
    fn the_protocol_is_immutable_once_the_record_exists() {
        let document = reference();
        assert!(immutable_now(&document, &saved_record()).contains("kind"));
        let mut creating = saved_record();
        creating.set_view("mode", json!("new"));
        assert!(immutable_now(&document, &creating).is_empty());
    }

    fn tunnel_document() -> Document {
        let source = json!({
            "schema": 1,
            "kind": "sftp",
            "fields": [
                { "bind": "kind", "type": "text" },
                { "bind": "use_tunnel", "type": "bool" },
                { "bind": "tunnel_auth", "type": "text" },
                { "bind": "tunnel_passphrase", "type": "text", "secret": true,
                  "when": { "all": [
                      { "eq": ["state.kind", "sftp"] },
                      { "truthy": "state.use_tunnel" },
                      { "eq": ["state.tunnel_auth", "key"] } ] } }
            ],
            "form": { "t": "column", "children": [] }
        });
        Document::parse(&source.to_string()).expect("parses")
    }

    #[test]
    fn a_three_level_predicate_gates_the_tunnel_passphrase() {
        let document = tunnel_document();
        let field = document.field("tunnel_passphrase").expect("declared");
        let mut state = State::default();
        state.set_state("kind", json!("sftp"));
        assert!(!applicable(field, &state));
        state.set_state("use_tunnel", json!(true));
        assert!(!applicable(field, &state));
        state.set_state("tunnel_auth", json!("key"));
        assert!(applicable(field, &state));
        state.set_state("tunnel_auth", json!("password"));
        assert!(!applicable(field, &state));
        state.set_state("kind", json!("ftp"));
        state.set_state("tunnel_auth", json!("key"));
        assert!(!applicable(field, &state));
    }

    #[test]
    fn an_inapplicable_field_is_pruned_instead_of_written() {
        let document = tunnel_document();
        let mut state = State::default();
        state
            .set_state("kind", json!("ftp"))
            .set_state("use_tunnel", json!(false))
            .set_state("tunnel_auth", json!("key"))
            .set_state("tunnel_passphrase", json!("leftover"));
        let outcome = commit(&document, &state);
        assert!(!outcome.settings.contains_key("tunnel_passphrase"));
        assert_eq!(
            outcome.settings.get("kind").map(String::as_str),
            Some("ftp")
        );
    }

    #[test]
    fn a_pruned_required_field_is_not_reported_missing() {
        let source = json!({
            "schema": 1,
            "fields": [
                { "bind": "kind", "type": "text" },
                { "bind": "key_path", "type": "path", "required": true, "empty_as_absent": true,
                  "when": { "eq": ["state.kind", "sftp"] } }
            ],
            "form": { "t": "column", "children": [] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        let mut state = State::default();
        state.set_state("kind", json!("webdav"));
        assert!(commit(&document, &state).is_valid());
        state.set_state("kind", json!("sftp"));
        assert_eq!(
            commit(&document, &state).missing,
            vec!["key_path".to_string()]
        );
    }

    fn integer_document(on_parse_error: &str) -> Document {
        let source = json!({
            "schema": 1,
            "fields": [
                { "bind": "port", "type": "integer", "min": 1, "max": 65535,
                  "default": 22, "derive": "while_untouched",
                  "on_parse_error": on_parse_error }
            ],
            "form": { "t": "column", "children": [] }
        });
        Document::parse(&source.to_string()).expect("parses")
    }

    #[test]
    fn an_unparsable_integer_falls_back_to_the_default_when_asked_to() {
        let document = integer_document("default");
        let mut state = State::default();
        state.set_state("port", json!("twenty two"));
        state.touched.insert("port".to_string());
        let outcome = commit(&document, &state);
        assert_eq!(outcome.settings.get("port").map(String::as_str), Some("22"));
        assert!(outcome.problems.is_empty());
    }

    #[test]
    fn an_unparsable_integer_is_reported_when_the_field_refuses_a_fallback() {
        let document = integer_document("reject");
        let mut state = State::default();
        state.set_state("port", json!("twenty two"));
        state.touched.insert("port".to_string());
        let outcome = commit(&document, &state);
        assert_eq!(
            outcome.problems,
            vec![CommitProblem::NotAnInteger {
                bind: "port".to_string(),
                got: "twenty two".to_string()
            }]
        );
    }

    #[test]
    fn an_integer_outside_its_declared_range_is_reported() {
        let document = integer_document("reject");
        let mut state = State::default();
        state.set_state("port", json!("70000"));
        state.touched.insert("port".to_string());
        let outcome = commit(&document, &state);
        assert_eq!(
            outcome.problems,
            vec![CommitProblem::OutOfRange {
                bind: "port".to_string(),
                got: 70000
            }]
        );
    }

    #[test]
    fn a_default_fills_a_blank_field_but_never_overwrites_what_was_typed() {
        let document = integer_document("reject");
        let mut blank = State::default();
        assert_eq!(
            commit(&document, &blank)
                .settings
                .get("port")
                .map(String::as_str),
            Some("22")
        );
        blank.set_state("port", json!("2222"));
        blank.touched.insert("port".to_string());
        assert_eq!(
            commit(&document, &blank)
                .settings
                .get("port")
                .map(String::as_str),
            Some("2222")
        );
    }

    #[test]
    fn a_stored_port_survives_a_form_that_never_showed_it() {
        let document = reference();
        let mut state = saved_record();
        state.set_state("port", json!("8443"));
        let outcome = commit(&document, &state);
        assert_eq!(
            outcome.settings.get("port").map(String::as_str),
            Some("8443")
        );
    }

    #[test]
    fn every_translated_string_in_a_document_is_reported_once() {
        let keys = translation_keys(&reference());
        assert!(keys.contains("conn_manager.name_placeholder"));
        assert!(keys.contains("conn_manager.password_stored"));
        assert!(keys.contains("conn_manager.password"));
        assert!(keys.contains("common.cancel"));
        assert!(keys.contains("conn_manager.kind_webdav"));
        assert!(
            !keys.iter().any(|key| key.contains("WebDAV")),
            "a literal is not a translation key"
        );
    }

    #[test]
    fn a_number_and_its_text_form_compare_equal() {
        assert!(values_equal(&json!("21"), &json!(21)));
        assert!(values_equal(&json!(true), &json!("true")));
        assert!(!values_equal(&json!("21"), &json!(22)));
        assert!(values_equal(&json!(null), &json!("")));
    }

    #[test]
    fn a_canvas_is_a_known_node_and_needs_no_source() {
        let source = json!({
            "schema": 1,
            "fields": [],
            "form": { "t": "column", "children": [
                { "t": "canvas", "id": "picture", "weight": 1 } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        assert_eq!(document.form.children[0].kind(), NodeKind::Canvas);
        assert!(validate(&document).is_empty(), "{:?}", validate(&document));
    }

    #[test]
    fn an_unknown_node_type_is_a_warning_so_a_newer_document_still_renders() {
        let source = json!({
            "schema": 1,
            "fields": [],
            "form": { "t": "column", "children": [
                { "t": "sparkline", "id": "cpu", "children": [
                    { "t": "text", "text": "fallback" } ] } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        let issues = validate(&document);
        assert!(errors(&document).is_empty(), "{issues:?}");
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("unknown node type sparkline")));
        assert_eq!(document.form.children[0].kind(), NodeKind::Unknown);
        assert_eq!(document.form.children[0].children.len(), 1);
    }

    /// The one misreading that lies rather than goes missing: a predicate
    /// nobody could read becomes "always", and the control it guards is then
    /// always there. A plugin writing `{"op": ">"}` — which this language does
    /// not have — must be told, not quietly obeyed.
    #[test]
    fn a_predicate_that_could_not_be_read_is_an_error_rather_than_always_true() {
        let source = json!({
            "schema": 1,
            "fields": [],
            "form": { "t": "column", "children": [
                { "t": "button", "id": "on", "label": { "literal": "Go" },
                  "intent": { "do": "emit", "node": "on" },
                  "sensitive": { "op": ">", "left": "data.at", "right": 1 } } ] }
        });
        let (document, issues) = validate_source(&source.to_string()).expect("it still parses");
        let complaints: Vec<&str> = issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
            .map(|issue| issue.message.as_str())
            .collect();
        assert!(
            complaints
                .iter()
                .any(|said| said.contains("sensitive is not a predicate")),
            "{issues:?}"
        );
        assert_eq!(
            document.form.children[0].sensitive, None,
            "what could not be read is gone, and gone means always — which is \
             exactly why it is said out loud"
        );
    }

    /// What the language does understand is not complained about.
    #[test]
    fn a_predicate_the_language_has_is_left_alone() {
        let source = json!({
            "schema": 1,
            "fields": [],
            "form": { "t": "column", "children": [
                { "t": "text", "id": "said", "text": { "literal": "here" },
                  "visible": { "truthy": "data.can_go_on" } },
                { "t": "text", "id": "always", "text": { "literal": "here" },
                  "visible": true } ] }
        });
        let (_, issues) = validate_source(&source.to_string()).expect("parses");
        let complaints: Vec<&Issue> = issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
            .collect();
        assert!(complaints.is_empty(), "{complaints:?}");
    }

    /// A picture that names nothing is an empty box in front of the user, and
    /// nothing later in the drawing would notice.
    #[test]
    fn a_picture_that_names_nothing_is_an_error() {
        let source = json!({
            "schema": 1,
            "fields": [],
            "form": { "t": "column", "children": [
                { "t": "image", "id": "shown", "fit": "contain" } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        let complaints = errors(&document);
        assert!(
            complaints
                .iter()
                .any(|issue| issue.message.contains("image node has no src")),
            "{complaints:?}"
        );
    }

    #[test]
    fn an_unknown_enum_value_falls_back_to_the_default_and_is_reported() {
        let source = json!({
            "schema": 1,
            "fields": [ { "bind": "user", "type": "text" } ],
            "form": { "t": "column", "children": [
                { "t": "input", "id": "user", "bind": "user", "chrome": "inline" } ] }
        });
        let (document, issues) = validate_source(&source.to_string()).expect("parses");
        assert_eq!(document.form.children[0].chrome, Chrome::Bare);
        assert!(issues
            .iter()
            .any(|issue| issue.message == "property chrome could not be understood"));
    }

    #[test]
    fn a_mistyped_conditional_property_is_reported_rather_than_silently_dropped() {
        let source = json!({
            "schema": 1,
            "fields": [ { "bind": "user", "type": "text" } ],
            "form": { "t": "column", "children": [
                { "t": "input", "id": "user", "bind": "user",
                  "placeholder": { "cases": [ { "iff": { "truthy": "state.user" }, "then": "x" } ] } } ] }
        });
        let (document, issues) = validate_source(&source.to_string()).expect("parses");
        assert!(document.form.children[0].placeholder.is_none());
        assert!(issues
            .iter()
            .any(|issue| issue.message == "property placeholder could not be understood"));
    }

    #[test]
    fn a_value_node_without_a_declared_field_is_an_error() {
        let source = json!({
            "schema": 1,
            "fields": [ { "bind": "user", "type": "text" } ],
            "form": { "t": "column", "children": [
                { "t": "input", "id": "pass", "bind": "pass" } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        let blocking = errors(&document);
        assert_eq!(blocking.len(), 1, "{blocking:?}");
        assert!(blocking[0]
            .message
            .contains("bind pass is not a declared field"));
        assert!(accept(&document).is_err());
    }

    #[test]
    fn a_slider_owes_a_field_only_when_it_is_part_of_a_form() {
        let readout = Document::parse(
            &json!({
                "schema": 1,
                "fields": [],
                "form": { "t": "column", "children": [
                    { "t": "slider", "id": "seek", "value_key": "at", "max": 1000 } ] }
            })
            .to_string(),
        )
        .expect("parses");
        assert!(errors(&readout).is_empty(), "{:?}", errors(&readout));

        let in_a_form = Document::parse(
            &json!({
                "schema": 1,
                "fields": [ { "bind": "volume", "type": "integer" } ],
                "form": { "t": "column", "children": [
                    { "t": "slider", "id": "volume", "bind": "volume", "max": 100 } ] }
            })
            .to_string(),
        )
        .expect("parses");
        assert!(errors(&in_a_form).is_empty(), "{:?}", errors(&in_a_form));

        let neither = Document::parse(
            &json!({
                "schema": 1,
                "fields": [],
                "form": { "t": "column", "children": [
                    { "t": "slider", "id": "nowhere", "max": 100 } ] }
            })
            .to_string(),
        )
        .expect("parses");
        let blocking = errors(&neither);
        assert_eq!(blocking.len(), 1, "{blocking:?}");
        assert!(blocking[0].message.contains("slider node has no bind"));
    }

    #[test]
    fn duplicate_node_ids_are_an_error() {
        let source = json!({
            "schema": 1,
            "fields": [ { "bind": "user", "type": "text" } ],
            "form": { "t": "column", "children": [
                { "t": "text", "id": "twice", "text": "a" },
                { "t": "text", "id": "twice", "text": "b" } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        assert!(errors(&document)
            .iter()
            .any(|issue| issue.message.contains("duplicate node id twice")));
    }

    #[test]
    fn an_intent_pointing_at_no_node_is_an_error() {
        let source = json!({
            "schema": 1,
            "fields": [],
            "form": { "t": "column", "children": [
                { "t": "button", "id": "browse", "intent": { "do": "pick", "node": "key_path" } } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        assert!(errors(&document).iter().any(|issue| issue
            .message
            .contains("intent targets unknown node key_path")));
    }

    #[test]
    fn an_intent_the_host_does_not_understand_is_an_error_rather_than_a_dead_control() {
        let source = json!({
            "schema": 1,
            "fields": [],
            "form": { "t": "column", "children": [
                { "t": "button", "id": "test", "label": "Test connection",
                  "intent": { "do": "probe_the_server" } } ] },
            "actions": [
                { "id": "primary", "label": "Save", "intent": { "do": "submit" } },
                { "id": "later", "label": "Schedule", "intent": { "do": "enqueue" } } ]
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        let blocking = errors(&document);
        assert_eq!(blocking.len(), 2, "{blocking:?}");
        assert!(blocking
            .iter()
            .all(|issue| issue.message == "intent is not understood"));
        assert_eq!(blocking[0].at, "form.children[0]");
        assert_eq!(blocking[1].at, "actions[1]");
    }

    #[test]
    fn a_choice_without_options_and_a_table_without_columns_are_errors() {
        let source = json!({
            "schema": 1,
            "fields": [ { "bind": "auth", "type": "text" } ],
            "form": { "t": "column", "children": [
                { "t": "choice", "id": "auth", "bind": "auth" },
                { "t": "table", "id": "ifaces" } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        let messages: Vec<String> = errors(&document)
            .into_iter()
            .map(|issue| issue.message)
            .collect();
        assert!(messages
            .iter()
            .any(|m| m.contains("choice node has no options")));
        assert!(messages
            .iter()
            .any(|m| m.contains("table node has no columns")));
    }

    #[test]
    fn a_predicate_over_an_undeclared_field_is_warned_about() {
        let source = json!({
            "schema": 1,
            "fields": [ { "bind": "kind", "type": "text" } ],
            "form": { "t": "column", "children": [
                { "t": "text", "id": "hint", "text": "x",
                  "visible": { "eq": ["state.protocol", "sftp"] } } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        assert!(errors(&document).is_empty());
        assert!(validate(&document)
            .iter()
            .any(|issue| issue.message == "state.protocol is not a declared field"));
    }

    #[test]
    fn an_identity_that_names_no_field_is_an_error() {
        let source = json!({
            "schema": 1,
            "identity": "label",
            "fields": [ { "bind": "name", "type": "text" } ],
            "form": { "t": "column", "children": [] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        assert!(errors(&document).iter().any(|issue| issue
            .message
            .contains("identity label is not a declared field")));
    }

    #[test]
    fn a_folder_to_open_at_that_names_no_field_is_an_error() {
        let source = json!({
            "schema": 1,
            "opens_at": "somewhere",
            "fields": [ { "bind": "name", "type": "text" } ],
            "form": { "t": "column", "children": [] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        assert!(errors(&document).iter().any(|issue| issue
            .message
            .contains("opens_at somewhere is not a declared field")));
    }

    #[test]
    fn a_document_from_a_newer_schema_is_refused() {
        let source = json!({ "schema": 99, "fields": [], "form": { "t": "column" } });
        assert!(Document::parse(&source.to_string()).is_err());
    }

    #[test]
    fn a_document_round_trips_through_json() {
        let document = reference();
        let encoded = serde_json::to_string(&document).expect("encodes");
        let decoded = Document::parse(&encoded).expect("decodes");
        assert_eq!(document, decoded);
    }

    #[test]
    fn a_reference_is_only_a_reference_inside_a_known_namespace() {
        let mut state = State::default();
        state.set_state("kind", json!("webdav"));
        assert_eq!(expr::operand(&json!("state.kind"), &state), json!("webdav"));
        assert_eq!(expr::operand(&json!("kind"), &state), json!("kind"));
        assert_eq!(
            expr::operand(&json!("other.kind"), &state),
            json!("other.kind")
        );
        assert_eq!(expr::operand(&json!("state.absent"), &state), json!(null));
    }

    #[test]
    fn one_of_matches_any_listed_value() {
        let predicate: Pred = serde_json::from_value(json!({
            "one_of": { "value": "state.kind", "of": ["ftp", "sftp"] }
        }))
        .expect("parses");
        let mut state = State::default();
        state.set_state("kind", json!("sftp"));
        assert!(evaluate(&predicate, &state));
        state.set_state("kind", json!("webdav"));
        assert!(!evaluate(&predicate, &state));
    }

    fn protocol_choice() -> Node {
        let source = json!({
            "t": "choice", "id": "kind", "bind": "kind",
            "options": [
                { "value": "ftp", "label": { "literal": "FTP" } },
                { "value": "sftp", "label": { "literal": "SFTP" } },
                { "value": "webdav", "label": { "literal": "WebDAV" } } ],
            "decode": { "case_insensitive": true, "unknown": "webdav" }
        });
        serde_json::from_value(source).expect("parses")
    }

    #[test]
    fn a_choice_finds_its_stored_value_whatever_its_case() {
        let node = protocol_choice();
        assert_eq!(choice_index(&node, &json!("sftp")), Some(1));
        assert_eq!(choice_index(&node, &json!("SFTP")), Some(1));
        assert_eq!(choice_index(&node, &json!("Ftp")), Some(0));
    }

    #[test]
    fn an_unrecognised_choice_value_falls_through_to_the_declared_unknown() {
        let node = protocol_choice();
        assert_eq!(choice_index(&node, &json!("nextcloud")), Some(2));
        assert_eq!(choice_index(&node, &json!("")), Some(2));
    }

    #[test]
    fn a_choice_without_a_decode_rule_reports_no_match() {
        let mut node = protocol_choice();
        node.decode = None;
        assert_eq!(choice_index(&node, &json!("nextcloud")), None);
        assert_eq!(choice_index(&node, &json!("SFTP")), None);
        assert_eq!(choice_index(&node, &json!("sftp")), Some(1));
    }

    fn chart_node(extra: serde_json::Value) -> Node {
        let mut source = json!({ "t": "chart", "id": "cpu", "series_key": "cpu_history" });
        if let (Some(base), Some(more)) = (source.as_object_mut(), extra.as_object()) {
            for (key, value) in more {
                base.insert(key.clone(), value.clone());
            }
        }
        serde_json::from_value(source).expect("parses")
    }

    fn history() -> State {
        let mut state = State::default();
        state.data.insert(
            "cpu_history".to_string(),
            json!([
                { "label": { "literal": "CPU" }, "color": [0.2, 0.6, 1.0, 0.8],
                  "values": [10.0, 40.0, 25.0] }
            ]),
        );
        state
    }

    #[test]
    fn a_control_can_be_declared_as_output_only() {
        let source = json!({
            "schema": 1,
            "fields": [ { "bind": "digest", "type": "text" } ],
            "form": { "t": "column", "children": [
                { "t": "input", "id": "digest", "bind": "digest",
                  "variant": "multiline", "read_only": true } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        assert!(errors(&document).is_empty());
        assert!(document.form.children[0].read_only);
        assert!(!document.form.children[0].selectable);
    }

    #[test]
    fn a_container_can_ask_for_an_inset_and_a_gap_above_itself() {
        let source = json!({
            "schema": 1,
            "fields": [],
            "form": { "t": "view", "padding": 32, "spacing": 12, "children": [
                { "t": "row", "id": "section", "margin_top": 24, "children": [] } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        assert!(errors(&document).is_empty());
        assert_eq!(document.form.padding, Some(32));
        assert_eq!(document.form.spacing, Some(12));
        assert_eq!(document.form.children[0].margin_top, Some(24));
        assert_eq!(document.form.children[0].padding, None);
    }

    #[test]
    fn a_chart_reads_its_series_from_the_data_the_document_carries() {
        let series = series_of(&history(), "cpu_history");
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].values, vec![10.0, 40.0, 25.0]);
        assert_eq!(series[0].color, Some([0.2, 0.6, 1.0, 0.8]));
        assert!(series_of(&State::default(), "cpu_history").is_empty());
    }

    #[test]
    fn a_chart_with_declared_bounds_never_rescales_itself() {
        let node = chart_node(json!({ "min": 0, "max": 100 }));
        let series = series_of(&history(), "cpu_history");
        assert_eq!(series_bounds(&series, &node), (0.0, 100.0));
    }

    #[test]
    fn a_chart_without_bounds_fits_itself_to_the_samples() {
        let node = chart_node(json!({}));
        let series = series_of(&history(), "cpu_history");
        assert_eq!(series_bounds(&series, &node), (10.0, 40.0));
    }

    #[test]
    fn a_flat_or_empty_chart_still_yields_a_usable_range() {
        let node = chart_node(json!({}));
        assert_eq!(series_bounds(&[], &node), (0.0, 1.0));
        let flat = vec![Series {
            label: None,
            color: None,
            values: vec![5.0, 5.0, 5.0],
        }];
        assert_eq!(series_bounds(&flat, &node), (5.0, 6.0));
    }

    #[test]
    fn a_chart_is_a_known_node_rather_than_an_unrecognised_one() {
        assert_eq!(chart_node(json!({})).kind(), NodeKind::Chart);
        assert!(NODE_KINDS.contains(&"chart"));
    }

    #[test]
    fn a_document_can_carry_the_values_it_wants_shown() {
        let source = json!({
            "schema": 1,
            "data": { "mem": "7.2 GB", "hosts": [ { "name": "eth0", "address": "10.0.0.2" } ] },
            "fields": [],
            "form": { "t": "view", "surface": "dialog", "children": [
                { "t": "text", "id": "memory", "text": "Memory: {data.mem}" },
                { "t": "table", "id": "hosts", "rows_key": "hosts",
                  "columns": [ { "key": "name" }, { "key": "address" } ] } ] }
        });
        let document = Document::parse(&source.to_string()).expect("parses");
        assert!(errors(&document).is_empty());
        let state = State::for_document(&document);
        assert_eq!(
            resolve_text(
                document.form.children[0].text.as_ref(),
                &state,
                &no_translation
            )
            .as_deref(),
            Some("Memory: 7.2 GB")
        );
        assert_eq!(
            state
                .data
                .get("hosts")
                .and_then(|rows| rows.as_array())
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn a_fresh_reading_is_recognised_as_the_same_tree_so_only_values_change() {
        let shape = |mem: &str| {
            json!({
                "schema": 1,
                "data": { "mem": mem },
                "fields": [],
                "form": { "t": "view", "children": [
                    { "t": "text", "id": "memory", "text": "Memory: {data.mem}" } ] }
            })
            .to_string()
        };
        let first = Document::parse(&shape("7.2 GB")).expect("parses");
        let second = Document::parse(&shape("7.9 GB")).expect("parses");
        assert!(
            first.shows_the_same_tree_as(&second),
            "only the readings moved, so the widgets can stay"
        );
        let mut grown = second.clone();
        grown.form.children.push(
            serde_json::from_value(json!({
                "t": "text", "id": "swap", "text": "Swap: {data.swap}"
            }))
            .expect("parses"),
        );
        assert!(!first.shows_the_same_tree_as(&grown));
    }

    #[test]
    fn a_label_can_show_a_value_from_the_state() {
        let node: Node = serde_json::from_value(json!({
            "t": "text", "id": "memory",
            "text": { "tr": "sysinfo.memory", "en": "Memory: {mem_used} of {mem_total}" }
        }))
        .expect("parses");
        let mut state = State::default();
        state
            .set_state("mem_used", json!("7.2 GB"))
            .set_state("mem_total", json!("16 GB"));
        assert_eq!(
            resolve_text(node.text.as_ref(), &state, &no_translation).as_deref(),
            Some("Memory: 7.2 GB of 16 GB")
        );
    }

    #[test]
    fn a_label_reads_the_display_namespace_as_well_as_the_record() {
        let node: Node = serde_json::from_value(json!({
            "t": "text", "id": "load", "text": "load {data.load} on {host.kind}"
        }))
        .expect("parses");
        let mut state = State::default();
        state.data.insert("load".to_string(), json!(0.75));
        state.set_host("kind", json!("gtk"));
        assert_eq!(
            resolve_text(node.text.as_ref(), &state, &no_translation).as_deref(),
            Some("load 0.75 on gtk")
        );
    }

    #[test]
    fn a_placeholder_whose_value_is_itself_translatable_reads_as_text() {
        let node: Node = serde_json::from_value(json!({
            "t": "text", "id": "status", "text": "{data.status}"
        }))
        .expect("parses");
        let mut state = State::default();
        state.data.insert(
            "status".to_string(),
            json!({ "tr": "devtools.copied", "en": "Copied" }),
        );
        assert_eq!(
            resolve_text(node.text.as_ref(), &state, &no_translation).as_deref(),
            Some("Copied")
        );
        assert_eq!(
            resolve_text(node.text.as_ref(), &state, &|key| (key
                == "devtools.copied")
                .then(|| {
                    "\u{421}\u{43a}\u{43e}\u{43f}\u{438}\u{440}\u{43e}\u{432}\u{430}\u{43d}\u{43e}"
                        .to_string()
                }))
            .as_deref(),
            Some("\u{421}\u{43a}\u{43e}\u{43f}\u{438}\u{440}\u{43e}\u{432}\u{430}\u{43d}\u{43e}")
        );
    }

    #[test]
    fn a_label_keeps_an_unknown_placeholder_visible_instead_of_vanishing() {
        let node: Node = serde_json::from_value(json!({
            "t": "text", "id": "x", "text": "cpu {missing}"
        }))
        .expect("parses");
        assert_eq!(
            resolve_text(node.text.as_ref(), &State::default(), &no_translation).as_deref(),
            Some("cpu {missing}"),
            "a missing key is left in place so the author sees it, rather than silently blanking"
        );
    }

    #[test]
    fn a_value_that_has_not_arrived_yet_shows_as_nothing_rather_than_as_braces() {
        let node: Node = serde_json::from_value(json!({
            "t": "text", "id": "x", "text": "digest: {data.md5}"
        }))
        .expect("parses");
        assert_eq!(
            resolve_text(node.text.as_ref(), &State::default(), &no_translation).as_deref(),
            Some("digest: "),
            "a known namespace with no value yet renders empty"
        );
        let mut filled = State::default();
        filled
            .data
            .insert("md5".to_string(), json!("900150983cd24fb0"));
        assert_eq!(
            resolve_text(node.text.as_ref(), &filled, &no_translation).as_deref(),
            Some("digest: 900150983cd24fb0")
        );
    }

    #[test]
    fn what_a_window_was_opened_with_is_readable_from_its_document() {
        let node: Node = serde_json::from_value(json!({
            "t": "text", "id": "which", "text": "{arg.path}"
        }))
        .expect("parses");
        let mut state = State::default();
        state.arg = json!({ "path": "/films/album.torrent" });
        assert_eq!(
            resolve_text(node.text.as_ref(), &state, &no_translation).as_deref(),
            Some("/films/album.torrent"),
            "the dialogue has to name the file it is about"
        );
        assert_eq!(
            resolve_text(node.text.as_ref(), &State::default(), &no_translation).as_deref(),
            Some(""),
            "a window opened with nothing draws nothing, not the placeholder"
        );
    }

    #[test]
    fn an_argument_is_read_as_deep_as_it_goes_including_through_arrays() {
        let mut state = State::default();
        state.arg = json!({ "files": [{ "name": "one.flac" }, { "name": "two.flac" }] });
        assert_eq!(substitute("{arg.files.1.name}", &state), "two.flac");
        assert_eq!(substitute("{arg.files.9.name}", &state), "");
    }

    #[test]
    fn an_argument_that_is_not_an_object_is_still_kept_whole() {
        let mut state = State::default();
        state.arg = json!("account");
        assert_eq!(state.lookup("arg.path"), None);
        assert_eq!(state.arg, json!("account"));
    }

    #[test]
    fn a_predicate_reads_the_argument_the_same_way_a_placeholder_does() {
        let predicate: Pred =
            serde_json::from_value(json!({ "eq": ["arg.kind", "torrent"] })).expect("parses");
        let mut state = State::default();
        state.arg = json!({ "kind": "torrent" });
        assert!(evaluate(&predicate, &state));
        state.arg = json!({ "kind": "magnet" });
        assert!(!evaluate(&predicate, &state));
    }

    #[test]
    fn a_label_without_braces_is_left_exactly_as_it_was() {
        let node: Node = serde_json::from_value(json!({
            "t": "text", "id": "x", "text": "https://dav.example.org/remote.php/dav"
        }))
        .expect("parses");
        assert_eq!(
            resolve_text(node.text.as_ref(), &State::default(), &no_translation).as_deref(),
            Some("https://dav.example.org/remote.php/dav")
        );
    }

    #[test]
    fn a_summary_still_refuses_an_empty_placeholder_even_though_a_label_would_not() {
        let mut state = State::default();
        state.set_state("user", json!("ivan"));
        assert_eq!(fill_template("{user}@{host}", &state), None);
        assert_eq!(substitute("{user}@{host}", &state), "ivan@{host}");
    }

    #[test]
    fn a_template_yields_nothing_when_a_placeholder_is_empty() {
        let mut state = State::default();
        state.set_state("user", json!("ivan"));
        assert_eq!(fill_template("{user}@{host}", &state), None);
        state.set_state("host", json!("example.org"));
        assert_eq!(
            fill_template("{user}@{host}", &state).as_deref(),
            Some("ivan@example.org")
        );
    }
}
