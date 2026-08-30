pub fn restart_app() {
    ic_utils::app::restart_app();
}


pub fn read_blocking(path: &str) -> bool {
    const MAX: u64 = 8 * 1024 * 1024;
    std::fs::metadata(path).map(|m| m.len() <= MAX).unwrap_or(false)
}

pub fn open_with_system(path: &std::path::Path) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();

    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();

    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", ""])
        .arg(path)
        .spawn();
}
