use std::env::consts::OS;
use std::path::Path;
use std::process::Command;

pub fn install_update(file_path: &Path, is_gui: bool) -> Result<(), String> {
    let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let path_str = file_path.to_string_lossy();

    match OS {
        "windows" => {
            Command::new(file_path)
                .spawn()
                .map_err(|e| format!("Failed to start Windows installer: {}", e))?;

            std::process::exit(0);
        }
        "macos" => {
            let mut child = Command::new("open")
                .arg(&*path_str)
                .spawn()
                .map_err(|e| format!("Failed to open DMG: {}", e))?;

            if is_gui {
                std::thread::spawn(move || {
                    let _ = child.wait();
                    std::thread::sleep(std::time::Duration::from_millis(3000));
                    std::process::exit(0);
                });
                Ok(())
            } else {
                let _ = child.wait();
                std::process::exit(0);
            }
        }
        "linux" => {
            let privilege_cmd = if is_gui { "pkexec" } else { "sudo" };
            let mut args = Vec::new();

            match ext {
                "deb" => {
                    args.extend(vec!["apt-get", "install", "-y", &*path_str]);
                }
                "rpm" => {
                    args.extend(vec!["dnf", "install", "-y", &*path_str]);
                }
                "zst" => {
                    args.extend(vec!["pacman", "-U", "--noconfirm", &*path_str]);
                }
                _ => return Err(format!("Unsupported package extension for Linux: {}", ext)),
            }

            if is_gui {
                let mut child = Command::new(privilege_cmd)
                    .args(&args)
                    .spawn()
                    .map_err(|e| format!("Failed to spawn {}: {}", privilege_cmd, e))?;

                std::thread::spawn(move || {
                    if let Ok(status) = child.wait() {
                        if status.success() {
                            ic_utils::app::restart_app();
                        } else {
                            println!(
                                "Update installer failed or was cancelled (exit code: {:?})",
                                status.code()
                            );
                        }
                    }
                });

                Ok(())
            } else {
                let mut child = Command::new(privilege_cmd)
                    .args(&args)
                    .spawn()
                    .map_err(|e| format!("Failed to spawn {}: {}", privilege_cmd, e))?;

                println!("Installing update. Please enter your sudo password if prompted.");
                let status = child.wait().map_err(|e| e.to_string())?;
                if status.success() {
                    ic_utils::app::restart_app();
                    Ok(())
                } else {
                    Err(format!("Installer failed with code: {:?}", status.code()))
                }
            }
        }
        _ => Err(format!("Unsupported OS: {}", OS)),
    }
}

pub fn spawn_linux_gui_update(file_path: &Path) -> Result<std::process::Child, String> {
    #[cfg(target_os = "linux")]
    {
        let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let path_str = file_path.to_string_lossy();
        let mut args = Vec::new();

        match ext {
            "deb" => {
                args.extend(vec!["apt-get", "install", "-y", &*path_str]);
            }
            "rpm" => {
                args.extend(vec!["dnf", "install", "-y", &*path_str]);
            }
            "zst" => {
                args.extend(vec!["pacman", "-U", "--noconfirm", &*path_str]);
            }
            _ => return Err(format!("Unsupported package extension for Linux: {}", ext)),
        }

        Command::new("pkexec")
            .args(&args)
            .spawn()
            .map_err(|e| format!("Failed to spawn pkexec: {}", e))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = file_path;
        Err("spawn_linux_gui_update is only supported on Linux".to_string())
    }
}


