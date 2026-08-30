
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const BLOB_PREFIX_V1: &str = "icv1:";
const BLOB_PREFIX_V2: &str = "icv2:";
const HKDF_CONTEXT: &[u8] = b"ice-commander.conn.v1";
const AAD_V2: &[u8] = b"ice-commander.conn.v2";
const AAD_KEYRING: &[u8] = b"ice-commander.keyring.v1";
const KEYRING_MAGIC: &str = "ic_keyring";
const KEYRING_VERSION: u32 = 1;
const NONCE_LEN: usize = 24;
const SALT_LEN: usize = 32;
const EXPORT_SALT_LEN: usize = 16;

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

static STORE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_store_dir(dir: PathBuf) {
    let _ = STORE_DIR.set(dir);
}

fn store_dir() -> PathBuf {
    if let Some(dir) = STORE_DIR.get() {
        return dir.clone();
    }
    let mut p = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("ice-commander");
    p
}

fn salt_path() -> PathBuf {
    store_dir().join("keysalt")
}

fn keyring_path() -> PathBuf {
    store_dir().join("keyring")
}

fn install_salt() -> Vec<u8> {
    let path = salt_path();
    if let Ok(data) = std::fs::read(&path) {
        if data.len() == SALT_LEN {
            return data;
        }
    }
    let mut salt = vec![0u8; SALT_LEN];
    rand::rng().fill_bytes(&mut salt);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, &salt);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    salt
}

fn machine_key() -> [u8; 32] {
    let machine_id = machine_uid::get().unwrap_or_default();
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let salt = install_salt();
    let ikm = [machine_id.as_bytes(), b"\x00", user.as_bytes()].concat();
    let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut key = [0u8; 32];
    hk.expand(HKDF_CONTEXT, &mut key)
        .expect("32 bytes is a valid HKDF output length");
    key
}

fn seal(key: &[u8; 32], plain: &[u8], aad: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad })
        .expect("XChaCha20-Poly1305 encryption is infallible for in-memory data");
    let mut packed = nonce.to_vec();
    packed.extend_from_slice(&ct);
    packed
}

fn open(key: &[u8; 32], packed: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
    if packed.len() < NONCE_LEN {
        return None;
    }
    let (nonce, ct) = packed.split_at(NONCE_LEN);
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad })
        .ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    Machine,
    Password,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockError {
    NotPasswordProtected,
    WrongPassword,
    Malformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyringError {
    Locked,
    Io,
}

struct Keyring {
    protection: Protection,
    salt: Vec<u8>,
    wrapped_dek: Vec<u8>,
}

fn parse_keyring(json: &str) -> Option<Keyring> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get(KEYRING_MAGIC)?.as_u64()? != KEYRING_VERSION as u64 {
        return None;
    }
    let protection = match v.get("protection")?.as_str()? {
        "password" => Protection::Password,
        "machine" => Protection::Machine,
        _ => return None,
    };
    let get = |k: &str| -> Option<Vec<u8>> {
        b64().decode(v.get(k)?.as_str()?).ok()
    };
    Some(Keyring {
        protection,
        salt: get("salt").unwrap_or_default(),
        wrapped_dek: get("wrapped_dek")?,
    })
}

fn write_keyring(ring: &Keyring) -> Result<(), KeyringError> {
    let mut doc = serde_json::json!({
        KEYRING_MAGIC: KEYRING_VERSION,
        "protection": match ring.protection {
            Protection::Machine => "machine",
            Protection::Password => "password",
        },
        "wrapped_dek": b64().encode(&ring.wrapped_dek),
    });
    if ring.protection == Protection::Password {
        doc["kdf"] = serde_json::json!({
            "alg": "argon2id", "m_kib": A2_MEM_KIB, "t": A2_ITERS, "p": A2_LANES
        });
        doc["salt"] = serde_json::Value::String(b64().encode(&ring.salt));
    }

    let path = keyring_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|_| KeyringError::Io)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&doc).map_err(|_| KeyringError::Io)?)
        .map_err(|_| KeyringError::Io)?;
    harden_file_permissions(&path);
    Ok(())
}

fn read_keyring() -> Option<Keyring> {
    parse_keyring(&std::fs::read_to_string(keyring_path()).ok()?)
}

fn dek_slot() -> &'static Mutex<Option<[u8; 32]>> {
    static SLOT: OnceLock<Mutex<Option<[u8; 32]>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

pub fn protection() -> Protection {
    read_keyring().map_or(Protection::Machine, |r| r.protection)
}

pub fn is_unlocked() -> bool {
    if dek_slot().lock().is_ok_and(|d| d.is_some()) {
        return true;
    }
    protection() == Protection::Machine && machine_dek().is_some()
}

