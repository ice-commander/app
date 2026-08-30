use crate::dialogs::{open_downloaded_file, show_error_dialog, show_info_dialog};
use common::AppError;
use fm_core::rpc::RemoteFileEntry;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

fn parse_date_str(date_str: &str) -> u64 {
    use chrono::TimeZone;
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
        if let chrono::LocalResult::Single(dt) = chrono::Local.from_local_datetime(&naive) {
            return dt.timestamp() as u64;
        }
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M") {
        if let chrono::LocalResult::Single(dt) = chrono::Local.from_local_datetime(&naive) {
            return dt.timestamp() as u64;
        }
    }
    0
}

pub struct TunnelSession {
    session: std::sync::Arc<std::sync::Mutex<ssh2::Session>>,
    local_port: u16,
}

fn ensure_tunnel_helper(
    target_host: &str,
    target_port: u16,
    tunnel_host: &Option<String>,
    tunnel_port: Option<u16>,
    tunnel_user: &Option<String>,
    tunnel_auth_type: &Option<String>,
    tunnel_pass: &Option<String>,
    tunnel_key_path: &Option<String>,
    tunnel_passphrase: &Option<String>,
    tunnel: &std::sync::Arc<std::sync::Mutex<Option<TunnelSession>>>,
) -> Result<(String, u16), String> {
    {
        let lock = tunnel.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref ts) = *lock {
            let authenticated = ts
                .session
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .authenticated();
            if authenticated {
                return Ok(("127.0.0.1".to_string(), ts.local_port));
            }
        }
    }

    *tunnel.lock().unwrap_or_else(|e| e.into_inner()) = None;

    let t_host = tunnel_host.as_ref().ok_or_else(|| "Tunnel host not configured".to_string())?;
    let t_port = tunnel_port.unwrap_or(22);
    let t_user = tunnel_user.as_ref().ok_or_else(|| "Tunnel user not configured".to_string())?;

    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind local loopback listener: {}", e))?;
    listener.set_nonblocking(true)
        .map_err(|e| format!("Failed to set listener non-blocking: {}", e))?;
    let local_port = listener.local_addr()
        .map_err(|e| format!("Failed to get local port: {}", e))?
        .port();

    let tcp = std::net::TcpStream::connect(format!("{}:{}", t_host, t_port))
        .map_err(|e| format!("Tunnel TCP connect error: {}", e))?;
    let mut sess = ssh2::Session::new()
        .map_err(|e| format!("Failed to create SSH session: {}", e))?;
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| format!("Tunnel SSH Handshake failed: {}", e))?;

    if let Some(ref auth) = tunnel_auth_type {
        if auth == "key" {
            if let Some(ref kp) = tunnel_key_path {
                let kp_expanded = if kp.starts_with("~/") {
                    if let Some(mut home) = dirs::home_dir() {
                        home.push(&kp[2..]);
                        home.to_string_lossy().to_string()
                    } else {
                        kp.clone()
                    }
                } else if kp == "~" {
                    if let Some(home) = dirs::home_dir() {
                        home.to_string_lossy().to_string()
                    } else {
                        kp.clone()
                    }
                } else {
                    kp.clone()
                };

                sess.userauth_pubkey_file(t_user, None, std::path::Path::new(&kp_expanded), tunnel_passphrase.as_deref())
                    .map_err(|e| format!("Tunnel SSH key auth failed: {}", e))?;
            } else {
                return Err("Tunnel SSH private key path not configured".to_string());
            }
        } else {
            if let Some(ref p) = tunnel_pass {
                sess.userauth_password(t_user, p)
                    .map_err(|e| format!("Tunnel SSH password auth failed: {}", e))?;
            } else {
                return Err("Tunnel SSH password not configured".to_string());
            }
        }
    } else {
        return Err("Tunnel SSH auth type not specified".to_string());
    }

    sess.set_blocking(false);

    let sess_arc = std::sync::Arc::new(std::sync::Mutex::new(sess));
    *tunnel.lock().unwrap_or_else(|e| e.into_inner()) = Some(TunnelSession {
        session: sess_arc.clone(),
        local_port,
    });

    let target_host_owned = target_host.to_string();
    let sess_for_loop = sess_arc.clone();
    let tunnel_for_loop = tunnel.clone();

    tokio::spawn(async move {
        if let Ok(tokio_listener) = tokio::net::TcpListener::from_std(listener) {
            while let Ok((client_stream, _)) = tokio_listener.accept().await {
                let target_host_c = target_host_owned.clone();
                let sess_c = sess_for_loop.clone();
                let tunnel_c = tunnel_for_loop.clone();

                tokio::spawn(async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        let mut channel = {
                            let sess_lock = sess_c.lock().unwrap_or_else(|e| e.into_inner());
                            sess_lock.set_blocking(true);
                            let ch_res = sess_lock.channel_direct_tcpip(&target_host_c, target_port, None);
                            sess_lock.set_blocking(false);
                            match ch_res {
                                Ok(ch) => ch,
                                Err(_) => {
                                    *tunnel_c.lock().unwrap_or_else(|e| e.into_inner()) = None;
                                    return;
                                }
                            }
                        };

                        if let Ok(mut std_stream) = client_stream.into_std() {
                            let _ = std_stream.set_nonblocking(true);

                            let mut buf_tcp = vec![0u8; 16384];
                            let mut buf_ssh = vec![0u8; 16384];
                            let mut tcp_offset = 0;
                            let mut tcp_len = 0;
                            let mut ssh_offset = 0;
                            let mut ssh_len = 0;

                            loop {
                                let mut progressed = false;

                                if tcp_len == 0 {
                                    use std::io::Read;
                                    match std_stream.read(&mut buf_tcp) {
                                        Ok(0) => break,
                                        Ok(n) => {
                                            tcp_len = n;
                                            tcp_offset = 0;
                                            progressed = true;
                                        }
                                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                                        Err(_) => break,
                                    }
                                }

                                if tcp_len > 0 {
                                    let write_res = {
                                        let _sess_lock = sess_c.lock().unwrap_or_else(|e| e.into_inner());
                                        use std::io::Write;
                                        channel.write(&buf_tcp[tcp_offset..tcp_offset + tcp_len])
                                    };
                                    match write_res {
                                        Ok(n) => {
                                            tcp_offset += n;
                                            tcp_len -= n;
                                            progressed = true;
                                        }
                                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                                        Err(_) => break,
                                    }
                                }

                                if ssh_len == 0 {
                                    let read_res = {
                                        let _sess_lock = sess_c.lock().unwrap_or_else(|e| e.into_inner());
                                        use std::io::Read;
                                        channel.read(&mut buf_ssh)
                                    };
                                    match read_res {
                                        Ok(0) => break,
                                        Ok(n) => {
                                            ssh_len = n;
                                            ssh_offset = 0;
                                            progressed = true;
                                        }
                                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                                        Err(_) => break,
                                    }
                                }

                                if ssh_len > 0 {
                                    use std::io::Write;
                                    match std_stream.write(&buf_ssh[ssh_offset..ssh_offset + ssh_len]) {
                                        Ok(n) => {
                                            ssh_offset += n;
                                            ssh_len -= n;
                                            progressed = true;
                                        }
                                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                                        Err(_) => break,
                                    }
                                }

                                if !progressed {
                                    std::thread::sleep(std::time::Duration::from_millis(5));
                                }
                            }
                        }

                        *tunnel_c.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    }).await;
                });
            }
        }
    });

    Ok(("127.0.0.1".to_string(), local_port))
}

