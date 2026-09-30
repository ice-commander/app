#[test]
fn the_readme_path_is_the_one_the_loader_uses() {
    let folder = ic_plugin_host::loader::plugin_directory().expect("a data directory");
    eprintln!("loader: {}", folder.display());
    let expected = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let mut p = std::path::PathBuf::from(std::env::var("HOME").expect("HOME"));
            p.push(".local/share");
            p
        })
        .join("ice-commander")
        .join("plugins");
    assert_eq!(folder, expected, "the README documents a different folder");

    std::env::set_var("IC_PLUGIN_DIR", "/tmp/elsewhere");
    assert_eq!(
        ic_plugin_host::loader::plugin_directory(),
        Some(std::path::PathBuf::from("/tmp/elsewhere")),
        "IC_PLUGIN_DIR must override, as the README says"
    );
    std::env::remove_var("IC_PLUGIN_DIR");
}
