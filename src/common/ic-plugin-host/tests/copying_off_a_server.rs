//! Copies a file off a real SFTP server through the real plugin and the real transfer engine,
//! and counts how often the dialog would have been told where the transfer had got to.
//!
//! The whole point of the streaming slots is that the count is more than one. Before them a file
//! was handed over in a single call and the bar had nothing to move with.
//!
//! Run it against a throwaway server:
//!
//!   IC_PLUGIN_DIR=<folder holding libic_sftp_fs.so> \
//!   IC_SFTP_E2E_HOST=127.0.0.1 IC_SFTP_E2E_PORT=2222 IC_SFTP_E2E_USER=$USER \
//!   IC_SFTP_E2E_KEY=<private key> IC_SFTP_E2E_HOME=<the server's folder> \
//!   cargo test -p ic-plugin-host --test copying_off_a_server -- --ignored --nocapture

use std::cell::RefCell;
use std::rc::Rc;

fn need(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("{name} is not set"))
}

#[test]
#[ignore = "needs a reachable SFTP server: see the header of this file"]
fn a_file_off_a_server_is_reported_as_it_arrives() {
    let config = client_config::AppConfig::new("ice-commander-sftp-copy-live");
    let chosen: Vec<String> = ic_plugin_host::loader::installed()
        .iter()
        .map(|path| ic_plugin_host::loader::name_of_library(path))
        .filter(|name| name.contains("sftp"))
        .collect();
    assert_eq!(
        chosen.len(),
        1,
        "expected the sftp plugin, found {chosen:?}"
    );
    ic_plugin_host::loader::set_chosen(&config, &chosen);
    for attempt in ic_plugin_host::loader::load_for(&config) {
        assert!(attempt.loaded(), "{attempt:?}");
    }

    let home = need("IC_SFTP_E2E_HOME");
    let home = home.trim_end_matches('/').to_string();
    let settings: std::collections::BTreeMap<String, String> = [
        ("host", need("IC_SFTP_E2E_HOST")),
        (
            "port",
            std::env::var("IC_SFTP_E2E_PORT").unwrap_or_else(|_| "22".to_string()),
        ),
        ("user", need("IC_SFTP_E2E_USER")),
        ("key_path", need("IC_SFTP_E2E_KEY")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async move {
        let server = ic_plugin_host::mount_connection("sftp", &settings)
            .expect("the plugin mounts the connection");

        let listed = fm_core::rpc::FileSystemRpc::list_dir(&*server, home.clone())
            .await
            .expect("the server lists its home");
        let file = listed
            .iter()
            .filter(|entry| !entry.is_dir)
            .max_by_key(|entry| entry.size)
            .filter(|entry| entry.size > 1024 * 1024)
            .expect("put a file of a megabyte or more in the server's home");

        let landing = tempfile::tempdir().expect("somewhere to copy to");
        let here: Rc<dyn fm_core::rpc::FileSystemRpc> =
            Rc::new(localfs::local_rpc::LocalFileSystemRpc::new(
                client_config::AppConfig::new("ice-commander-sftp-copy-dest"),
            ));

        let items = vec![transfer_core::TransferItem {
            src_path: format!("{home}/{}", file.name),
            relative_path: file.name.clone(),
            name: file.name.clone(),
            is_dir: false,
            size: file.size,
            permissions: file.permissions,
        }];

        let (progress_tx, progress_rx) = std::sync::mpsc::channel();
        let (error_tx, _error_rx) = tokio::sync::mpsc::unbounded_channel();
        let said = Rc::new(RefCell::new(Vec::<u64>::new()));

        // The path the application itself takes: a thread of its own with its own runtime.
        // Calling execute_transfer directly would not prove the dialog ever hears anything.
        let settings_for_src = settings.clone();
        let src_factory: transfer_core::ProviderFactory = Box::new(move || {
            ic_plugin_host::mount_connection("sftp", &settings_for_src)
                .expect("the plugin mounts the connection on its own thread")
        });
        let dest_factory: transfer_core::ProviderFactory = Box::new(|| {
            Rc::new(localfs::local_rpc::LocalFileSystemRpc::new(
                client_config::AppConfig::new("ice-commander-sftp-copy-dest"),
            ))
        });
        drop(server);
        drop(here);

        transfer_core::execute_transfer_offthread(
            src_factory,
            dest_factory,
            items,
            landing.path().to_string_lossy().to_string(),
            false,
            progress_tx,
            error_tx,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .await
        .expect("the copy goes through");

        // The same bookkeeping the dialog does, so the lines below are the lines it would log.
        let mut current_name = String::new();
        let mut current_size = 0u64;
        let mut current_copied = 0u64;
        let mut overall = 0u64;
        let mut done = 0usize;
        let total_bytes = file.size;
        for message in progress_rx.into_iter() {
            match message {
                transfer_core::CopyMessage::FileStart { name, size } => {
                    current_name = name;
                    current_size = size;
                    current_copied = 0;
                }
                transfer_core::CopyMessage::Progress {
                    file_bytes,
                    total_bytes_copied,
                } => {
                    current_copied = file_bytes;
                    overall = total_bytes_copied;
                    said.borrow_mut().push(file_bytes);
                }
                transfer_core::CopyMessage::FileDone => {
                    done += 1;
                    current_copied = current_size;
                }
                transfer_core::CopyMessage::FileSkipped => done += 1,
            }
            eprintln!(
                "copy progress: {current_name} {current_copied}/{current_size} bytes, \
                 overall {overall}/{total_bytes} bytes, {done}/1 files"
            );
        }

        let landed = std::fs::metadata(landing.path().join(&file.name))
            .expect("the file is there afterwards");
        assert_eq!(landed.len(), file.size, "and all of it arrived");

        let said = said.borrow();
        eprintln!(
            "{} ({} bytes): the dialog was told {} times — {:?}",
            file.name,
            file.size,
            said.len(),
            said
        );
        assert!(
            said.len() > 2,
            "a file this size must be reported as it travels, not once at the end: {said:?}"
        );
        for pair in said.windows(2) {
            assert!(pair[1] >= pair[0], "the bar never goes backwards: {said:?}");
        }
    });
}