#[derive(Clone)]
struct SftpEndpoint {
    host: String,
    port: u16,
    use_tunnel: Option<bool>,
    tunnel_host: Option<String>,
    tunnel_port: Option<u16>,
    tunnel_user: Option<String>,
    tunnel_auth_type: Option<String>,
    tunnel_pass: Option<String>,
    tunnel_key_path: Option<String>,
    tunnel_passphrase: Option<String>,
    tunnel: std::sync::Arc<std::sync::Mutex<Option<TunnelSession>>>,
}

impl SftpEndpoint {
    fn resolve(&self) -> Result<(String, u16), String> {
        if self.use_tunnel.unwrap_or(false) {
            ensure_tunnel_helper(
                &self.host, self.port, &self.tunnel_host, self.tunnel_port,
                &self.tunnel_user, &self.tunnel_auth_type, &self.tunnel_pass,
                &self.tunnel_key_path, &self.tunnel_passphrase, &self.tunnel,
            )
        } else {
            Ok((self.host.clone(), self.port))
        }
    }
}

pub struct LocalSftpRpc {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: Option<String>,
    pub auth_type: String, // "password" or "key"
    pub key_path: Option<String>,
    pub passphrase: Option<String>,
    pub use_tunnel: Option<bool>,
    pub tunnel_host: Option<String>,
    pub tunnel_port: Option<u16>,
    pub tunnel_user: Option<String>,
    pub tunnel_auth_type: Option<String>,
    pub tunnel_pass: Option<String>,
    pub tunnel_key_path: Option<String>,
    pub tunnel_passphrase: Option<String>,
    pub sftp_session:
        std::sync::Arc<std::sync::Mutex<Option<crate::fs_sftp::SftpSession>>>,
    pub tunnel: std::sync::Arc<std::sync::Mutex<Option<TunnelSession>>>,
}

