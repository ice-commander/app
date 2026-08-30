use std::path::PathBuf;

pub const DEFAULT_LEVEL: &str = "info";
pub const DEFAULT_MAX_MB: u64 = 5;

pub fn default_path() -> PathBuf {
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("ice-commander");
    p.push("app.log");
    p
}

pub fn path(config: &client_config::AppConfig) -> PathBuf {
    config
        .get::<String>("ui.log_file_path")
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_path)
}

pub fn target(config: &client_config::AppConfig) -> ic_logging::Target {
    match config.get::<String>("ui.log_target") {
        Some(s) => ic_logging::Target::from_str_or_off(&s),
        None if config.get::<bool>("ui.enable_logging").unwrap_or(false) => {
            ic_logging::Target::File
        }
        None => ic_logging::Target::Off,
    }
}

pub fn level(config: &client_config::AppConfig) -> ic_logging::LevelFilter {
    ic_logging::level_from_str(
        &config
            .get::<String>("ui.log_level")
            .unwrap_or_else(|| DEFAULT_LEVEL.to_string()),
    )
}

pub fn max_mb(config: &client_config::AppConfig) -> u64 {
    config.get::<u64>("ui.log_max_mb").unwrap_or(DEFAULT_MAX_MB).max(1)
}

pub fn apply(config: &client_config::AppConfig) {
    ic_logging::apply(ic_logging::Settings {
        target: target(config),
        level: level(config),
        file: path(config),
        max_bytes: max_mb(config) * 1024 * 1024,
    });
}
