use crate::player::AudioPlayer;
use gtk::prelude::*;
use panel_router::PanelRouter;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
pub struct AudioPlayerView {
    pub container: gtk::Box,
    pub play_btn: gtk::Button,
    pub title_label: gtk::Label,
    pub volume_scale: gtk::Scale,
    pub player: AudioPlayer,
    pub router: Rc<PanelRouter>,
    pub on_show: Rc<RefCell<Option<Box<dyn Fn() + 'static>>>>,
    pub on_hide: Rc<RefCell<Option<Box<dyn Fn() + 'static>>>>,
}

impl AudioPlayerView {
    pub fn new(player: AudioPlayer, router: Rc<PanelRouter>) -> Self {
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
            router,
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
            view_close_clone.title_label.set_text(&*crate::i18n::tr("player.no_audio_playing"));
            if let Some(ref cb) = *view_close_clone.on_hide.borrow() {
                cb();
            }
        });

        let view_prev_clone = view.clone();
        prev_btn.connect_clicked(move |_| {
            if let Some(idx) = view_prev_clone.player.current_idx() {
                if idx > 0 {
                    view_prev_clone.play_track_at(idx - 1);
                }
            }
        });

        let view_next_clone = view.clone();
        next_btn.connect_clicked(move |_| {
            if let Some(idx) = view_next_clone.player.current_idx() {
                let playlist = view_next_clone.player.playlist();
                if idx + 1 < playlist.len() {
                    view_next_clone.play_track_at(idx + 1);
                }
            }
        });

        let view_timer_clone = view.clone();
        let was_attached = std::cell::Cell::new(false);
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            if view_timer_clone.container.root().is_some() {
                was_attached.set(true);
            } else if was_attached.get() {
                view_timer_clone.player.stop();
                return gtk::glib::ControlFlow::Break;
            }
            let p = &view_timer_clone.player;
            if !p.is_playing() && !p.current_title().is_empty() {
                if let Some(idx) = p.current_idx() {
                    let playlist = p.playlist();
                    if idx + 1 < playlist.len() {
                        view_timer_clone.play_track_at(idx + 1);
                    } else {
                        p.stop();
                        view_timer_clone
                            .play_btn
                            .set_child(Some(&gtk::Image::from_resource(
                                "/com/icecommander/gtk/play.svg",
                            )));
                        view_timer_clone.title_label.set_text(&*crate::i18n::tr("player.playback_finished"));
                        if let Some(ref cb) = *view_timer_clone.on_hide.borrow() {
                            cb();
                        }
                    }
                } else {
                    p.stop();
                    view_timer_clone
                        .play_btn
                        .set_child(Some(&gtk::Image::from_resource(
                            "/com/icecommander/gtk/play.svg",
                        )));
                    view_timer_clone.title_label.set_text(&*crate::i18n::tr("player.playback_finished"));
                    if let Some(ref cb) = *view_timer_clone.on_hide.borrow() {
                        cb();
                    }
                }
            }
            gtk::glib::ControlFlow::Continue
        });

        view
    }

    pub fn play_track_at(&self, idx: usize) {
        let playlist = self.player.playlist();
        if idx >= playlist.len() {
            return;
        }
        let (name, path) = &playlist[idx];
        self.player.set_current_idx(Some(idx));
        self.set_playing_track(name);

        if let Some(ref cb) = *self.on_show.borrow() {
            cb();
        }

        let player_clone = self.player.clone();
        let view_clone = self.clone();
        let name_clone = name.clone();
        let provider = self.router.provider();
        let path_c = path.clone();

        gtk::glib::spawn_future_local(async move {
            let blocking = crate::utils::read_blocking(&path_c);
            match provider.read_file_opt(path_c, None, blocking).await {
                Ok(bytes) => {
                    if let Err(e) = player_clone.play_bytes(name_clone, bytes) {
                        view_clone.title_label.set_text(&*crate::i18n::trf("player.error", &[("error", &*(e.to_string()).to_string())]));
                    }
                }
                Err(e) => {
                    view_clone.title_label.set_text(&*crate::i18n::trf("player.read_error", &[("error", &*(e.to_string()).to_string())]));
                }
            }
        });
    }

    pub fn set_playing_track(&self, title: &str) {
        self.title_label.set_text(&*crate::i18n::trf("player.playing_status", &[("title", &*(title).to_string())]));
        self.play_btn.set_child(Some(&gtk::Image::from_resource(
            "/com/icecommander/gtk/pause.svg",
        )));
        self.container.set_visible(true);
    }
}
