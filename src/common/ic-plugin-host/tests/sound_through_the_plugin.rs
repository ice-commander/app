//! The real `ic-videocodecs`, loaded as the application loads it, asked to play a real file.
//!
//! The point is the join, not the decoding: that the plugin claims the formats, that the host
//! picks it over its own playback, and that what comes back through the C boundary is a track
//! playing — a length, a position, and an end that arrives.
//!
//! Skipped unless it is pointed at a build of the plugin and something to play:
//!
//!     IC_SOUND_PLUGIN=plugins/ic-codecs/bin/target/debug/libic_codecs.so \
//!     IC_SOUND_FILE=/some/song.mp3 \
//!     cargo test -p ic-plugin-host --test sound_through_the_plugin -- --ignored --nocapture

use std::ffi::CString;

fn told(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Puts the one plugin where the loader looks and loads it, as the application would.
fn load_the_plugin(library: &std::path::Path) -> client_config::AppConfig {
    let folder = std::env::temp_dir().join(format!("ic-sound-check-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let landed = folder.join(
        library
            .file_name()
            .expect("the plugin is a file with a name"),
    );
    std::fs::copy(library, &landed).expect("the plugin can be copied where the loader looks");
    std::env::set_var(ic_plugin_host::loader::DIRECTORY_OVERRIDE, &folder);

    let config = client_config::AppConfig::new("ice-commander-sound-check");
    let chosen: Vec<String> = ic_plugin_host::loader::installed()
        .iter()
        .map(|path| ic_plugin_host::loader::name_of_library(path))
        .collect();
    assert!(!chosen.is_empty(), "the plugin is where the loader looks");
    ic_plugin_host::loader::set_chosen(&config, &chosen);
    for attempt in ic_plugin_host::loader::load_for(&config) {
        assert!(attempt.loaded(), "{attempt:?}");
    }
    config
}

#[test]
#[ignore = "needs a built plugin and a file: see the top of this file"]
fn the_plugin_takes_the_formats_it_claimed_and_plays_one() {
    let Some(library) = told("IC_SOUND_PLUGIN") else {
        panic!("IC_SOUND_PLUGIN names the built plugin");
    };
    let song = told("IC_SOUND_FILE").expect("IC_SOUND_FILE names something to play");
    let song = std::path::PathBuf::from(song);
    assert!(song.is_file(), "{} is there to play", song.display());

    let _config = load_the_plugin(std::path::Path::new(&library));

    // It claimed the formats, and the host would hand it these rather than play them itself.
    let offered = ic_plugin_host::players_offered();
    assert!(!offered.is_empty(), "the plugin registered a player");
    eprintln!("  registered: {offered:?}");
    for named in ["song.mp3", "song.flac", "song.opus"] {
        assert!(
            ic_plugin_host::player_for(named).is_some(),
            "{named} goes to the plugin"
        );
    }
    assert_eq!(
        ic_plugin_host::player_for("notes.txt"),
        None,
        "and nothing it did not claim does"
    );

    // Now play the file the way the application does: a source on its folder, the name on it.
    let folder = song.parent().expect("a file sits in a folder");
    let name = song.file_name().expect("and has a name").to_string_lossy();
    let id = ic_plugin_host::player_for(&name).expect("this file goes to the plugin");

    let source = fm_core::host_fs::HostSource::rooted_at(folder, "local", "").open();
    let named = CString::new(name.as_ref()).expect("a plain name");

    let played = ic_plugin_host::with_players(|players| {
        let player = players.iter().find(|player| player.id == id)?;
        let open = player.open_fn()?;
        let answer = open(
            source,
            named.as_ptr(),
            player.user_data as *mut std::ffi::c_void,
        );
        Some((answer, player.user_data))
    });
    let Some((answer, user_data)) = played else {
        panic!("the player that claimed the file is in the registry");
    };
    assert_eq!(answer, ic_plugin_api::IC_OK, "the plugin opened the file");

    // Decoding starts on its own thread, so the length arrives a moment later.
    let mut said = None;
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let now = ic_plugin_host::with_players(|players| {
            let player = players.iter().find(|player| player.id == id)?;
            let asked = player.played_so_far_fn()?;
            Some(asked(user_data as *mut std::ffi::c_void))
        })
        .expect("the player is still registered");
        if now.of > 0.0 {
            said = Some(now);
            break;
        }
    }
    let said = said.expect("the plugin says how long the track is");
    eprintln!("  playing: {:.2}s of {:.2}s", said.at, said.of);
    assert!(said.of > 0.0, "a length");
    assert_eq!(said.ended, 0, "and it has not finished the moment it began");

    // Pause is obeyed and reported, which is what the player bar reads back.
    ic_plugin_host::with_players(|players| {
        let player = players.iter().find(|player| player.id == id).unwrap();
        let pause = player.pause_fn().unwrap();
        assert_eq!(
            pause(1, user_data as *mut std::ffi::c_void),
            ic_plugin_api::IC_OK
        );
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    let paused = ic_plugin_host::with_players(|players| {
        let player = players.iter().find(|player| player.id == id).unwrap();
        (player.played_so_far_fn().unwrap())(user_data as *mut std::ffi::c_void)
    });
    assert_ne!(paused.paused, 0, "it says it is paused");

    ic_plugin_host::with_players(|players| {
        let player = players.iter().find(|player| player.id == id).unwrap();
        (player.stop_fn().unwrap())(user_data as *mut std::ffi::c_void);
    });
    fm_core::host_fs::HostSource::close(source);
}
