//! Copies off a real WebDAV server through the real plugin and the real engine, printing what
//! the dialog would show each second.
//!
//!   IC_PLUGIN_DIR=<folder with libic_ftp_fs.so> IC_FTP_URL=http://127.0.0.1:8088 \
//!   cargo test -p ic-plugin-host --test ftp_streaming -- --ignored --nocapture

use std::rc::Rc;

#[test]
#[ignore = "needs a WebDAV server at IC_FTP_URL"]
fn the_bar_moves_the_whole_way() {
    let url = std::env::var("IC_FTP_URL").expect("IC_FTP_URL is not set");
    let config = client_config::AppConfig::new("ice-commander-ftp-stream");
    let chosen: Vec<String> = ic_plugin_host::loader::installed()
        .iter()
        .map(|p| ic_plugin_host::loader::name_of_library(p))
        .filter(|n| n.contains("ftp") && !n.contains("sftp"))
        .collect();
    assert_eq!(chosen.len(), 1, "expected the ftp plugin, found {chosen:?}");
    ic_plugin_host::loader::set_chosen(&config, &chosen);
    for a in ic_plugin_host::loader::load_for(&config) {
        assert!(a.loaded(), "{a:?}");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async move {
        let (host, port) = url.split_once(':').expect("host:port");
        let settings: std::collections::BTreeMap<String, String> = [
            ("host".to_string(), host.to_string()),
            ("port".to_string(), port.to_string()),
            ("user".to_string(), "anonymous".to_string()),
            ("pass".to_string(), "x@x".to_string()),
        ]
        .into_iter()
        .collect();
        let server = ic_plugin_host::mount_connection("ftp", &settings).expect("mounted");

        let listed = fm_core::rpc::FileSystemRpc::list_dir(&*server, "/".to_string())
            .await
            .expect("the server lists its root");
        let file = listed
            .iter()
            .filter(|e| !e.is_dir)
            .max_by_key(|e| e.size)
            .expect("a file on the server");

        let landing = tempfile::tempdir().unwrap();
        let here: Rc<dyn fm_core::rpc::FileSystemRpc> =
            Rc::new(localfs::local_rpc::LocalFileSystemRpc::new(
                client_config::AppConfig::new("ice-commander-ftp-dest"),
            ));

        let items = vec![transfer_core::TransferItem {
            src_path: format!("/{}", file.name),
            relative_path: file.name.clone(),
            name: file.name.clone(),
            is_dir: false,
            size: file.size,
            permissions: None,
        }];
        let (ptx, prx) = std::sync::mpsc::channel();
        let (etx, _erx) = tokio::sync::mpsc::unbounded_channel();
        let total = file.size;

        let watcher = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut last = std::time::Instant::now();
            let (mut seen, mut updates) = (0u64, 0u64);
            while let Ok(m) = prx.recv() {
                if let transfer_core::CopyMessage::Progress {
                    total_bytes_copied, ..
                } = m
                {
                    seen = total_bytes_copied;
                    updates += 1;
                }
                if last.elapsed().as_millis() >= 1000 {
                    eprintln!(
                        "  {:>5.1}s  {:>3}%  {:>10} / {}",
                        start.elapsed().as_secs_f32(),
                        seen * 100 / total.max(1),
                        seen,
                        total
                    );
                    last = std::time::Instant::now();
                }
            }
            updates
        });

        // The panel must still be able to list while a transfer runs: the stream takes a
        // connection of its own precisely so the control one stays free.
        let listing_during = {
            let server = server.clone();
            tokio::task::spawn_local(async move {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                fm_core::rpc::FileSystemRpc::list_dir(&*server, "/".to_string()).await
            })
        };

        transfer_core::execute_transfer(
            server,
            here,
            items,
            landing.path().to_string_lossy().to_string(),
            false,
            ptx,
            etx,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .await
        .expect("the copy goes through");

        let landed = std::fs::metadata(landing.path().join(&file.name)).expect("it arrived");
        assert_eq!(landed.len(), total, "all of it arrived");
        let listed_mid = listing_during.await.expect("the task ran");
        assert!(
            listed_mid.is_ok(),
            "listing while copying failed: {:?}",
            listed_mid.err()
        );
        eprintln!("  листинг во время копирования: получилось");
        let updates = watcher.join().unwrap();
        eprintln!("  итого обновлений: {updates}");
        assert!(updates > 3, "the bar must move while the file travels");
    });
}
