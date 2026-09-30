//! Runs in its own process, because the registries a plugin fills are global.

#[test]
#[ignore = "needs deployed plugins: run ./build.sh && ./deploy-local.sh in the plugins repo, then cargo test -p ic-plugin-host --test disabled_is_absent -- --ignored --nocapture"]
fn only_the_plugins_switched_on_bring_their_functionality() {
    let names: Vec<String> = ic_plugin_host::loader::installed()
        .iter()
        .map(|path| ic_plugin_host::loader::name_of_library(path))
        .collect();
    for wanted in ["ic_ftp_fs", "ic_sftp_fs"] {
        assert!(
            names.iter().any(|name| name == wanted),
            "this check needs {wanted} deployed; found {names:?}"
        );
    }

    let config = client_config::AppConfig::new("ice-commander-chosen-probe");
    // everything except ftp
    let chosen: Vec<String> = names
        .iter()
        .filter(|name| *name != "ic_ftp_fs")
        .cloned()
        .collect();
    ic_plugin_host::loader::set_chosen(&config, &chosen);

    let attempts = ic_plugin_host::loader::load_for(&config);
    let tried: Vec<&str> = attempts.iter().map(|a| a.name.as_str()).collect();
    assert!(
        !tried.contains(&"ic_ftp_fs"),
        "a plugin left out must not even be opened; tried {tried:?}"
    );
    assert_eq!(tried.len(), names.len() - 1, "everything chosen was loaded");

    let kinds = ic_plugin_host::connection_kind_ids();
    eprintln!("connection kinds with ftp left out: {kinds:?}");
    assert!(
        !kinds.iter().any(|kind| kind.eq_ignore_ascii_case("ftp")),
        "the kind the excluded plugin registers must be gone"
    );
    assert!(
        ic_plugin_host::connection_document("ftp").is_none(),
        "and so must its form"
    );
    assert!(
        kinds.iter().any(|kind| kind.eq_ignore_ascii_case("sftp")),
        "while a plugin that was chosen still works"
    );
}
