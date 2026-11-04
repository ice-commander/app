use crate::player::AudioPlayer;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
pub struct AudioPlayerView {
    pub container: gtk::Box,
    pub play_btn: gtk::Button,
    pub title_label: gtk::Label,
    pub volume_scale: gtk::Scale,
    pub player: AudioPlayer,
    pub on_show: Rc<RefCell<Option<Box<dyn Fn() + 'static>>>>,
    pub on_hide: Rc<RefCell<Option<Box<dyn Fn() + 'static>>>>,
}

impl AudioPlayerView {
    pub fn new(player: AudioPlayer) -> Self {
        let container = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(8)
            .css_classes(vec!["card"])
            .build();

        let play_icon_img = gtk::Image::from_resource("/com/icecommander/gtk/music.svg");
        container.append(&play_icon_img);

        let title_label = gtk::Label::builder()
            .label(&*crate::i18n::tr("player.no_audio_playing"))
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .hexpand(true)
            .halign(gtk::Align::Start)
            .build();
        container.append(&title_label);

        let prev_btn = gtk::Button::builder()
            .child(&gtk::Image::from_resource("/com/icecommander/gtk/prev.svg"))
            .build();
        prev_btn.set_cursor_from_name(Some("pointer"));
        container.append(&prev_btn);

        let play_btn = gtk::Button::builder()
            .child(&gtk::Image::from_resource("/com/icecommander/gtk/play.svg"))
            .build();
        play_btn.set_cursor_from_name(Some("pointer"));
        container.append(&play_btn);

        let next_btn = gtk::Button::builder()
            .child(&gtk::Image::from_resource("/com/icecommander/gtk/next.svg"))
            .build();
        next_btn.set_cursor_from_name(Some("pointer"));
        container.append(&next_btn);

        let stop_btn = gtk::Button::builder()
            .child(&gtk::Image::from_resource("/com/icecommander/gtk/stop.svg"))
            .build();
        stop_btn.set_cursor_from_name(Some("pointer"));
        container.append(&stop_btn);

        let vol_sep = gtk::Separator::new(gtk::Orientation::Vertical);
        container.append(&vol_sep);

        let vol_icon = gtk::Image::from_resource("/com/icecommander/gtk/high-volume.svg");
        vol_icon.set_pixel_size(16);
        container.append(&vol_icon);

        let volume_scale = gtk::Scale::builder()
            .orientation(gtk::Orientation::Horizontal)
            .adjustment(&gtk::Adjustment::new(0.8, 0.0, 1.0, 0.05, 0.1, 0.0))
            .width_request(80)
            .draw_value(false)
            .build();
        volume_scale.set_cursor_from_name(Some("pointer"));
        container.append(&volume_scale);

        let close_sep = gtk::Separator::new(gtk::Orientation::Vertical);
        container.append(&close_sep);

        let close_btn = gtk::Button::builder()
            .child(&gtk::Image::from_resource(
                "/com/icecommander/gtk/close.svg",
            ))
            .css_classes(vec!["flat"])
            .build();
        close_btn.set_cursor_from_name(Some("pointer"));
        container.append(&close_btn);

        let view = Self {
            container,
            play_btn,
            title_label,
            volume_scale,
            player: player.clone(),
            on_show: Rc::new(RefCell::new(None)),
            on_hide: Rc::new(RefCell::new(None)),
        };

        let player_clone = player.clone();
        let play_btn_clone = view.play_btn.clone();
        view.play_btn.connect_clicked(move |_| {
            let is_now_playing = player_clone.toggle_play();
            if is_now_playing {
                play_btn_clone.set_child(Some(&gtk::Image::from_resource(
                    "/com/icecommander/gtk/pause.svg",
                )));
            } else {
                play_btn_clone.set_child(Some(&gtk::Image::from_resource(
                    "/com/icecommander/gtk/play.svg",
                )));
            }
        });

        let player_clone = player.clone();
        let play_btn_clone2 = view.play_btn.clone();
        let title_label_clone = view.title_label.clone();
        stop_btn.connect_clicked(move |_| {
            player_clone.stop();
            play_btn_clone2.set_child(Some(&gtk::Image::from_resource(
                "/com/icecommander/gtk/play.svg",
            )));
            title_label_clone.set_text(&*crate::i18n::tr("player.playback_stopped"));
        });

        let player_clone = player.clone();
        let vol_icon_clone = vol_icon.clone();
        view.volume_scale.connect_value_changed(move |scale| {
            let val = scale.value() as f32;
            player_clone.set_volume(val);
            vol_icon_clone.set_resource(Some(if val >= 0.5 {
                "/com/icecommander/gtk/high-volume.svg"
            } else {
                "/com/icecommander/gtk/low-volume.svg"
            }));
        });

        let view_close_clone = view.clone();
        close_btn.connect_clicked(move |_| {
            view_close_clone.container.set_visible(false);
            view_close_clone.player.stop();
            view_close_clone
                .play_btn
                .set_child(Some(&gtk::Image::from_resource(
                    "/com/icecommander/gtk/play.svg",
                )));
            view_close_clone
                .title_label
                .set_text(&*crate::i18n::tr("player.no_audio_playing"));
            if let Some(ref cb) = *view_close_clone.on_hide.borrow() {
                cb();
            }
        });

        let player_prev = player.clone();
        prev_btn.connect_clicked(move |_| player_prev.previous());

        let player_next = player.clone();
        next_btn.connect_clicked(move |_| player_next.next());

        let listening = view.clone();
        let was_attached = std::cell::Cell::new(false);
        player.listen(move |moment| {
            if listening.container.root().is_some() {
                was_attached.set(true);
            } else if was_attached.get() {
                return false;
            }
            match moment {
                crate::player::Moment::Track => {
                    listening.set_playing_track(&listening.player.current_title());
                    if let Some(ref cb) = *listening.on_show.borrow() {
                        cb();
                    }
                }
                crate::player::Moment::State => listening.show_state(),
                crate::player::Moment::Finished => {
                    listening.show_state();
                    listening
                        .title_label
                        .set_text(&*crate::i18n::tr("player.playback_finished"));
                    if let Some(ref cb) = *listening.on_hide.borrow() {
                        cb();
                    }
                }
            }
            true
        });

        view
    }

    fn show_state(&self) {
        self.play_btn.set_child(Some(&gtk::Image::from_resource(
            if self.player.is_playing() {
                "/com/icecommander/gtk/pause.svg"
            } else {
                "/com/icecommander/gtk/play.svg"
            },
        )));
    }

    pub fn set_playing_track(&self, title: &str) {
        self.title_label.set_text(&*crate::i18n::trf(
            "player.playing_status",
            &[("title", &*(title).to_string())],
        ));
        self.play_btn.set_child(Some(&gtk::Image::from_resource(
            "/com/icecommander/gtk/pause.svg",
        )));
        self.container.set_visible(true);
    }
}
