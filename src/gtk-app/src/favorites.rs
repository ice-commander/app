pub fn get_favorites(config: &client_config::AppConfig) -> Vec<String> {
    config.get::<Vec<String>>("ui.favorites").unwrap_or_default()
}

pub fn is_favorite(config: &client_config::AppConfig, path: &str) -> bool {
    get_favorites(config).contains(&path.to_string())
}

#[allow(dead_code)] // pair with `toggle_favorite`, exercised by this module's tests
pub fn add_favorite(config: &client_config::AppConfig, path: &str) {
    let mut favs = get_favorites(config);
    if !favs.contains(&path.to_string()) {
        favs.push(path.to_string());
        config.set("ui.favorites", favs);
        config.save();
    }
}

#[allow(dead_code)] // pair with `toggle_favorite`, exercised by this module's tests
pub fn remove_favorite(config: &client_config::AppConfig, path: &str) {
    let mut favs = get_favorites(config);
    if let Some(pos) = favs.iter().position(|x| x == path) {
        favs.remove(pos);
        config.set("ui.favorites", favs);
        config.save();
    }
}

pub fn toggle_favorite(config: &client_config::AppConfig, path: &str) {
    let mut favs = get_favorites(config);
    if let Some(pos) = favs.iter().position(|x| x == path) {
        favs.remove(pos);
    } else {
        favs.push(path.to_string());
    }
    config.set("ui.favorites", favs);
    config.save();
}

pub fn set_favorites(config: &client_config::AppConfig, favs: Vec<String>) {
    config.set("ui.favorites", favs);
    config.save();
}

pub fn is_favorites_only(config: &client_config::AppConfig) -> bool {
    config.get::<bool>("ui.drives_toolbar_favorites_only").unwrap_or(false)
}

pub fn set_favorites_only(config: &client_config::AppConfig, val: bool) {
    config.set("ui.drives_toolbar_favorites_only", val);
    config.save();
}

pub fn reset_to_defaults(config: &client_config::AppConfig) {
    set_favorites(config, vec!["local_fs:/".to_string(), "local_fs:~".to_string()]);
    set_favorites_only(config, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(suffix: &str) -> client_config::AppConfig {
        client_config::AppConfig::new(&format!("_ice_test_favorites_{}", suffix))
    }

    #[test]
    fn fresh_config_has_no_favorites() {
        let cfg = test_config("fresh");
        assert!(get_favorites(&cfg).is_empty());
    }

    #[test]
    fn is_favorite_returns_false_on_fresh_config() {
        let cfg = test_config("is_fav_false");
        assert!(!is_favorite(&cfg, "/home/user/docs"));
    }

    #[test]
    fn is_favorites_only_defaults_to_false() {
        let cfg = test_config("favonly_default");
        assert!(!is_favorites_only(&cfg));
    }

    #[test]
    fn add_and_get_favorite_roundtrip() {
        let cfg = test_config("add_get");
        add_favorite(&cfg, "/home/user/projects");
        assert!(is_favorite(&cfg, "/home/user/projects"));
    }

    #[test]
    fn add_duplicate_does_not_duplicate() {
        let cfg = test_config("no_dup");
        add_favorite(&cfg, "/path/a");
        add_favorite(&cfg, "/path/a");
        assert_eq!(get_favorites(&cfg).len(), 1);
    }

    #[test]
    fn remove_favorite_works() {
        let cfg = test_config("remove");
        add_favorite(&cfg, "/path/to/remove");
        remove_favorite(&cfg, "/path/to/remove");
        assert!(!is_favorite(&cfg, "/path/to/remove"));
    }

    #[test]
    fn toggle_adds_then_removes() {
        let cfg = test_config("toggle");
        toggle_favorite(&cfg, "/toggle/path");
        assert!(is_favorite(&cfg, "/toggle/path"));
        toggle_favorite(&cfg, "/toggle/path");
        assert!(!is_favorite(&cfg, "/toggle/path"));
    }

    #[test]
    fn reset_to_defaults_sets_two_favorites() {
        let cfg = test_config("reset");
        reset_to_defaults(&cfg);
        let favs = get_favorites(&cfg);
        assert_eq!(favs.len(), 2);
        assert!(favs.contains(&"local_fs:/".to_string()));
        assert!(favs.contains(&"local_fs:~".to_string()));
    }

    #[test]
    fn reset_to_defaults_sets_favorites_only_false() {
        let cfg = test_config("reset_favonly");
        set_favorites_only(&cfg, true);
        reset_to_defaults(&cfg);
        assert!(!is_favorites_only(&cfg));
    }
}