fn machine_dek() -> Option<[u8; 32]> {
    if let Some(cached) = dek_slot().lock().ok().and_then(|d| *d) {
        return Some(cached);
    }
    let mkey = machine_key();
    let dek = match read_keyring() {
        Some(ring) if ring.protection == Protection::Machine => {
            let raw = open(&mkey, &ring.wrapped_dek, AAD_KEYRING)?;
            let mut dek = [0u8; 32];
            if raw.len() != 32 {
                return None;
            }
            dek.copy_from_slice(&raw);
            dek
        }
        Some(_) => return None, // password protected: only `unlock_with_password` can open it
        None => {
            let mut dek = [0u8; 32];
            rand::rng().fill_bytes(&mut dek);
            write_keyring(&Keyring {
                protection: Protection::Machine,
                salt: Vec::new(),
                wrapped_dek: seal(&mkey, &dek, AAD_KEYRING),
            })
            .ok()?;
            dek
        }
    };
    if let Ok(mut slot) = dek_slot().lock() {
        *slot = Some(dek);
    }
    Some(dek)
}

fn dek() -> Option<[u8; 32]> {
    if let Some(cached) = dek_slot().lock().ok().and_then(|d| *d) {
        return Some(cached);
    }
    machine_dek()
}

pub fn unlock_with_password(password: &str) -> Result<(), UnlockError> {
    let ring = read_keyring().ok_or(UnlockError::Malformed)?;
    if ring.protection != Protection::Password {
        return Err(UnlockError::NotPasswordProtected);
    }
    let key = argon2_key(password, &ring.salt);
    let raw = open(&key, &ring.wrapped_dek, AAD_KEYRING).ok_or(UnlockError::WrongPassword)?;
    if raw.len() != 32 {
        return Err(UnlockError::Malformed);
    }
    let mut dek = [0u8; 32];
    dek.copy_from_slice(&raw);
    if let Ok(mut slot) = dek_slot().lock() {
        *slot = Some(dek);
    }
    Ok(())
}

pub fn lock() {
    if let Ok(mut slot) = dek_slot().lock() {
        *slot = None;
    }
}

pub fn set_master_password(password: Option<&str>) -> Result<(), KeyringError> {
    let dek = dek().ok_or(KeyringError::Locked)?;
    let ring = match password.filter(|p| !p.is_empty()) {
        Some(pw) => {
            let mut salt = vec![0u8; EXPORT_SALT_LEN];
            rand::rng().fill_bytes(&mut salt);
            let key = argon2_key(pw, &salt);
            Keyring {
                protection: Protection::Password,
                wrapped_dek: seal(&key, &dek, AAD_KEYRING),
                salt,
            }
        }
        None => Keyring {
            protection: Protection::Machine,
            salt: Vec::new(),
            wrapped_dek: seal(&machine_key(), &dek, AAD_KEYRING),
        },
    };
    write_keyring(&ring)
}

pub fn is_encrypted(s: &str) -> bool {
    s.starts_with(BLOB_PREFIX_V2) || s.starts_with(BLOB_PREFIX_V1)
}

pub fn is_legacy(s: &str) -> bool {
    s.starts_with(BLOB_PREFIX_V1)
}

pub fn encrypt_secret(plain: &str) -> String {
    try_encrypt_secret(plain).unwrap_or_default()
}

pub fn try_encrypt_secret(plain: &str) -> Option<String> {
    if plain.is_empty() || is_encrypted(plain) {
        return Some(plain.to_string());
    }
    let dek = dek()?;
    Some(format!(
        "{}{}",
        BLOB_PREFIX_V2,
        b64().encode(seal(&dek, plain.as_bytes(), AAD_V2))
    ))
}

pub fn decrypt_secret(stored: &str) -> Option<String> {
    if let Some(enc) = stored.strip_prefix(BLOB_PREFIX_V2) {
        let packed = b64().decode(enc).ok()?;
        let plain = open(&dek()?, &packed, AAD_V2)?;
        return String::from_utf8(plain).ok();
    }
    if let Some(enc) = stored.strip_prefix(BLOB_PREFIX_V1) {
        let packed = b64().decode(enc).ok()?;
        let plain = open(&machine_key(), &packed, HKDF_CONTEXT)?;
        return String::from_utf8(plain).ok();
    }
    Some(stored.to_string())
}

pub fn harden_file_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

const EXPORT_MAGIC: &str = "ic_connections";
const EXPORT_VERSION: u32 = 1;
const A2_MEM_KIB: u32 = 19_456;
const A2_ITERS: u32 = 2;
const A2_LANES: u32 = 1;

pub enum ImportError {
    Malformed,
    NeedsPassword,
    WrongPassword,
}

fn argon2_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let params = argon2::Params::new(A2_MEM_KIB, A2_ITERS, A2_LANES, Some(32))
        .expect("valid static Argon2 params");
    let a2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = [0u8; 32];
    a2.hash_password_into(password.as_bytes(), salt, &mut key)
        .expect("static params and 32-byte output are valid");
    key
}

