//! Copies a file every way round between the filesystems a panel can hold — SFTP, FTP, WebDAV
//! and this machine's own disk — through the real plugins and the real engine.
//!
//! Start the servers and run it with `tests/servers/run-with-servers.sh`; on its own it is
//! skipped, because it needs them.

use std::rc::Rc;

fn told(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Every filesystem the servers were started for, as the panel would hold them.
struct Reachable {
    kinds: Vec<Place>,
}

struct Place {
    /// What the panel would call it.
    name: &'static str,
    /// The plugin that mounts it, and what to mount.
    settings: std::collections::BTreeMap<String, String>,
    kind: &'static str,
    /// Where its files sit on this machine, so a test can put one there and read it back.
    root: std::path::PathBuf,
    /// The path a mount of it answers to.
    at: String,
}

impl Reachable {
    fn found() -> Self {
        let mut kinds = Vec::new();

        if let (Some(url), Some(root)) = (told("IC_DAV_URL"), told("IC_DAV_ROOT")) {
            kinds.push(Place {
                name: "webdav",
                settings: [("url".to_string(), url)].into_iter().collect(),
                kind: "webdav",
                root: root.into(),
                at: "/".to_string(),
            });
        }

        if let (Some(addr), Some(root)) = (told("IC_FTP_ADDR"), told("IC_FTP_ROOT")) {
            let (host, port) = addr.split_once(':').expect("host:port");
            kinds.push(Place {
                name: "ftp",
                settings: [
                    ("host".to_string(), host.to_string()),
                    ("port".to_string(), port.to_string()),
                    ("user".to_string(), "anonymous".to_string()),
                    ("pass".to_string(), "x@x".to_string()),
                ]
                .into_iter()
                .collect(),
                kind: "ftp",
                root: root.into(),
                at: "/".to_string(),
            });
        }

        if let (Some(addr), Some(user), Some(key), Some(root)) = (
            told("IC_SFTP_ADDR"),
            told("IC_SFTP_USER"),
            told("IC_SFTP_KEY"),
            told("IC_SFTP_ROOT"),
        ) {
            let (host, port) = addr.split_once(':').expect("host:port");
            kinds.push(Place {
                name: "sftp",
                settings: [
                    ("host".to_string(), host.to_string()),
                    ("port".to_string(), port.to_string()),
                    ("user".to_string(), user),
                    ("key_path".to_string(), key),
                    ("auth_type".to_string(), "key".to_string()),
                ]
                .into_iter()
                .collect(),
                kind: "sftp",
                root: root.clone().into(),
                at: root,
            });
        }

        Self { kinds }
    }
}

fn load_the_plugins() -> client_config::AppConfig {
    let config = client_config::AppConfig::new("ice-commander-cross-plugin");
    let chosen: Vec<String> = ic_plugin_host::loader::installed()
        .iter()
        .map(|p| ic_plugin_host::loader::name_of_library(p))
        .filter(|n| n.contains("sftp") || n.contains("ftp") || n.contains("webdav"))
        .collect();
    assert!(!chosen.is_empty(), "no filesystem plugins to test with");
    ic_plugin_host::loader::set_chosen(&config, &chosen);
    for attempt in ic_plugin_host::loader::load_for(&config) {
        assert!(attempt.loaded(), "{attempt:?}");
    }
    config
}

fn local_disk() -> Rc<dyn fm_core::rpc::FileSystemRpc> {
    Rc::new(localfs::local_rpc::LocalFileSystemRpc::new(
        client_config::AppConfig::new("ice-commander-cross-plugin-disk"),
    ))
}

struct Carried {
    reports: Vec<u64>,
    files_done: usize,
}

/// Copies one file from somewhere to somewhere and says what the dialog was told.
async fn copy_one(
    from: Rc<dyn fm_core::rpc::FileSystemRpc>,
    src_path: &str,
    into: Rc<dyn fm_core::rpc::FileSystemRpc>,
    dest_parent: &str,
    name: &str,
    size: u64,
) -> Carried {
    let (progress_tx, progress_rx) = std::sync::mpsc::channel();
    let (error_tx, mut error_rx) =
        tokio::sync::mpsc::unbounded_channel::<transfer_core::ErrorRequest>();

    let complaining = tokio::task::spawn_local(async move {
        let mut said = Vec::new();
        while let Some(asked) = error_rx.recv().await {
            said.push(asked.message.clone());
            let _ = asked.reply.send(transfer_core::ErrorAction::Abort);
        }
        said
    });

    let outcome = transfer_core::execute_transfer(
        from,
        into,
        vec![transfer_core::TransferItem {
            src_path: src_path.to_string(),
            relative_path: name.to_string(),
            name: name.to_string(),
            is_dir: false,
            size,
            permissions: None,
        }],
        dest_parent.to_string(),
        false,
        progress_tx,
        error_tx,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .await;

    complaining.abort();
    let said: Vec<transfer_core::CopyMessage> = progress_rx.into_iter().collect();
    outcome.unwrap_or_else(|why| panic!("{src_path} -> {dest_parent}/{name}: {why}"));

    Carried {
        reports: said
            .iter()
            .filter_map(|m| match m {
                transfer_core::CopyMessage::Progress { file_bytes, .. } => Some(*file_bytes),
                _ => None,
            })
            .collect(),
        files_done: said
            .iter()
            .filter(|m| matches!(m, transfer_core::CopyMessage::FileDone))
            .count(),
    }
}

/// The body every copy carries, big enough to be reported more than once.
fn body() -> Vec<u8> {
    (0..3_000_000u32).map(|n| (n % 251) as u8).collect()
}

#[test]
#[ignore = "needs the servers: tests/servers/run-with-servers.sh"]
fn a_file_crosses_every_way_round() {
    let reachable = Reachable::found();
    assert!(
        !reachable.kinds.is_empty(),
        "no servers were started: run this through tests/servers/run-with-servers.sh"
    );
    let config = load_the_plugins();
    let _ = &config;
    let landing: std::path::PathBuf = told("IC_LANDING").expect("IC_LANDING").into();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();

    local.block_on(&rt, async move {
        let sample = body();
        // sshd is the real thing and cannot be held back here, so only the two written for
        // this test obey IC_SLOW_KBPS.
        let throttled = told("IC_SLOW_KBPS").is_some_and(|kbps| kbps != "0");

        // Each server gets the same file to hand out.
        for place in &reachable.kinds {
            std::fs::create_dir_all(&place.root).unwrap();
            std::fs::write(place.root.join("source.bin"), &sample).unwrap();
        }
        std::fs::create_dir_all(&landing).unwrap();
        std::fs::write(landing.join("source.bin"), &sample).unwrap();

        let mounted = |place: &Place| {
            ic_plugin_host::mount_connection(place.kind, &place.settings)
                .unwrap_or_else(|| panic!("{} would not mount", place.name))
        };

        // Off each server onto this disk.
        for place in &reachable.kinds {
            let into = landing.join(format!("from-{}", place.name));
            std::fs::create_dir_all(&into).unwrap();
            let carried = copy_one(
                mounted(place),
                &format!(
                    "{}source.bin",
                    place.at.trim_end_matches('/').to_string() + "/"
                ),
                local_disk(),
                &into.to_string_lossy(),
                "source.bin",
                sample.len() as u64,
            )
            .await;

            let landed = std::fs::read(into.join("source.bin")).expect("it arrived");
            assert_eq!(landed, sample, "{} -> disk changed the bytes", place.name);
            assert_eq!(carried.files_done, 1);
            // Only where the server was told to be slow: an instant one finishes inside a
            // single reporting interval and there is nothing to see.
            if throttled && place.name != "sftp" {
                assert!(
                    carried.reports.len() > 3,
                    "{} is slow and should be reported as it goes, got {:?}",
                    place.name,
                    carried.reports
                );
            }
            eprintln!(
                "  {:>7} → disk      {} reports, {} bytes",
                place.name,
                carried.reports.len(),
                landed.len()
            );
        }

        // Off this disk onto each server.
        for place in &reachable.kinds {
            let carried = copy_one(
                local_disk(),
                &landing.join("source.bin").to_string_lossy(),
                mounted(place),
                place.at.trim_end_matches('/'),
                "uploaded.bin",
                sample.len() as u64,
            )
            .await;

            let landed = std::fs::read(place.root.join("uploaded.bin")).expect("it arrived");
            assert_eq!(landed, sample, "disk -> {} changed the bytes", place.name);
            assert_eq!(carried.files_done, 1);
            eprintln!(
                "  {:>7} ← disk      {} reports, {} bytes",
                place.name,
                carried.reports.len(),
                landed.len()
            );
        }

        // And between every pair of servers, both ways round.
        for from in &reachable.kinds {
            for into in &reachable.kinds {
                if from.name == into.name {
                    continue;
                }
                let name = format!("{}-to-{}.bin", from.name, into.name);
                let carried = copy_one(
                    mounted(from),
                    &format!("{}/source.bin", from.at.trim_end_matches('/')),
                    mounted(into),
                    into.at.trim_end_matches('/'),
                    &name,
                    sample.len() as u64,
                )
                .await;

                let landed = std::fs::read(into.root.join(&name)).expect("it arrived");
                assert_eq!(
                    landed, sample,
                    "{} -> {} changed the bytes",
                    from.name, into.name
                );
                assert_eq!(carried.files_done, 1);
                eprintln!(
                    "  {:>7} → {:<9} {} reports, {} bytes",
                    from.name,
                    into.name,
                    carried.reports.len(),
                    landed.len()
                );
            }
        }
    });
}
