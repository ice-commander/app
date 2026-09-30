use ic_plugin_api::{
    IcAbout, IcHost, IcPluginAbout, IcPluginInit, IcPluginShutdown, IcPluginText, IC_OK,
    IC_PLUGIN_ABOUT_SYMBOL, IC_PLUGIN_INIT_SYMBOL, IC_PLUGIN_NAME_SYMBOL,
    IC_PLUGIN_SHUTDOWN_SYMBOL, IC_PLUGIN_VERSION_SYMBOL,
};
use std::path::{Path, PathBuf};

pub const DIRECTORY_OVERRIDE: &str = "IC_PLUGIN_DIR";

/// The plugins the user switched on, by library name. Nothing outside this list
/// is opened at start-up, so a plugin left out is genuinely absent — and an
/// absent key means the user has not chosen yet, which is no plugins at all.
pub const ENABLED_KEY: &str = "plugins.enabled";

/// What a plugin says about itself, copied out of its own memory so the answer
/// outlives the library it came from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct About {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// The contract the plugin says it was built against. `None` for one that
    /// introduces itself the old way, with plain name and version exports and
    /// no `IcAbout` — there is nothing to check then, and nothing it can call
    /// that would go wrong.
    pub abi_version: Option<u32>,
}

/// A plugin library present in the folder, with whatever it says about itself.
#[derive(Debug, Clone)]
pub struct Installed {
    pub path: PathBuf,
    pub library: String,
    pub about: Option<About>,
}

/// Which of the libraries in a folder may be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chosen<'a> {
    /// Every library found. Used by tests and by a host with no settings.
    All,
    /// Only these library names. An empty slice means none.
    Only(&'a [String]),
}

impl Chosen<'_> {
    fn admits(&self, library: &str) -> bool {
        match self {
            Chosen::All => true,
            Chosen::Only(allowed) => allowed.iter().any(|held| held == library),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Loaded,
    NotALibrary(String),
    NoEntryPoint,
    Refused(i32),
    /// The plugin said this is not the application it is for.
    NotForThisHost,
    /// Built against a different contract from this application's.
    ///
    /// Not called at all, rather than called and hoped for: a slot that moved
    /// is in the same place and the same size as before, so the call goes
    /// through with the wrong signature and takes the application down with
    /// it. Nothing later would catch that.
    WrongAbi {
        built_for: u32,
        ours: u32,
    },
}

#[derive(Debug, Clone)]
pub struct Attempt {
    pub name: String,
    pub path: PathBuf,
    pub outcome: Outcome,
    /// Read before init, so it is there even when the plugin then refused.
    pub about: Option<About>,
}

impl Attempt {
    pub fn loaded(&self) -> bool {
        self.outcome == Outcome::Loaded
    }

    /// A plugin that said it is not for this application did what it was asked to.
    pub fn went_wrong(&self) -> bool {
        !matches!(self.outcome, Outcome::Loaded | Outcome::NotForThisHost)
    }
}

pub fn library_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

pub fn plugin_directory() -> Option<PathBuf> {
    if let Some(chosen) = std::env::var_os(DIRECTORY_OVERRIDE) {
        let path = PathBuf::from(chosen);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }
    let mut path = dirs::data_dir()?;
    path.push("ice-commander");
    path.push("plugins");
    Some(path)
}

pub fn libraries_in(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let wanted = library_extension();
    let mut found: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case(wanted))
                .unwrap_or(false)
        })
        .collect();
    found.sort();
    found
}

fn held_libraries() -> &'static std::sync::Mutex<Vec<libloading::Library>> {
    static HELD: std::sync::OnceLock<std::sync::Mutex<Vec<libloading::Library>>> =
        std::sync::OnceLock::new();
    HELD.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

pub fn held_count() -> usize {
    match held_libraries().lock() {
        Ok(held) => held.len(),
        Err(poisoned) => poisoned.into_inner().len(),
    }
}

