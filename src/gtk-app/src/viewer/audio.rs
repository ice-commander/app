
use adw::prelude::*;
use fm_core::rpc::FileSystemRpc;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::player::AudioPlayer;

struct PlayerWindow {
    player: AudioPlayer,
    cover: gtk::Image,
    provider: Rc<dyn FileSystemRpc>,
    tracks: RefCell<Vec<(String, String)>>,
    current: Cell<Option<usize>>,
    list: gtk::ListBox,
    title_label: gtk::Label,
    time_label: gtk::Label,
    seek: gtk::Scale,
    play_btn: gtk::Button,
    seeking: Cell<bool>,
    loading: Cell<bool>,
}

fn format_time(d: Duration) -> String {
    let total = d.as_secs();
    format!("{}:{:02}", total / 60, total % 60)
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(idx) => path[..idx].to_string(),
    }
}

fn icon_button(resource: &str, tooltip: &str) -> gtk::Button {
    let btn = gtk::Button::builder()
        .child(&gtk::Image::from_resource(resource))
        .tooltip_text(tooltip)
        .css_classes(vec!["flat".to_string()])
        .build();
    btn.set_cursor_from_name(Some("pointer"));
    btn
}

impl PlayerWindow {
    fn play(self: &Rc<Self>, idx: usize) {
        let Some((name, path)) = self.tracks.borrow().get(idx).cloned() else {
            return;
        };
        self.current.set(Some(idx));
        self.loading.set(true);
        self.title_label.set_text(&name);
        self.select_row(idx);

        let this = self.clone();
        let provider = self.provider.clone();
        gtk::glib::spawn_future_local(async move {
            let blocking = crate::utils::read_blocking(&path);
            let result = provider.read_file_opt(path, None, blocking).await;
            this.loading.set(false);
            match result {
                Ok(bytes) => {
                    let tags = super::tags::read_id3(&bytes);
                    this.title_label.set_text(&tags.headline(&name));
                    this.show_cover(tags.cover.as_deref());
                    if let Err(e) = this.player.play_bytes(name, bytes) {
                        this.title_label.set_text(&crate::i18n::trf(
                            "player.error",
                            &[("error", &e)],
                        ));
                    }
                }
                Err(e) => {
                    this.title_label.set_text(&crate::i18n::trf(
                        "player.read_error",
                        &[("error", &e.to_string())],
                    ));
                }
            }
        });
    }

    fn show_cover(&self, data: Option<&[u8]>) {
        let texture = data.and_then(|bytes| {
            gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from(bytes)).ok()
        });
        match texture {
            Some(texture) => {
                self.cover.set_paintable(Some(&texture));
                self.cover.set_pixel_size(160);
            }
            None => {
                self.cover.set_resource(Some("/com/icecommander/gtk/music.svg"));
                self.cover.set_pixel_size(96);
            }
        }
    }

    fn select_row(&self, idx: usize) {
        if let Some(row) = self.list.row_at_index(idx as i32) {
            self.list.select_row(Some(&row));
        }
    }

    fn step(self: &Rc<Self>, delta: i32) {
        let len = self.tracks.borrow().len() as i32;
        if len == 0 {
            return;
        }
        let current = self.current.get().unwrap_or(0) as i32;
        let next = current + delta;
        if next >= 0 && next < len {
            self.play(next as usize);
        }
    }

    fn tick(self: &Rc<Self>) {
        let position = self.player.position();
        let duration = self.player.duration();

        if !self.seeking.get() {
            if let Some(total) = duration {
                self.seek.set_range(0.0, total.as_secs_f64().max(1.0));
                self.seek.set_value(position.as_secs_f64());
            }
        }
        self.time_label.set_text(&match duration {
            Some(total) => format!("{} / {}", format_time(position), format_time(total)),
            None => format_time(position),
        });

        let icon = if self.player.is_playing() { "pause" } else { "play" };
        self.play_btn
            .set_child(Some(&gtk::Image::from_resource(&format!(
                "/com/icecommander/gtk/{icon}.svg"
            ))));

        if !self.loading.get() && self.player.is_finished() {
            let last = self.tracks.borrow().len().saturating_sub(1);
            match self.current.get() {
                Some(idx) if idx < last => self.step(1),
                _ => {}
            }
        }
    }

    fn fill_playlist(self: &Rc<Self>, dir: String, current_path: String) {
        let this = self.clone();
        let provider = self.provider.clone();
        gtk::glib::spawn_future_local(async move {
            let Ok(entries) = provider.list_dir(dir.clone()).await else {
                return;
            };
            let mut tracks: Vec<(String, String)> = entries
                .into_iter()
                .filter(|e| !e.is_dir && super::AUDIO_EXT.contains(&super::extension_of(&e.name).as_str()))
                .map(|e| {
                    let path = format!("{}/{}", dir.trim_end_matches('/'), e.name);
                    (e.name, path)
                })
                .collect();
            tracks.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

            let current = tracks.iter().position(|(_, p)| *p == current_path);
            for (name, _) in &tracks {
                let row = gtk::ListBoxRow::new();
                let label = gtk::Label::builder()
                    .label(name)
                    .halign(gtk::Align::Start)
                    .ellipsize(gtk::pango::EllipsizeMode::Middle)
                    .margin_start(8)
                    .margin_end(8)
                    .margin_top(4)
                    .margin_bottom(4)
                    .build();
                row.set_child(Some(&label));
                this.list.append(&row);
            }
            *this.tracks.borrow_mut() = tracks;
            if let Some(idx) = current {
                this.current.set(Some(idx));
                this.select_row(idx);
            }
        });
    }
}

