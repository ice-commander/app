
pub fn detect_linux_distro() -> Result<String, String> {
    let os_release_path = std::path::Path::new("/etc/os-release");
    if !os_release_path.exists() {
        return Err("Cannot find /etc/os-release file to detect distribution.".to_string());
    }

    let content = std::fs::read_to_string(os_release_path)
        .map_err(|e| format!("Failed to read /etc/os-release: {}", e))?;

    let mut id = None;
    let mut id_like = None;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("ID=") {
            id = Some(
                line.trim_start_matches("ID=")
                    .trim_matches('"')
                    .to_lowercase(),
            );
        } else if line.starts_with("ID_LIKE=") {
            id_like = Some(
                line.trim_start_matches("ID_LIKE=")
                    .trim_matches('"')
                    .to_lowercase(),
            );
        }
    }

    if let Some(dist) = id {
        if dist.contains("ubuntu")
            || dist.contains("debian")
            || dist.contains("pop")
            || dist.contains("mint")
        {
            return Ok("ubuntu".to_string());
        }
        if dist.contains("fedora") || dist.contains("rhel") || dist.contains("centos") {
            return Ok("fedora".to_string());
        }
        if dist.contains("arch") || dist.contains("manjaro") {
            return Ok("arch".to_string());
        }
    }

    if let Some(like) = id_like {
        if like.contains("ubuntu") || like.contains("debian") {
            return Ok("ubuntu".to_string());
        }
        if like.contains("fedora") || like.contains("rhel") {
            return Ok("fedora".to_string());
        }
        if like.contains("arch") {
            return Ok("arch".to_string());
        }
    }

    Err("Unknown Linux distribution. Only Debian/Ubuntu, Fedora, and Arch Linux are supported for automatic installation.".to_string())
}

pub async fn run_package_installer(
    arch_pkgs: &[&str],
    fedora_pkgs: &[&str],
    ubuntu_pkgs: &[&str],
) -> Result<(), String> {
    let distro = detect_linux_distro()?;

    let (cmd, args): (&str, Vec<String>) = match distro.as_str() {
        "ubuntu" | "debian" => {
            let mut sh_cmd = "apt-get update && apt-get install -y".to_string();
            for pkg in ubuntu_pkgs {
                sh_cmd = format!("{} {}", sh_cmd, pkg);
            }
            ("pkexec", vec!["sh".to_string(), "-c".to_string(), sh_cmd])
        }
        "fedora" => {
            let mut arg_list = vec!["dnf".to_string(), "install".to_string(), "-y".to_string()];
            for pkg in fedora_pkgs {
                arg_list.push(pkg.to_string());
            }
            ("pkexec", arg_list)
        }
        "arch" => {
            let mut arg_list = vec![
                "pacman".to_string(),
                "-S".to_string(),
                "--noconfirm".to_string(),
            ];
            for pkg in arch_pkgs {
                arg_list.push(pkg.to_string());
            }
            ("pkexec", arg_list)
        }
        _ => {
            return Err(format!(
                "Unsupported distribution: {}. Please install required packages manually.",
                distro
            ));
        }
    };

    let mut child = tokio::process::Command::new(cmd)
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to execute pkexec: {}", e))?;

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Command waiting failed: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        use tokio::io::AsyncReadExt;
        let mut err_output = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut err_output).await;
        }
        if err_output.trim().is_empty() {
            Err(
                "Authentication failed, cancelled, or installer returned non-zero exit code."
                    .to_string(),
            )
        } else {
            Err(err_output)
        }
    }
}

pub fn is_video_decoding_available() -> bool {
    #[cfg(all(target_os = "linux", feature = "video-codecs"))]
    {
        if gstreamer::init().is_err() {
            return false;
        }
        gstreamer::ElementFactory::find("avdec_h264").is_some()
            || gstreamer::ElementFactory::find("openh264dec").is_some()
    }
    #[cfg(not(all(target_os = "linux", feature = "video-codecs")))]
    {
        false
    }
}

pub async fn run_video_codecs_installer() -> Result<(), String> {
    run_package_installer(
        &[
            "gst-plugins-good",
            "gst-plugins-bad",
            "gst-plugins-ugly",
            "gst-libav",
        ],
        &[
            "gstreamer1-plugins-good",
            "gstreamer1-plugins-bad-free",
            "gstreamer1-plugins-ugly-free",
            "gstreamer1-libav",
        ],
        &[
            "gstreamer1.0-plugins-good",
            "gstreamer1.0-plugins-bad",
            "gstreamer1.0-plugins-ugly",
            "gstreamer1.0-libav",
        ],
    )
    .await
}

pub fn update_gstreamer_registry() {
    #[cfg(all(target_os = "linux", feature = "video-codecs"))]
    unsafe {
        gstreamer::ffi::gst_update_registry();
    }
}