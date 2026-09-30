pub use ::secret_store::{
    decrypt_secret, encrypt_secret, is_unlocked, protection, set_master_password,
    unlock_with_password, ImportError, Protection,
};

use crate::connection_manager::Connection;

fn map_secret_fields(c: &mut Connection, f: impl FnMut(&str) -> Option<String>) {
    let kind = c.kind.clone();
    connection_form::map_secrets(c, &kind, f);
}

pub fn saving_enabled(config: &client_config::AppConfig) -> bool {
    config.get::<bool>("ui.save_passwords").unwrap_or(true)
}

pub fn seal_connection(config: &client_config::AppConfig, c: &mut Connection) {
    if saving_enabled(config) {
        map_secret_fields(c, |v| Some(encrypt_secret(v)));
    } else {
        map_secret_fields(c, |_| Some(String::new()));
    }
}

pub fn open_connection(c: &mut Connection) {
    map_secret_fields(c, decrypt_secret);
}

pub fn forget_stored_secrets(config: &client_config::AppConfig) {
    let mut conns = connection_form::stored_connections(config);
    if conns.is_empty() {
        return;
    }
    for c in &mut conns {
        map_secret_fields(c, |_| Some(String::new()));
    }
    connection_form::save_connections(config, conns);
    config.save();
}

pub fn opened(c: &Connection) -> Connection {
    let mut c = c.clone();
    open_connection(&mut c);
    c
}

pub use ::secret_store::harden_file_permissions;

pub fn export_connections(conns: &[Connection], password: Option<&str>) -> String {
    let opened_conns: Vec<_> = conns.iter().map(opened).collect();
    let payload = serde_json::to_string(&opened_conns).unwrap_or_else(|_| "[]".to_string());
    ::secret_store::wrap_export(&payload, password)
}

pub use ::secret_store::import_needs_password;

pub fn parse_import(json: &str, password: Option<&str>) -> Result<Vec<Connection>, ImportError> {
    let payload = ::secret_store::unwrap_export(json, password)?;
    // An export written by an older version is FTP-shaped; both shapes read here.
    let raw: serde_json::Value =
        serde_json::from_str(&payload).map_err(|_| ImportError::Malformed)?;
    connection_form::connections_from_json(&raw).ok_or(ImportError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::secret_store::is_encrypted;

    fn conn(pass: &str) -> Connection {
        let mut built = Connection::new("ftp");
        built.name = "t".into();
        built.put("host", "h".into());
        built.put("port", "21".into());
        built.put("user", "u".into());
        built.put("pass", pass.into());
        built
    }

    // The application knows no protocol: a kind exists only because something
    // declared it, and its secrets are whatever that declaration marked.
    fn declare(kind: &str, secrets: &[&str]) {
        let fields: Vec<serde_json::Value> = secrets
            .iter()
            .map(|bind| serde_json::json!({ "bind": bind, "type": "text", "secret": true }))
            .collect();
        let document = serde_json::json!({
            "schema": 1,
            "kind": kind,
            "fields": fields,
            "form": { "t": "column", "children": [] },
        })
        .to_string();
        let host = ic_plugin_host::host_table();
        let id = std::ffi::CString::new(kind).expect("a kind id");
        assert_eq!(
            (host.register_connection_kind)(
                id.as_ptr(),
                document.as_ptr(),
                document.len() as u64,
                std::ptr::null(),
                std::ptr::null_mut(),
            ),
            ic_plugin_api::IC_OK
        );
    }

    fn test_config(save_passwords: bool) -> client_config::AppConfig {
        let config = client_config::AppConfig::new("ice-commander-secret-test");
        config.set("ui.save_passwords", save_passwords);
        config
    }

    #[test]
    fn seal_open_roundtrip() {
        declare("ftp", &["pass"]);
        let config = test_config(true);
        let mut c = conn("pw");
        seal_connection(&config, &mut c);
        assert!(is_encrypted(&c.value("pass").unwrap()));
        open_connection(&mut c);
        assert_eq!(c.value("pass").as_deref(), Some("pw"));
    }

    #[test]
    fn saving_turned_off_drops_the_secret_instead_of_encrypting_it() {
        declare("ftp", &["pass"]);
        let config = test_config(false);
        let mut c = conn("pw");
        seal_connection(&config, &mut c);
        assert_eq!(c.value("pass").as_deref(), Some(""));
    }

    #[test]
    fn export_plain_and_import() {
        let out = export_connections(&[conn("pw")], None);
        assert!(!import_needs_password(&out));
        let back = parse_import(&out, None).ok().unwrap();
        assert_eq!(back[0].value("pass").as_deref(), Some("pw"));
    }

    #[test]
    fn export_encrypted_roundtrip() {
        let secret = "pw-plain-secret";
        let out = export_connections(&[conn(secret)], Some("master"));
        assert!(import_needs_password(&out));
        assert!(!out.contains(secret));
        let back = parse_import(&out, Some("master")).ok().unwrap();
        assert_eq!(back[0].value("pass").as_deref(), Some(secret));
        assert!(matches!(
            parse_import(&out, Some("no")),
            Err(ImportError::WrongPassword)
        ));
    }

    fn probe_conn(kind: &str) -> Connection {
        let mut c = Connection::new(kind);
        c.name = "probe".to_string();
        for (bind, value) in [
            ("host", "example.org"),
            ("port", "22"),
            ("user", "ivan"),
            ("pass", "plain-pass"),
            ("passphrase", "plain-phrase"),
            ("tunnel_pass", "plain-tunnel"),
        ] {
            c.put(bind, value.to_string());
        }
        c
    }

    #[test]
    fn whatever_a_kind_declares_secret_is_what_gets_sealed_even_if_it_is_made_up() {
        declare("invented", &["passphrase", "tunnel_pass"]);
        let mut keys = connection_form::secret_binds("invented");
        keys.sort();
        assert_eq!(keys, vec!["passphrase", "tunnel_pass"]);

        let config = test_config(true);
        let mut c = probe_conn("invented");
        seal_connection(&config, &mut c);
        assert!(is_encrypted(&c.value("passphrase").unwrap()));
        assert!(is_encrypted(&c.value("tunnel_pass").unwrap()));
        assert_eq!(
            c.value("pass").as_deref(),
            Some("plain-pass"),
            "a field this kind did not mark secret is left alone"
        );
    }

    #[test]
    fn a_secret_the_plugin_keeps_outside_the_named_fields_is_sealed_too() {
        declare("boxed", &["api_token"]);
        let config = test_config(true);
        let mut c = probe_conn("boxed");
        c.settings
            .insert("api_token".to_string(), "t-0007".to_string());
        seal_connection(&config, &mut c);
        let sealed = c.settings.get("api_token").expect("still present");
        assert!(
            is_encrypted(sealed),
            "a secret kept in the settings map must not reach the config in plain text"
        );
        open_connection(&mut c);
        assert_eq!(
            c.settings.get("api_token").map(String::as_str),
            Some("t-0007")
        );
    }

    #[test]
    fn a_kind_whose_plugin_is_absent_has_its_record_left_untouched() {
        assert!(
            connection_form::secret_binds("webdav").is_empty(),
            "with no plugin loaded the application knows of no fields at all"
        );
        let before = probe_conn("webdav");
        let mut after = before.clone();
        map_secret_fields(&mut after, |v| Some(format!("sealed:{v}")));
        assert_eq!(
            after, before,
            "nothing is rewritten when nothing declared it"
        );
    }
}
