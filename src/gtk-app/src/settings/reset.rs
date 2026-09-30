/// Records the application keeps but never invented: connections the user typed
/// in, and the identity and peer table the p2p side is built on. A reset of the
/// settings must leave every one of them alone.
fn is_a_record(key: &str) -> bool {
    matches!(
        key,
        connection_form::CONNECTIONS_KEY
            | connection_form::LEGACY_CONNECTIONS_KEY
            | "ui.ftp_connection_folders"
    ) || key == "peers"
        || key == "app-name"
        || key.starts_with("app.")
}

/// Whether clearing this key returns something to its default. Defaults live at
/// each point of use (`config.get(..).unwrap_or(..)`), so forgetting a key *is*
/// the reset — there is no second table of defaults to keep in step.
pub(crate) fn is_a_setting(key: &str) -> bool {
    if is_a_record(key) {
        return false;
    }
    key.starts_with("ui.") || key.starts_with("plugins.")
}

/// Forgets every setting, including the one that marks the setup wizard as
/// done, so the next start asks again. Returns how many keys were cleared.
pub(crate) fn reset_settings(config: &client_config::AppConfig) -> usize {
    let doomed: Vec<String> = config
        .keys()
        .into_iter()
        .filter(|key| is_a_setting(key))
        .collect();
    for key in &doomed {
        config.forget(key);
    }
    config.save();
    doomed.len()
}

#[cfg(test)]
mod tests {
    use super::{is_a_setting, reset_settings};

    #[test]
    fn what_the_user_typed_in_is_never_a_setting() {
        for kept in [
            "ui.connections",
            "ui.ftp_connections",
            "ui.ftp_connection_folders",
            "peers",
            "app-name",
            "app.device_id",
            "app.private_key_b64",
            "app.refresh_token",
            "app.shares",
        ] {
            assert!(!is_a_setting(kept), "{kept} must survive a reset");
        }
    }

    #[test]
    fn everything_the_settings_pages_write_is_a_setting() {
        for cleared in [
            "ui.theme_index",
            "ui.language",
            "ui.hotkeys",
            "ui.favorites",
            "ui.toolbar.devtools",
            "ui.log_level",
            "ui.window_width",
            "ui.setup_wizard_done",
            "plugins.enabled",
            "plugins.catalog",
        ] {
            assert!(is_a_setting(cleared), "{cleared} should be reset");
        }
    }

    #[test]
    fn a_reset_clears_the_settings_and_keeps_the_records() {
        let config = client_config::AppConfig::new("ice-commander-reset-probe");
        config.set("ui.theme_index", 2u32);
        config.set("ui.setup_wizard_done", true);
        config.set("plugins.enabled", vec!["ic_ftp_fs".to_string()]);
        config.set("ui.ftp_connections", vec!["a record".to_string()]);
        config.set("app.device_id", "keep-me".to_string());

        let cleared = reset_settings(&config);
        assert_eq!(cleared, 3);
        assert_eq!(config.get::<u32>("ui.theme_index"), None);
        assert_eq!(config.get::<Vec<String>>("plugins.enabled"), None);
        assert_eq!(
            config.get::<bool>("ui.setup_wizard_done"),
            None,
            "the wizard must run again after a reset"
        );
        assert_eq!(
            config.get::<Vec<String>>("ui.ftp_connections"),
            Some(vec!["a record".to_string()])
        );
        assert_eq!(
            config.get::<String>("app.device_id"),
            Some("keep-me".to_string())
        );
    }

    #[test]
    fn resetting_twice_is_not_an_error() {
        let config = client_config::AppConfig::new("ice-commander-reset-twice");
        config.set("ui.theme_index", 1u32);
        assert_eq!(reset_settings(&config), 1);
        assert_eq!(reset_settings(&config), 0);
    }
}
