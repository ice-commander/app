#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct HotKey {
    pub id: String,
    pub description: String,
    pub keys: String,
}

pub fn description(id: &str) -> String {
    let key = format!("hotkey.{id}");
    let text = crate::i18n::tr(&key);
    if text == key {
        return get_default_hotkeys()
            .into_iter()
            .find(|h| h.id == id)
            .map(|h| h.description)
            .unwrap_or(text);
    }
    text
}

pub fn get_default_hotkeys() -> Vec<HotKey> {
    vec![
        HotKey {
            id: "refresh".to_string(),
            description: "Refresh active panel".to_string(),
            keys: "Ctrl+R".to_string(),
        },
        HotKey {
            id: "move_left".to_string(),
            description: "Move active panel left".to_string(),
            keys: "Ctrl+Left".to_string(),
        },
        HotKey {
            id: "move_right".to_string(),
            description: "Move active panel right".to_string(),
            keys: "Ctrl+Right".to_string(),
        },
        HotKey {
            id: "select_left".to_string(),
            description: "Select left panel".to_string(),
            keys: "Alt+F1".to_string(),
        },
        HotKey {
            id: "select_right".to_string(),
            description: "Select right panel".to_string(),
            keys: "Alt+F2".to_string(),
        },
        HotKey {
            id: "go_root_win".to_string(),
            description: "Go to root disk (Windows)".to_string(),
            keys: "Ctrl+\\".to_string(),
        },
        HotKey {
            id: "go_root_unix".to_string(),
            description: "Go to root disk (Unix)".to_string(),
            keys: "Ctrl+/".to_string(),
        },
        HotKey {
            id: "find_files".to_string(),
            description: "Search files recursively".to_string(),
            keys: "Ctrl+S".to_string(),
        },
        HotKey {
            id: "filter_files".to_string(),
            description: "Quick filter files in directory".to_string(),
            keys: "Ctrl+F".to_string(),
        },
        HotKey {
            id: "manage_connections".to_string(),
            description: "Manage FTP/SFTP/WebDAV connections".to_string(),
            keys: "Ctrl+N".to_string(),
        },
        HotKey {
            id: "toggle_video_fullscreen".to_string(),
            description: "Toggle video fullscreen".to_string(),
            keys: "Alt+Return".to_string(),
        },
        HotKey {
            id: "expand_terminal".to_string(),
            description: "Expand/collapse active terminal".to_string(),
            keys: "Alt+Return".to_string(),
        },
        HotKey {
            id: "editor_save".to_string(),
            description: "Save file in editor".to_string(),
            keys: "Ctrl+S".to_string(),
        },
        HotKey {
            id: "clip_cut".to_string(),
            description: "Cut selected files".to_string(),
            keys: "Ctrl+X".to_string(),
        },
        HotKey {
            id: "clip_copy".to_string(),
            description: "Copy selected files".to_string(),
            keys: "Ctrl+C".to_string(),
        },
        HotKey {
            id: "clip_paste".to_string(),
            description: "Paste files into active panel".to_string(),
            keys: "Ctrl+V".to_string(),
        },
    ]
}

fn shares_combo_by_design(a: &str, b: &str) -> bool {
    const PAIRS: [[&str; 2]; 2] = [
        ["toggle_video_fullscreen", "expand_terminal"],
        ["find_files", "editor_save"],
    ];
    PAIRS.iter().any(|p| p.contains(&a) && p.contains(&b))
}

fn conflict_in(hotkeys: &[HotKey], id: &str, keys: &str) -> Option<String> {
    hotkeys
        .iter()
        .find(|h| {
            h.id != id
                && !shares_combo_by_design(&h.id, id)
                && h.keys.eq_ignore_ascii_case(keys)
        })
        .map(|h| h.id.clone())
}

pub fn conflicting_action(
    config: &client_config::AppConfig,
    id: &str,
    keys: &str,
) -> Option<String> {
    conflict_in(&get_hotkeys(config), id, keys)
}

pub fn get_hotkeys(config: &client_config::AppConfig) -> Vec<HotKey> {
    let mut hotkeys = config.get::<Vec<HotKey>>("ui.hotkeys").unwrap_or_else(|| {
        let defaults = get_default_hotkeys();
        config.set("ui.hotkeys", &defaults);
        config.save();
        defaults
    });

    let defaults = get_default_hotkeys();
    let mut changed = false;

    for def in &defaults {
        if !hotkeys.iter().any(|h| h.id == def.id) {
            hotkeys.push(def.clone());
            changed = true;
        }
    }

    let before = hotkeys.len();
    hotkeys.retain(|h| defaults.iter().any(|d| d.id == h.id));
    if hotkeys.len() != before {
        changed = true;
    }

    if changed {
        save_hotkeys(config, &hotkeys);
    }

    hotkeys
}

pub fn save_hotkeys(config: &client_config::AppConfig, hotkeys: &[HotKey]) {
    config.set("ui.hotkeys", hotkeys);
    config.save();
}

