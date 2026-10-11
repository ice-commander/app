//! The local terminal the way the desktop application runs it — the user's login shell in the panel's folder,
//! closed by letting go of its input — opened and closed over and over, and no shell left behind.
#![cfg(unix)]

use ic_platform::terminal::{spawn_pty_command, PtySession};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

fn login_shell() -> Vec<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    if cfg!(target_os = "macos") {
        vec![shell, "-l".to_string()]
    } else {
        vec![shell]
    }
}

/// The count below is of every shell this process started, so the cases take turns.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The shells this process started and has not yet reaped.
fn shells_left() -> usize {
    let out = std::process::Command::new("pgrep")
        .args(["-P", &std::process::id().to_string()])
        .output()
        .expect("pgrep");
    String::from_utf8_lossy(&out.stdout).lines().count()
}

/// Everything the shell writes, answering a cursor-position question the way a terminal would.
fn listen(session: PtySession) -> (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let PtySession {
        input_tx,
        mut output_rx,
        resize_tx,
    } = session;
    let (seen_tx, seen_rx) = mpsc::channel(256);
    // Weak: holding the input would keep the session open after the caller let go of it.
    let answer = input_tx.downgrade();
    tokio::spawn(async move {
        let _resize = resize_tx;
        while let Some(data) = output_rx.recv().await {
            if String::from_utf8_lossy(&data).contains("\x1b[6n") {
                if let Some(answer) = answer.upgrade() {
                    let _ = answer.send(b"\x1b[24;80R".to_vec()).await;
                }
            }
            if seen_tx.send(data).await.is_err() {
                break;
            }
        }
    });
    (input_tx, seen_rx)
}

async fn wait_for(seen: &mut mpsc::Receiver<Vec<u8>>, marker: &str, within: Duration) -> bool {
    let until = Instant::now() + within;
    let mut text = String::new();
    while Instant::now() < until {
        match tokio::time::timeout(Duration::from_millis(100), seen.recv()).await {
            Ok(Some(data)) => {
                text.push_str(&String::from_utf8_lossy(&data));
                if text.contains(marker) {
                    return true;
                }
            }
            Ok(None) => return false,
            Err(_) => {}
        }
    }
    false
}

async fn ended(seen: &mut mpsc::Receiver<Vec<u8>>, within: Duration) -> bool {
    let until = Instant::now() + within;
    while Instant::now() < until {
        match tokio::time::timeout(Duration::from_millis(100), seen.recv()).await {
            Ok(None) => return true,
            Ok(Some(_)) | Err(_) => {}
        }
    }
    false
}

async fn none_left(within: Duration) -> bool {
    let until = Instant::now() + within;
    while Instant::now() < until {
        if shells_left() == 0 {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

fn here() -> Option<String> {
    Some(std::env::temp_dir().to_string_lossy().to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_login_shell_opens_answers_and_closes_twenty_times() {
    let _turn = ONE_AT_A_TIME.lock().await;
    for round in 0..20 {
        let (input, mut seen) = listen(spawn_pty_command(login_shell(), here()).expect("spawn"));
        input
            .send(format!("echo cycle-{round}-done\r").into_bytes())
            .await
            .unwrap();
        assert!(
            wait_for(
                &mut seen,
                &format!("cycle-{round}-done"),
                Duration::from_secs(10)
            )
            .await,
            "round {round}: the shell did not answer"
        );
        drop(input);
        assert!(
            ended(&mut seen, Duration::from_secs(5)).await,
            "round {round}: output never ended"
        );
        assert!(
            none_left(Duration::from_secs(5)).await,
            "round {round}: a shell was left running"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_at_once_after_opening_leaves_nothing() {
    let _turn = ONE_AT_A_TIME.lock().await;
    for _ in 0..20 {
        let (input, mut seen) = listen(spawn_pty_command(login_shell(), here()).expect("spawn"));
        drop(input);
        assert!(ended(&mut seen, Duration::from_secs(5)).await);
    }
    assert!(
        none_left(Duration::from_secs(5)).await,
        "{} shell(s) left running",
        shells_left()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shell_that_exits_by_itself_ends_the_session() {
    let _turn = ONE_AT_A_TIME.lock().await;
    for _ in 0..5 {
        let (input, mut seen) = listen(spawn_pty_command(login_shell(), here()).expect("spawn"));
        input.send(b"exit\r".to_vec()).await.unwrap();
        assert!(
            ended(&mut seen, Duration::from_secs(10)).await,
            "exit did not end the session"
        );
        drop(input);
    }
    assert!(none_left(Duration::from_secs(5)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn several_terminals_at_once_each_answer_and_all_close() {
    let _turn = ONE_AT_A_TIME.lock().await;
    let mut open = Vec::new();
    for side in 0..4 {
        let (input, mut seen) = listen(spawn_pty_command(login_shell(), here()).expect("spawn"));
        input
            .send(format!("echo side-{side}\r").into_bytes())
            .await
            .unwrap();
        assert!(wait_for(&mut seen, &format!("side-{side}"), Duration::from_secs(10)).await);
        open.push((input, seen));
    }
    for (input, mut seen) in open {
        drop(input);
        assert!(ended(&mut seen, Duration::from_secs(5)).await);
    }
    assert!(none_left(Duration::from_secs(5)).await);
}