pub(super) fn build_audio_content(ctx: &gtk_viewer_ui::ViewerCtx) {
    let file_path_str = ctx.path.as_str();
    let file_name = ctx.name.clone();
    let window = ctx.window.clone();
    window.set_title(Some(&format!(
        "{} - {}",
        crate::i18n::tr("player.audio_title"),
        file_name
    )));

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let cover = gtk::Image::from_resource("/com/icecommander/gtk/music.svg");
    cover.set_pixel_size(96);
    cover.set_margin_top(8);
    root.append(&cover);

    let title_label = gtk::Label::builder()
        .label(&file_name)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .max_width_chars(40)
        .build();
    title_label.add_css_class("title-3");
    root.append(&title_label);

    let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
    seek.set_draw_value(false);
    seek.set_hexpand(true);
    root.append(&seek);

    let time_label = gtk::Label::new(Some("0:00"));
    time_label.add_css_class("dim-label");
    root.append(&time_label);

    let controls = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::Center)
        .build();
    let prev_btn = icon_button("/com/icecommander/gtk/prev.svg", &crate::i18n::tr("player.previous"));
    let play_btn = icon_button("/com/icecommander/gtk/play.svg", &crate::i18n::tr("player.play_pause"));
    let next_btn = icon_button("/com/icecommander/gtk/next.svg", &crate::i18n::tr("player.next"));
    let stop_btn = icon_button("/com/icecommander/gtk/stop.svg", &crate::i18n::tr("player.stop"));
    controls.append(&prev_btn);
    controls.append(&play_btn);
    controls.append(&next_btn);
    controls.append(&stop_btn);

    let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
    volume.set_value(0.8);
    volume.set_draw_value(false);
    volume.set_width_request(110);
    volume.set_tooltip_text(Some(&crate::i18n::tr("player.volume")));
    controls.append(&gtk::Image::from_resource("/com/icecommander/gtk/high-volume.svg"));
    controls.append(&volume);
    root.append(&controls);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .build();
    list.add_css_class("boxed-list");
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    root.append(&scrolled);

    ctx.stack.add_named(&root, Some("content"));
    ctx.stack.set_visible_child_name("content");

    let player = AudioPlayer::new();
    let state = Rc::new(PlayerWindow {
        player: player.clone(),
        cover: cover.clone(),
        provider: ctx.provider.clone(),
        tracks: RefCell::new(vec![(file_name.clone(), file_path_str.to_string())]),
        current: Cell::new(Some(0)),
        list: list.clone(),
        title_label,
        time_label,
        seek: seek.clone(),
        play_btn: play_btn.clone(),
        seeking: Cell::new(false),
        loading: Cell::new(false),
    });

    {
        let state = state.clone();
        play_btn.connect_clicked(move |_| {
            state.player.toggle_play();
        });
    }
    {
        let state = state.clone();
        prev_btn.connect_clicked(move |_| state.step(-1));
    }
    {
        let state = state.clone();
        next_btn.connect_clicked(move |_| state.step(1));
    }
    {
        let player = player.clone();
        stop_btn.connect_clicked(move |_| player.stop());
    }
    {
        let player = player.clone();
        volume.connect_value_changed(move |scale| player.set_volume(scale.value() as f32));
    }
    {
        let state_press = state.clone();
        let gesture = gtk::GestureClick::new();
        gesture.connect_pressed(move |_, _, _, _| state_press.seeking.set(true));
        let state_release = state.clone();
        gesture.connect_released(move |_, _, _, _| {
            let target = Duration::from_secs_f64(state_release.seek.value());
            let _ = state_release.player.seek(target);
            state_release.seeking.set(false);
        });
        seek.add_controller(gesture);
    }
    {
        let state = state.clone();
        list.connect_row_activated(move |_, row| {
            state.play(row.index() as usize);
        });
    }

    let player_close = player.clone();
    window.connect_close_request(move |_| {
        player_close.stop();
        gtk::glib::Propagation::Proceed
    });

    let state_tick = state.clone();
    let root_tick = root.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(500), move || {
        if root_tick.root().is_none() {
            return gtk::glib::ControlFlow::Break;
        }
        state_tick.tick();
        gtk::glib::ControlFlow::Continue
    });

    state.play(0);
    state.fill_playlist(parent_dir(file_path_str), file_path_str.to_string());
}
