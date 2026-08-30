use std::sync::{Arc, Mutex};
use suppaftp::FtpStream;
use tokio::sync::mpsc as tokio_mpsc;

pub fn resolve_ftp_path(base_path: Option<&str>, user_path: &str) -> String {
    let base = base_path.unwrap_or("");
    let base_trimmed = base.trim_end_matches('/');

    let rel_path = user_path.trim_start_matches('/');
    for component in std::path::Path::new(rel_path).components() {
        if matches!(component, std::path::Component::ParentDir) {
            return if base_trimmed.is_empty() {
                "/".to_string()
            } else {
                base_trimmed.to_string()
            };
        }
    }

    if base_trimmed.is_empty() {
        format!("/{}", rel_path)
    } else {
        if rel_path.is_empty() {
            base_trimmed.to_string()
        } else {
            format!("{}/{}", base_trimmed, rel_path)
        }
    }
}

fn connect_ftp(host_port: &str, user: &str, pass: &str) -> Result<suppaftp::FtpStream, String> {
    let mut ftp_stream = match common::connect_timeout() {
        Some(d) => {
            use std::net::ToSocketAddrs;
            let addr = host_port
                .to_socket_addrs()
                .map_err(|e| e.to_string())?
                .next()
                .ok_or_else(|| "DNS: no address for host".to_string())?;
            FtpStream::connect_timeout(addr, d).map_err(|e| e.to_string())?
        }
        None => FtpStream::connect(host_port).map_err(|e| e.to_string())?,
    };
    if let Some(d) = common::request_timeout() {
        let _ = ftp_stream.get_ref().set_read_timeout(Some(d));
        let _ = ftp_stream.get_ref().set_write_timeout(Some(d));
    }
    ftp_stream.login(user, pass).map_err(|e| e.to_string())?;
    Ok(ftp_stream)
}

pub fn start_ftp_download(
    host: String,
    port: u16,
    user: String,
    pass: String,
    req_path: String,
    tx: tokio_mpsc::Sender<Result<Vec<u8>, String>>,
) -> Result<u64, String> {
    let host_port = format!("{}:{}", host, port);

    let mut ftp_stream = connect_ftp(&host_port, &user, &pass)?;

    let total_size = ftp_stream.size(&req_path).unwrap_or(0) as u64;

    tokio::task::spawn_blocking(move || {
        if let Ok(mut reader) = ftp_stream.retr_as_stream(&req_path) {
            let mut buffer = vec![0u8; 16 * 1024];
            use std::io::Read;
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        let _ = tx.blocking_send(Ok(Vec::new()));
                        break;
                    }
                    Ok(n) => {
                        if tx.blocking_send(Ok(buffer[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.blocking_send(Err(e.to_string()));
                        break;
                    }
                }
            }
            let _ = ftp_stream.finalize_retr_stream(reader);
        } else {
            let _ = tx.blocking_send(Err("Failed to start ftp RETR stream".into()));
        }
        let _ = ftp_stream.quit();
    });

    Ok(total_size)
}

