//! A copy that stays on one WebDAV server should be done by the server, not by dragging the
//! bytes here and back.
//!
//!   IC_PLUGIN_DIR=<folder with libic_webdav_fs.so> IC_DAV_URL=http://127.0.0.1:8088 \
//!   cargo test -p ic-plugin-host --test webdav_copy_within -- --ignored --nocapture

use std::rc::Rc;

#[test]
#[ignore = "needs a WebDAV server at IC_DAV_URL"]
fn the_server_copies_its_own_file() {
    let url = std::env::var("IC_DAV_URL").expect("IC_DAV_URL is not set");
    let config = client_config::AppConfig::new("ice-commander-dav-copy");
    let chosen: Vec<String> = ic_plugin_host::loader::installed()
        .iter()
        .map(|p| ic_plugin_host::loader::name_of_library(p))
        .filter(|n| n.contains("webdav"))
        .collect();
    assert_eq!(
        chosen.len(),
        1,
        "expected the webdav plugin, found {chosen:?}"
    );
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
        let settings: std::collections::BTreeMap<String, String> =
            [("url".to_string(), url)].into_iter().collect();
        let server: Rc<dyn fm_core::rpc::FileSystemRpc> =
            ic_plugin_host::mount_connection("webdav", &settings).expect("mounted");

        let listed = fm_core::rpc::FileSystemRpc::list_dir(&*server, "/".to_string())
            .await
            .expect("the server lists its root");
        let file = listed
            .iter()
            .filter(|e| !e.is_dir && !e.name.starts_with("copy-of-"))
            .max_by_key(|e| e.size)
            .expect("a file on the server");

        let made = format!("/copy-of-{}", file.name);
        let started = std::time::Instant::now();
        let done = server
            .copy_within(format!("/{}", file.name), made.clone())
            .await;
        let took = started.elapsed();
        assert!(done, "the server refused to copy its own file");

        let after = fm_core::rpc::FileSystemRpc::list_dir(&*server, "/".to_string())
            .await
            .expect("listed again");
        let copy = after
            .iter()
            .find(|e| made.ends_with(&e.name))
            .expect("the copy is there");
        assert_eq!(copy.size, file.size, "and it is the same size");

        eprintln!(
            "  {} байт скопированы сервером за {:?}, байты не ходили через приложение",
            file.size, took
        );

        server.delete_entries(vec![made]).await.ok();
    });
}