fn shutdowns() -> &'static std::sync::Mutex<Vec<(String, IcPluginShutdown)>> {
    static HELD: std::sync::OnceLock<std::sync::Mutex<Vec<(String, IcPluginShutdown)>>> =
        std::sync::OnceLock::new();
    HELD.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Tells every loaded plugin that the application is going away, in the order
/// they were loaded. A plugin that exports nothing is simply skipped.
pub fn shutdown_plugins() {
    let held = match shutdowns().lock() {
        Ok(mut held) => std::mem::take(&mut *held),
        Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
    };
    for (name, stop) in held {
        ic_logging::info!("plugin {name} is being told to stop");
        stop();
    }
}

pub fn shutdown_count() -> usize {
    match shutdowns().lock() {
        Ok(held) => held.len(),
        Err(poisoned) => poisoned.into_inner().len(),
    }
}

fn name_of(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("plugin");
    if cfg!(target_os = "windows") {
        return stem.to_string();
    }
    stem.strip_prefix("lib").unwrap_or(stem).to_string()
}

fn text_at(about: *const IcAbout, offset: usize) -> String {
    let Some(pointer) = ic_plugin_api::about_field::<*const std::os::raw::c_char>(about, offset)
    else {
        return String::new();
    };
    if pointer.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

fn structured(library: &libloading::Library) -> Option<About> {
    let export = unsafe { library.get::<IcPluginAbout>(IC_PLUGIN_ABOUT_SYMBOL) }.ok()?;
    let about = export();
    if about.is_null() {
        return None;
    }
    Some(About {
        id: text_at(about, std::mem::offset_of!(IcAbout, id)),
        name: text_at(about, std::mem::offset_of!(IcAbout, name)),
        version: text_at(about, std::mem::offset_of!(IcAbout, version)),
        description: text_at(about, std::mem::offset_of!(IcAbout, description)),
        abi_version: ic_plugin_api::about_field::<u32>(
            about,
            std::mem::offset_of!(IcAbout, abi_version),
        ),
    })
}

fn one_text(library: &libloading::Library, symbol: &[u8]) -> Option<String> {
    let export = unsafe { library.get::<IcPluginText>(symbol) }.ok()?;
    let pointer = export();
    if pointer.is_null() {
        return None;
    }
    let held = unsafe { std::ffi::CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    (!held.is_empty()).then_some(held)
}

/// Folds the three ways a plugin may introduce itself into one answer. The
/// structured `about` wins; the single-purpose exports fill what it left empty;
/// the library's own file name is the last resort for the id.
fn merged(
    about: Option<About>,
    name: Option<String>,
    version: Option<String>,
    fallback: &str,
) -> Option<About> {
    if about.is_none() && name.is_none() && version.is_none() {
        return None;
    }
    let mut held = about.unwrap_or_default();
    if held.name.is_empty() {
        held.name = name.unwrap_or_default();
    }
    if held.version.is_empty() {
        held.version = version.unwrap_or_default();
    }
    if held.id.is_empty() {
        held.id = fallback.to_string();
    }
    Some(held)
}

/// Whether this plugin speaks another contract, and so must not be called.
///
/// A slot that changed meaning is in the same place and of the same size as
/// before, so the call goes through with the wrong signature and takes the
/// application down with it. Nothing later would catch that, which is why the
/// question is asked before `init` rather than after.
fn wrong_contract(about: &Option<About>) -> Option<Outcome> {
    let built_for = about.as_ref()?.abi_version?;
    (built_for != ic_plugin_api::IC_ABI_VERSION).then_some(Outcome::WrongAbi {
        built_for,
        ours: ic_plugin_api::IC_ABI_VERSION,
    })
}

/// Asks an already-open library what it is. Never calls init.
fn about_in(library: &libloading::Library, fallback: &str) -> Option<About> {
    merged(
        structured(library),
        one_text(library, IC_PLUGIN_NAME_SYMBOL),
        one_text(library, IC_PLUGIN_VERSION_SYMBOL),
        fallback,
    )
}

/// Opens a library only to read its `about`, then closes it again. Used for the
/// plugins the user switched off, which are otherwise never opened.
pub fn inspect(path: &Path) -> Option<About> {
    let library = unsafe { libloading::Library::new(path) }.ok()?;
    let found = about_in(&library, &name_of(path));
    drop(library);
    found
}

pub fn load_library(path: &Path) -> Attempt {
    let name = name_of(path);
    let library = match unsafe { libloading::Library::new(path) } {
        Ok(library) => library,
        Err(reason) => {
            return Attempt {
                name,
                path: path.to_path_buf(),
                outcome: Outcome::NotALibrary(reason.to_string()),
                about: None,
            }
        }
    };
    let about = about_in(&library, &name);
    if let Some(outcome) = wrong_contract(&about) {
        return Attempt {
            name,
            path: path.to_path_buf(),
            outcome,
            about,
        };
    }
    let entry = unsafe { library.get::<IcPluginInit>(IC_PLUGIN_INIT_SYMBOL) };
    let Ok(entry) = entry else {
        return Attempt {
            name,
            path: path.to_path_buf(),
            outcome: Outcome::NoEntryPoint,
            about,
        };
    };
    let kind = std::ffi::CString::new(crate::host_kind()).unwrap_or_default();
    let code = entry(crate::host_ref() as *const IcHost, kind.as_ptr());
    let outcome = if code == IC_OK {
        Outcome::Loaded
    } else if code == ic_plugin_api::IC_ERR_NOT_THIS_HOST {
        Outcome::NotForThisHost
    } else {
        Outcome::Refused(code)
    };
    if outcome == Outcome::Loaded {
        if let Ok(stop) = unsafe { library.get::<IcPluginShutdown>(IC_PLUGIN_SHUTDOWN_SYMBOL) } {
            let stop = *stop;
            match shutdowns().lock() {
                Ok(mut held) => held.push((name.clone(), stop)),
                Err(poisoned) => poisoned.into_inner().push((name.clone(), stop)),
            }
        }
        match held_libraries().lock() {
            Ok(mut held) => held.push(library),
            Err(poisoned) => poisoned.into_inner().push(library),
        }
    }
    Attempt {
        name,
        path: path.to_path_buf(),
        outcome,
        about,
    }
}

pub fn load_from_directory(folder: &Path) -> Vec<Attempt> {
    load_chosen_from(folder, Chosen::All)
}

/// Loads the libraries `chosen` admits. One left out is not opened at all, so
/// nothing of it runs and nothing of it is registered.
pub fn load_chosen_from(folder: &Path, chosen: Chosen) -> Vec<Attempt> {
    libraries_in(folder)
        .iter()
        .filter(|path| {
            let name = name_of(path);
            let on = chosen.admits(&name);
            if !on {
                ic_logging::info!("plugin {name} is switched off and was not opened");
            }
            on
        })
        .map(|path| {
            let attempt = load_library(path);
            match &attempt.outcome {
                Outcome::Loaded => {
                    ic_logging::info!(
                        "plugin {} loaded from {}",
                        attempt.name,
                        attempt.path.display()
                    )
                }
                Outcome::NotALibrary(reason) => ic_logging::warn!(
                    "plugin {} could not be opened: {reason}",
                    attempt.path.display()
                ),
                Outcome::NoEntryPoint => ic_logging::warn!(
                    "plugin {} exports no {} symbol",
                    attempt.path.display(),
                    String::from_utf8_lossy(IC_PLUGIN_INIT_SYMBOL)
                ),
                Outcome::Refused(code) => {
                    ic_logging::warn!("plugin {} refused to initialise, code {code}", attempt.name)
                }
                Outcome::NotForThisHost => ic_logging::debug!(
                    "plugin {} is not for this application, and said so",
                    attempt.name
                ),
                Outcome::WrongAbi { built_for, ours } => ic_logging::warn!(
                    "plugin {} was built for contract {built_for} and this application \
                     speaks {ours}; it was not called at all",
                    attempt.name
                ),
            }
            attempt
        })
        .collect()
}

pub fn enabled(config: &client_config::AppConfig) -> Vec<String> {
    config.get::<Vec<String>>(ENABLED_KEY).unwrap_or_default()
}

/// Whether the user has ever said which plugins to load. Until then the answer
/// is none, and the setup wizard is what asks.
pub fn has_chosen(config: &client_config::AppConfig) -> bool {
    config.get::<Vec<String>>(ENABLED_KEY).is_some()
}

pub fn is_enabled(config: &client_config::AppConfig, library: &str) -> bool {
    enabled(config).iter().any(|held| held == library)
}

/// Switches a plugin on or off for the next start. Nothing changes in this run.
pub fn set_enabled(config: &client_config::AppConfig, library: &str, on: bool) {
    let mut held = enabled(config);
    let present = held.iter().position(|name| name == library);
    match (on, present) {
        (true, None) => held.push(library.to_string()),
        (false, Some(at)) => {
            held.remove(at);
        }
        _ => {
            // Still write it out: an absent key and an empty list differ.
            held.sort();
            config.set(ENABLED_KEY, held);
            config.save();
            return;
        }
    }
    held.sort();
    config.set(ENABLED_KEY, held);
    config.save();
}

/// Records the whole choice at once, which is what the wizard does.
pub fn set_chosen(config: &client_config::AppConfig, libraries: &[String]) {
    let mut held = libraries.to_vec();
    held.sort();
    held.dedup();
    config.set(ENABLED_KEY, held);
    config.save();
}

/// What the frontends call at start-up: only what the user switched on.
pub fn load_for(config: &client_config::AppConfig) -> Vec<Attempt> {
    let allowed = enabled(config);
    load_installed_chosen(Chosen::Only(&allowed))
}

pub fn load_installed_plugins() -> Vec<Attempt> {
    load_installed_chosen(Chosen::All)
}

pub fn load_installed_chosen(chosen: Chosen) -> Vec<Attempt> {
    match plugin_directory() {
        Some(folder) => load_chosen_from(&folder, chosen),
        None => Vec::new(),
    }
}

/// The library name the disabled list and the settings page key off.
pub fn name_of_library(path: &Path) -> String {
    name_of(path)
}

/// Every plugin library present, whether it is switched on or not.
pub fn installed() -> Vec<PathBuf> {
    match plugin_directory() {
        Some(folder) => libraries_in(&folder),
        None => Vec::new(),
    }
}

/// The same list, each library asked what it is. Opens and closes every one, so
/// it belongs to a page the user asked for, not to start-up.
pub fn installed_with_about() -> Vec<Installed> {
    installed()
        .into_iter()
        .map(|path| Installed {
            library: name_of(&path),
            about: inspect(&path),
            path,
        })
        .collect()
}

/// Deletes a plugin from the folder. It stays loaded until the next start.
pub fn remove(path: &Path) -> Result<(), String> {
    let inside = plugin_directory()
        .map(|folder| path.starts_with(&folder))
        .unwrap_or(false);
    if !inside {
        return Err(format!("{} is not in the plugin folder", path.display()));
    }
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_can_be_pointed_somewhere_else_for_a_run() {
        let chosen = std::env::temp_dir().join("ic-plugins-probe");
        std::env::set_var(DIRECTORY_OVERRIDE, &chosen);
        assert_eq!(plugin_directory(), Some(chosen));
        std::env::remove_var(DIRECTORY_OVERRIDE);
        let settled = plugin_directory().expect("a data directory");
        assert!(settled.ends_with("ice-commander/plugins"), "{settled:?}");
    }

    #[test]
    fn an_empty_override_is_ignored_rather_than_used_as_a_path() {
        std::env::set_var(DIRECTORY_OVERRIDE, "");
        let settled = plugin_directory().expect("a data directory");
        assert!(settled.ends_with("ice-commander/plugins"), "{settled:?}");
        std::env::remove_var(DIRECTORY_OVERRIDE);
    }

    #[test]
    fn a_folder_that_is_not_there_yields_nothing_rather_than_an_error() {
        let missing = std::env::temp_dir().join("ic-plugins-absent-folder");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(libraries_in(&missing).is_empty());
        assert!(load_from_directory(&missing).is_empty());
    }

    #[test]
    fn only_libraries_are_offered_and_always_in_the_same_order() {
        let folder = std::env::temp_dir().join("ic-plugins-order-probe");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a scratch folder");
        let extension = library_extension();
        for name in [
            format!("libzebra.{extension}"),
            format!("libalpha.{extension}"),
            "readme.txt".to_string(),
            "libnotes.json".to_string(),
        ] {
            std::fs::write(folder.join(name), b"not really a library").expect("a file");
        }
        let found = libraries_in(&folder);
        let names: Vec<String> = found.iter().map(|path| name_of(path)).collect();
        assert_eq!(
            names,
            vec!["alpha".to_string(), "zebra".to_string()],
            "only libraries, sorted"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_file_that_only_pretends_to_be_a_library_is_reported_not_fatal() {
        let folder = std::env::temp_dir().join("ic-plugins-rubbish-probe");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a scratch folder");
        let fake = folder.join(format!("libnonsense.{}", library_extension()));
        std::fs::write(&fake, b"this is not an elf file").expect("a file");
        let before = held_count();
        let attempts = load_from_directory(&folder);
        assert_eq!(attempts.len(), 1);
        assert!(!attempts[0].loaded());
        assert!(matches!(attempts[0].outcome, Outcome::NotALibrary(_)));
        assert_eq!(
            held_count(),
            before,
            "a library that failed to load is not held open"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    #[ignore = "needs a deployed plugin: run ./build.sh && ./deploy-local.sh in the plugins repo, then cargo test -p ic-plugin-host -- --ignored"]
    fn a_deployed_plugin_registers_itself_through_the_loader() {
        let folder = plugin_directory().expect("a data directory");
        let attempts = load_from_directory(&folder);
        assert!(
            !attempts.is_empty(),
            "no plugin libraries in {}",
            folder.display()
        );
        for attempt in &attempts {
            assert!(attempt.loaded(), "{attempt:?}");
        }
        assert!(
            held_count() >= attempts.len(),
            "a loaded library must stay open for the process lifetime"
        );
        let registered = !crate::view_ids().is_empty() || !crate::connection_kind_ids().is_empty();
        assert!(
            registered,
            "a loaded plugin registered nothing with the host"
        );
        eprintln!(
            "loaded {} plugin(s); views: {:?}; connection kinds: {:?}; panel sources: {}",
            attempts.len(),
            crate::view_ids(),
            crate::connection_kind_ids(),
            crate::with_views(|views| views.len())
        );
    }

    #[test]
    fn a_plugin_that_only_states_a_name_and_a_version_is_enough() {
        let held = merged(
            None,
            Some("Node In Net".to_string()),
            Some("0.8.0".to_string()),
            "ic_node_in_net",
        )
        .expect("two facts are an answer");
        assert_eq!(held.name, "Node In Net");
        assert_eq!(held.version, "0.8.0");
        assert_eq!(
            held.id, "ic_node_in_net",
            "the file name stands in for an id"
        );
        assert!(held.description.is_empty());
    }

    #[test]
    fn the_structured_answer_wins_and_the_simple_ones_fill_the_gaps() {
        let full = About {
            id: "ic-sftp-fs".to_string(),
            name: "SFTP".to_string(),
            version: "0.1.0".to_string(),
            description: "SFTP connections".to_string(),
            abi_version: Some(ic_plugin_api::IC_ABI_VERSION),
        };
        let held = merged(
            Some(full.clone()),
            Some("something else".to_string()),
            Some("9.9.9".to_string()),
            "ic_sftp_fs",
        )
        .expect("an answer");
        assert_eq!(held, full, "what the struct states is not overridden");

        let sparse = About {
            id: "ic-quiet".to_string(),
            ..About::default()
        };
        let filled = merged(
            Some(sparse),
            Some("Quiet".to_string()),
            Some("0.2.0".to_string()),
            "ic_quiet",
        )
        .expect("an answer");
        assert_eq!(filled.id, "ic-quiet");
        assert_eq!(filled.name, "Quiet");
        assert_eq!(filled.version, "0.2.0");
    }

    /// A plugin built against another contract is turned away before it is
    /// asked anything. A plugin that never said which contract it was built
    /// against is from before the question existed, and is let through.
    #[test]
    fn a_plugin_from_another_contract_is_not_called_at_all() {
        let ours = ic_plugin_api::IC_ABI_VERSION;
        let built_for = |version: Option<u32>| {
            Some(About {
                id: "ic-probe".to_string(),
                abi_version: version,
                ..About::default()
            })
        };
        assert_eq!(wrong_contract(&built_for(Some(ours))), None);
        assert_eq!(
            wrong_contract(&built_for(Some(ours - 1))),
            Some(Outcome::WrongAbi {
                built_for: ours - 1,
                ours
            })
        );
        assert_eq!(
            wrong_contract(&built_for(Some(ours + 1))),
            Some(Outcome::WrongAbi {
                built_for: ours + 1,
                ours
            }),
            "a plugin from a later contract is refused the same way"
        );
        assert_eq!(wrong_contract(&built_for(None)), None);
        assert_eq!(wrong_contract(&None), None);
    }

    #[test]
    fn a_plugin_that_states_nothing_at_all_reports_nothing() {
        assert_eq!(merged(None, None, None, "ic_silent"), None);
    }

    fn two_fake_libraries(folder: &Path) {
        let _ = std::fs::remove_dir_all(folder);
        std::fs::create_dir_all(folder).expect("a scratch folder");
        let extension = library_extension();
        for name in ["libalpha", "libomega"] {
            std::fs::write(folder.join(format!("{name}.{extension}")), b"not a library")
                .expect("a file");
        }
    }

    #[test]
    fn only_the_chosen_libraries_are_opened() {
        let folder = std::env::temp_dir().join("ic-plugins-chosen-probe");
        two_fake_libraries(&folder);

        let all = load_chosen_from(&folder, Chosen::All);
        assert_eq!(all.len(), 2, "Chosen::All means every library found");

        let one = load_chosen_from(&folder, Chosen::Only(&["alpha".to_string()]));
        assert_eq!(one.len(), 1, "a plugin left out yields no attempt at all");
        assert_eq!(one[0].name, "alpha");

        let none = load_chosen_from(&folder, Chosen::Only(&[]));
        assert!(
            none.is_empty(),
            "an empty choice is none, never a shorthand for all"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn nothing_is_loaded_until_the_user_has_chosen() {
        let config = client_config::AppConfig::new("ice-commander-plugins-firstrun");
        config.forget(ENABLED_KEY);
        assert!(
            !has_chosen(&config),
            "a fresh configuration has made no choice"
        );
        assert!(enabled(&config).is_empty());
        assert!(!is_enabled(&config, "ic_sftp_fs"));

        // choosing nothing is still a choice, and is remembered as one
        set_chosen(&config, &[]);
        assert!(has_chosen(&config));
        assert!(enabled(&config).is_empty());
    }

    #[test]
    fn the_switch_is_remembered_and_is_what_decides_the_next_start() {
        let config = client_config::AppConfig::new("ice-commander-plugins-switch");
        set_chosen(&config, &[]);
        assert!(!is_enabled(&config, "ic_sftp_fs"));

        set_enabled(&config, "ic_sftp_fs", true);
        assert!(is_enabled(&config, "ic_sftp_fs"));
        assert_eq!(enabled(&config), vec!["ic_sftp_fs".to_string()]);

        set_enabled(&config, "ic_sftp_fs", true);
        assert_eq!(
            enabled(&config),
            vec!["ic_sftp_fs".to_string()],
            "switching it on twice does not list it twice"
        );

        set_enabled(&config, "ic_ftp_fs", true);
        assert_eq!(
            enabled(&config),
            vec!["ic_ftp_fs".to_string(), "ic_sftp_fs".to_string()]
        );

        set_enabled(&config, "ic_sftp_fs", false);
        assert_eq!(enabled(&config), vec!["ic_ftp_fs".to_string()]);
        assert!(!is_enabled(&config, "ic_sftp_fs"));
    }

    #[test]
    fn a_whole_choice_is_recorded_sorted_and_without_repeats() {
        let config = client_config::AppConfig::new("ice-commander-plugins-chosen");
        set_chosen(
            &config,
            &[
                "ic_sftp_fs".to_string(),
                "ic_ftp_fs".to_string(),
                "ic_sftp_fs".to_string(),
            ],
        );
        assert_eq!(
            enabled(&config),
            vec!["ic_ftp_fs".to_string(), "ic_sftp_fs".to_string()]
        );
    }

    #[test]
    fn a_library_outside_the_plugin_folder_is_not_deleted() {
        let outside = std::env::temp_dir().join("ic-not-a-plugin.so");
        std::fs::write(&outside, b"x").expect("a file");
        assert!(remove(&outside).is_err());
        assert!(outside.exists(), "nothing outside the folder is touched");
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn a_library_that_says_nothing_about_itself_still_loads() {
        let folder = std::env::temp_dir().join("ic-plugins-noabout-probe");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a scratch folder");
        let fake = folder.join(format!("libquiet.{}", library_extension()));
        std::fs::write(&fake, b"not an elf file").expect("a file");
        let attempts = load_from_directory(&folder);
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].about, None,
            "a library that cannot even be opened reports nothing"
        );
        assert!(inspect(&fake).is_none());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_name_of_a_plugin_drops_the_library_prefix_and_extension() {
        assert_eq!(
            name_of(Path::new("/tmp/libic_sysinfo_dlg.so")),
            "ic_sysinfo_dlg"
        );
        assert_eq!(
            name_of(Path::new("/tmp/libic_sysinfo_dlg.dylib")),
            "ic_sysinfo_dlg"
        );
        assert_eq!(
            name_of(Path::new("/tmp/ic_sysinfo_dlg.dll")),
            "ic_sysinfo_dlg"
        );
    }

    #[test]
    fn only_the_prefix_the_linker_adds_is_dropped_never_the_plugins_own_letters() {
        if cfg!(target_os = "windows") {
            assert_eq!(name_of(Path::new("/tmp/library.dll")), "library");
            assert_eq!(name_of(Path::new("/tmp/libarchive.dll")), "libarchive");
        } else {
            assert_eq!(name_of(Path::new("/tmp/liblibrary.so")), "library");
            assert_eq!(name_of(Path::new("/tmp/liblibarchive.so")), "libarchive");
        }
    }

    #[test]
    fn the_extension_the_loader_looks_for_is_the_one_this_platform_builds() {
        let expected = if cfg!(target_os = "windows") {
            "dll"
        } else if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        assert_eq!(library_extension(), expected);
    }

    #[test]
    fn a_library_is_recognised_however_its_extension_is_spelled() {
        let folder = std::env::temp_dir().join("ic-plugins-case-probe");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a scratch folder");
        let shouted = library_extension().to_uppercase();
        std::fs::write(folder.join(format!("ic_shouting.{shouted}")), b"x").expect("a file");
        assert_eq!(
            libraries_in(&folder).len(),
            1,
            "windows writes DLL as often as dll"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }
}
