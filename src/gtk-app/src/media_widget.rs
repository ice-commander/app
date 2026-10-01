//! The transport a `media` node is drawn as: play, pause, where we are.
//!
//! The sound is the application's own — `rodio`, the same player the panel uses — rather
//! than the toolkit's, which would load a second decoder stack. A plugin says only what to play.

use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// How often the position is read while something is playing. Often enough to
/// look live, rarely enough that a window costs nothing to leave open.
const TICK: Duration = Duration::from_millis(500);

/// The transport for one file, playing through the application's one player.
pub fn sound(path: &str, autoplay: bool, ended: Rc<dyn Fn()>) -> gtk::Widget {
    let player = crate::player::the_one();
    let name = std::path::Path::new(path)
        .file_name()
        .map(|held| held.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    if let Err(why) = player.play_unless_already(name, path) {
        let failed = gtk::Label::new(Some(&why));
        failed.add_css_class("dim-label");
        failed.set_wrap(true);
        return failed.upcast();
    }
    // The viewer follows this track now; the panel bar must not advance its playlist over it.
    player.set_current_idx(None);
    if !autoplay && player.is_playing() {
        player.toggle_play();
    }

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .hexpand(true)
        .build();

    let play = gtk::Button::from_icon_name(if autoplay {
        "media-playback-pause-symbolic"
    } else {
        "media-playback-start-symbolic"
    });
    play.set_cursor_from_name(Some("pointer"));
    row.append(&play);

    let along = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.001);
    along.set_draw_value(false);
    along.set_hexpand(true);
    row.append(&along);

    let clock = gtk::Label::new(Some("0:00"));
    clock.add_css_class("dim-label");
    row.append(&clock);

    let stop = gtk::Button::from_icon_name("media-playback-stop-symbolic");
    stop.set_cursor_from_name(Some("pointer"));
    row.append(&stop);

    // The same range and the same starting point the application's own window
    // had, so a track is not suddenly loud.
    let loudness = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
    loudness.set_draw_value(false);
    loudness.set_width_request(110);
    row.append(&loudness);

    let playing = Rc::new(Cell::new(autoplay));
    let held = player;
    loudness.set_value(held.volume() as f64);

    let pressed = held.clone();
    let shown = playing.clone();
    let face = play.clone();
    play.connect_clicked(move |_| {
        let now = pressed.toggle_play();
        shown.set(now);
        face.set_icon_name(if now {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
    });

    let stopping = held.clone();
    let quiet = playing.clone();
    let face = play.clone();
    let back_to_start = along.clone();
    stop.connect_clicked(move |_| {
        stopping.stop();
        quiet.set(false);
        face.set_icon_name("media-playback-start-symbolic");
        back_to_start.set_value(0.0);
    });

    let turning = held.clone();
    loudness.connect_value_changed(move |scale| {
        turning.set_volume(scale.value() as f32);
    });

    // Moving the slider is the user asking to be somewhere else in the track.
    // A file that does not say how long it is cannot be moved about by a
    // fraction of its length, so the slider says so by not being there to
    // press rather than by doing nothing when pressed.
    along.set_sensitive(held.duration().is_some());
    let seeking = held.clone();
    let moved = Rc::new(Cell::new(false));
    let while_moving = moved.clone();
    along.connect_change_value(move |_, _, value| {
        while_moving.set(true);
        if let Some(whole) = seeking.duration() {
            let _ = seeking.seek(whole.mul_f64(value.clamp(0.0, 1.0)));
        }
        while_moving.set(false);
        gtk::glib::Propagation::Proceed
    });

    // Weak: the player outlives every window, so each tick only asks whether this one stands.
    let moving = along.downgrade();
    let counting = clock.downgrade();
    let holding = moved.clone();
    let still_playing = playing.clone();
    let face = play.downgrade();
    let ours = path.to_string();
    let tick = gtk::glib::timeout_add_local(TICK, move || {
        let (Some(moving), Some(counting), Some(face)) =
            (moving.upgrade(), counting.upgrade(), face.upgrade())
        else {
            // The window it belonged to has gone.
            return gtk::glib::ControlFlow::Break;
        };
        let ticking = crate::player::the_one();
        let at = ticking.position();
        let whole = ticking.duration();
        if !holding.get() {
            if let Some(whole) = whole.filter(|whole| !whole.is_zero()) {
                moving.set_value(at.as_secs_f64() / whole.as_secs_f64());
            }
        }
        // Some files do not say how long they are — a stream, an mp3 without
        // the header that carries it. Then the clock shows what has been
        // played and does not invent the rest.
        counting.set_text(&match whole.filter(|whole| !whole.is_zero()) {
            Some(whole) => format!("{} / {}", as_clock(at), as_clock(whole)),
            None => as_clock(at),
        });
        // A track that ran out leaves the button showing what pressing it does.
        if still_playing.get() && ticking.is_finished() {
            still_playing.set(false);
            face.set_icon_name("media-playback-start-symbolic");
            if ticking.now_playing().as_deref() == Some(ours.as_str()) {
                // Idle: the answer may rebuild the window this tick belongs to.
                let ended = ended.clone();
                gtk::glib::idle_add_local_once(move || ended());
            }
        }
        gtk::glib::ControlFlow::Continue
    });

    // Nothing stops the sound here: this widget is unrealized on every redraw of the page.
    let ticking = RefCell::new(Some(tick));
    row.connect_destroy(move |_| {
        if let Some(tick) = ticking.borrow_mut().take() {
            tick.remove();
        }
    });

    row.upcast()
}

fn as_clock(held: Duration) -> String {
    let seconds = held.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_is_said_the_way_a_player_says_it() {
        assert_eq!(as_clock(Duration::ZERO), "0:00");
        assert_eq!(as_clock(Duration::from_secs(9)), "0:09");
        assert_eq!(as_clock(Duration::from_secs(61)), "1:01");
        assert_eq!(as_clock(Duration::from_secs(3600)), "60:00");
    }
}
