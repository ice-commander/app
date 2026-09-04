use fm_core::rpc::SshShellTarget;
use ic_platform::terminal::PtySession;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tokio::sync::mpsc;

const READ_BUF: usize = 8192;
const IDLE_SLEEP: Duration = Duration::from_millis(8);
const WRITE_RETRY: Duration = Duration::from_millis(2);

pub fn open_ssh_shell(target: SshShellTarget, rows: u16, cols: u16) -> PtySession {
    let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(256);
    let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(256);
    let (resize_tx, resize_rx) = mpsc::channel::<(u16, u16)>(16);

    std::thread::spawn(move || {
        if let Err(e) = run(target, rows, cols, input_rx, &output_tx, resize_rx) {
            let _ = output_tx.blocking_send(format!("\r\n{e}\r\n").into_bytes());
        }
    });

    PtySession { input_tx, output_rx, resize_tx }
}

fn authenticate(session: &ssh2::Session, target: &SshShellTarget) -> Result<(), String> {
    if let Some(key) = target.key_path.as_deref().filter(|k| !k.is_empty()) {
        session
            .userauth_pubkey_file(
                &target.user,
                None,
                std::path::Path::new(key),
                target.passphrase.as_deref().filter(|p| !p.is_empty()),
            )
            .map_err(|e| format!("key authentication failed: {e}"))?;
    } else if let Some(pass) = target.pass.as_deref() {
        session
            .userauth_password(&target.user, pass)
            .map_err(|e| format!("password authentication failed: {e}"))?;
    } else {
        return Err("no stored credentials for this connection".to_string());
    }
    if session.authenticated() {
        Ok(())
    } else {
        Err("authentication failed".to_string())
    }
}

fn write_all(channel: &mut ssh2::Channel, data: &[u8]) -> bool {
    let mut sent = 0;
    while sent < data.len() {
        match channel.write(&data[sent..]) {
            Ok(0) => return false,
            Ok(n) => sent += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(WRITE_RETRY)
            }
            Err(_) => return false,
        }
    }
    true
}

fn run(
    target: SshShellTarget,
    rows: u16,
    cols: u16,
    mut input_rx: mpsc::Receiver<Vec<u8>>,
    output_tx: &mpsc::Sender<Vec<u8>>,
    mut resize_rx: mpsc::Receiver<(u16, u16)>,
) -> Result<(), String> {
    let addr = format!("{}:{}", target.host, target.port);
    let tcp = TcpStream::connect(&addr).map_err(|e| format!("cannot reach {addr}: {e}"))?;
    let mut session = ssh2::Session::new().map_err(|e| e.to_string())?;
    session.set_tcp_stream(tcp);
    session.handshake().map_err(|e| format!("ssh handshake failed: {e}"))?;
    authenticate(&session, &target)?;

    let mut channel = session
        .channel_session()
        .map_err(|e| format!("cannot open channel: {e}"))?;
    channel
        .request_pty(
            "xterm-256color",
            None,
            Some((cols.max(1) as u32, rows.max(1) as u32, 0, 0)),
        )
        .map_err(|e| format!("cannot request a terminal: {e}"))?;
    channel.shell().map_err(|e| format!("cannot start a shell: {e}"))?;

    if !target.remote_dir.is_empty() {
        let quoted = target.remote_dir.replace('\'', r"'\''");
        let _ = write_all(&mut channel, format!("cd '{quoted}'\n").as_bytes());
    }

    session.set_blocking(false);
    let mut buf = [0u8; READ_BUF];
    loop {
        let mut idle = true;
        match channel.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                idle = false;
                if output_tx.blocking_send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        let mut done = false;
        loop {
            match input_rx.try_recv() {
                Ok(data) => {
                    idle = false;
                    if !write_all(&mut channel, &data) {
                        done = true;
                        break;
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    done = true;
                    break;
                }
            }
        }
        while let Ok((r, c)) = resize_rx.try_recv() {
            idle = false;
            let _ = channel.request_pty_size(c.max(1) as u32, r.max(1) as u32, None, None);
        }
        if done || channel.eof() {
            break;
        }
        if idle {
            std::thread::sleep(IDLE_SLEEP);
        }
    }

    session.set_blocking(true);
    let _ = channel.close();
    let _ = channel.wait_close();
    Ok(())
}
