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
            })),
        }
    }

    /// Plays a file on this machine, reading it as it goes rather than
    /// holding it: a long recording costs a buffer, not its own length.
    ///
    /// Whatever was playing stops. One player, one sound: that is the whole
    /// of the rule, and it is here rather than in whoever asked, so that no
    /// window can leave a song playing behind it.
    pub fn play_file(&self, title: String, path: &str) -> Result<(), String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        self.play_source(title, std::io::BufReader::new(file))?;
        self.inner.borrow_mut().current = Some(path.to_string());
        Ok(())
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
        self.inner.borrow().duration
    }

    #[allow(dead_code)]
    pub fn position(&self) -> std::time::Duration {
        self.inner
            .borrow()
            .sink
            .as_ref()
            .map(|s| s.get_pos())
            .unwrap_or_default()
    }

    #[allow(dead_code)]
    pub fn seek(&self, pos: std::time::Duration) -> Result<(), String> {
        let inner = self.inner.borrow();
        let Some(sink) = inner.sink.as_ref() else {
            return Err("nothing is playing".to_string());
        };
        sink.try_seek(pos).map_err(|e| e.to_string())
    }

    #[allow(dead_code)]
    pub fn is_finished(&self) -> bool {
        self.inner
            .borrow()
            .sink
            .as_ref()
            .map(|s| s.empty())
            .unwrap_or(false)
    }

    pub fn toggle_play(&self) -> bool {
        let inner = self.inner.borrow();
        if let Some(sink) = &inner.sink {
            if sink.is_paused() {
                sink.play();
                true
            } else {
                sink.pause();
                false
            }
        } else {
            false
        }
    }

    pub fn stop(&self) {
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
    }

    pub fn is_playing(&self) -> bool {
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
        let mut inner = self.inner.borrow_mut();
        inner.volume = vol;
        if let Some(sink) = &inner.sink {
            sink.set_volume(vol);
        }
    }

    pub fn set_playlist(&self, playlist: Vec<(String, String)>, current_idx: Option<usize>) {
        let mut inner = self.inner.borrow_mut();
        inner.playlist = playlist;
        inner.current_idx = current_idx;
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
    ///
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
    fn the_one_player_is_the_same_player_every_time() {
        assert!(Rc::ptr_eq(&the_one().inner, &the_one().inner));
        assert!(
            !Rc::ptr_eq(&AudioPlayer::new().inner, &the_one().inner),
            "and a player made by hand is somebody else's"
        );
    }
}
