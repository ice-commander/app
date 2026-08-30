#[cfg(not(target_os = "android"))]
use ssh2::Session;
#[cfg(not(target_os = "android"))]
use std::io::{Read, Write};
#[cfg(not(target_os = "android"))]
use std::net::TcpStream;
use tokio::sync::mpsc as tokio_mpsc;

fn expand_tilde(path: &str) -> std::path::PathBuf {
    if path.starts_with("~/") {
        if let Some(mut home) = dirs::home_dir() {
            home.push(&path[2..]);
            return home;
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    std::path::PathBuf::from(path)
}

#[cfg(not(target_os = "android"))]
fn connect_sftp(
    host: &str,
    port: u16,
    user: &str,
    pass: Option<&str>,
    key_path: Option<&str>,
    passphrase: Option<&str>,
) -> Result<(Session, ssh2::Sftp), String> {
    let tcp = match common::connect_timeout() {
        Some(d) => {
            use std::net::ToSocketAddrs;
            let addr = format!("{}:{}", host, port)
                .to_socket_addrs()
                .map_err(|e| format!("DNS: {}", e))?
                .next()
                .ok_or_else(|| "DNS: no address for host".to_string())?;
            TcpStream::connect_timeout(&addr, d)
        }
        None => TcpStream::connect(format!("{}:{}", host, port)),
    }
    .map_err(|e| format!("TCP Connect: {}", e))?;
    let mut sess = Session::new().map_err(|e| format!("Session Create: {}", e))?;
    if let Some(d) = common::request_timeout() {
        sess.set_timeout(d.as_millis().min(u32::MAX as u128) as u32);
    }
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| format!("SSH Handshake: {}", e))?;

    if let Some(kp) = key_path {
        let expanded = expand_tilde(kp);
        sess.userauth_pubkey_file(user, None, &expanded, passphrase)
            .map_err(|e| format!("SSH Key Auth failed: {}", e))?;
    } else if let Some(p) = pass {
        sess.userauth_password(user, p)
            .map_err(|e| format!("SSH Password Auth failed: {}", e))?;
    } else {
        return Err("No SSH authentication method specified".to_string());
    }

    let sftp = sess.sftp().map_err(|e| format!("SFTP Init: {}", e))?;
    Ok((sess, sftp))
}

pub fn start_sftp_download_ext(
    host: String,
    port: u16,
    user: String,
    pass: Option<String>,
    key_path: Option<String>,
    passphrase: Option<String>,
    req_path: String,
    tx: tokio_mpsc::Sender<Result<Vec<u8>, String>>,
) -> Result<u64, String> {
    #[cfg(target_os = "android")]
    {
        return Err("SFTP is not supported on Android".to_string());
    }

    #[cfg(not(target_os = "android"))]
    {
        let (_sess, sftp) = connect_sftp(
            &host,
            port,
            &user,
            pass.as_deref(),
            key_path.as_deref(),
            passphrase.as_deref(),
        )?;
        let path = std::path::Path::new(&req_path);
        let stat = sftp.stat(path).map_err(|e| format!("SFTP Stat: {}", e))?;
        let total_size = stat.size.unwrap_or(0);

        let target_path_buf = path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            match sftp.open(&target_path_buf) {
                Ok(mut file) => {
                    let mut buffer = vec![0u8; 16 * 1024];
                    loop {
                        match file.read(&mut buffer) {
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
                }
                Err(e) => {
                    let _ = tx.blocking_send(Err(format!("SFTP Open Error: {}", e)));
                }
            }
        });

        Ok(total_size)
    }
}

#[cfg(not(target_os = "android"))]
fn delete_sftp_recursive(sftp: &ssh2::Sftp, path: &std::path::Path) -> Result<(), String> {
    if let Ok(stat) = sftp.stat(path) {
        if stat.is_dir() {
            if let Ok(entries) = sftp.readdir(path) {
                for (sub_path, sub_stat) in entries {
                    let name = sub_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    if name == "." || name == ".." {
                        continue;
                    }
                    if sub_stat.is_dir() {
                        let _ = delete_sftp_recursive(sftp, &sub_path);
                    } else {
                        let _ = sftp.unlink(&sub_path);
                    }
                }
            }
            sftp.rmdir(path).map_err(|e| e.to_string())?;
        } else {
            sftp.unlink(path).map_err(|e| e.to_string())?;
        }
    } else {
        if sftp.unlink(path).is_err() {
            sftp.rmdir(path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn upload_sftp_file(
    host: &str,
    port: u16,
    user: &str,
    pass: Option<&str>,
    key_path: Option<&str>,
    passphrase: Option<&str>,
    remote_path: &str,
    local_file_path: &std::path::Path,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return Err("SFTP is not supported on Android".to_string());
    }

    #[cfg(not(target_os = "android"))]
    {
        let (_sess, sftp) = connect_sftp(host, port, user, pass, key_path, passphrase)?;
        let mut remote_file = sftp
            .create(std::path::Path::new(remote_path))
            .map_err(|e| e.to_string())?;
        let mut local_file = std::fs::File::open(local_file_path).map_err(|e| e.to_string())?;

        let mut buffer = vec![0u8; 16 * 1024];
        loop {
            let n = local_file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            remote_file
                .write_all(&buffer[..n])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(not(target_os = "android"))]
pub struct SftpSession {
    pub session: Session,
    pub sftp: ssh2::Sftp,
}

#[cfg(not(target_os = "android"))]
impl SftpSession {
    pub fn connect(
        host: &str,
        port: u16,
        user: &str,
        pass: Option<&str>,
        key_path: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<Self, String> {
        let (session, sftp) = connect_sftp(host, port, user, pass, key_path, passphrase)?;
        Ok(Self { session, sftp })
    }

    pub fn is_alive(&self) -> bool {
        self.sftp.stat(std::path::Path::new(".")).is_ok()
    }

    pub fn list_dir(&self, path: &str) -> Result<(Vec<(String, String, Option<u32>)>, Vec<(String, u64, String, Option<u32>)>), String> {
        let target = if path.is_empty() { "." } else { path };
        let path_obj = std::path::Path::new(target);
        let mut directories = Vec::new();
        let mut files = Vec::new();

        let entries = self
            .sftp
            .readdir(path_obj)
            .map_err(|e| format!("SFTP ReadDir: {}", e))?;
        for (entry_path, stat) in entries {
            let name = entry_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if name == "." || name == ".." {
                continue;
            }
            let date_str = stat.mtime
                .and_then(|sec| chrono::DateTime::from_timestamp(sec as i64, 0))
                .map(|dt| {
                    let local_dt: chrono::DateTime<chrono::Local> = dt.into();
                    local_dt.format("%Y-%m-%d %H:%M:%S").to_string()
                })
                .unwrap_or_default();
            if stat.is_dir() {
                directories.push((name, date_str, stat.perm));
            } else {
                files.push((name, stat.size.unwrap_or(0), date_str, stat.perm));
            }
        }
        Ok((directories, files))
    }

    pub fn create_dir(&self, path: &str) -> Result<(), String> {
        self.sftp
            .mkdir(std::path::Path::new(path), 0o755)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_entry(&self, path: &str) -> Result<(), String> {
        delete_sftp_recursive(&self.sftp, std::path::Path::new(path))
    }

    pub fn rename(&self, path: &str, new_path: &str) -> Result<(), String> {
        self.sftp
            .rename(
                std::path::Path::new(path),
                std::path::Path::new(new_path),
                None,
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn write_file(&self, path: &str, content: &[u8]) -> Result<(), String> {
        let mut remote_file = self
            .sftp
            .create(std::path::Path::new(path))
            .map_err(|e| e.to_string())?;
        remote_file.write_all(content).map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(target_os = "android")]
pub struct SftpSession;

#[cfg(target_os = "android")]
impl SftpSession {
    pub fn connect(
        _host: &str,
        _port: u16,
        _user: &str,
        _pass: Option<&str>,
        _key_path: Option<&str>,
        _passphrase: Option<&str>,
    ) -> Result<Self, String> {
        Err("SFTP is not supported on Android".to_string())
    }

    pub fn is_alive(&self) -> bool {
        false
    }
}

#[cfg(not(target_os = "android"))]
fn with_sftp_session<F, T>(
    sftp_session: &std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: Option<&str>,
    key_path: Option<&str>,
    passphrase: Option<&str>,
    mut op: F,
) -> Result<T, String>
where
    F: FnMut(&SftpSession) -> Result<T, String>,
{
    let mut attempts = 0;
    loop {
        let mut lock = sftp_session.lock().unwrap();
        let needs_connect = match &*lock {
            Some(sess) => !sess.is_alive(),
            None => true,
        };

        if needs_connect {
            *lock = None;
            match SftpSession::connect(host, port, user, pass, key_path, passphrase) {
                Ok(sess) => *lock = Some(sess),
                Err(e) => return Err(e),
            }
        }

        let session_ref = lock.as_ref().unwrap();
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

#[cfg(not(target_os = "android"))]
pub fn list_sftp_directory_cached(
    sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: Option<&str>,
    key_path: Option<&str>,
    passphrase: Option<&str>,
    req_path: &str,
) -> Result<(Vec<(String, String, Option<u32>)>, Vec<(String, u64, String, Option<u32>)>), String> {
    with_sftp_session(
        &sftp_session,
        host,
        port,
        user,
        pass,
        key_path,
        passphrase,
        |sess| sess.list_dir(req_path),
    )
}

#[cfg(not(target_os = "android"))]
pub fn create_sftp_directory_cached(
    sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: Option<&str>,
    key_path: Option<&str>,
    passphrase: Option<&str>,
    path: &str,
) -> Result<(), String> {
    with_sftp_session(
        &sftp_session,
        host,
        port,
        user,
        pass,
        key_path,
        passphrase,
        |sess| sess.create_dir(path),
    )
}

#[cfg(not(target_os = "android"))]
pub fn delete_sftp_entry_cached(
    sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: Option<&str>,
    key_path: Option<&str>,
    passphrase: Option<&str>,
    path: &str,
) -> Result<(), String> {
    with_sftp_session(
        &sftp_session,
        host,
        port,
        user,
        pass,
        key_path,
        passphrase,
        |sess| sess.delete_entry(path),
    )
}

#[cfg(not(target_os = "android"))]
pub fn delete_sftp_entries_cached(
    sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: Option<&str>,
    key_path: Option<&str>,
    passphrase: Option<&str>,
    paths: &[String],
) -> Result<(), String> {
    with_sftp_session(
        &sftp_session,
        host,
        port,
        user,
        pass,
        key_path,
        passphrase,
        |sess| {
            for path in paths {
                sess.delete_entry(path)?;
            }
            Ok(())
        },
    )
}

#[cfg(not(target_os = "android"))]
pub fn rename_sftp_entry_cached(
    sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: Option<&str>,
    key_path: Option<&str>,
    passphrase: Option<&str>,
    path: &str,
    new_path: &str,
) -> Result<(), String> {
    with_sftp_session(
        &sftp_session,
        host,
        port,
        user,
        pass,
        key_path,
        passphrase,
        |sess| sess.rename(path, new_path),
    )
}

#[cfg(not(target_os = "android"))]
pub fn write_sftp_file_cached(
    sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    host: &str,
    port: u16,
    user: &str,
    pass: Option<&str>,
    key_path: Option<&str>,
    passphrase: Option<&str>,
    path: &str,
    content: &[u8],
) -> Result<(), String> {
    with_sftp_session(
        &sftp_session,
        host,
        port,
        user,
        pass,
        key_path,
        passphrase,
        |sess| sess.write_file(path, content),
    )
}

#[cfg(target_os = "android")]
pub fn list_sftp_directory_cached(
    _sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    _host: &str,
    _port: u16,
    _user: &str,
    _pass: Option<&str>,
    _key_path: Option<&str>,
    _passphrase: Option<&str>,
    _req_path: &str,
) -> Result<(Vec<(String, String, Option<u32>)>, Vec<(String, u64, String, Option<u32>)>), String> {
    Err("SFTP is not supported on Android".to_string())
}

#[cfg(target_os = "android")]
pub fn create_sftp_directory_cached(
    _sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    _host: &str,
    _port: u16,
    _user: &str,
    _pass: Option<&str>,
    _key_path: Option<&str>,
    _passphrase: Option<&str>,
    _path: &str,
) -> Result<(), String> {
    Err("SFTP is not supported on Android".to_string())
}

#[cfg(target_os = "android")]
pub fn delete_sftp_entry_cached(
    _sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    _host: &str,
    _port: u16,
    _user: &str,
    _pass: Option<&str>,
    _key_path: Option<&str>,
    _passphrase: Option<&str>,
    _path: &str,
) -> Result<(), String> {
    Err("SFTP is not supported on Android".to_string())
}

#[cfg(target_os = "android")]
pub fn delete_sftp_entries_cached(
    _sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    _host: &str,
    _port: u16,
    _user: &str,
    _pass: Option<&str>,
    _key_path: Option<&str>,
    _passphrase: Option<&str>,
    _paths: &[String],
) -> Result<(), String> {
    Err("SFTP is not supported on Android".to_string())
}

#[cfg(target_os = "android")]
pub fn rename_sftp_entry_cached(
    _sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    _host: &str,
    _port: u16,
    _user: &str,
    _pass: Option<&str>,
    _key_path: Option<&str>,
    _passphrase: Option<&str>,
    _path: &str,
    _new_path: &str,
) -> Result<(), String> {
    Err("SFTP is not supported on Android".to_string())
}

#[cfg(target_os = "android")]
pub fn write_sftp_file_cached(
    _sftp_session: std::sync::Arc<std::sync::Mutex<Option<SftpSession>>>,
    _host: &str,
    _port: u16,
    _user: &str,
    _pass: Option<&str>,
    _key_path: Option<&str>,
    _passphrase: Option<&str>,
    _path: &str,
    _content: &[u8],
) -> Result<(), String> {
    Err("SFTP is not supported on Android".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_path_unchanged() {
        let p = expand_tilde("/absolute/path");
        assert_eq!(p, std::path::PathBuf::from("/absolute/path"));
    }

    #[test]
    fn relative_path_unchanged() {
        let p = expand_tilde("relative/path");
        assert_eq!(p, std::path::PathBuf::from("relative/path"));
    }

    #[test]
    fn tilde_alone_expands_to_home_or_stays() {
        let p = expand_tilde("~");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(p, home);
        } else {
            assert_eq!(p, std::path::PathBuf::from("~"));
        }
    }

    #[test]
    fn tilde_slash_prefix_expands() {
        let p = expand_tilde("~/documents/file.txt");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(p, home.join("documents/file.txt"));
        } else {
            assert_eq!(p, std::path::PathBuf::from("~/documents/file.txt"));
        }
    }

    #[test]
    fn tilde_in_middle_not_expanded() {
        let p = expand_tilde("/path/~/file");
        assert_eq!(p, std::path::PathBuf::from("/path/~/file"));
    }

    #[test]
    fn empty_string_returns_empty_path() {
        let p = expand_tilde("");
        assert_eq!(p, std::path::PathBuf::from(""));
    }
}