impl LocalSftpRpc {
    fn endpoint(&self) -> SftpEndpoint {
        SftpEndpoint {
            host: self.host.clone(),
            port: self.port,
            use_tunnel: self.use_tunnel,
            tunnel_host: self.tunnel_host.clone(),
            tunnel_port: self.tunnel_port,
            tunnel_user: self.tunnel_user.clone(),
            tunnel_auth_type: self.tunnel_auth_type.clone(),
            tunnel_pass: self.tunnel_pass.clone(),
            tunnel_key_path: self.tunnel_key_path.clone(),
            tunnel_passphrase: self.tunnel_passphrase.clone(),
            tunnel: self.tunnel.clone(),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl fm_core::rpc::FileSystemRpc for LocalSftpRpc {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn content_wait(&self) -> fm_core::rpc::ContentWait {
        crate::net_content_wait()
    }
    fn connection_id(&self) -> Option<String> {
        Some(format!("sftp://{}@{}:{}", self.user, self.host, self.port))
    }

    fn display_name(&self) -> Option<String> {
        Some(if self.name.is_empty() { self.host.clone() } else { self.name.clone() })
    }

    fn get_icon(&self, path: &str) -> String {
        let path_norm = path.replace('\\', "/");
        if path_norm == "/" || path_norm.is_empty() {
            "/com/icecommander/gtk/ftp.svg".to_string()
        } else {
            "/com/icecommander/gtk/folder.svg".to_string()
        }
    }

    fn get_ssh_connection_command(&self, remote_path: &str) -> Option<Vec<String>> {
        #[cfg(target_os = "windows")]
        let ssh_bin = "ssh.exe".to_string();
        #[cfg(not(target_os = "windows"))]
        let ssh_bin = "ssh".to_string();

        let remote_dir = if remote_path.is_empty() { "/" } else { remote_path };
        let launch = format!("cd {:?} && exec $SHELL -l", remote_dir);

        if self.use_tunnel.unwrap_or(false) {
            let held_port = {
                let lock = self.tunnel.lock().unwrap_or_else(|e| e.into_inner());
                lock.as_ref()
                    .filter(|ts| ts.session.lock().unwrap_or_else(|e| e.into_inner()).authenticated())
                    .map(|ts| ts.local_port)
            };
            if let Some(port) = held_port {
                let mut cmd = vec![
                    ssh_bin.clone(),
                    "-p".to_string(),
                    port.to_string(),
                    "-o".to_string(),
                    "StrictHostKeyChecking=accept-new".to_string(),
                ];
                if let Some(ref kp) = self.key_path {
                    cmd.push("-i".to_string());
                    cmd.push(kp.clone());
                }
                cmd.push(format!("{}@127.0.0.1", self.user));
                cmd.push("-t".to_string());
                cmd.push(launch.clone());
                return Some(cmd);
            }
        }

        let mut cmd = vec![ssh_bin.clone(), "-p".to_string(), self.port.to_string()];
        if let Some(ref kp) = self.key_path {
            cmd.push("-i".to_string());
            cmd.push(kp.clone());
        }
        if self.use_tunnel.unwrap_or(false) {
            if let (Some(t_host), Some(t_user)) = (self.tunnel_host.as_ref(), self.tunnel_user.as_ref()) {
                let t_port = self.tunnel_port.unwrap_or(22);
                let mut proxy = format!("{} -p {} -o StrictHostKeyChecking=accept-new", ssh_bin, t_port);
                if self.tunnel_auth_type.as_deref() == Some("key") {
                    if let Some(ref tk) = self.tunnel_key_path {
                        proxy.push_str(&format!(" -i \"{}\"", tk));
                    }
                }
                proxy.push_str(&format!(" -W %h:%p {}@{}", t_user, t_host));
                cmd.push("-o".to_string());
                cmd.push(format!("ProxyCommand={}", proxy));
            }
        }
        cmd.push(format!("{}@{}", self.user, self.host));
        cmd.push("-t".to_string());
        cmd.push(launch);
        Some(cmd)
    }

    async fn list_dir(&self, path: String) -> Result<Vec<RemoteFileEntry>, AppError> {
        let endpoint = self.endpoint();
        let user = self.user.clone();
        let pass = self.pass.clone();
        let key_path = self.key_path.clone();
        let passphrase = self.passphrase.clone();
        let sftp_session = self.sftp_session.clone();

        let res = tokio::task::spawn_blocking(move || {
            let (connect_host, connect_port) = endpoint.resolve()?;
            crate::fs_sftp::list_sftp_directory_cached(
                sftp_session, &connect_host, connect_port, &user, pass.as_deref(), key_path.as_deref(), passphrase.as_deref(), &path,
            )
        })
        .await;

        match res {
            Ok(Ok((dirs, files))) => {
                let mut entries = Vec::new();
                for d in dirs {
                    entries.push(RemoteFileEntry { name: d.0, is_dir: true, size: 0, modified: parse_date_str(&d.1), permissions: d.2 });
                }
                for f in files {
                    entries.push(RemoteFileEntry { name: f.0, is_dir: false, size: f.1, modified: parse_date_str(&f.2), permissions: f.3 });
                }
                Ok(entries)
            }
            Ok(Err(e)) => Err(AppError::Remote(e)),
            Err(e) => Err(AppError::Remote(e.to_string())),
        }
    }

    async fn create_directory(&self, parent_path: String, dir_name: String, permissions: Option<u32>) -> Result<(), AppError> {
        let user = self.user.clone();
        let pass = self.pass.clone();
        let key_path = self.key_path.clone();
        let passphrase = self.passphrase.clone();
        let mut path = PathBuf::from(&parent_path);
        path.push(&dir_name);
        let path_str = path.to_string_lossy().to_string();
        let sftp_session = self.sftp_session.clone();
        let endpoint = self.endpoint();

        tokio::task::spawn_blocking(move || {
            let (connect_host, connect_port) = endpoint.resolve()?;
            let res = crate::fs_sftp::create_sftp_directory_cached(
                sftp_session.clone(), &connect_host, connect_port, &user, pass.as_deref(), key_path.as_deref(), passphrase.as_deref(), &path_str,
            );
            if res.is_ok() {
                if let Some(mode) = permissions {
                    let lock = sftp_session.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(ref sess) = *lock {
                        let _ = sess.sftp.setstat(std::path::Path::new(&path_str), ssh2::FileStat {
                            size: None,
                            uid: None,
                            gid: None,
                            perm: Some(mode),
                            atime: None,
                            mtime: None,
                        });
                    }
                }
            }
            res
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))
    }

    async fn delete_entries(&self, paths: Vec<String>) -> Result<(), AppError> {
        let user = self.user.clone();
        let pass = self.pass.clone();
        let key_path = self.key_path.clone();
        let passphrase = self.passphrase.clone();
        let sftp_session = self.sftp_session.clone();
        let endpoint = self.endpoint();

        tokio::task::spawn_blocking(move || {
            let (connect_host, connect_port) = endpoint.resolve()?;
            crate::fs_sftp::delete_sftp_entries_cached(
                sftp_session, &connect_host, connect_port, &user, pass.as_deref(), key_path.as_deref(), passphrase.as_deref(), &paths,
            )
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))
    }

    async fn rename_entry(&self, path: String, new_path: String) -> Result<(), AppError> {
        let user = self.user.clone();
        let pass = self.pass.clone();
        let key_path = self.key_path.clone();
        let passphrase = self.passphrase.clone();
        let sftp_session = self.sftp_session.clone();
        let endpoint = self.endpoint();

        tokio::task::spawn_blocking(move || {
            let (connect_host, connect_port) = endpoint.resolve()?;
            crate::fs_sftp::rename_sftp_entry_cached(
                sftp_session, &connect_host, connect_port, &user, pass.as_deref(), key_path.as_deref(), passphrase.as_deref(), &path, &new_path,
            )
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))
    }

    fn request_file_download(&self, file_path: String, _transfer_id: uuid::Uuid) {
        let user = self.user.clone();
        let pass = self.pass.clone();
        let key_path = self.key_path.clone();
        let passphrase = self.passphrase.clone();

        let endpoint = self.endpoint();

        let filename = Path::new(&file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("downloaded_file")
            .to_string();

        let mut dest_path = dirs::download_dir().unwrap_or_else(|| std::env::temp_dir());
        dest_path.push(&filename);

        crate::spawn_local(async move {
            let (tx, mut rx) = mpsc::channel::<Result<Vec<u8>, String>>(100);

            let dest_path_clone = dest_path.clone();
            let writer_handle = tokio::spawn(async move {
                let mut file = match std::fs::File::create(&dest_path_clone) {
                    Ok(f) => f,
                    Err(e) => return Err(e.to_string()),
                };
                while let Some(res) = rx.recv().await {
                    match res {
                        Ok(bytes) => {
                            if bytes.is_empty() {
                                break;
                            }
                            if let Err(e) = file.write_all(&bytes) {
                                return Err(e.to_string());
                            }
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(())
            });

            let download_res = tokio::task::spawn_blocking(move || {
                let (connect_host, connect_port) = endpoint.resolve()?;

                crate::fs_sftp::start_sftp_download_ext(
                    connect_host, connect_port, user, pass, key_path, passphrase, file_path, tx,
                )
            })
            .await;

            let result = match download_res {
                Ok(Ok(_)) => writer_handle
                    .await
                    .unwrap_or(Err("Writer task panicked".to_string())),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(e.to_string()),
            };

            match result {
                Ok(()) => {
                    show_info_dialog(
                        &*crate::i18n::tr("connections.download_complete"),
                        &crate::i18n::trf("connections.download_success_body", &[("path", &*(dest_path.to_string_lossy()).to_string())]),
                    );
                    open_downloaded_file(&dest_path);
                }
                Err(e) => {
                    show_error_dialog(&*crate::i18n::tr("connections.download_error"), &e);
                }
            }
        });
    }

    fn trigger_file_upload(
        &self,
        target_path: String,
        file_name: String,
        local_file_path: std::path::PathBuf,
        _transfer_id: uuid::Uuid,
    ) {
        let user = self.user.clone();
        let pass = self.pass.clone();
        let key_path = self.key_path.clone();
        let passphrase = self.passphrase.clone();

        let endpoint = self.endpoint();

        let mut dest_path = PathBuf::from(&target_path);
        dest_path.push(&file_name);
        let dest_path_str = dest_path.to_string_lossy().to_string();

        crate::spawn_local(async move {
            let user_c = user.clone();
            let pass_c = pass.clone();
            let key_path_c = key_path.clone();
            let passphrase_c = passphrase.clone();
            let dest_path_str_c = dest_path_str.clone();
            let local_file_path_c = local_file_path.clone();
            let res = tokio::task::spawn_blocking(move || {
                let (connect_host, connect_port) = endpoint.resolve()?;

                crate::fs_sftp::upload_sftp_file(
                    &connect_host,
                    connect_port,
                    &user_c,
                    pass_c.as_deref(),
                    key_path_c.as_deref(),
                    passphrase_c.as_deref(),
                    &dest_path_str_c,
                    &local_file_path_c,
                )
            })
            .await;

            match res {
                Ok(Ok(())) => {
                }
                Ok(Err(e)) => {
                    show_error_dialog(&*crate::i18n::tr("connections.upload_error"), &e);
                }
                Err(e) => {
                    show_error_dialog(&*crate::i18n::tr("connections.upload_error"), &e.to_string());
                }
            }
        });
    }

    async fn read_file(
        &self,
        path: String,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<Vec<u8>, AppError> {
        let user = self.user.clone();
        let pass = self.pass.clone();
        let key_path = self.key_path.clone();
        let passphrase = self.passphrase.clone();
        let endpoint = self.endpoint();

        let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>(100);

        let download_res = tokio::task::spawn_blocking(move || {
            let (connect_host, connect_port) = endpoint.resolve()?;
            crate::fs_sftp::start_sftp_download_ext(
                connect_host, connect_port, user, pass, key_path, passphrase, path, tx,
            )
        });

        let (prog_tx, mut prog_rx) = mpsc::channel::<u64>(8);
        let mut assemble = tokio::spawn(async move {
            let mut rx = rx;
            let mut all = Vec::new();
            let mut last_sent = std::time::Instant::now();
            while let Some(res) = rx.recv().await {
                match res {
                    Ok(bytes) => {
                        if bytes.is_empty() {
                            break;
                        }
                        all.extend_from_slice(&bytes);
                        if last_sent.elapsed().as_millis() >= 100 {
                            let _ = prog_tx.try_send(all.len() as u64);
                            last_sent = std::time::Instant::now();
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
            Ok(all)
        });

        let assembled = if progress_callback.is_some() {
            loop {
                tokio::select! {
                    biased;
                    res = &mut assemble => break res,
                    Some(total) = prog_rx.recv() => {
                        if let Some(ref cb) = progress_callback {
                            cb(total);
                        }
                    }
                }
            }
        } else {
            drop(prog_rx);
            assemble.await
        };
        let all_bytes = match assembled {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Err(AppError::Remote(e)),
            Err(e) => return Err(AppError::Remote(e.to_string())),
        };

        match download_res.await {
            Ok(Ok(_)) => Ok(all_bytes),
            Ok(Err(e)) => Err(AppError::Remote(e)),
            Err(e) => Err(AppError::Remote(e.to_string())),
        }
    }

    async fn write_file(
        &self,
        path: String,
        content: Vec<u8>,
        permissions: Option<u32>,
        progress_callback: Option<Box<dyn Fn(u64) + 'static>>,
    ) -> Result<(), AppError> {
        let user = self.user.clone();
        let pass = self.pass.clone();
        let key_path = self.key_path.clone();
        let passphrase = self.passphrase.clone();
        let sftp_session = self.sftp_session.clone();

        let endpoint = self.endpoint();

        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<u64>(100);

            let upload_res = tokio::task::spawn_blocking(move || {
                let mut attempts = 0;
                loop {
                    let mut lock = sftp_session.lock().unwrap_or_else(|e| e.into_inner());
                    let needs_connect = match &*lock {
                        Some(sess) => !sess.is_alive(),
                        None => true,
                    };

                    if needs_connect {
                        *lock = None;
                        let (connect_host, connect_port) = endpoint.resolve()?;

                        match crate::fs_sftp::SftpSession::connect(
                            &connect_host,
                            connect_port,
                            &user,
                            pass.as_deref(),
                            key_path.as_deref(),
                            passphrase.as_deref(),
                        ) {
                            Ok(sess) => *lock = Some(sess),
                            Err(e) => return Err(e),
                        }
                    }

                    let session_ref = lock.as_ref().expect("session was just connected");

                    let create_res = session_ref.sftp.create(std::path::Path::new(&path))
                        .map_err(|e| e.to_string());

                    match create_res {
                        Ok(mut remote_file) => {
                            use std::io::Write;
                            let chunk_size = 16 * 1024;
                            let mut offset = 0;
                            let len = content.len();
                            let mut last_sent = std::time::Instant::now()
                                - std::time::Duration::from_millis(200);
                            while offset < len {
                                let end = std::cmp::min(offset + chunk_size, len);
                                if let Err(e) = remote_file.write_all(&content[offset..end]) {
                                    return Err(e.to_string());
                                }
                                offset = end;
                                if last_sent.elapsed().as_millis() >= 100 {
                                    let _ = progress_tx.try_send(offset as u64);
                                    last_sent = std::time::Instant::now();
                                }
                            }
                            if let Some(mode) = permissions {
                                let _ = session_ref.sftp.setstat(std::path::Path::new(&path), ssh2::FileStat {
                                    size: None,
                                    uid: None,
                                    gid: None,
                                    perm: Some(mode),
                                    atime: None,
                                    mtime: None,
                                });
                            }
                            return Ok(());
                        }
                        Err(e) => {
                            attempts += 1;
                            if attempts >= 2 {
                                return Err(e);
                            }
                            *lock = None;
                        }
                    }
                }
            });

            let mut progress_done = false;
            while !progress_done {
                tokio::select! {
                    Some(bytes) = progress_rx.recv() => {
                        if let Some(ref cb) = progress_callback {
                            cb(bytes);
                        }
                    }
                    else => {
                        progress_done = true;
                    }
                }
            }

        match upload_res.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(AppError::Remote(e)),
            Err(e) => Err(AppError::Remote(e.to_string())),
        }
    }

    async fn get_permissions(&self, path: String) -> Result<u32, AppError> {
        let user = self.user.clone();
        let pass = self.pass.clone();
        let key_path = self.key_path.clone();
        let passphrase = self.passphrase.clone();
        let sftp_session = self.sftp_session.clone();
        let endpoint = self.endpoint();

        tokio::task::spawn_blocking(move || {
            let (connect_host, connect_port) = endpoint.resolve()?;

            let mut attempts = 0;
            loop {
                let mut lock = sftp_session.lock().unwrap_or_else(|e| e.into_inner());
                let needs_connect = match &*lock {
                    Some(sess) => !sess.is_alive(),
                    None => true,
                };

                if needs_connect {
                    *lock = None;
                    match crate::fs_sftp::SftpSession::connect(
                        &connect_host,
                        connect_port,
                        &user,
                        pass.as_deref(),
                        key_path.as_deref(),
                        passphrase.as_deref(),
                    ) {
                        Ok(sess) => *lock = Some(sess),
                        Err(e) => return Err(e),
                    }
                }

                let session_ref = lock.as_ref().expect("session was just connected");
                let path_obj = std::path::Path::new(&path);
                match session_ref.sftp.stat(path_obj) {
                    Ok(stat) => {
                        return Ok(stat.perm.unwrap_or(0o644));
                    }
                    Err(e) => {
                        attempts += 1;
                        if attempts >= 2 {
                            return Err(e.to_string());
                        }
                        *lock = None;
                    }
                }
            }
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))
    }

    async fn set_permissions(&self, path: String, permissions: u32) -> Result<(), AppError> {
        let user = self.user.clone();
        let pass = self.pass.clone();
        let key_path = self.key_path.clone();
        let passphrase = self.passphrase.clone();
        let sftp_session = self.sftp_session.clone();
        let endpoint = self.endpoint();

        tokio::task::spawn_blocking(move || {
            let (connect_host, connect_port) = endpoint.resolve()?;

            let mut attempts = 0;
            loop {
                let mut lock = sftp_session.lock().unwrap_or_else(|e| e.into_inner());
                let needs_connect = match &*lock {
                    Some(sess) => !sess.is_alive(),
                    None => true,
                };

                if needs_connect {
                    *lock = None;
                    match crate::fs_sftp::SftpSession::connect(
                        &connect_host,
                        connect_port,
                        &user,
                        pass.as_deref(),
                        key_path.as_deref(),
                        passphrase.as_deref(),
                    ) {
                        Ok(sess) => *lock = Some(sess),
                        Err(e) => return Err(e),
                    }
                }

                let session_ref = lock.as_ref().expect("session was just connected");
                let path_obj = std::path::Path::new(&path);
                
                let stat = ssh2::FileStat {
                    size: None,
                    uid: None,
                    gid: None,
                    perm: Some(permissions),
                    atime: None,
                    mtime: None,
                };

                match session_ref.sftp.setstat(path_obj, stat) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        attempts += 1;
                        if attempts >= 2 {
                            return Err(e.to_string());
                        }
                        *lock = None;
                    }
                }
            }
        })
        .await
        .map_err(|e| AppError::Remote(e.to_string()))?
        .map_err(|e| AppError::Remote(e))
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use fm_core::rpc::FileSystemRpc;

    fn rpc() -> LocalSftpRpc {
        LocalSftpRpc {
            name: String::new(),
            host: "example.test".to_string(),
            port: 2222,
            user: "alice".to_string(),
            pass: Some("secret".to_string()),
            auth_type: "password".to_string(),
            key_path: None,
            passphrase: None,
            use_tunnel: None,
            tunnel_host: None,
            tunnel_port: None,
            tunnel_user: None,
            tunnel_auth_type: None,
            tunnel_pass: None,
            tunnel_key_path: None,
            tunnel_passphrase: None,
            sftp_session: std::sync::Arc::new(std::sync::Mutex::new(None)),
            tunnel: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    #[test]
    fn iso_date_with_seconds_parses_to_that_instant() {
        let ts = parse_date_str("2024-01-15 10:30:00");
        assert!(ts > 1_705_000_000 && ts < 1_705_500_000, "ts={}", ts);
    }

    #[test]
    fn iso_date_without_seconds_parses_to_the_same_minute() {
        assert_eq!(
            parse_date_str("2024-01-15 10:30:00"),
            parse_date_str("2024-01-15 10:30")
        );
    }

    #[test]
    fn format_emitted_by_the_listing_round_trips() {
        use chrono::TimeZone;
        let dt = chrono::Local.timestamp_opt(1_718_452_800, 0).unwrap();
        let rendered = dt.format("%Y-%m-%d %H:%M:%S").to_string();
        assert_eq!(parse_date_str(&rendered), 1_718_452_800);
    }

    #[test]
    fn unix_ls_style_date_returns_zero() {
        assert_eq!(parse_date_str("Jun 28 14:12"), 0);
        assert_eq!(parse_date_str("Jun 28 2021"), 0);
    }

    #[test]
    fn empty_and_malformed_dates_return_zero() {
        assert_eq!(parse_date_str(""), 0);
        assert_eq!(parse_date_str("not a date"), 0);
        assert_eq!(parse_date_str("2024-02-31 00:00:00"), 0);
    }

    #[test]
    fn connection_id_carries_user_host_and_port() {
        assert_eq!(rpc().connection_id().unwrap(), "sftp://alice@example.test:2222");
    }

    #[test]
    fn display_name_falls_back_to_host_when_unnamed() {
        assert_eq!(rpc().display_name().unwrap(), "example.test");
    }

    #[test]
    fn display_name_prefers_the_configured_name() {
        let mut r = rpc();
        r.name = "Build server".to_string();
        assert_eq!(r.display_name().unwrap(), "Build server");
    }

    #[test]
    fn root_gets_the_ftp_icon_and_subdirs_the_folder_icon() {
        let r = rpc();
        assert!(r.get_icon("/").ends_with("/ftp.svg"));
        assert!(r.get_icon("").ends_with("/ftp.svg"));
        assert!(r.get_icon("\\").ends_with("/ftp.svg"));
        assert!(r.get_icon("/var/log").ends_with("/folder.svg"));
    }

    #[test]
    fn endpoint_without_tunnel_resolves_to_the_configured_host() {
        assert_eq!(
            rpc().endpoint().resolve().unwrap(),
            ("example.test".to_string(), 2222)
        );
    }

    #[test]
    fn endpoint_with_tunnel_explicitly_disabled_resolves_directly() {
        let mut r = rpc();
        r.use_tunnel = Some(false);
        r.tunnel_host = Some("jump.test".to_string());
        assert_eq!(
            r.endpoint().resolve().unwrap(),
            ("example.test".to_string(), 2222)
        );
    }

    #[test]
    fn endpoint_with_tunnel_but_no_tunnel_host_fails() {
        let mut r = rpc();
        r.use_tunnel = Some(true);
        assert_eq!(
            r.endpoint().resolve().unwrap_err(),
            "Tunnel host not configured"
        );
    }

    #[test]
    fn endpoint_with_tunnel_but_no_tunnel_user_fails() {
        let mut r = rpc();
        r.use_tunnel = Some(true);
        r.tunnel_host = Some("jump.test".to_string());
        assert_eq!(
            r.endpoint().resolve().unwrap_err(),
            "Tunnel user not configured"
        );
    }

    #[test]
    fn ssh_command_targets_user_at_host_on_the_configured_port() {
        assert_eq!(
            rpc().get_ssh_connection_command("/srv/data").unwrap(),
            vec![
                "ssh",
                "-p",
                "2222",
                "alice@example.test",
                "-t",
                "cd \"/srv/data\" && exec $SHELL -l",
            ]
        );
    }

    #[test]
    fn ssh_command_defaults_an_empty_remote_path_to_root() {
        let cmd = rpc().get_ssh_connection_command("").unwrap();
        assert_eq!(cmd.last().unwrap(), "cd \"/\" && exec $SHELL -l");
    }

    #[test]
    fn ssh_command_quotes_paths_with_spaces() {
        let cmd = rpc().get_ssh_connection_command("/srv/my data").unwrap();
        assert_eq!(cmd.last().unwrap(), "cd \"/srv/my data\" && exec $SHELL -l");
    }

    #[test]
    fn ssh_command_passes_the_identity_file_when_configured() {
        let mut r = rpc();
        r.key_path = Some("/keys/id_ed25519".to_string());
        let cmd = r.get_ssh_connection_command("/").unwrap();
        let i = cmd.iter().position(|a| a == "-i").expect("-i missing");
        assert_eq!(cmd[i + 1], "/keys/id_ed25519");
    }

    #[test]
    fn ssh_command_omits_the_identity_file_for_password_auth() {
        assert!(!rpc().get_ssh_connection_command("/").unwrap().iter().any(|a| a == "-i"));
    }

    #[test]
    fn ssh_command_builds_a_proxy_command_for_a_configured_tunnel() {
        let mut r = rpc();
        r.use_tunnel = Some(true);
        r.tunnel_host = Some("jump.test".to_string());
        r.tunnel_user = Some("bob".to_string());
        let cmd = r.get_ssh_connection_command("/").unwrap();
        let i = cmd.iter().position(|a| a == "-o").expect("-o missing");
        assert_eq!(
            cmd[i + 1],
            "ProxyCommand=ssh -p 22 -o StrictHostKeyChecking=accept-new -W %h:%p bob@jump.test"
        );
    }

    #[test]
    fn proxy_command_uses_the_configured_tunnel_port() {
        let mut r = rpc();
        r.use_tunnel = Some(true);
        r.tunnel_host = Some("jump.test".to_string());
        r.tunnel_user = Some("bob".to_string());
        r.tunnel_port = Some(2022);
        let cmd = r.get_ssh_connection_command("/").unwrap();
        assert!(cmd.iter().any(|a| a.contains("ProxyCommand=ssh -p 2022 ")), "{:?}", cmd);
    }

    #[test]
    fn proxy_command_carries_the_tunnel_key_when_the_jump_uses_key_auth() {
        let mut r = rpc();
        r.use_tunnel = Some(true);
        r.tunnel_host = Some("jump.test".to_string());
        r.tunnel_user = Some("bob".to_string());
        r.tunnel_auth_type = Some("key".to_string());
        r.tunnel_key_path = Some("/keys/jump".to_string());
        let cmd = r.get_ssh_connection_command("/").unwrap();
        assert!(cmd.iter().any(|a| a.contains("-i \"/keys/jump\"")), "{:?}", cmd);
    }

    #[test]
    fn proxy_command_omits_the_tunnel_key_for_password_jump_auth() {
        let mut r = rpc();
        r.use_tunnel = Some(true);
        r.tunnel_host = Some("jump.test".to_string());
        r.tunnel_user = Some("bob".to_string());
        r.tunnel_auth_type = Some("password".to_string());
        r.tunnel_key_path = Some("/keys/jump".to_string());
        let cmd = r.get_ssh_connection_command("/").unwrap();
        assert!(!cmd.iter().any(|a| a.contains("/keys/jump")), "{:?}", cmd);
    }

    #[test]
    fn incomplete_tunnel_config_falls_back_to_a_direct_command() {
        let mut r = rpc();
        r.use_tunnel = Some(true);
        r.tunnel_host = Some("jump.test".to_string());
        let cmd = r.get_ssh_connection_command("/").unwrap();
        assert_eq!(cmd, rpc().get_ssh_connection_command("/").unwrap());
    }
}
