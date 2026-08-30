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
                current_title: String::new(),
                volume: 0.8,
                playlist: Vec::new(),
                current_idx: None,
            })),
        }
    }

    pub fn play_bytes(&self, title: String, bytes: Vec<u8>) -> Result<(), String> {
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
            match Sink::try_new(h) {
                Ok(s) => {
                    s.set_volume(inner.volume);
                    inner.sink = Some(s);
                }
                Err(e) => return Err(format!("Could not create audio sink: {}", e)),
            }
        }

        let cursor = Cursor::new(bytes);
        match Decoder::new(cursor) {
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

    pub fn duration(&self) -> Option<std::time::Duration> {
        self.inner.borrow().duration
    }

    pub fn position(&self) -> std::time::Duration {
        self.inner
            .borrow()
            .sink
            .as_ref()
            .map(|s| s.get_pos())
            .unwrap_or_default()
    }

    pub fn seek(&self, pos: std::time::Duration) -> Result<(), String> {
        let inner = self.inner.borrow();
        let Some(sink) = inner.sink.as_ref() else {
            return Err("nothing is playing".to_string());
        };
        sink.try_seek(pos).map_err(|e| e.to_string())
    }

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
        inner.current_title = String::new();
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
