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
    ]
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