pub fn keyval_to_string(keyval: gtk::gdk::Key, state: gtk::gdk::ModifierType) -> String {
    let mut parts = Vec::new();

    let clean_state = state
        & (gtk::gdk::ModifierType::CONTROL_MASK
            | gtk::gdk::ModifierType::ALT_MASK
            | gtk::gdk::ModifierType::SHIFT_MASK
            | gtk::gdk::ModifierType::SUPER_MASK
            | gtk::gdk::ModifierType::META_MASK);

    if clean_state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
        parts.push("Ctrl".to_string());
    }
    if clean_state.contains(gtk::gdk::ModifierType::ALT_MASK) {
        parts.push("Alt".to_string());
    }
    if clean_state.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
        if keyval != gtk::gdk::Key::Shift_L && keyval != gtk::gdk::Key::Shift_R {
            parts.push("Shift".to_string());
        }
    }
    if clean_state.contains(gtk::gdk::ModifierType::SUPER_MASK)
        || clean_state.contains(gtk::gdk::ModifierType::META_MASK)
    {
        parts.push("Super".to_string());
    }

    if let Some(name) = keyval.name() {
        let name_str = name.as_str();
        let name_lower = name_str.to_lowercase();
        let mapped_name = match name_lower.as_str() {
            "cyrillic_shorti" => "q",
            "cyrillic_tse" => "w",
            "cyrillic_u" => "e",
            "cyrillic_ka" => "r",
            "cyrillic_ie" => "t",
            "cyrillic_en" => "y",
            "cyrillic_ghe" => "u",
            "cyrillic_sha" => "i",
            "cyrillic_shcha" => "o",
            "cyrillic_ze" => "p",
            "cyrillic_ha" => "[",
            "cyrillic_hardsign" => "]",
            "cyrillic_ef" => "a",
            "cyrillic_yeru" => "s",
            "cyrillic_ve" => "d",
            "cyrillic_a" => "f",
            "cyrillic_pe" => "g",
            "cyrillic_er" => "h",
            "cyrillic_o" => "j",
            "cyrillic_el" => "k",
            "cyrillic_de" => "l",
            "cyrillic_zhe" => ";",
            "cyrillic_e" => "'",
            "cyrillic_ya" => "z",
            "cyrillic_che" => "x",
            "cyrillic_es" => "c",
            "cyrillic_em" => "v",
            "cyrillic_i" => "b",
            "cyrillic_te" => "n",
            "cyrillic_softsign" => "m",
            "cyrillic_be" => ",",
            "cyrillic_yu" => ".",
            other => other,
        };

        match mapped_name {
            "Control_L" | "Control_R" | "Alt_L" | "Alt_R" | "Shift_L" | "Shift_R" | "Super_L"
            | "Super_R" | "Meta_L" | "Meta_R" => {}
            "Left" => parts.push("Left".to_string()),
            "Right" => parts.push("Right".to_string()),
            "Up" => parts.push("Up".to_string()),
            "Down" => parts.push("Down".to_string()),
            "backslash" => parts.push("\\".to_string()),
            "slash" => parts.push("/".to_string()),
            other => {
                if other.len() == 1 {
                    parts.push(other.to_uppercase());
                } else {
                    parts.push(other.to_string());
                }
            }
        }
    }

    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkeys_is_not_empty() {
        assert!(!get_default_hotkeys().is_empty());
    }

    #[test]
    fn default_hotkeys_contains_refresh() {
        let hotkeys = get_default_hotkeys();
        assert!(hotkeys.iter().any(|h| h.id == "refresh"), "missing 'refresh' hotkey");
    }

    #[test]
    fn default_hotkeys_all_have_non_empty_keys() {
        for h in get_default_hotkeys() {
            assert!(!h.keys.is_empty(), "hotkey '{}' has empty keys field", h.id);
        }
    }

    #[test]
    fn default_hotkeys_all_have_unique_ids() {
        let hotkeys = get_default_hotkeys();
        let mut ids = std::collections::HashSet::new();
        for h in &hotkeys {
            assert!(ids.insert(h.id.clone()), "duplicate hotkey id: {}", h.id);
        }
    }

    #[test]
    fn default_hotkeys_contains_the_clipboard_actions() {
        let ids: Vec<_> = get_default_hotkeys().into_iter().map(|h| (h.id, h.keys)).collect();
        for (id, keys) in [("clip_cut", "Ctrl+X"), ("clip_copy", "Ctrl+C"), ("clip_paste", "Ctrl+V")] {
            assert!(
                ids.iter().any(|(i, k)| i == id && k == keys),
                "missing default '{id}' bound to {keys}"
            );
        }
    }

    #[test]
    fn clipboard_defaults_do_not_collide_with_other_defaults() {
        let hotkeys = get_default_hotkeys();
        let clip: Vec<_> = hotkeys.iter().filter(|h| h.id.starts_with("clip_")).collect();
        assert_eq!(clip.len(), 3);
        for c in &clip {
            for other in hotkeys.iter().filter(|h| !h.id.starts_with("clip_")) {
                assert!(
                    !other.keys.eq_ignore_ascii_case(&c.keys),
                    "'{}' shadows '{}' — both bound to {}",
                    other.id,
                    c.id,
                    c.keys
                );
            }
        }
    }

    #[test]
    fn every_default_hotkey_is_described_in_all_fifteen_locales() {
        let locales: [(&str, &str); 15] = [
            ("en", include_str!("../locales/en.json")),
            ("ru", include_str!("../locales/ru.json")),
            ("pl", include_str!("../locales/pl.json")),
            ("cs", include_str!("../locales/cs.json")),
            ("sk", include_str!("../locales/sk.json")),
            ("de", include_str!("../locales/de.json")),
            ("es", include_str!("../locales/es.json")),
            ("uk", include_str!("../locales/uk.json")),
            ("it", include_str!("../locales/it.json")),
            ("fr", include_str!("../locales/fr.json")),
            ("ro", include_str!("../locales/ro.json")),
            ("hu", include_str!("../locales/hu.json")),
            ("be", include_str!("../locales/be.json")),
            ("bg", include_str!("../locales/bg.json")),
            ("sr", include_str!("../locales/sr.json")),
        ];
        let ids: Vec<String> = get_default_hotkeys().into_iter().map(|h| h.id).collect();
        for (lang, raw) in locales {
            let map: std::collections::HashMap<String, String> =
                serde_json::from_str(raw).unwrap_or_else(|e| panic!("{lang}.json is not valid: {e}"));
            let keys: Vec<String> = ids
                .iter()
                .map(|id| format!("hotkey.{id}"))
                .chain(["settings.hotkey_conflict".to_string()])
                .collect();
            for key in keys {
                let text = map.get(&key).unwrap_or_else(|| panic!("{lang}.json is missing {key}"));
                assert!(!text.trim().is_empty(), "{lang}.json has an empty {key}");
                if key == "settings.hotkey_conflict" {
                    for ph in ["%{key}", "%{action}"] {
                        assert!(text.contains(ph), "{lang}.json {key} lost the {ph} placeholder");
                    }
                }
            }
        }
    }

    fn hk(id: &str, keys: &str) -> HotKey {
        HotKey { id: id.to_string(), description: String::new(), keys: keys.to_string() }
    }

    #[test]
    fn a_free_combination_conflicts_with_nothing() {
        let list = [hk("refresh", "Ctrl+R"), hk("clip_copy", "Ctrl+C")];
        assert_eq!(conflict_in(&list, "clip_copy", "Ctrl+Alt+K"), None);
    }

    #[test]
    fn a_taken_combination_names_the_action_holding_it() {
        let list = [hk("filter_files", "Ctrl+F"), hk("clip_copy", "Ctrl+C")];
        assert_eq!(
            conflict_in(&list, "clip_copy", "Ctrl+F"),
            Some("filter_files".to_string())
        );
    }

    #[test]
    fn rebinding_an_action_to_its_own_key_is_not_a_conflict() {
        let list = [hk("clip_copy", "Ctrl+C")];
        assert_eq!(conflict_in(&list, "clip_copy", "Ctrl+C"), None);
    }

    #[test]
    fn the_match_ignores_case() {
        let list = [hk("filter_files", "ctrl+f")];
        assert_eq!(
            conflict_in(&list, "clip_copy", "Ctrl+F"),
            Some("filter_files".to_string())
        );
    }

    #[test]
    fn the_two_pairs_that_share_a_combo_by_design_are_allowed() {
        let list = [
            hk("toggle_video_fullscreen", "Alt+Return"),
            hk("find_files", "Ctrl+S"),
        ];
        assert_eq!(conflict_in(&list, "expand_terminal", "Alt+Return"), None);
        assert_eq!(conflict_in(&list, "editor_save", "Ctrl+S"), None);
    }

    #[test]
    fn a_design_pair_still_conflicts_with_an_unrelated_action() {
        let list = [hk("find_files", "Ctrl+S")];
        assert_eq!(
            conflict_in(&list, "clip_cut", "Ctrl+S"),
            Some("find_files".to_string())
        );
    }

    #[test]
    fn every_clipboard_default_is_free_against_the_shipped_set() {
        let defaults = get_default_hotkeys();
        for id in ["clip_cut", "clip_copy", "clip_paste"] {
            let keys = defaults.iter().find(|h| h.id == id).unwrap().keys.clone();
            assert_eq!(conflict_in(&defaults, id, &keys), None, "{id} ships shadowed");
        }
    }

    #[test]
    fn hotkey_equality_works() {
        let a = HotKey { id: "x".to_string(), description: "d".to_string(), keys: "Ctrl+X".to_string() };
        let b = a.clone();
        assert_eq!(a, b);
    }
}

pub fn resolve_action(config: &client_config::AppConfig, keyval: gtk::gdk::Key, state: gtk::gdk::ModifierType) -> Option<String> {
    let pressed_str = keyval_to_string(keyval, state);
    if pressed_str.is_empty() {
        return None;
    }

    let hotkeys = get_hotkeys(config);
    for hk in hotkeys {
        if hk.keys.eq_ignore_ascii_case(&pressed_str) {
            return Some(hk.id);
        }
    }
    None
}
