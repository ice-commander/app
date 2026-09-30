//! Its own process: what the registries end up holding is global state.

#[test]
#[ignore = "needs deployed plugins: run ./build.sh && ./deploy-local.sh in the plugins repo, then cargo test -p ic-plugin-host --test first_run -- --ignored --nocapture"]
fn a_first_run_loads_nothing_at_all() {
    assert!(
        !ic_plugin_host::loader::installed().is_empty(),
        "this check needs plugins deployed"
    );

    let config = client_config::AppConfig::new("ice-commander-first-run-probe");
    config.forget(ic_plugin_host::loader::ENABLED_KEY);
    assert!(!ic_plugin_host::loader::has_chosen(&config));

    let attempts = ic_plugin_host::loader::load_for(&config);
    assert!(
        attempts.is_empty(),
        "a configuration that has chosen nothing must open nothing"
    );
    assert!(
        ic_plugin_host::connection_kind_ids().is_empty(),
        "so no connection kind exists"
    );
    assert!(
        ic_plugin_host::view_ids().is_empty(),
        "and no plugin window either"
    );
    assert!(
        !fm_core::plugin_fs::handles_extension("holiday.zip"),
        "and no archive can be entered"
    );
    eprintln!(
        "first run: {} libraries present, none loaded",
        ic_plugin_host::loader::installed().len()
    );
}
