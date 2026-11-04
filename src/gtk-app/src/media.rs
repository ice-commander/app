//! What the system shows of the playback: the lock screen, the notification shade, the headset.
//!
//! Only Android has somewhere to show it. Everywhere else this is a no-op, so the player does not
//! have to know which machine it is on.

use crate::player::{AudioPlayer, Moment};

/// Android's PlaybackState codes, which travel over JNI as they are.
mod state {
    pub const NONE: i32 = 0;
    pub const STOPPED: i32 = 1;
    pub const PAUSED: i32 = 2;
    pub const PLAYING: i32 = 3;
}

/// A position this far from where it was said to be is worth saying again.
const DRIFT: std::time::Duration = std::time::Duration::from_secs(2);
/// And it is said again this often regardless, so a stale shade cannot sit there for ever.
const AT_LEAST_EVERY: std::time::Duration = std::time::Duration::from_secs(300);
const TICK: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq)]
struct Told {
    state: i32,
    position: std::time::Duration,
    has_previous: bool,
    has_next: bool,
}

thread_local! {
    static TOLD: std::cell::RefCell<Option<(Told, std::time::Instant)>> =
        const { std::cell::RefCell::new(None) };
    static BEGUN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The name and length last published. The length is only known once the track has been read,
    /// which is after it was named, so this is republished rather than sent once.
    static NAMED: std::cell::RefCell<Option<(String, i64)>> = const { std::cell::RefCell::new(None) };
}

fn length_of(player: &AudioPlayer) -> i64 {
    player
        .duration()
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn now_of(player: &AudioPlayer) -> Told {
    let state = if player.is_playing() {
        state::PLAYING
    } else if player.is_paused() {
        state::PAUSED
    } else if player.current_title().is_empty() {
        state::NONE
    } else {
        state::STOPPED
    };
    Told {
        state,
        position: player.position(),
        has_previous: player.has_previous(),
        has_next: player.has_next(),
    }
}

/// Worth telling: anything but the position moved, the position is not where it would have drifted
/// to on its own, or it has simply been too long.
fn worth_telling(now: Told, at: std::time::Instant) -> bool {
    TOLD.with(|told| {
        let Some((last, when)) = *told.borrow() else {
            return true;
        };
        if last.state != now.state
            || last.has_previous != now.has_previous
            || last.has_next != now.has_next
        {
            return true;
        }
        let since = at.duration_since(when);
        if since >= AT_LEAST_EVERY {
            return true;
        }
        let expected = if now.state == state::PLAYING {
            last.position + since
        } else {
            last.position
        };
        now.position.abs_diff(expected) > DRIFT
    })
}

#[cfg(target_os = "android")]
fn publish(now: Told) {
    crate::android::media_transport(
        now.state,
        now.position.as_millis() as i64,
        if now.state == state::PLAYING {
            1.0
        } else {
            0.0
        },
        now.has_previous,
        now.has_next,
    );
}

#[cfg(not(target_os = "android"))]
fn publish(_now: Told) {}

#[cfg(target_os = "android")]
fn begin() {
    crate::android::media_begin();
}

#[cfg(not(target_os = "android"))]
fn begin() {}

#[cfg(target_os = "android")]
fn name(player: &AudioPlayer, title: &str, length: i64) {
    let index = player.current_idx().map(|idx| idx as i32 + 1).unwrap_or(0);
    crate::android::media_now_playing(title, length, index, player.playlist().len() as i32);
}

#[cfg(not(target_os = "android"))]
fn name(_player: &AudioPlayer, _title: &str, _length: i64) {}

#[cfg(target_os = "android")]
fn withdraw() {
    crate::android::media_end();
}

#[cfg(not(target_os = "android"))]
fn withdraw() {}

/// What the shade, the lock screen or the headset asked for. Runs on the GTK thread.
#[allow(dead_code)] // reached over JNI only, and only on Android
pub fn obey(command: i32, arg: i64) {
    let player = crate::player::the_one();
    match command {
        1 => player.play(),
        2 => player.pause(),
        3 => {
            player.toggle_play();
        }
        4 => player.next(),
        5 => player.previous(),
        6 => {
            let _ = player.seek(std::time::Duration::from_millis(arg.max(0) as u64));
        }
        7 => {
            let at = player.position().as_millis() as i64 + arg;
            let _ = player.seek(std::time::Duration::from_millis(at.max(0) as u64));
        }
        8 => player.stop(),
        _ => {}
    }
    tell(&player);
}

fn tell(player: &AudioPlayer) {
    let now = now_of(player);
    let at = std::time::Instant::now();
    let begun = BEGUN.with(std::cell::Cell::get);

    if now.state == state::NONE {
        if begun {
            BEGUN.with(|flag| flag.set(false));
            TOLD.with(|told| *told.borrow_mut() = None);
            NAMED.with(|named| *named.borrow_mut() = None);
            withdraw();
        }
        return;
    }
    if !begun {
        BEGUN.with(|flag| flag.set(true));
        begin();
    }
    let naming = (player.current_title(), length_of(player));
    if NAMED.with(|named| named.borrow().as_ref() != Some(&naming)) {
        name(player, &naming.0, naming.1);
        NAMED.with(|named| *named.borrow_mut() = Some(naming));
    }
    if worth_telling(now, at) {
        TOLD.with(|told| *told.borrow_mut() = Some((now, at)));
        publish(now);
    }
}

/// Follows the one player for as long as the process runs.
pub fn follow(player: &AudioPlayer) {
    let listening = player.clone();
    player.listen(move |moment| {
        if moment == Moment::Track {
            TOLD.with(|told| *told.borrow_mut() = None);
        }
        tell(&listening);
        true
    });

    let ticking = player.clone();
    gtk::glib::timeout_add_local(TICK, move || {
        tell(&ticking);
        gtk::glib::ControlFlow::Continue
    });
}
