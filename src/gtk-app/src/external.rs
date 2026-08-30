
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
}

pub fn parse_command(template: &str, path: &Path) -> Option<Command> {
    let placeholder = path.to_string_lossy().to_string();
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;

    for ch in template.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        tokens.push(current);
    }

    let mut used_placeholder = false;
    let mut tokens: Vec<String> = tokens
        .into_iter()
        .map(|t| {
            if t.contains("%f") {
                used_placeholder = true;
                t.replace("%f", &placeholder)
            } else {
                t
            }
        })
        .collect();

    if tokens.is_empty() {
        return None;
    }
    if !used_placeholder {
        tokens.push(placeholder);
    }

    let program = tokens.remove(0);
    if program.is_empty() {
        return None;
    }
    Some(Command { program, args: tokens })
}

pub fn associations(config: &client_config::AppConfig) -> HashMap<String, String> {
    config
        .get::<HashMap<String, String>>("ui.custom_associations")
        .unwrap_or_default()
}

pub fn association_for(config: &client_config::AppConfig, file_name: &str) -> Option<String> {
    let ext = file_name.rsplit_once('.')?.1.to_lowercase();
    associations(config)
        .get(&ext)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fallback {
    #[default]
    Viewer,
    System,
}

pub fn fallback(config: &client_config::AppConfig) -> Fallback {
    match config.get::<String>("ui.double_click_action").as_deref() {
        Some("system") => Fallback::System,
        _ => Fallback::Viewer,
    }
}

pub struct Staged {
    pub path: PathBuf,
    temporary: bool,
}

impl Staged {
    pub fn is_temporary(&self) -> bool {
        self.temporary
    }

    pub fn read(&self) -> std::io::Result<Vec<u8>> {
        std::fs::read(&self.path)
    }

    fn discard(self) {
        if self.temporary {
            let _ = std::fs::remove_file(&self.path);
        }
        std::mem::forget(self);
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub async fn stage(
    provider: &Rc<dyn fm_core::FileSystemRpc>,
    display_path: &str,
) -> Result<Staged, String> {
    if provider.is_local() {
        return Ok(Staged { path: PathBuf::from(display_path), temporary: false });
    }

    let bytes = provider
        .read_file_opt(display_path.to_string(), None, false)
        .await
        .map_err(|e| e.to_string())?;

    let path = temp_path(display_path);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(Staged { path, temporary: true })
}

fn temp_path(display_path: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let name = display_path
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("file");
    std::env::temp_dir().join(format!(
        "ice-open-{}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed),
        name
    ))
}

async fn run(command: &Command) -> Result<(), String> {
    let mut argv: Vec<&std::ffi::OsStr> = Vec::with_capacity(command.args.len() + 1);
    argv.push(command.program.as_ref());
    for a in &command.args {
        argv.push(a.as_ref());
    }

    let proc = gio::Subprocess::newv(&argv, gio::SubprocessFlags::NONE)
        .map_err(|e| e.message().to_string())?;
    proc.wait_check_future().await.map_err(|e| e.message().to_string())
}

async fn run_default_handler(path: &Path) -> Result<(), String> {
    let file = gio::File::for_path(path);
    gio::AppInfo::launch_default_for_uri_future(&file.uri(), gio::AppLaunchContext::NONE)
        .await
        .map_err(|e| e.message().to_string())
}

pub async fn open_detached(
    provider: Rc<dyn fm_core::FileSystemRpc>,
    display_path: String,
    template: Option<String>,
) -> Result<(), String> {
    let staged = stage(&provider, &display_path).await?;

    let result = match &template {
        Some(t) => match parse_command(t, &staged.path) {
            Some(cmd) => run(&cmd).await,
            None => Err(format!("empty command for {display_path}")),
        },
        None => {
            let r = run_default_handler(&staged.path).await;
            if r.is_ok() && staged.is_temporary() {
                std::mem::forget(staged);
                return Ok(());
            }
            r
        }
    };

    staged.discard();
    result
}

pub async fn edit_and_write_back(
    provider: Rc<dyn fm_core::FileSystemRpc>,
    display_path: String,
    template: String,
) -> Result<bool, String> {
    let staged = stage(&provider, &display_path).await?;
    let Some(cmd) = parse_command(&template, &staged.path) else {
        staged.discard();
        return Err("the external editor command is empty".to_string());
    };

    let before = staged.read().ok();
    let result = run(&cmd).await;
    if let Err(e) = result {
        staged.discard();
        return Err(e);
    }

    if !staged.is_temporary() {
        return Ok(false);
    }

    let after = staged.read().map_err(|e| e.to_string())?;
    if before.as_deref() == Some(after.as_slice()) {
        staged.discard();
        return Ok(false);
    }

    let write = provider
        .write_file(display_path, after, None, None)
        .await
        .map_err(|e| e.to_string());
    staged.discard();
    write.map(|_| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(template: &str) -> Option<Command> {
        parse_command(template, Path::new("/tmp/a b.txt"))
    }

    #[test]
    fn a_bare_program_gets_the_path_appended() {
        assert_eq!(
            cmd("gedit"),
            Some(Command { program: "gedit".into(), args: vec!["/tmp/a b.txt".into()] })
        );
    }

    #[test]
    fn the_placeholder_decides_where_the_path_goes() {
        assert_eq!(
            cmd("code --wait %f"),
            Some(Command {
                program: "code".into(),
                args: vec!["--wait".into(), "/tmp/a b.txt".into()],
            })
        );
    }

    #[test]
    fn a_placeholder_inside_a_token_is_substituted() {
        assert_eq!(
            cmd("mpv --input=%f"),
            Some(Command {
                program: "mpv".into(),
                args: vec!["--input=/tmp/a b.txt".into()],
            })
        );
    }

    #[test]
    fn quotes_keep_a_path_with_spaces_together() {
        assert_eq!(
            cmd("\"/opt/my app/bin\" -n"),
            Some(Command {
                program: "/opt/my app/bin".into(),
                args: vec!["-n".into(), "/tmp/a b.txt".into()],
            })
        );
    }

    #[test]
    fn an_empty_template_is_not_a_command() {
        assert_eq!(cmd("   "), None);
        assert_eq!(cmd(""), None);
    }

    #[test]
    fn the_extension_lookup_is_case_insensitive_and_ignores_blanks() {
        let config = client_config::AppConfig::new("ice-commander-external-test");
        let mut map = HashMap::new();
        map.insert("png".to_string(), "gimp".to_string());
        map.insert("bmp".to_string(), "   ".to_string());
        config.set("ui.custom_associations", map);

        assert_eq!(association_for(&config, "photo.PNG").as_deref(), Some("gimp"));
        assert_eq!(association_for(&config, "photo.png").as_deref(), Some("gimp"));
        assert_eq!(association_for(&config, "photo.bmp"), None);
        assert_eq!(association_for(&config, "photo.jpg"), None);
        assert_eq!(association_for(&config, "noextension"), None);
    }
}
