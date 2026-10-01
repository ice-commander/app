//! One question from a plugin, drawn the way this frontend draws questions.
//!
//! A plugin used to need a registered view and a document of its own to ask
//! "are you sure"; the torrent plugin carries 176 lines for exactly that. The
//! host owns the look now, so every plugin's question matches the application
//! and matches every other plugin's.

use adw::prelude::*;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;

pub fn install(parent: &gtk::Window) {
    let holder = parent.clone();
    ic_plugin_host::set_ask_handler(Rc::new(
        move |spec: &str, answer: Box<dyn FnOnce(&str)>| {
            show(&holder, spec, answer);
        },
    ));
}

/// Text in a spec is either a plain string or the `{tr, en}` pair the view
/// documents use, so a plugin writes it once and every language gets it.
fn phrase(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(_) => {
            if let Some(key) = value.get("tr").and_then(Value::as_str) {
                let said = crate::i18n::tr(key).to_string();
                if said != key {
                    return said;
                }
            }
            value
                .get("en")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        }
        _ => String::new(),
    }
}

struct Button {
    id: String,
    label: String,
    role: adw::ResponseAppearance,
}

fn buttons_of(spec: &Value) -> Vec<Button> {
    let listed: Vec<Button> = spec["buttons"]
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|one| {
                    let id = one.get("id")?.as_str()?.to_string();
                    Some(Button {
                        label: one
                            .get("label")
                            .map(phrase)
                            .filter(|said| !said.is_empty())
                            .unwrap_or_else(|| id.clone()),
                        role: match one.get("role").and_then(Value::as_str) {
                            Some("destructive") => adw::ResponseAppearance::Destructive,
                            Some("primary") | Some("suggested") => {
                                adw::ResponseAppearance::Suggested
                            }
                            _ => adw::ResponseAppearance::Default,
                        },
                        id,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if listed.is_empty() {
        return vec![Button {
            id: "ok".to_string(),
            label: crate::i18n::tr("common.ok").to_string(),
            role: adw::ResponseAppearance::Suggested,
        }];
    }
    listed
}

/// A list of options and the dropdown showing them. The ids travel back in the
/// answer; the labels are only ever seen.
struct Picked {
    ids: Vec<String>,
    widget: gtk::DropDown,
}

impl Picked {
    fn chosen(&self) -> String {
        self.ids
            .get(self.widget.selected() as usize)
            .cloned()
            .unwrap_or_default()
    }
}

fn picked_from(spec: &Value) -> Option<Picked> {
    let listed = spec.get("choice")?.get("options")?.as_array()?;
    let mut ids = Vec::new();
    let mut labels = Vec::new();
    for one in listed {
        let Some(id) = one.get("id").and_then(Value::as_str) else {
            continue;
        };
        labels.push(
            one.get("label")
                .map(phrase)
                .filter(|said| !said.is_empty())
                .unwrap_or_else(|| id.to_string()),
        );
        ids.push(id.to_string());
    }
    if ids.is_empty() {
        return None;
    }
    let shown: Vec<&str> = labels.iter().map(String::as_str).collect();
    let widget = gtk::DropDown::from_strings(&shown);
    // `value` names which option starts selected, by id.
    if let Some(wanted) = spec["choice"].get("value").and_then(Value::as_str) {
        if let Some(at) = ids.iter().position(|id| id == wanted) {
            widget.set_selected(at as u32);
        }
    }
    Some(Picked { ids, widget })
}

fn typed_into(spec: &Value) -> Option<gtk::Entry> {
    let asked = spec.get("input")?;
    let entry = gtk::Entry::builder()
        .text(asked.get("value").map(phrase).unwrap_or_default())
        .hexpand(true)
        .build();
    if let Some(hint) = asked.get("placeholder").map(phrase) {
        if !hint.is_empty() {
            entry.set_placeholder_text(Some(&hint));
        }
    }
    match asked.get("variant").and_then(Value::as_str) {
        Some("masked") => entry.set_visibility(false),
        Some("integer") => entry.set_input_purpose(gtk::InputPurpose::Digits),
        Some("hex") => entry.add_css_class("monospace"),
        _ => {}
    }
    Some(entry)
}

fn show(parent: &gtk::Window, spec: &str, answer: Box<dyn FnOnce(&str)>) {
    let spec: Value = serde_json::from_str(spec).unwrap_or(Value::Null);
    let buttons = buttons_of(&spec);
    // The way out is first, and it is also what a dismissed question reports.
    let escaped = buttons[0].id.clone();

    let dialog = adw::AlertDialog::builder()
        .heading(spec.get("heading").map(phrase).unwrap_or_default())
        .body(spec.get("body").map(phrase).unwrap_or_default())
        .build();

    let typed = typed_into(&spec);
    let picked = picked_from(&spec);
    let detail = spec
        .get("detail")
        .map(phrase)
        .filter(|said| !said.is_empty());
    if typed.is_some() || picked.is_some() || detail.is_some() {
        let holder = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        if let Some(said) = detail {
            let label = gtk::Label::builder().label(said).xalign(0.0).wrap(true).build();
            label.add_css_class("dim-label");
            holder.append(&label);
        }
        if let Some(entry) = typed.as_ref() {
            holder.append(entry);
        }
        if let Some(choice) = picked.as_ref() {
            holder.append(&choice.widget);
        }
        dialog.set_extra_child(Some(&holder));
    }

    for button in &buttons {
        dialog.add_response(&button.id, &button.label);
        dialog.set_response_appearance(&button.id, button.role);
    }
    dialog.set_close_response(&escaped);
    if let Some(last) = buttons.last() {
        dialog.set_default_response(Some(&last.id));
    }

    // `answer` must be called exactly once, and `connect_response` is an `Fn`.
    let once: Rc<RefCell<Option<Box<dyn FnOnce(&str)>>>> = Rc::new(RefCell::new(Some(answer)));
    dialog.connect_response(None, move |_, chosen| {
        let Some(reply) = once.borrow_mut().take() else {
            return;
        };
        let mut said = json!({ "button": chosen });
        if let Some(entry) = typed.as_ref() {
            said["text"] = json!(entry.text().to_string());
        }
        if let Some(choice) = picked.as_ref() {
            said["choice"] = json!(choice.chosen());
        }
        reply(&said.to_string());
    });
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_with_no_buttons_still_has_a_way_out() {
        let only = buttons_of(&json!({}));
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].id, "ok");
    }

    #[test]
    fn buttons_keep_the_order_they_were_given_so_the_first_is_the_way_out() {
        let listed = buttons_of(&json!({
            "buttons": [
                { "id": "cancel", "label": { "en": "Cancel" } },
                { "id": "delete", "label": { "en": "Delete" }, "role": "destructive" }
            ]
        }));
        let ids: Vec<&str> = listed.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(ids, vec!["cancel", "delete"]);
        assert_eq!(listed[1].role, adw::ResponseAppearance::Destructive);
    }

    #[test]
    fn a_button_without_a_label_falls_back_to_its_own_id() {
        let listed = buttons_of(&json!({ "buttons": [ { "id": "wipe" } ] }));
        assert_eq!(listed[0].label, "wipe");
    }

    #[test]
    fn plain_text_and_the_translatable_pair_both_read() {
        assert_eq!(phrase(&json!("Delete?")), "Delete?");
        assert_eq!(phrase(&json!({ "en": "Delete?" })), "Delete?");
        assert_eq!(phrase(&json!({ "tr": "nothing.here", "en": "Delete?" })), "Delete?");
        assert_eq!(phrase(&json!(7)), "");
    }
}
