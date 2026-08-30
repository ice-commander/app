use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct PtySession {
    pub input_tx: mpsc::Sender<Vec<u8>>,
    pub output_rx: mpsc::Receiver<Vec<u8>>,
    pub resize_tx: mpsc::Sender<(u16, u16)>,
}

pub fn spawn_pty_session(cwd: Option<String>) -> Result<PtySession, String> {
    #[cfg(target_os = "windows")]
    let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
    #[cfg(not(target_os = "windows"))]
    let shell = "bash".to_string();

    spawn_pty_command(vec![shell], cwd)
}

pub fn spawn_pty_command(args: Vec<String>, cwd: Option<String>) -> Result<PtySession, String> {
    if args.is_empty() {
        return Err("Command args cannot be empty".to_string());
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let mut cmd = CommandBuilder::new(&args[0]);
    for arg in &args[1..] {
        cmd.arg(arg);
    }
    if let Some(dir) = cwd {
        cmd.cwd(dir);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    if cfg!(target_os = "macos")
        && std::env::var("LANG").map(|v| v.is_empty()).unwrap_or(true)
        && std::env::var("LC_ALL")
            .map(|v| v.is_empty())
            .unwrap_or(true)
    {
        cmd.env("LANG", "en_US.UTF-8");
    }
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = Arc::new(std::sync::Mutex::new(
        pair.master.take_writer().map_err(|e| e.to_string())?,
    ));

    let (out_tx, out_rx) = mpsc::channel(100);
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = [0u8; 2048];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let (in_tx, mut in_rx) = mpsc::channel::<Vec<u8>>(32);
    let (resize_tx, mut resize_rx) = mpsc::channel::<(u16, u16)>(8);

    let mut slave_holder = Some(pair.slave);
    let mut master_holder = Some(pair.master);
    let mut writer_holder = Some(writer);

    tokio::spawn(async move {
        let mut child = child;

        loop {
            tokio::select! {
                maybe_data = in_rx.recv() => {
                    match maybe_data {
                        Some(data) => {
                            if let Some(ref w) = writer_holder {
                                let w = w.clone();
                                let _ = tokio::task::spawn_blocking(move || {
                                    use std::io::Write;
                                    if let Ok(mut lock) = w.lock() {
                                        let _ = lock.write_all(&data);
                                        let _ = lock.flush();
                                    }
                                }).await;
                            }
                        }
                        None => {
                            drop(slave_holder.take());
                            drop(master_holder.take());
                            drop(writer_holder.take());

                            let _ = tokio::task::spawn_blocking(move || {
                                let _ = child.kill();
                                let _ = child.wait();
                            }).await;
                            break;
                        }
                    }
                }
                maybe_size = resize_rx.recv() => {
                    if let Some((rows, cols)) = maybe_size {
                        if let Some(ref m) = master_holder {
                            let _ = m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
                        }
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                    let result = tokio::task::spawn_blocking(move || {
                        match child.try_wait() {
                            Ok(Some(_)) => (true, child),
                            Ok(None) => (false, child),
                            Err(_) => (true, child),
                        }
                    }).await;
                    if let Ok((exited, returned_child)) = result {
                        child = returned_child;
                        if exited {
                            drop(slave_holder.take());
                            drop(master_holder.take());
                            drop(writer_holder.take());

                            let mut c = child;
                            let _ = tokio::task::spawn_blocking(move || {
                                let _ = c.wait();
                            }).await;
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    });

    Ok(PtySession {
        input_tx: in_tx,
        output_rx: out_rx,
        resize_tx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[test]
    fn empty_args_returns_error() {
        let result = spawn_pty_command(vec![], None);
        assert!(result.is_err());
        assert!(result.err().unwrap().contains("empty"));
    }

    #[tokio::test]
    async fn test_terminal_lifecycle_loop() {
        for _ in 0..16 {
            let session = spawn_pty_session(None).expect("Failed to spawn PTY session");

            let input_tx_opt = Arc::new(std::sync::Mutex::new(Some(session.input_tx.clone())));
            let input_tx_clone = input_tx_opt.clone();
            let mut rx = session.output_rx;

            let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
            tokio::spawn(async move {
                while let Some(data) = rx.recv().await {
                    let text = String::from_utf8_lossy(&data);
                    if text.contains("\x1b[6n") {
                        let tx_opt = {
                            let tx_lock = input_tx_clone.lock().unwrap();
                            tx_lock.clone()
                        };
                        if let Some(tx) = tx_opt {
                            let _ = tx.send(b"\x1b[24;80R".to_vec()).await;
                        }
                    }
                    let _ = done_tx.send(data).await;
                }
            });

            sleep(Duration::from_millis(500)).await;

            while done_rx.try_recv().is_ok() {}

            let cmd = b"echo test_command_123\r\n";
            session
                .input_tx
                .send(cmd.to_vec())
                .await
                .expect("Failed to send command");

            sleep(Duration::from_millis(500)).await;

            let mut response = Vec::new();
            while let Ok(data) = done_rx.try_recv() {
                response.extend_from_slice(&data);
            }
            let response_str = String::from_utf8_lossy(&response);
            assert!(
                response_str.contains("test_command_123"),
                "Output did not contain echo response!"
            );

            *input_tx_opt.lock().unwrap() = None;

            drop(session.input_tx);

            let mut closed = false;
            for _ in 0..10 {
                if done_rx.recv().await.is_none() {
                    closed = true;
                    break;
                }
                sleep(Duration::from_millis(100)).await;
            }
            assert!(closed, "session did not close after dropping input_tx!");
        }
    }
}
