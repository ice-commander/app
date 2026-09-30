use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct ConfigManager {
    store: HashMap<String, Value>,
    config_path: PathBuf,
}

impl ConfigManager {
    fn new(app_name: &str) -> Self {
        let config_path = Self::get_config_path(app_name);
        let store = Self::load_from_disk(&config_path).unwrap_or_default();
        Self { store, config_path }
    }

    fn get_config_path(app_name: &str) -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let _ = fs::create_dir_all(&path);
        path.push(format!("{app_name}.conf"));
        path
    }

    fn load_from_disk(path: &PathBuf) -> Option<HashMap<String, Value>> {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(store) = serde_json::from_str(&content) {
                return Some(store);
            }
        }
        None
    }

    fn save_to_disk(&self) {
        if let Ok(content) = serde_json::to_string_pretty(&self.store) {
            let temp_path = self.config_path.with_extension("tmp");
            if fs::write(&temp_path, content).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600));
                }
                let _ = fs::rename(temp_path, &self.config_path);
            }
        }
    }
}

#[derive(Clone)]
pub struct AppConfig {
    inner: Arc<Mutex<ConfigManager>>,
}

impl AppConfig {
    pub fn new(app_name: &str) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ConfigManager::new(app_name))),
        }
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        if let Ok(manager) = self.inner.lock() {
            if let Some(val) = manager.store.get(key) {
                return serde_json::from_value(val.clone()).ok();
            }
        }
        None
    }

    pub fn get_or_default<T: DeserializeOwned + Default>(&self, key: &str) -> T {
        self.get::<T>(key).unwrap_or_default()
    }

    pub fn set<T: Serialize>(&self, key: &str, value: T) {
        if let Ok(mut manager) = self.inner.lock() {
            if let Ok(json_value) = serde_json::to_value(value) {
                manager.store.insert(key.to_string(), json_value);
            }
        }
    }

    /// Removes a key, so that reading it is absent rather than a default. The
    /// two differ wherever "never chosen" and "chose nothing" are not the same.
    pub fn forget(&self, key: &str) {
        if let Ok(mut manager) = self.inner.lock() {
            manager.store.remove(key);
        }
    }

    /// Every key currently held, so a caller can decide which to forget without
    /// keeping a second list that could drift from what is actually stored.
    pub fn keys(&self) -> Vec<String> {
        match self.inner.lock() {
            Ok(manager) => {
                let mut held: Vec<String> = manager.store.keys().cloned().collect();
                held.sort();
                held
            }
            Err(_) => Vec::new(),
        }
    }

    pub fn save(&self) {
        if let Ok(manager) = self.inner.lock() {
            manager.save_to_disk();
        }
    }

    pub fn update<T, F>(&self, key: &str, mut f: F)
    where
        T: Serialize + DeserializeOwned + Default,
        F: FnMut(&mut T),
    {
        if let Ok(mut manager) = self.inner.lock() {
            let mut current: T = if let Some(val) = manager.store.get(key) {
                serde_json::from_value(val.clone()).unwrap_or_default()
            } else {
                T::default()
            };

            f(&mut current);

            if let Ok(json_value) = serde_json::to_value(current) {
                manager.store.insert(key.to_string(), json_value);
                manager.save_to_disk();
            }
        }
    }

    pub fn config_path(&self) -> PathBuf {
        if let Ok(manager) = self.inner.lock() {
            manager.config_path.clone()
        } else {
            PathBuf::new()
        }
    }
}

#[cfg(test)]
mod tests {

    #[cfg(unix)]
    #[test]
    fn saving_keeps_the_config_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("ic-cfg-mode-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("ice-commander.conf");
        let _ = fs::remove_file(&path);

        let mut mgr = ConfigManager::new("unused");
        mgr.config_path = path.clone();
        mgr.store.insert("k".into(), Value::String("v".into()));
        mgr.save_to_disk();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config is group/world readable: {mode:o}");

        mgr.save_to_disk();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode lost on re-save: {mode:o}");

        let _ = fs::remove_dir_all(&dir);
    }
    use super::*;

    fn test_config() -> AppConfig {
        AppConfig::new("_ice_commander_unit_tests_do_not_use")
    }

    #[test]
    fn get_missing_key_returns_none() {
        let cfg = test_config();
        let val: Option<String> = cfg.get("nonexistent_key_xyz_abc");
        assert!(val.is_none());
    }

    #[test]
    fn set_and_get_string_roundtrip() {
        let cfg = test_config();
        cfg.set("test_theme", "dark".to_string());
        assert_eq!(cfg.get::<String>("test_theme"), Some("dark".to_string()));
    }

    #[test]
    fn set_and_get_bool() {
        let cfg = test_config();
        cfg.set("test_show_hidden", true);
        assert_eq!(cfg.get::<bool>("test_show_hidden"), Some(true));
    }

    #[test]
    fn set_and_get_integer() {
        let cfg = test_config();
        cfg.set("test_font_size", 14u32);
        assert_eq!(cfg.get::<u32>("test_font_size"), Some(14));
    }

    #[test]
    fn get_or_default_returns_default_for_missing_key() {
        let cfg = test_config();
        let val: String = cfg.get_or_default("totally_missing_key_zzz");
        assert_eq!(val, "");
    }

    #[test]
    fn set_overwrites_previous_value() {
        let cfg = test_config();
        cfg.set("test_overwrite_key", "first".to_string());
        cfg.set("test_overwrite_key", "second".to_string());
        assert_eq!(
            cfg.get::<String>("test_overwrite_key"),
            Some("second".to_string())
        );
    }

    #[test]
    fn config_path_is_a_flat_conf_file() {
        let cfg = test_config();
        let path = cfg.config_path();
        assert!(
            path.to_string_lossy().ends_with(".conf"),
            "path: {}",
            path.display()
        );
    }
}