pub fn wrap_export(payload_json: &str, password: Option<&str>) -> String {
    match password.filter(|p| !p.is_empty()) {
        None => {
            let conns: serde_json::Value =
                serde_json::from_str(payload_json).unwrap_or(serde_json::Value::Array(vec![]));
            serde_json::to_string_pretty(&serde_json::json!({
                EXPORT_MAGIC: EXPORT_VERSION,
                "encrypted": false,
                "connections": conns,
            }))
            .unwrap_or_default()
        }
        Some(pw) => {
            let mut salt = [0u8; EXPORT_SALT_LEN];
            rand::rng().fill_bytes(&mut salt);
            let mut nonce = [0u8; NONCE_LEN];
            rand::rng().fill_bytes(&mut nonce);
            let cipher = XChaCha20Poly1305::new((&argon2_key(pw, &salt)).into());
            let ct = cipher
                .encrypt(
                    XNonce::from_slice(&nonce),
                    Payload { msg: payload_json.as_bytes(), aad: EXPORT_MAGIC.as_bytes() },
                )
                .expect("in-memory encryption is infallible");
            serde_json::to_string_pretty(&serde_json::json!({
                EXPORT_MAGIC: EXPORT_VERSION,
                "encrypted": true,
                "kdf": { "alg": "argon2id", "m_kib": A2_MEM_KIB, "t": A2_ITERS, "p": A2_LANES },
                "salt": b64().encode(salt),
                "nonce": b64().encode(nonce),
                "data": b64().encode(ct),
            }))
            .unwrap_or_default()
        }
    }
}

pub fn import_needs_password(json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .map(|v| {
            v.get(EXPORT_MAGIC).is_some()
                && v.get("encrypted").and_then(|e| e.as_bool()) == Some(true)
        })
        .unwrap_or(false)
}

