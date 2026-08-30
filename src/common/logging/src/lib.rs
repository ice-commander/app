
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

pub use log::{debug, error, info, trace, warn, Level, LevelFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    #[default]
    Off,
    Console,
    File,
    Both,
}

impl Target {
    pub fn as_str(self) -> &'static str {
        match self {
            Target::Off => "off",
            Target::Console => "console",
            Target::File => "file",
            Target::Both => "both",
        }
    }

    pub fn from_str_or_off(s: &str) -> Self {
        match s {
            "console" => Target::Console,
            "file" => Target::File,
            "both" => Target::Both,
            _ => Target::Off,
        }
    }

    fn writes_file(self) -> bool {
        matches!(self, Target::File | Target::Both)
    }

    fn writes_console(self) -> bool {
        matches!(self, Target::Console | Target::Both)
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub target: Target,
    pub level: LevelFilter,
    pub file: PathBuf,
    pub max_bytes: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            target: Target::Off,
            level: LevelFilter::Off,
            file: PathBuf::new(),
            max_bytes: 5 * 1024 * 1024,
        }
    }
}

pub fn level_from_str(s: &str) -> LevelFilter {
    match s.to_ascii_lowercase().as_str() {
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Off,
    }
}

pub fn level_as_str(level: LevelFilter) -> &'static str {
    match level {
        LevelFilter::Error => "error",
        LevelFilter::Warn => "warn",
        LevelFilter::Info => "info",
        LevelFilter::Debug => "debug",
        LevelFilter::Trace => "trace",
        LevelFilter::Off => "off",
    }
}

struct State {
    settings: Settings,
    file: Option<File>,
    file_error_reported: bool,
}

struct Sink {
    state: Mutex<State>,
}

static SINK: OnceLock<&'static Sink> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);

fn sink() -> &'static Sink {
    SINK.get_or_init(|| {
        Box::leak(Box::new(Sink {
            state: Mutex::new(State {
                settings: Settings::default(),
                file: None,
                file_error_reported: false,
            }),
        }))
    })
}

impl log::Log for Sink {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let Ok(state) = self.state.lock() else {
            return false;
        };
        state.settings.target != Target::Off && metadata.level() <= state.settings.level
    }

    fn log(&self, record: &log::Record) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.settings.target == Target::Off || record.level() > state.settings.level {
            return;
        }

        let line = format!(
            "{} {:<5} [{}] {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            record.level(),
            record.target(),
            record.args()
        );

        if state.settings.target.writes_console() {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }

        if state.settings.target.writes_file() {
            state.write_to_file(&line);
        }
    }

    fn flush(&self) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(f) = state.file.as_mut() {
                let _ = f.flush();
            }
        }
    }
}

impl State {
    fn write_to_file(&mut self, line: &str) {
        if self.file.is_none() {
            match open_log_file(&self.settings.file) {
                Ok(f) => {
                    self.file = Some(f);
                    self.file_error_reported = false;
                }
                Err(e) => {
                    if !self.file_error_reported {
                        self.file_error_reported = true;
                        let _ = writeln!(
                            std::io::stderr(),
                            "[logging] cannot open {}: {e}",
                            self.settings.file.display()
                        );
                    }
                    return;
                }
            }
        }

        if self.settings.max_bytes > 0 {
            let size = self.file.as_ref().and_then(|f| f.metadata().ok()).map_or(0, |m| m.len());
            if size + line.len() as u64 > self.settings.max_bytes {
                self.rotate();
            }
        }

        if let Some(f) = self.file.as_mut() {
            if f.write_all(line.as_bytes()).is_err() {
                self.file = None;
            }
        }
    }

    fn rotate(&mut self) {
        self.file = None;
        let path = &self.settings.file;
        let mut backup = path.clone().into_os_string();
        backup.push(".1");
        let _ = std::fs::rename(path, PathBuf::from(backup));
        self.file = open_log_file(path).ok();
    }
}

fn open_log_file(path: &Path) -> std::io::Result<File> {
    if path.as_os_str().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "empty log path",
        ));
    }
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    OpenOptions::new().create(true).append(true).open(path)
}

pub fn apply(settings: Settings) {
    let sink = sink();

    if !INSTALLED.swap(true, Ordering::SeqCst) && log::set_logger(sink).is_err() {
        let _ = writeln!(std::io::stderr(), "[logging] another logger is already installed");
    }

    if let Ok(mut state) = sink.state.lock() {
        let reopen = state.settings.file != settings.file || !settings.target.writes_file();
        if reopen {
            state.file = None;
            state.file_error_reported = false;
        }
        state.settings = settings;
    }

    let level = sink.state.lock().map(|s| s.settings.level).unwrap_or(LevelFilter::Off);
    let target_off = sink.state.lock().map(|s| s.settings.target == Target::Off).unwrap_or(true);
    log::set_max_level(if target_off { LevelFilter::Off } else { level });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ic-logging-test-{name}-{}", std::process::id()));
        p
    }

    #[test]
    fn off_by_default_writes_nothing() {
        let path = temp_path("off");
        let _ = std::fs::remove_file(&path);
        let mut state = State {
            settings: Settings { file: path.clone(), ..Settings::default() },
            file: None,
            file_error_reported: false,
        };
        assert_eq!(state.settings.target, Target::Off);
        assert!(!state.settings.target.writes_file());
        state.settings.target = Target::File;
        state.write_to_file("hello\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rotation_moves_the_old_file_aside() {
        let path = temp_path("rotate");
        let backup = PathBuf::from(format!("{}.1", path.display()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup);

        let mut state = State {
            settings: Settings {
                target: Target::File,
                level: LevelFilter::Info,
                file: path.clone(),
                max_bytes: 16,
            },
            file: None,
            file_error_reported: false,
        };
        state.write_to_file("0123456789\n");
        state.write_to_file("abcdefghij\n");

        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "0123456789\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "abcdefghij\n");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup);
    }

    #[test]
    fn an_unwritable_path_does_not_panic() {
        let mut state = State {
            settings: Settings {
                target: Target::File,
                level: LevelFilter::Info,
                file: PathBuf::from("/proc/definitely/not/writable.log"),
                max_bytes: 0,
            },
            file: None,
            file_error_reported: false,
        };
        state.write_to_file("nope\n");
        state.write_to_file("still nope\n");
        assert!(state.file_error_reported);
    }

    #[test]
    fn level_round_trips_through_the_config_string() {
        for level in [
            LevelFilter::Off,
            LevelFilter::Error,
            LevelFilter::Warn,
            LevelFilter::Info,
            LevelFilter::Debug,
            LevelFilter::Trace,
        ] {
            assert_eq!(level_from_str(level_as_str(level)), level);
        }
        assert_eq!(level_from_str("nonsense"), LevelFilter::Off);
    }

    #[test]
    fn target_round_trips_through_the_config_string() {
        for target in [Target::Off, Target::Console, Target::File, Target::Both] {
            assert_eq!(Target::from_str_or_off(target.as_str()), target);
        }
        assert_eq!(Target::from_str_or_off("nonsense"), Target::Off);
    }
}
