use client_config::AppConfig;

pub const NAME_KEY: &str = "app-name";

pub fn current(config: &AppConfig) -> String {
    config
        .get::<String>(NAME_KEY)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(default_name)
}

pub fn default_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "IceCommander Client".to_string())
}