fn delete_ftp_recursive(ftp_stream: &mut suppaftp::FtpStream, path: &str) -> Result<(), String> {
    if ftp_stream.rm(path).is_ok() {
        return Ok(());
    }

    let prev_dir = ftp_stream.pwd().map_err(|e| e.to_string())?;
    if ftp_stream.cwd(path).is_ok() {
        let mut sub_dirs = Vec::new();
        let mut sub_files = Vec::new();
        if let Ok(list) = ftp_stream.list(None) {
            for line in list {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 9 && (parts[0].starts_with('d') || parts[0].starts_with('-')) {
                    let is_dir = parts[0].starts_with('d');
                    let name = parts[8..].join(" ");
                    if name == "." || name == ".." {
                        continue;
                    }
                    if is_dir {
                        sub_dirs.push(name);
                    } else {
                        sub_files.push(name);
                    }
                } else if parts.len() >= 4 && parts[2] == "<DIR>" {
                    let name = parts[3..].join(" ");
                    if name == "." || name == ".." {
                        continue;
                    }
                    sub_dirs.push(name);
                } else if parts.len() >= 4 {
                    let size_val = parts[2];
                    let name = parts[3..].join(" ");
                    if name == "." || name == ".." {
                        continue;
                    }
                    if size_val == "<DIR>" {
                        sub_dirs.push(name);
                    } else {
                        sub_files.push(name);
                    }
                }
            }
        }

        for f in sub_files {
            ftp_stream.rm(&f).map_err(|e| e.to_string())?;
        }

        for d in sub_dirs {
            delete_ftp_recursive(ftp_stream, &d)?;
        }

        let _ = ftp_stream.cwd(&prev_dir);
        ftp_stream.rmdir(path).map_err(|e| e.to_string())?;
    } else {
        ftp_stream.rmdir(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn upload_ftp_file(
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    remote_path: &str,
    local_file_path: &std::path::Path,
) -> Result<(), String> {
    use std::fs::File;
    let host_port = format!("{}:{}", host, port);
    let mut ftp_stream = connect_ftp(&host_port, user, pass)?;

    let mut file = File::open(local_file_path).map_err(|e| e.to_string())?;
    ftp_stream
        .put_file(remote_path, &mut file)
        .map_err(|e| e.to_string())?;

    let _ = ftp_stream.quit();
    Ok(())
}

pub struct FtpSession {
    pub ftp_stream: suppaftp::FtpStream,
}

impl FtpSession {
    pub fn connect(host: &str, port: u16, user: &str, pass: &str) -> Result<Self, String> {
        let host_port = format!("{}:{}", host, port);
        let ftp_stream = connect_ftp(&host_port, user, pass)?;
        Ok(Self { ftp_stream })
    }

    pub fn is_alive(&mut self) -> bool {
        self.ftp_stream.pwd().is_ok()
    }

    pub fn list_dir(
        &mut self,
        req_path: &str,
    ) -> Result<(Vec<(String, String)>, Vec<(String, u64, String)>), String> {
        let mut directories = Vec::new();
        let mut files = Vec::new();
        let dir_to_list = if req_path == "/" || req_path.is_empty() {
            "/"
        } else {
            req_path
        };
        self.ftp_stream
            .cwd(dir_to_list)
            .map_err(|e| e.to_string())?;
        let list = self.ftp_stream.list(None).map_err(|e| e.to_string())?;
        for line in list {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 9 && (parts[0].starts_with('d') || parts[0].starts_with('-')) {
                let is_dir = parts[0].starts_with('d');
                let size: u64 = parts[4].parse().unwrap_or(0);
                let name = parts[8..].join(" ");
                let date_str = parts[5..8].join(" ");
                if name == "." || name == ".." {
                    continue;
                }
                if is_dir {
                    directories.push((name, date_str));
                } else {
                    files.push((name, size, date_str));
                }
            } else if parts.len() >= 4 && parts[2] == "<DIR>" {
                let name = parts[3..].join(" ");
                let date_str = format!("{} {}", parts[0], parts[1]);
                if name == "." || name == ".." {
                    continue;
                }
                directories.push((name, date_str));
            } else if parts.len() >= 4 {
                let size: u64 = parts[2].parse().unwrap_or(0);
                let name = parts[3..].join(" ");
                let date_str = format!("{} {}", parts[0], parts[1]);
                files.push((name, size, date_str));
            }
        }
        Ok((directories, files))
    }

    pub fn create_dir(&mut self, path: &str) -> Result<(), String> {
        self.ftp_stream.mkdir(path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_entry(&mut self, path: &str) -> Result<(), String> {
        delete_ftp_recursive(&mut self.ftp_stream, path)
    }

    pub fn rename(&mut self, path: &str, new_path: &str) -> Result<(), String> {
        self.ftp_stream
            .rename(path, new_path)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), String> {
        let mut cursor = std::io::Cursor::new(content);
        self.ftp_stream
            .put_file(path, &mut cursor)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn with_ftp_session<F, T>(
    ftp_session: &Arc<Mutex<Option<FtpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    mut op: F,
) -> Result<T, String>
where
    F: FnMut(&mut FtpSession) -> Result<T, String>,
{
    let mut attempts = 0;
    loop {
        let mut lock = ftp_session.lock().unwrap_or_else(|e| e.into_inner());
        let needs_connect = match &mut *lock {
            Some(sess) => !sess.is_alive(),
            None => true,
        };

        if needs_connect {
            *lock = None;
            match FtpSession::connect(host, port, user, pass) {
                Ok(sess) => *lock = Some(sess),
                Err(e) => return Err(e),
            }
        }

        let session_ref = lock.as_mut().expect("session was just initialized");
        match op(session_ref) {
            Ok(val) => return Ok(val),
            Err(e) => {
                attempts += 1;
                if attempts >= 2 {
                    return Err(e);
                }
                *lock = None;
            }
        }
    }
}

pub fn list_ftp_directory_cached(
    ftp_session: Arc<Mutex<Option<FtpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    req_path: &str,
) -> Result<(Vec<(String, String)>, Vec<(String, u64, String)>), String> {
    with_ftp_session(&ftp_session, host, port, user, pass, |sess| {
        sess.list_dir(req_path)
    })
}

pub fn create_ftp_directory_cached(
    ftp_session: Arc<Mutex<Option<FtpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    path: &str,
) -> Result<(), String> {
    with_ftp_session(&ftp_session, host, port, user, pass, |sess| {
        sess.create_dir(path)
    })
}

pub fn delete_ftp_entry_cached(
    ftp_session: Arc<Mutex<Option<FtpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    path: &str,
) -> Result<(), String> {
    with_ftp_session(&ftp_session, host, port, user, pass, |sess| {
        sess.delete_entry(path)
    })
}

pub fn delete_ftp_entries_cached(
    ftp_session: Arc<Mutex<Option<FtpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    paths: &[String],
) -> Result<(), String> {
    with_ftp_session(&ftp_session, host, port, user, pass, |sess| {
        for path in paths {
            sess.delete_entry(path)?;
        }
        Ok(())
    })
}

pub fn rename_ftp_entry_cached(
    ftp_session: Arc<Mutex<Option<FtpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    path: &str,
    new_path: &str,
) -> Result<(), String> {
    with_ftp_session(&ftp_session, host, port, user, pass, |sess| {
        sess.rename(path, new_path)
    })
}

pub fn write_ftp_file_cached(
    ftp_session: Arc<Mutex<Option<FtpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    path: &str,
    content: &[u8],
) -> Result<(), String> {
    with_ftp_session(&ftp_session, host, port, user, pass, |sess| {
        sess.write_file(path, content)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_base_simple_path() {
        assert_eq!(resolve_ftp_path(None, "docs"), "/docs");
    }

    #[test]
    fn no_base_leading_slash_stripped() {
        assert_eq!(resolve_ftp_path(None, "/docs"), "/docs");
    }

    #[test]
    fn base_and_subpath() {
        assert_eq!(resolve_ftp_path(Some("/home/user"), "docs"), "/home/user/docs");
    }

    #[test]
    fn base_trailing_slash_normalised() {
        assert_eq!(resolve_ftp_path(Some("/home/user/"), "docs"), "/home/user/docs");
    }

    #[test]
    fn empty_user_path_returns_base() {
        assert_eq!(resolve_ftp_path(Some("/home/user"), ""), "/home/user");
    }

    #[test]
    fn no_base_empty_path_returns_root() {
        assert_eq!(resolve_ftp_path(None, ""), "/");
    }

    #[test]
    fn nested_subpath() {
        assert_eq!(resolve_ftp_path(Some("/base"), "a/b/c"), "/base/a/b/c");
    }

    #[test]
    fn traversal_with_no_base_returns_root() {
        assert_eq!(resolve_ftp_path(None, "../etc/passwd"), "/");
    }

    #[test]
    fn traversal_returns_base_unchanged() {
        assert_eq!(resolve_ftp_path(Some("/home/user"), "../etc/passwd"), "/home/user");
    }

    #[test]
    fn nested_traversal_still_blocked() {
        assert_eq!(resolve_ftp_path(Some("/home"), "sub/../etc"), "/home");
    }

    #[test]
    fn no_base_none_same_as_empty_string() {
        assert_eq!(resolve_ftp_path(None, "file.txt"), resolve_ftp_path(Some(""), "file.txt"));
    }

    #[test]
    fn root_base_behaves_like_no_base() {
        assert_eq!(resolve_ftp_path(Some("/"), "docs"), "/docs");
        assert_eq!(resolve_ftp_path(Some("///"), "docs"), "/docs");
    }

    #[test]
    fn repeated_trailing_slashes_on_base_collapse() {
        assert_eq!(resolve_ftp_path(Some("/home/user///"), "docs"), "/home/user/docs");
    }

    #[test]
    fn repeated_leading_slashes_on_user_path_collapse() {
        assert_eq!(resolve_ftp_path(Some("/home"), "//docs"), "/home/docs");
        assert_eq!(resolve_ftp_path(None, "///docs"), "/docs");
    }

    #[test]
    fn absolute_user_path_is_reanchored_under_the_base() {
        assert_eq!(resolve_ftp_path(Some("/home/user"), "/etc/passwd"), "/home/user/etc/passwd");
    }

    #[test]
    fn base_without_leading_slash_is_kept_as_is() {
        assert_eq!(resolve_ftp_path(Some("home/user"), "docs"), "home/user/docs");
    }

    #[test]
    fn trailing_slash_on_user_path_is_preserved() {
        assert_eq!(resolve_ftp_path(Some("/base"), "docs/"), "/base/docs/");
    }

    #[test]
    fn traversal_at_the_end_of_the_path_is_blocked() {
        assert_eq!(resolve_ftp_path(Some("/home"), "docs/.."), "/home");
    }

    #[test]
    fn deep_traversal_is_blocked() {
        assert_eq!(resolve_ftp_path(Some("/home"), "a/b/c/../../../../etc"), "/home");
    }

    #[test]
    fn triple_dot_is_an_ordinary_name() {
        assert_eq!(resolve_ftp_path(Some("/home"), "..."), "/home/...");
    }

    #[test]
    fn a_name_merely_starting_with_two_dots_is_not_traversal() {
        assert_eq!(resolve_ftp_path(Some("/home"), "..hidden"), "/home/..hidden");
    }

    #[test]
    fn current_dir_components_are_passed_through_verbatim() {
        assert_eq!(resolve_ftp_path(Some("/home"), "./a"), "/home/./a");
        assert_eq!(resolve_ftp_path(Some("/home"), "a/./b"), "/home/a/./b");
    }

    #[test]
    fn names_with_spaces_survive_intact() {
        assert_eq!(resolve_ftp_path(Some("/my base"), "my docs/a b.txt"), "/my base/my docs/a b.txt");
    }

    #[test]
    fn non_ascii_names_survive_intact() {
        assert_eq!(resolve_ftp_path(Some("/\u{431}\u{430}\u{437}\u{430}"), "\u{434}\u{43e}\u{43a}/\u{1f4c1}.txt"), "/\u{431}\u{430}\u{437}\u{430}/\u{434}\u{43e}\u{43a}/\u{1f4c1}.txt");
    }

    #[test]
    fn result_always_starts_at_the_base_when_one_is_set() {
        for user_path in ["", "a", "/a", "../a", "a/../..", "a/b"] {
            assert!(
                resolve_ftp_path(Some("/home/user"), user_path).starts_with("/home/user"),
                "escaped for {:?}",
                user_path
            );
        }
    }
}
