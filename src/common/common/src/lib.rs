use serde::{Deserialize, Serialize};

pub mod error;
pub mod installer;
pub mod version;

pub use error::AppError;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdatePackage {
    pub app_type: String,
    pub build_type: String,
    pub version: String,
    pub url: String,
    #[serde(default)]
    pub md5: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UpdatesFile {
    #[serde(default)]
    pub packages: Vec<UpdatePackage>,
    #[serde(rename = "updateTs", default)]
    pub update_ts: i64,
}

pub static CONNECT_TIMEOUT_SECS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(20);

pub static REQUEST_TIMEOUT_SECS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(20);

pub fn set_connect_timeout_secs(secs: u64) {
    CONNECT_TIMEOUT_SECS.store(secs, std::sync::atomic::Ordering::Relaxed);
}

pub fn set_request_timeout_secs(secs: u64) {
    REQUEST_TIMEOUT_SECS.store(secs, std::sync::atomic::Ordering::Relaxed);
}

pub fn connect_timeout() -> Option<std::time::Duration> {
    secs_to_duration(&CONNECT_TIMEOUT_SECS)
}

pub fn request_timeout() -> Option<std::time::Duration> {
    secs_to_duration(&REQUEST_TIMEOUT_SECS)
}

fn secs_to_duration(cell: &std::sync::atomic::AtomicU64) -> Option<std::time::Duration> {
    match cell.load(std::sync::atomic::Ordering::Relaxed) {
        0 => None,
        s => Some(std::time::Duration::from_secs(s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_served_updates_json_parses() {
        let body = r#"{"packages":[{"app_type":"gui","build_type":"deb","version":"0.7.116","url":"/download/IceCommander-gui-deb-0.7.116.deb","md5":"9b73"}],"updateTs":1787761175}"#;
        let file: UpdatesFile = serde_json::from_str(body).unwrap();
        assert_eq!(file.packages.len(), 1);
        assert_eq!(file.packages[0].version, "0.7.116");
        assert_eq!(file.update_ts, 1787761175);
    }

    #[test]
    fn a_bare_array_is_no_longer_accepted() {
        let body = r#"[{"app_type":"gui","build_type":"deb","version":"0.7.116","url":"/download/x.deb","md5":"9b73"}]"#;
        assert!(serde_json::from_str::<UpdatesFile>(body).is_err());
    }
}
