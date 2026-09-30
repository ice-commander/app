#[test]
#[ignore = "needs deployed plugins: run ./build.sh && ./deploy-local.sh in the plugins repo, then cargo test -p ic-plugin-host --test deployed_about -- --ignored --nocapture"]
fn every_deployed_plugin_says_what_it_is() {
    let present = ic_plugin_host::loader::installed();
    assert!(!present.is_empty(), "no plugin libraries deployed");
    for path in &present {
        let name = ic_plugin_host::loader::name_of_library(path);
        let about = ic_plugin_host::loader::inspect(path);
        match about {
            Some(about) => eprintln!(
                "{name:<20} id={:<20} version={:<8} name={:<32} desc={}",
                about.id, about.version, about.name, about.description
            ),
            None => eprintln!("{name:<20} (says nothing)"),
        }
    }
    let silent: Vec<String> = present
        .iter()
        .filter(|path| ic_plugin_host::loader::inspect(path).is_none())
        .map(|path| ic_plugin_host::loader::name_of_library(path))
        .collect();
    assert!(
        silent.is_empty(),
        "these do not export ic_plugin_about: {silent:?}"
    );
}

/// Reads the two single-purpose exports on their own, the way a host that knows
/// nothing about `IcAbout` would.
#[test]
#[ignore = "needs deployed plugins: run ./build.sh && ./deploy-local.sh in the plugins repo, then cargo test -p ic-plugin-host --test deployed_about -- --ignored --nocapture"]
fn the_simple_exports_answer_without_the_struct() {
    let present = ic_plugin_host::loader::installed();
    assert!(!present.is_empty(), "no plugin libraries deployed");
    for path in &present {
        let library = unsafe { libloading::Library::new(path) }.expect("opens");
        let read = |symbol: &[u8]| -> String {
            let export = unsafe { library.get::<ic_plugin_api::IcPluginText>(symbol) }
                .expect("the plugin exports it");
            let pointer = export();
            assert!(
                !pointer.is_null(),
                "an export that answers nothing is useless"
            );
            unsafe { std::ffi::CStr::from_ptr(pointer) }
                .to_string_lossy()
                .into_owned()
        };
        let name = read(ic_plugin_api::IC_PLUGIN_NAME_SYMBOL);
        let version = read(ic_plugin_api::IC_PLUGIN_VERSION_SYMBOL);
        eprintln!(
            "{:<22} name={name:<32} version={version}",
            ic_plugin_host::loader::name_of_library(path)
        );
        assert!(!name.is_empty());
        assert!(
            version
                .split('.')
                .next()
                .and_then(|part| part.parse::<u64>().ok())
                .is_some(),
            "{version:?} does not start with a number"
        );
    }
}
