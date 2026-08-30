pub use ::secret_store::{
    decrypt_secret, encrypt_secret, is_legacy, is_unlocked, protection, set_master_password,
    unlock_with_password, ImportError, Protection,
};

use crate::connection_manager::FtpConnection;

const CONNECTIONS_KEY: &str = "ui.ftp_connections";

fn map_secret_fields(c: &mut FtpConnection, mut f: impl FnMut(&str) -> Option<String>) {
    for field in [
        &mut c.pass,
        &mut c.passphrase,
        &mut c.tunnel_pass,
        &mut c.tunnel_passphrase,
    ] {
        if let Some(v) = field.as_deref() {
            *field = f(v).or_else(|| field.clone());
        }
    }
}

pub fn saving_enabled(config: &client_config::AppConfig) -> bool {
    config.get::<bool>("ui.save_passwords").unwrap_or(true)
}

pub fn seal_connection(config: &client_config::AppConfig, c: &mut FtpConnection) {
    if saving_enabled(config) {
        map_secret_fields(c, |v| Some(encrypt_secret(v)));
    } else {
        map_secret_fields(c, |_| Some(String::new()));
    }
}

pub fn open_connection(c: &mut FtpConnection) {
    map_secret_fields(c, decrypt_secret);
}

pub fn upgrade_legacy_secrets(config: &client_config::AppConfig) {
    let mut conns: Vec<FtpConnection> = config.get(CONNECTIONS_KEY).unwrap_or_default();
    let mut changed = false;
    for c in &mut conns {
        map_secret_fields(c, |v| {
            if !is_legacy(v) {
                return Some(v.to_string());
            }
            changed = true;
            decrypt_secret(v).map(|plain| encrypt_secret(&plain))
        });
    }
    if changed {
        config.set(CONNECTIONS_KEY, conns);
        config.save();
    }
}

pub fn forget_stored_secrets(config: &client_config::AppConfig) {
    let mut conns: Vec<FtpConnection> = config.get(CONNECTIONS_KEY).unwrap_or_default();
    if conns.is_empty() {
        return;
    }
    for c in &mut conns {
        map_secret_fields(c, |_| Some(String::new()));
    }
    config.set(CONNECTIONS_KEY, conns);
    config.save();
}

pub fn opened(c: &FtpConnection) -> FtpConnection {
    let mut c = c.clone();
    open_connection(&mut c);
    c
}

pub use ::secret_store::harden_file_permissions;

pub fn export_connections(conns: &[FtpConnection], password: Option<&str>) -> String {
    let opened_conns: Vec<_> = conns.iter().map(opened).collect();
    let payload = serde_json::to_string(&opened_conns).unwrap_or_else(|_| "[]".to_string());
    ::secret_store::wrap_export(&payload, password)
}

pub use ::secret_store::import_needs_password;

pub fn parse_import(json: &str, password: Option<&str>) -> Result<Vec<FtpConnection>, ImportError> {
    let payload = ::secret_store::unwrap_export(json, password)?;
    serde_json::from_str(&payload).map_err(|_| ImportError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::secret_store::is_encrypted;

    fn conn(pass: &str) -> FtpConnection {
        FtpConnection {
            name: "t".into(),
            protocol: "ftp".into(),
            host: "h".into(),
            port: 21,
            user: "u".into(),
            pass: Some(pass.into()),
            ..Default::default()
        }
    }

    fn test_config(save_passwords: bool) -> client_config::AppConfig {
        let config = client_config::AppConfig::new("ice-commander-secret-test");
        config.set("ui.save_passwords", save_passwords);
        config
    }

    #[test]
    fn seal_open_roundtrip() {
        let config = test_config(true);
        let mut c = conn("pw");
        seal_connection(&config, &mut c);
        assert!(is_encrypted(c.pass.as_deref().unwrap()));
        open_connection(&mut c);
        assert_eq!(c.pass.as_deref(), Some("pw"));
    }

    #[test]
    fn saving_turned_off_drops_the_secret_instead_of_encrypting_it() {
        let config = test_config(false);
        let mut c = conn("pw");
        seal_connection(&config, &mut c);
        assert_eq!(c.pass.as_deref(), Some(""));
    }

    #[test]
    fn export_plain_and_import() {
        let out = export_connections(&[conn("pw")], None);
        assert!(!import_needs_password(&out));
        let back = parse_import(&out, None).ok().unwrap();
        assert_eq!(back[0].pass.as_deref(), Some("pw"));
    }

    #[test]
    fn export_encrypted_roundtrip() {
        let secret = "pw-plain-secret";
        let out = export_connections(&[conn(secret)], Some("master"));
        assert!(import_needs_password(&out));
        assert!(!out.contains(secret));
        let back = parse_import(&out, Some("master")).ok().unwrap();
        assert_eq!(back[0].pass.as_deref(), Some(secret));
        assert!(matches!(parse_import(&out, Some("no")), Err(ImportError::WrongPassword)));
    }
}
