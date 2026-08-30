use std::path::PathBuf;
use std::sync::OnceLock;

static EXE_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init_exe_path() {
    if let Ok(exe_path) = std::env::current_exe() {
        let _ = EXE_PATH.set(exe_path);
    }
}

fn log_restart_event(msg: &str) {
    if let Some(mut path) = dirs::config_dir() {
        path.push("ice-commander");
        let _ = std::fs::create_dir_all(&path);
        path.push("restart.log");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write;
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(file, "[{}] {}", now, msg);
        }
    }
}

pub fn restart_app() {
    let log_msg = |msg: &str| {
        log_restart_event(msg);
    };

    log_msg("--- Restart requested ---");

    let exe_path_opt = EXE_PATH.get().cloned().or_else(|| {
        log_msg("OnceLock path was not initialized, retrieving from current_exe()");
        std::env::current_exe().ok()
    });

    match exe_path_opt {
        Some(exe_path) => {
            let original_exe_str = exe_path.to_string_lossy().into_owned();
            log_msg(&format!("Executable path to run: {}", original_exe_str));

            let mut exe_str = original_exe_str.clone();
            #[cfg(not(target_os = "windows"))]
            {
                if exe_str.ends_with(" (deleted)") {
                    exe_str.truncate(exe_str.len() - " (deleted)".len());
                    log_msg(&format!("Detected '(deleted)' suffix. Stripped path: {}", exe_str));
                }
            }

            let file_exists = std::path::Path::new(&exe_str).exists();
            log_msg(&format!("Target file exists: {}", file_exists));

            #[cfg(target_os = "windows")]
            {
                log_msg(&format!("Spawning Windows process: {}", exe_str));
                match std::process::Command::new(&exe_str).spawn() {
                    Ok(_) => log_msg("Spawned successfully"),
                    Err(e) => log_msg(&format!("Spawn failed: {:?}", e)),
                }
            }

            #[cfg(target_os = "macos")]
            {
                let mut launched_via_open = false;
                if let Some(pos) = exe_str.find(".app/Contents/MacOS/") {
                    let app_bundle_path = &exe_str[..pos + 4];
                    let cmd = format!("open -n {:?}", app_bundle_path);
                    log_msg(&format!("Attempting macOS open bundle: {}", cmd));
                    match std::process::Command::new("sh").arg("-c").arg(&cmd).spawn() {
                        Ok(_) => {
                            log_msg("open bundle spawned successfully");
                            launched_via_open = true;
                        }
                        Err(e) => log_msg(&format!("open bundle spawn failed: {:?}", e)),
                    }
                }
                if !launched_via_open {
                    let cmd = format!("nohup {} > /dev/null 2>&1 &", exe_str);
                    log_msg(&format!("Spawning macOS fallback shell command: {}", cmd));
                    match std::process::Command::new("sh").arg("-c").arg(&cmd).spawn() {
                        Ok(_) => log_msg("Fallback shell spawned successfully"),
                        Err(e) => log_msg(&format!("Fallback shell spawn failed: {:?}", e)),
                    }
                }
            }

            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            {
                let cmd = format!("nohup {} > /dev/null 2>&1 &", exe_str);
                log_msg(&format!("Spawning Linux shell command: {}", cmd));
                match std::process::Command::new("sh").arg("-c").arg(&cmd).spawn() {
                    Ok(_) => log_msg("Shell command spawned successfully"),
                    Err(e) => log_msg(&format!("Shell command spawn failed: {:?}", e)),
                }
            }
        }
        None => {
            log_msg("Failed to retrieve current executable path");
        }
    }

    log_msg("Exiting main process with exit code 0");
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_exe_path_stores_the_running_executable() {
        init_exe_path();
        let expected = std::env::current_exe().expect("current_exe must be available");
        assert_eq!(EXE_PATH.get(), Some(&expected));
    }

    #[test]
    fn init_exe_path_can_be_called_repeatedly_without_changing_the_stored_path() {
        init_exe_path();
        let first = EXE_PATH.get().cloned().expect("path must be set");
        init_exe_path();
        init_exe_path();
        assert_eq!(EXE_PATH.get(), Some(&first));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn restart_events_are_appended_to_the_log_with_a_timestamp() {
        let mut base = std::env::temp_dir();
        base.push(format!("ic-utils-restart-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let previous = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", &base);

        log_restart_event("first line");
        log_restart_event("second line");

        let log_path = base.join("ice-commander").join("restart.log");
        let contents = std::fs::read_to_string(&log_path).expect("restart.log must be created");

        match previous {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        let _ = std::fs::remove_dir_all(&base);

        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2, "each event appends exactly one line");
        for (line, msg) in lines.iter().zip(["first line", "second line"]) {
            let (stamp, rest) = line.split_once("] ").expect("line must start with [timestamp]");
            assert_eq!(rest, msg);
            assert!(stamp.starts_with('['));
            assert_eq!(stamp.len(), 20, "unexpected timestamp width in {:?}", line);
            let digits = stamp[1..].chars().filter(|c| c.is_ascii_digit()).count();
            assert_eq!(digits, 14, "unexpected timestamp shape in {:?}", line);
        }
    }
}