pub fn unwrap_export(json: &str, password: Option<&str>) -> Result<String, ImportError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| ImportError::Malformed)?;
    if v.get(EXPORT_MAGIC).is_none() {
        return Err(ImportError::Malformed);
    }
    if v.get("encrypted").and_then(|e| e.as_bool()) == Some(true) {
        let pw = password.filter(|p| !p.is_empty()).ok_or(ImportError::NeedsPassword)?;
        let get_b64 = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .and_then(|s| b64().decode(s).ok())
                .ok_or(ImportError::Malformed)
        };
        let salt = get_b64("salt")?;
        let nonce = get_b64("nonce")?;
        let data = get_b64("data")?;
        if nonce.len() != NONCE_LEN {
            return Err(ImportError::Malformed);
        }
        let cipher = XChaCha20Poly1305::new((&argon2_key(pw, &salt)).into());
        let plain = cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload { msg: &data, aad: EXPORT_MAGIC.as_bytes() },
            )
            .map_err(|_| ImportError::WrongPassword)?;
        String::from_utf8(plain).map_err(|_| ImportError::Malformed)
    } else {
        let conns = v.get("connections").ok_or(ImportError::Malformed)?;
        serde_json::to_string(conns).map_err(|_| ImportError::Malformed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_store<T>(f: impl FnOnce() -> T) -> T {
        use std::sync::Mutex as StdMutex;
        static GUARD: OnceLock<StdMutex<()>> = OnceLock::new();
        let _held = GUARD.get_or_init(|| StdMutex::new(())).lock();

        let dir = std::env::temp_dir().join(format!("secret-store-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        set_store_dir(dir);
        fresh_keyring();
        f()
    }

    fn fresh_keyring() {
        let _ = std::fs::remove_file(keyring_path());
        lock();
    }

    #[test]
    fn secret_roundtrip() {
        with_store(|| {
            let enc = encrypt_secret("hunter2");
            assert!(is_encrypted(&enc));
            assert_ne!(enc, "hunter2");
            assert_eq!(decrypt_secret(&enc).as_deref(), Some("hunter2"));
        });
    }

    #[test]
    fn plaintext_passes_through_decrypt() {
        with_store(|| {
            assert_eq!(decrypt_secret("legacy-pass").as_deref(), Some("legacy-pass"));
        });
    }

    #[test]
    fn encrypt_is_idempotent() {
        with_store(|| {
            let once = encrypt_secret("s3cret");
            assert_eq!(encrypt_secret(&once), once);
        });
    }

    #[test]
    fn tampered_blob_fails() {
        with_store(|| {
            let enc = encrypt_secret("hunter2");
            let mut bad = enc.clone();
            bad.pop();
            bad.push('A');
            assert_eq!(decrypt_secret(&bad), None);
        });
    }

    #[test]
    fn a_master_password_survives_lock_and_unlock() {
        with_store(|| {
            let enc = encrypt_secret("hunter2");
            assert_eq!(protection(), Protection::Machine);

            set_master_password(Some("open sesame")).expect("re-wrap succeeds");
            assert_eq!(protection(), Protection::Password);
            assert_eq!(decrypt_secret(&enc).as_deref(), Some("hunter2"));

            lock();
            assert!(!is_unlocked());
            assert_eq!(decrypt_secret(&enc), None);

            assert_eq!(unlock_with_password("wrong"), Err(UnlockError::WrongPassword));
            assert_eq!(decrypt_secret(&enc), None);

            unlock_with_password("open sesame").expect("the right password opens it");
            assert!(is_unlocked());
            assert_eq!(decrypt_secret(&enc).as_deref(), Some("hunter2"));
        });
    }

    #[test]
    fn changing_the_password_keeps_the_secrets_readable() {
        with_store(|| {
            let enc = encrypt_secret("hunter2");
            set_master_password(Some("first")).unwrap();
            set_master_password(Some("second")).unwrap();

            lock();
            assert_eq!(unlock_with_password("first"), Err(UnlockError::WrongPassword));
            unlock_with_password("second").unwrap();
            assert_eq!(decrypt_secret(&enc).as_deref(), Some("hunter2"));
        });
    }

    #[test]
    fn removing_the_password_goes_back_to_machine_protection() {
        with_store(|| {
            let enc = encrypt_secret("hunter2");
            set_master_password(Some("temporary")).unwrap();
            set_master_password(None).unwrap();
            assert_eq!(protection(), Protection::Machine);

            lock();
            assert!(is_unlocked(), "machine protection needs no prompt");
            assert_eq!(decrypt_secret(&enc).as_deref(), Some("hunter2"));
        });
    }

    #[test]
    fn a_secret_never_comes_back_as_plaintext_in_any_state() {
        with_store(|| {
            const SECRET: &str = "correct-horse-battery-staple";
            let check = |state: &str| {
                let out = encrypt_secret(SECRET);
                assert!(
                    !out.contains(SECRET),
                    "plaintext leaked with the store {state}: {out:?}"
                );
            };

            check("freshly created");
            assert!(is_encrypted(&encrypt_secret(SECRET)));

            set_master_password(Some("pw")).unwrap();
            check("password protected and unlocked");
            assert!(is_encrypted(&encrypt_secret(SECRET)));

            lock();
            check("password protected and locked");
            assert_eq!(encrypt_secret(SECRET), "");

            unlock_with_password("pw").unwrap();
            set_master_password(None).unwrap();
            lock();
            let saved = std::fs::read(keyring_path()).unwrap();
            std::fs::write(keyring_path(), b"not json").unwrap();
            check("keyring unreadable");
            std::fs::write(keyring_path(), saved).unwrap();
        });
    }

    #[test]
    fn a_locked_store_drops_the_secret_rather_than_leaking_it() {
        with_store(|| {
            let _ = encrypt_secret("warm up the keyring");
            set_master_password(Some("pw")).unwrap();
            lock();

            assert_eq!(try_encrypt_secret("hunter2"), None);
            assert_eq!(encrypt_secret("hunter2"), "");
            assert!(!encrypt_secret("hunter2").contains("hunter2"));
        });
    }

    #[test]
    fn a_v1_blob_is_still_readable_and_reports_as_legacy() {
        with_store(|| {
            let packed = seal(&machine_key(), b"old-pass", HKDF_CONTEXT);
            let legacy = format!("{}{}", BLOB_PREFIX_V1, b64().encode(packed));

            assert!(is_encrypted(&legacy));
            assert!(is_legacy(&legacy));
            assert_eq!(decrypt_secret(&legacy).as_deref(), Some("old-pass"));
            assert!(!is_legacy(&encrypt_secret("new-pass")));
        });
    }

    #[test]
    fn export_plain_roundtrip() {
        let payload = r#"[{"name":"t","pass":"pw"}]"#;
        let out = wrap_export(payload, None);
        assert!(!import_needs_password(&out));
        let back = unwrap_export(&out, None).ok().unwrap();
        assert!(back.contains("pw"));
    }

    #[test]
    fn export_encrypted_roundtrip_and_wrong_password() {
        let payload = r#"[{"name":"t","pass":"pw"}]"#;
        let out = wrap_export(payload, Some("master"));
        assert!(import_needs_password(&out));
        assert!(!out.contains("pw"));
        let back = unwrap_export(&out, Some("master")).ok().unwrap();
        assert!(back.contains("pw"));
        assert!(matches!(unwrap_export(&out, Some("nope")), Err(ImportError::WrongPassword)));
        assert!(matches!(unwrap_export(&out, None), Err(ImportError::NeedsPassword)));
    }
}
