use fm_core::rpc::FileSystemRpc;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;

pub struct AudioPlayer {
    inner: Rc<RefCell<AudioPlayerInner>>,
}

struct AudioPlayerInner {
    stream: Option<OutputStream>,
    stream_handle: Option<OutputStreamHandle>,
    sink: Option<Sink>,
    duration: Option<std::time::Duration>,
    /// What is on: the file it was given, not its title. Whoever asked for it
    /// is the only one who can say "stop that" and mean this.
    current: Option<String>,
    current_title: String,
    volume: f32,
    playlist: Vec<(String, String)>, // (name, path)
    current_idx: Option<usize>,
    provider: Option<Rc<dyn FileSystemRpc>>,
    listeners: Vec<Rc<dyn Fn(Moment) -> bool>>,
    watching: bool,
    played_by: Option<Playing>,
}

#[derive(Clone)]
struct Playing {
    id: String,
    source: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moment {
    Track,
    State,
    Finished,
}

impl AudioPlayer {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(AudioPlayerInner {
                stream: None,
                stream_handle: None,
                sink: None,
                duration: None,
                current: None,
                current_title: String::new(),
                volume: 0.8,
                playlist: Vec::new(),
                current_idx: None,
                provider: None,
                listeners: Vec::new(),
                watching: false,
                played_by: None,
            })),
        }
    }

    /// Plays a file on this machine, reading it as it goes rather than
    /// holding it: a long recording costs a buffer, not its own length.
    ///
    /// Whatever was playing stops. One player, one sound: that is the whole
    /// of the rule, and it is here rather than in whoever asked, so that no
    /// window can leave a song playing behind it.
    ///
    pub fn play_file(&self, title: String, path: &str) -> Result<(), String> {
        if self.hand_to_a_plugin(&title, path)? {
            return Ok(());
        }
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        self.play_source(title, std::io::BufReader::new(file))?;
        self.inner.borrow_mut().current = Some(path.to_string());
        Ok(())
    }

    fn hand_to_a_plugin(&self, title: &str, path: &str) -> Result<bool, String> {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(id) = ic_plugin_host::player_for(&name) else {
            return Ok(false);
        };
        let folder = std::path::Path::new(path)
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();

        self.stop();
        let started = ic_plugin_host::with_players(|players| {
            let player = players.iter().find(|player| player.id == id)?;
            let open = player.open_fn()?;
            let source = fm_core::host_fs::HostSource::rooted_at(&folder, "local", "").open();
            let Ok(named) = std::ffi::CString::new(name.as_str()) else {
                fm_core::host_fs::HostSource::close(source);
                return None;
            };
            let answer = open(
                source,
                named.as_ptr(),
                player.user_data as *mut std::ffi::c_void,
            );
            if answer != ic_plugin_api::IC_OK {
                fm_core::host_fs::HostSource::close(source);
                return None;
            }
            Some(source)
        });

        let Some(source) = started else {
            ic_logging::warn!("plugin: player {id} would not play {name}; playing it here instead");
            return Ok(false);
        };
        let mut inner = self.inner.borrow_mut();
        inner.played_by = Some(Playing {
            id,
            source: source as usize,
        });
        inner.current = Some(path.to_string());
        inner.current_title = title.to_string();
        inner.duration = None;
        drop(inner);
        self.tell(Moment::Track);
        Ok(true)
    }

    fn plugin_playing(&self) -> Option<Playing> {
        self.inner.borrow().played_by.clone()
    }

    fn stop_the_plugin(&self) {
        let Some(playing) = self.inner.borrow_mut().played_by.take() else {
            return;
        };
        ic_plugin_host::with_players(|players| {
            if let Some(player) = players.iter().find(|player| player.id == playing.id) {
                if let Some(stop) = player.stop_fn() {
                    stop(player.user_data as *mut std::ffi::c_void);
                }
            }
        });
        fm_core::host_fs::HostSource::close(playing.source as ic_plugin_api::IcFsSource);
    }

    fn plugin_where(&self) -> Option<ic_plugin_api::IcPlayedSoFar> {
        let playing = self.plugin_playing()?;
        ic_plugin_host::with_players(|players| {
            let player = players.iter().find(|player| player.id == playing.id)?;
            let asked = player.played_so_far_fn()?;
            Some(asked(player.user_data as *mut std::ffi::c_void))
        })
    }

    fn tell_the_plugin<R>(
        &self,
        act: impl FnOnce(&ic_plugin_host::PlayerPlugin) -> R,
    ) -> Option<R> {
        let playing = self.plugin_playing()?;
        ic_plugin_host::with_players(|players| {
            players
                .iter()
                .find(|player| player.id == playing.id)
                .map(act)
        })
    }

    /// The file it is playing, if it is playing one.
    pub fn now_playing(&self) -> Option<String> {
        self.inner.borrow().current.clone()
    }

    /// Plays it unless it is already the one on — a window redrawn around a
    /// song is not a reason to start the song again.
    pub fn play_unless_already(&self, title: String, path: &str) -> Result<(), String> {
        if self.now_playing().as_deref() == Some(path) {
            return Ok(());
        }
        self.play_file(title, path)
    }

    /// Stops whatever is playing if it came from inside that folder.
    pub fn stop_if_under(&self, folder: &std::path::Path) -> bool {
        let Some(now) = self.now_playing() else {
            return false;
        };
        if !std::path::Path::new(&now).starts_with(folder) {
            return false;
        }
        self.stop();
        true
    }

    /// As `play_file`, for bytes fetched from somewhere the player cannot open itself.
    /// `path` names the track, so only whoever started it can stop it.
    pub fn play_bytes_of(&self, title: String, path: &str, bytes: Vec<u8>) -> Result<(), String> {
        self.play_bytes(title, bytes)?;
        self.inner.borrow_mut().current = Some(path.to_string());
        Ok(())
    }

    pub fn play_bytes(&self, title: String, bytes: Vec<u8>) -> Result<(), String> {
        self.play_source(title, Cursor::new(bytes))
    }

    fn play_source<R>(&self, title: String, source: R) -> Result<(), String>
    where
        R: std::io::Read + std::io::Seek + Send + Sync + 'static,
    {
        let mut inner = self.inner.borrow_mut();

        if inner.stream.is_none() {
            match OutputStream::try_default() {
                Ok((s, h)) => {
                    inner.stream = Some(s);
                    inner.stream_handle = Some(h);
                }
                Err(e) => return Err(format!("Could not initialize audio output: {}", e)),
            }
        }

        if let Some(h) = &inner.stream_handle {
            // Dropping the sink it replaces is what stops the old song (rodio 0.19 sink.rs:337).
            match Sink::try_new(h) {
                Ok(s) => {
                    s.set_volume(inner.volume);
                    inner.sink = Some(s);
                }
                Err(e) => return Err(format!("Could not create audio sink: {}", e)),
            }
        }

        match Decoder::new(source) {
            Ok(source) => {
                inner.duration = rodio::Source::total_duration(&source);
                if let Some(sink) = &inner.sink {
                    sink.append(source);
                    sink.play();
                }
                inner.current_title = title;
                Ok(())
            }
            Err(e) => Err(format!("Could not decode the audio: {}", e)),
        }
    }

    // Asked for by the viewer's own player, which F3 no longer opens.
    #[allow(dead_code)]
    pub fn duration(&self) -> Option<std::time::Duration> {
        if let Some(said) = self.plugin_where() {
            return (said.of > 0.0).then(|| std::time::Duration::from_secs_f64(said.of));
        }
        self.inner.borrow().duration
    }

    #[allow(dead_code)]
    pub fn position(&self) -> std::time::Duration {
        if let Some(said) = self.plugin_where() {
            return std::time::Duration::from_secs_f64(said.at.max(0.0));
        }
        self.inner
            .borrow()
            .sink
            .as_ref()
            .map(|s| s.get_pos())
            .unwrap_or_default()
    }

    #[allow(dead_code)]
    pub fn seek(&self, pos: std::time::Duration) -> Result<(), String> {
        if self.plugin_playing().is_some() {
            let moved = self.tell_the_plugin(|player| {
                let seek = player.seek_fn()?;
                Some(seek(
                    pos.as_secs_f64(),
                    player.user_data as *mut std::ffi::c_void,
                ))
            });
            return match moved.flatten() {
                Some(answer) if answer == ic_plugin_api::IC_OK => Ok(()),
                Some(_) => Err("the player would not move there".to_string()),
                None => Err("this player cannot seek".to_string()),
            };
        }
        let inner = self.inner.borrow();
        let Some(sink) = inner.sink.as_ref() else {
            return Err("nothing is playing".to_string());
        };
        sink.try_seek(pos).map_err(|e| e.to_string())
    }

    #[allow(dead_code)]
    pub fn is_finished(&self) -> bool {
        if let Some(said) = self.plugin_where() {
            return said.ended != 0;
        }
        self.inner
            .borrow()
            .sink
            .as_ref()
            .map(|s| s.empty())
            .unwrap_or(false)
    }

    pub fn toggle_play(&self) -> bool {
        if let Some(said) = self.plugin_where() {
            let now_playing = said.paused != 0;
            self.tell_the_plugin(|player| {
                if let Some(pause) = player.pause_fn() {
                    pause(
                        i32::from(!now_playing),
                        player.user_data as *mut std::ffi::c_void,
                    );
                }
            });
            self.tell(Moment::State);
            return now_playing;
        }
        let playing = {
            let inner = self.inner.borrow();
            match &inner.sink {
                Some(sink) if sink.is_paused() => {
                    sink.play();
                    true
                }
                Some(sink) => {
                    sink.pause();
                    false
                }
                None => false,
            }
        };
        self.tell(Moment::State);
        playing
    }

    pub fn play(&self) {
        self.playing_is(true)
    }

    pub fn pause(&self) {
        self.playing_is(false)
    }

    fn playing_is(&self, on: bool) {
        if self.plugin_playing().is_some() {
            self.tell_the_plugin(|player| {
                if let Some(pause) = player.pause_fn() {
                    pause(i32::from(!on), player.user_data as *mut std::ffi::c_void);
                }
            });
            self.tell(Moment::State);
            return;
        }
        {
            let inner = self.inner.borrow();
            let Some(sink) = inner.sink.as_ref() else {
                return;
            };
            if on {
                sink.play();
            } else {
                sink.pause();
            }
        }
        self.tell(Moment::State);
    }

    pub fn is_paused(&self) -> bool {
        if let Some(said) = self.plugin_where() {
            return said.paused != 0;
        }
        self.inner
            .borrow()
            .sink
            .as_ref()
            .map(Sink::is_paused)
            .unwrap_or(false)
    }

    pub fn stop(&self) {
        self.stop_the_plugin();
        let mut inner = self.inner.borrow_mut();
        if let Some(sink) = &inner.sink {
            sink.stop();
        }
        inner.sink = None;
        inner.duration = None;
        inner.stream_handle = None;
        inner.stream = None;
        inner.current = None;
        inner.current_title = String::new();
        drop(inner);
        self.tell(Moment::State);
    }

    pub fn is_playing(&self) -> bool {
        if let Some(said) = self.plugin_where() {
            return said.paused == 0 && said.ended == 0;
        }
        let inner = self.inner.borrow();
        if let Some(sink) = &inner.sink {
            !sink.is_paused() && !sink.empty()
        } else {
            false
        }
    }

    pub fn current_title(&self) -> String {
        self.inner.borrow().current_title.clone()
    }

    #[allow(dead_code)] // getter half of the volume pair; `set_volume` drives the slider
    pub fn volume(&self) -> f32 {
        self.inner.borrow().volume
    }

    pub fn set_volume(&self, vol: f32) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.volume = vol;
            if let Some(sink) = &inner.sink {
                sink.set_volume(vol);
            }
        }
        self.tell_the_plugin(|player| {
            if let Some(volume) = player.volume_fn() {
                volume(f64::from(vol), player.user_data as *mut std::ffi::c_void);
            }
        });
    }

    pub fn set_playlist(
        &self,
        playlist: Vec<(String, String)>,
        current_idx: Option<usize>,
        provider: Rc<dyn FileSystemRpc>,
    ) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.playlist = playlist;
            inner.current_idx = current_idx;
            inner.provider = Some(provider);
        }
        self.watch_for_the_end();
    }

    pub fn play_at(&self, idx: usize) {
        let (name, path, provider) = {
            let inner = self.inner.borrow();
            let Some((name, path)) = inner.playlist.get(idx).cloned() else {
                return;
            };
            let Some(provider) = inner.provider.clone() else {
                return;
            };
            (name, path, provider)
        };
        {
            let mut inner = self.inner.borrow_mut();
            inner.current_idx = Some(idx);
            inner.current_title = name.clone();
        }
        self.tell(Moment::Track);

        if provider.is_local() {
            match self.play_file(name.clone(), &path) {
                Ok(()) => {
                    self.tell(Moment::State);
                    return;
                }
                Err(why) => ic_logging::warn!("cannot play {path}: {why}"),
            }
        }

        let player = self.clone();
        gtk::glib::spawn_future_local(async move {
            let blocking = crate::utils::read_blocking(&path);
            match provider.read_file_opt(path.clone(), None, blocking).await {
                Ok(bytes) => {
                    if let Err(e) = player.play_bytes_of(name, &path, bytes) {
                        ic_logging::warn!("cannot play {path}: {e}");
                    }
                }
                Err(e) => ic_logging::warn!("cannot read {path}: {e}"),
            }
            player.tell(Moment::State);
        });
    }

    pub fn has_next(&self) -> bool {
        let inner = self.inner.borrow();
        inner
            .current_idx
            .map(|idx| idx + 1 < inner.playlist.len())
            .unwrap_or(false)
    }

    pub fn has_previous(&self) -> bool {
        self.inner
            .borrow()
            .current_idx
            .map(|idx| idx > 0)
            .unwrap_or(false)
    }

    pub fn next(&self) {
        if let Some(idx) = self.current_idx().filter(|_| self.has_next()) {
            self.play_at(idx + 1);
        }
    }

    pub fn previous(&self) {
        if let Some(idx) = self.current_idx().filter(|_| self.has_previous()) {
            self.play_at(idx - 1);
        }
    }

    fn watch_for_the_end(&self) {
        if std::mem::replace(&mut self.inner.borrow_mut().watching, true) {
            return;
        }
        let player = self.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            if player.is_finished() && !player.current_title().is_empty() {
                if player.has_next() {
                    player.next();
                } else {
                    player.stop();
                    player.tell(Moment::Finished);
                }
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    pub fn listen(&self, on: impl Fn(Moment) -> bool + 'static) {
        self.inner.borrow_mut().listeners.push(Rc::new(on));
    }

    fn tell(&self, moment: Moment) {
        let listeners = std::mem::take(&mut self.inner.borrow_mut().listeners);
        let alive: Vec<_> = listeners
            .into_iter()
            .filter(|listener| listener(moment))
            .collect();
        self.inner.borrow_mut().listeners.extend(alive);
    }

    pub fn playlist(&self) -> Vec<(String, String)> {
        self.inner.borrow().playlist.clone()
    }

    pub fn current_idx(&self) -> Option<usize> {
        self.inner.borrow().current_idx
    }

    pub fn set_current_idx(&self, idx: Option<usize>) {
        let mut inner = self.inner.borrow_mut();
        inner.current_idx = idx;
    }
}

impl Clone for AudioPlayer {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

thread_local! {
    /// The one player the application has.
    /// One player for the whole application: a clone is the same player, and a new song
    /// always stops the one before it, wherever it was started from.
    static THE_ONE: AudioPlayer = AudioPlayer::new();
}

pub fn the_one() -> AudioPlayer {
    THE_ONE.with(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A player with nothing on claims nothing and stops nothing.
    #[test]
    fn a_player_that_is_not_playing_says_so() {
        let player = AudioPlayer::new();
        assert_eq!(player.now_playing(), None);
        assert!(!player.stop_if_under(std::path::Path::new("/music")));
    }

    /// A file that could not be played is not the file playing: the player
    /// must not claim a song it never started.
    #[test]
    fn a_play_that_failed_claims_nothing() {
        let player = AudioPlayer::new();
        let nowhere = std::env::temp_dir().join("ic-no-such-song.mp3");
        assert!(player
            .play_file("nothing".to_string(), &nowhere.to_string_lossy())
            .is_err());
        assert_eq!(player.now_playing(), None);
        assert!(!player.stop_if_under(&std::env::temp_dir()));
    }

    #[test]
    fn a_listener_that_answers_false_is_not_told_again() {
        let player = AudioPlayer::new();
        let calls = Rc::new(std::cell::Cell::new(0));
        let counting = calls.clone();
        player.listen(move |_| {
            counting.set(counting.get() + 1);
            false
        });
        player.stop();
        player.stop();
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn a_playlist_of_one_leads_nowhere() {
        let player = AudioPlayer::new();
        assert!(!player.has_next(), "with no playlist at all");
        player.inner.borrow_mut().playlist = vec![("one".into(), "/one.mp3".into())];
        player.inner.borrow_mut().current_idx = Some(0);
        assert!(!player.has_next());
        assert!(!player.has_previous());
    }

    #[test]
    fn the_one_player_is_the_same_player_every_time() {
        assert!(Rc::ptr_eq(&the_one().inner, &the_one().inner));
        assert!(
            !Rc::ptr_eq(&AudioPlayer::new().inner, &the_one().inner),
            "and a player made by hand is somebody else's"
        );
    }
}
