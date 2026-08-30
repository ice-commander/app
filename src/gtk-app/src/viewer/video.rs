use adw::prelude::*;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use libmpv2::{
    Mpv,
    render::{OpenGLInitParams, RenderParam, RenderParamApiType},
};
use std::cell::Cell;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct PlayerState {
    render_context: Option<libmpv2::render::RenderContext<'static>>,
    mpv: Box<Mpv>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Drop for PlayerState {
    fn drop(&mut self) {
        self.render_context = None;
    }
}

#[cfg(target_os = "macos")]
fn get_proc_address(_ctx: &(), name: &str) -> *mut std::ffi::c_void {
    let c_name = std::ffi::CString::new(name).expect("GL proc names never contain null bytes");
    unsafe {
        libc::dlsym(libc::RTLD_DEFAULT, c_name.as_ptr())
    }
}

#[cfg(target_os = "windows")]
extern "system" {
    fn GetModuleHandleA(lpModuleName: *const u8) -> isize;
    fn GetProcAddress(hModule: isize, lpProcName: *const u8) -> *mut std::ffi::c_void;
}

#[cfg(target_os = "windows")]
fn get_proc_address(_ctx: &(), name: &str) -> *mut std::ffi::c_void {
    let c_name = std::ffi::CString::new(name).expect("GL proc names never contain null bytes");
    let res = unsafe {
        let handle = GetModuleHandleA(b"libepoxy-0.dll\0".as_ptr());
        let mut addr = std::ptr::null_mut();
        if handle != 0 {
            type EpoxyGetProcAddressFn = unsafe extern "C" fn(name: *const i8) -> *mut std::ffi::c_void;
            let epoxy_gpa_ptr = GetProcAddress(handle, b"epoxy_get_proc_address\0".as_ptr());
            if !epoxy_gpa_ptr.is_null() {
                let epoxy_gpa: EpoxyGetProcAddressFn = std::mem::transmute(epoxy_gpa_ptr);
                addr = epoxy_gpa(c_name.as_ptr() as *const i8);
            }
        }
        if addr.is_null() {
            let opengl_handle = GetModuleHandleA(b"opengl32.dll\0".as_ptr());
            if opengl_handle != 0 {
                type WglGetProcAddressFn = unsafe extern "system" fn(name: *const i8) -> *mut std::ffi::c_void;
                let wgl_gpa_ptr = GetProcAddress(opengl_handle, b"wglGetProcAddress\0".as_ptr());
                if !wgl_gpa_ptr.is_null() {
                    let wgl_gpa: WglGetProcAddressFn = std::mem::transmute(wgl_gpa_ptr);
                    addr = wgl_gpa(c_name.as_ptr() as *const i8);
                }
                if addr.is_null() {
                    addr = GetProcAddress(opengl_handle, c_name.as_ptr() as *const u8);
                }
            }
        }
        addr
    };
    res
}

fn format_time(seconds: f64) -> String {
    let total_secs = seconds as u32;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn show_video_player_window(parent_window: &impl IsA<gtk::Window>, file_path_str: &str, config: client_config::AppConfig) -> gtk::Window {
    let file_name = std::path::Path::new(file_path_str)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    super::style::ensure_loaded();

    let window = gtk::Window::builder()
        .default_width(800)
        .default_height(600)
        .modal(true)
        .transient_for(parent_window)
        .title(&format!("{} - {}", crate::i18n::tr("player.video_title"), file_name))
        .build();

    window.add_css_class("video-player-window");
    window.set_decorated(false);




    let gl_area = gtk::GLArea::new();
    gl_area.set_hexpand(true);
    gl_area.set_vexpand(true);

    let double_click_gesture = gtk::GestureClick::new();
    let win_fs = window.clone();
    double_click_gesture.connect_pressed(move |gesture, n_press, _, _| {
        if n_press == 2 {
            if win_fs.is_fullscreen() {
                win_fs.unfullscreen();
            } else {
                win_fs.fullscreen();
            }
            gesture.set_state(gtk::EventSequenceState::Claimed);
        }
    });
    gl_area.add_controller(double_click_gesture);

    let handle = gtk::WindowHandle::builder()
        .child(&gl_area)
        .build();

    let overlay = gtk::Overlay::builder()
        .child(&handle)
        .build();

    let close_btn = gtk::Button::builder()
        .css_classes(vec!["flat", "circular"])
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_top(16)
        .margin_end(16)
        .child(&super::load_icon("/com/icecommander/gtk/close-white.svg"))
        .build();
    close_btn.add_css_class("video-overlay-close-btn");
    overlay.add_overlay(&close_btn);

    let title_label = gtk::Label::builder()
        .label(&file_name)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_top(16)
        .margin_start(16)
        .margin_end(80)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    title_label.add_css_class("video-overlay-title");
    overlay.add_overlay(&title_label);



    let win_close = window.clone();
    close_btn.connect_clicked(move |_| {
        win_close.close();
    });

    let controls_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_top(8)
        .margin_bottom(8)
        .valign(gtk::Align::End)
        .halign(gtk::Align::Fill)
        .build();

    let play_btn = gtk::Button::builder()
        .css_classes(vec!["flat", "circular"])
        .child(&super::load_icon("/com/icecommander/gtk/play.svg"))
        .build();
    controls_box.append(&play_btn);

    let time_label = gtk::Label::new(Some("00:00"));
    controls_box.append(&time_label);

    let seek_scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .draw_value(false)
        .build();
    controls_box.append(&seek_scale);

    let duration_label = gtk::Label::new(Some("00:00"));
    controls_box.append(&duration_label);

    let vol_icon = gtk::Image::from_resource("/com/icecommander/gtk/high-volume.svg");
    vol_icon.set_pixel_size(16);
    controls_box.append(&vol_icon);

    let volume_scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .adjustment(&gtk::Adjustment::new(0.8, 0.0, 1.0, 0.05, 0.1, 0.0))
        .width_request(80)
        .draw_value(false)
        .build();
    controls_box.append(&volume_scale);

    let fullscreen_btn = gtk::Button::builder()
        .css_classes(vec!["flat", "circular"])
        .child(&super::load_icon("/com/icecommander/gtk/expand.svg"))
        .build();
    controls_box.append(&fullscreen_btn);

    controls_box.add_css_class("video-controls-box");

    overlay.add_overlay(&controls_box);
    window.set_child(Some(&overlay));

    let win_fs_btn = window.clone();
    fullscreen_btn.connect_clicked(move |_| {
        if win_fs_btn.is_fullscreen() {
            win_fs_btn.unfullscreen();
        } else {
            win_fs_btn.fullscreen();
        }
    });

    let hide_timer = std::rc::Rc::new(std::cell::Cell::new(Option::<gtk::glib::SourceId>::None));
    let last_cursor_pos = std::rc::Rc::new(std::cell::Cell::new(Option::<(f64, f64)>::None));

    let motion_controller = gtk::EventControllerMotion::new();
    let win_motion = window.clone();
    let controls_box_motion = controls_box.clone();
    let close_btn_motion = close_btn.clone();
    let title_label_motion = title_label.clone();
    let hide_timer_motion = hide_timer.clone();
    let last_cursor_pos_motion = last_cursor_pos.clone();

    motion_controller.connect_motion(move |_, x, y| {
        if let Some((lx, ly)) = last_cursor_pos_motion.get() {
            if (x - lx).abs() < 1e-3 && (y - ly).abs() < 1e-3 {
                return;
            }
        }
        last_cursor_pos_motion.set(Some((x, y)));

        controls_box_motion.set_visible(true);
        close_btn_motion.set_visible(true);
        title_label_motion.set_visible(true);

        if let Some(source_id) = hide_timer_motion.replace(None) {
            source_id.remove();
        }

        if !win_motion.is_fullscreen() {
            let width = win_motion.width() as f64;
            let height = win_motion.height() as f64;
            let border = 8.0;
            let mut cursor_name = None;

            if x < border {
                if y < border {
                    cursor_name = Some("nw-resize");
                } else if y > height - border {
                    cursor_name = Some("sw-resize");
                } else {
                    cursor_name = Some("w-resize");
                }
            } else if x > width - border {
                if y < border {
                    cursor_name = Some("ne-resize");
                } else if y > height - border {
                    cursor_name = Some("se-resize");
                } else {
                    cursor_name = Some("e-resize");
                }
            } else if y < border {
                cursor_name = Some("n-resize");
            } else if y > height - border {
                cursor_name = Some("s-resize");
            }

            if let Some(name) = cursor_name {
                let cursor = gtk::gdk::Cursor::from_name(name, None);
                win_motion.set_cursor(cursor.as_ref());
            } else {
                win_motion.set_cursor(None);
            }
        } else {
            win_motion.set_cursor(None);
        }

        let win_timeout = win_motion.clone();
        let controls_timeout = controls_box_motion.clone();
        let close_btn_timeout = close_btn_motion.clone();
        let title_timeout = title_label_motion.clone();
        let hide_timer_timeout = hide_timer_motion.clone();
        let source_id = gtk::glib::timeout_add_local(
            std::time::Duration::from_millis(2500),
            move || {
                controls_timeout.set_visible(false);
                close_btn_timeout.set_visible(false);
                title_timeout.set_visible(false);
                let blank_cursor = gtk::gdk::Cursor::from_name("none", None);
                win_timeout.set_cursor(blank_cursor.as_ref());

                hide_timer_timeout.set(None);
                gtk::glib::ControlFlow::Break
            }
        );
        hide_timer_motion.set(Some(source_id));
    });
    motion_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    window.add_controller(motion_controller);

    let resize_gesture = gtk::GestureClick::new();
    resize_gesture.set_button(1);
    let win_resize = window.clone();
    resize_gesture.connect_pressed(move |gesture, n_press, x, y| {
        if win_resize.is_fullscreen() {
            return;
        }
        if n_press == 1 {
            let width = win_resize.width() as f64;
            let height = win_resize.height() as f64;
            let border = 8.0;
            let mut edge = None;

            if x < border {
                if y < border {
                    edge = Some(gtk::gdk::SurfaceEdge::NorthWest);
                } else if y > height - border {
                    edge = Some(gtk::gdk::SurfaceEdge::SouthWest);
                } else {
                    edge = Some(gtk::gdk::SurfaceEdge::West);
                }
            } else if x > width - border {
                if y < border {
                    edge = Some(gtk::gdk::SurfaceEdge::NorthEast);
                } else if y > height - border {
                    edge = Some(gtk::gdk::SurfaceEdge::SouthEast);
                } else {
                    edge = Some(gtk::gdk::SurfaceEdge::East);
                }
            } else if y < border {
                edge = Some(gtk::gdk::SurfaceEdge::North);
            } else if y > height - border {
                edge = Some(gtk::gdk::SurfaceEdge::South);
            }

            if let Some(edge) = edge {
                if let Some(surface) = win_resize.surface() {
                    if let Ok(toplevel) = surface.downcast::<gtk::gdk::Toplevel>() {
                        let device = gesture.device();
                        let sequence = gesture.current_sequence();
                        let event = gesture.last_event(sequence.as_ref());
                        let timestamp = event.map(|e| e.time()).unwrap_or(0);

                        toplevel.begin_resize(
                            edge,
                            device.as_ref(),
                            1,
                            x,
                            y,
                            timestamp,
                        );
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                    }
                }
            }
        }
    });
    resize_gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
    window.add_controller(resize_gesture);

    let controls_box_fs = controls_box.clone();
    let close_btn_fs = close_btn.clone();
    let title_label_fs = title_label.clone();
    let fullscreen_btn_clone = fullscreen_btn.clone();
    let hide_timer_fs = hide_timer.clone();
    window.connect_notify_local(Some("fullscreened"), move |win, _| {
        if win.is_fullscreen() {
            let img = super::load_icon("/com/icecommander/gtk/collapse.svg");
            fullscreen_btn_clone.set_child(Some(&img));

            let win_timeout = win.clone();
            let controls_timeout = controls_box_fs.clone();
            let close_btn_timeout = close_btn_fs.clone();
            let title_timeout = title_label_fs.clone();
            let hide_timer_timeout = hide_timer_fs.clone();

            if let Some(source_id) = hide_timer_fs.replace(None) {
                source_id.remove();
            }

            let source_id = gtk::glib::timeout_add_local(
                std::time::Duration::from_millis(2500),
                move || {
                    controls_timeout.set_visible(false);
                    close_btn_timeout.set_visible(false);
                    title_timeout.set_visible(false);
                    let blank_cursor = gtk::gdk::Cursor::from_name("none", None);
                    win_timeout.set_cursor(blank_cursor.as_ref());

                    hide_timer_timeout.set(None);
                    gtk::glib::ControlFlow::Break
                }
            );
            hide_timer_fs.set(Some(source_id));
        } else {
            let img = super::load_icon("/com/icecommander/gtk/expand.svg");
            fullscreen_btn_clone.set_child(Some(&img));

            if let Some(source_id) = hide_timer_fs.replace(None) {
                source_id.remove();
            }
            controls_box_fs.set_visible(true);
            close_btn_fs.set_visible(true);
            title_label_fs.set_visible(true);
            win.set_cursor(None);
        }
    });

    {
        let win_timeout = window.clone();
        let controls_timeout = controls_box.clone();
        let close_btn_timeout = close_btn.clone();
        let title_timeout = title_label.clone();
        let hide_timer_timeout = hide_timer.clone();
        let source_id = gtk::glib::timeout_add_local(
            std::time::Duration::from_millis(2500),
            move || {
                controls_timeout.set_visible(false);
                close_btn_timeout.set_visible(false);
                title_timeout.set_visible(false);
                let blank_cursor = gtk::gdk::Cursor::from_name("none", None);
                win_timeout.set_cursor(blank_cursor.as_ref());

                hide_timer_timeout.set(None);
                gtk::glib::ControlFlow::Break
            }
        );
        hide_timer.set(Some(source_id));
    }

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let gl_area_weak = gl_area.downgrade();
    gtk::glib::idle_add_local(move || {
        let mut redraw = false;
        while let Ok(_) = rx.try_recv() {
            redraw = true;
        }
        if redraw {
            if let Some(a) = gl_area_weak.upgrade() {
                a.queue_render();
            }
        }
        gtk::glib::ControlFlow::Continue
    });

    let state = Rc::new(RefCell::new(None));

    let state_realize = state.clone();
    let file_path_clone = file_path_str.to_string();
    gl_area.connect_realize(move |area| {
        area.make_current();
        if let Some(e) = area.error() {
            eprintln!("GLArea realize error: {:?}", e);
            return;
        }

        #[cfg(unix)]
        unsafe {
            libc::setlocale(libc::LC_NUMERIC, b"C\0".as_ptr() as *const libc::c_char);
        }
        #[cfg(windows)]
        unsafe {
            extern "C" {
                fn setlocale(category: i32, locale: *const u8) -> *mut u8;
            }
            setlocale(4, b"C\0".as_ptr()); // LC_NUMERIC = 4 in Windows
        }

        let mpv = Box::new(
            Mpv::with_initializer(|init| {
                init.set_property("vo", "libmpv")?;
                init.set_property("wid", "0")?;
                Ok(())
            })
            .expect("Failed to initialize MPV")
        );

        let mpv_ref: &'static Mpv = unsafe { std::mem::transmute(&*mpv) };

        let ctx = mpv_ref
            .create_render_context(vec![
                RenderParam::ApiType(RenderParamApiType::OpenGl),
                RenderParam::InitParams(OpenGLInitParams {
                    get_proc_address,
                    ctx: (),
                }),
            ])
            .expect("Failed to create MPV render context");

        let mut ctx_cb = ctx;
        let tx_clone = tx.clone();
        ctx_cb.set_update_callback(move || {
            let _ = tx_clone.send(());
        });

        mpv.set_property("volume", 80.0).ok();

        *state_realize.borrow_mut() = Some(PlayerState {
            render_context: Some(ctx_cb),
            mpv,
        });

        if let Some(ref s) = *state_realize.borrow() {
            s.mpv.command("loadfile", &[&file_path_clone]).expect("Failed to load video file");
        }
    });

    let state_draw = state.clone();
    gl_area.connect_render(move |area, _gl_ctx| {
        if let Some(ref mut s) = *state_draw.borrow_mut() {
            if let Some(ref mut ctx) = s.render_context {
                let scale_factor = area.scale_factor();
                let width = area.width() * scale_factor;
                let height = area.height() * scale_factor;
                let mut fbo: i32 = 0;
                unsafe {
                    type GlGetIntegervFn = unsafe extern "C" fn(pname: u32, params: *mut i32);
                    let gl_get_integerv_ptr = get_proc_address(&(), "glGetIntegerv");
                    if !gl_get_integerv_ptr.is_null() {
                        let gl_get_integerv: GlGetIntegervFn = std::mem::transmute(gl_get_integerv_ptr);
                        gl_get_integerv(0x8CA6, &mut fbo);
                    }
                }
                let _ = ctx.render::<()>(fbo, width, height, true);
            }
        }
        gtk::glib::Propagation::Proceed
    });

    let state_unrealize = state.clone();
    gl_area.connect_unrealize(move |_area| {
        *state_unrealize.borrow_mut() = None;
    });

    let state_play = state.clone();
    play_btn.connect_clicked(move |_| {
        if let Some(ref s) = *state_play.borrow() {
            let is_paused: bool = s.mpv.get_property("pause").unwrap_or(false);
            s.mpv.set_property("pause", !is_paused).ok();
        }
    });

    let state_volume = state.clone();
    let vol_icon_state = vol_icon.clone();
    volume_scale.connect_value_changed(move |scale| {
        if let Some(ref s) = *state_volume.borrow() {
            let val = scale.value() * 100.0;
            s.mpv.set_property("volume", val).ok();
        }
        vol_icon_state.set_resource(Some(if scale.value() >= 0.5 {
            "/com/icecommander/gtk/high-volume.svg"
        } else {
            "/com/icecommander/gtk/low-volume.svg"
        }));
    });

    let is_updating = Rc::new(std::cell::Cell::new(false));

    let state_seek = state.clone();
    let is_updating_seek = is_updating.clone();
    seek_scale.connect_value_changed(move |scale| {
        if !is_updating_seek.get() {
            if let Some(ref s) = *state_seek.borrow() {
                let val = scale.value();
                s.mpv.set_property("time-pos", val).ok();
            }
        }
    });

    let state_tick = state.clone();
    let seek_scale_clone = seek_scale.clone();
    let time_label_clone = time_label.clone();
    let duration_label_clone = duration_label.clone();
    let play_btn_clone = play_btn.clone();
    let is_updating_clone = is_updating.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        if let Some(ref s) = *state_tick.borrow() {
            if let Ok(duration) = s.mpv.get_property::<f64>("duration") {
                if let Ok(time_pos) = s.mpv.get_property::<f64>("time-pos") {
                    is_updating_clone.set(true);

                    seek_scale_clone.set_range(0.0, duration);
                    seek_scale_clone.set_value(time_pos);

                    time_label_clone.set_text(&format_time(time_pos));
                    duration_label_clone.set_text(&format_time(duration));

                    let is_paused: bool = s.mpv.get_property("pause").unwrap_or(false);
                    if is_paused {
                        let img = super::load_icon("/com/icecommander/gtk/play.svg");
                        play_btn_clone.set_child(Some(&img));
                    } else {
                        let img = super::load_icon("/com/icecommander/gtk/pause.svg");
                        play_btn_clone.set_child(Some(&img));
                    }

                    is_updating_clone.set(false);
                }
            }
        }
        gtk::glib::ControlFlow::Continue
    });

    let state_close = state.clone();
    window.connect_close_request(move |_| {
        if let Some(ref s) = *state_close.borrow() {
            s.mpv.command("stop", &[]).ok();
        }
        *state_close.borrow_mut() = None;
        crate::api::notify_viewer_closed();
        gtk::glib::Propagation::Proceed
    });

    let key_controller = gtk::EventControllerKey::new();
    let win_key = window.clone();
    let state_key = state.clone();
    let config_key = config.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, state| {
        if let Some(action) = crate::hotkey::resolve_action(&config_key, keyval, state) {
            if action == "toggle_video_fullscreen" {
                if win_key.is_fullscreen() {
                    win_key.unfullscreen();
                } else {
                    win_key.fullscreen();
                }
                return gtk::glib::Propagation::Stop;
            }
        }

        let is_alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
        let is_enter = keyval == gtk::gdk::Key::Return || keyval == gtk::gdk::Key::KP_Enter;
        let is_f = keyval == gtk::gdk::Key::f || keyval == gtk::gdk::Key::F || keyval == gtk::gdk::Key::F11;

        if (is_alt && is_enter) || is_f {
            if win_key.is_fullscreen() {
                win_key.unfullscreen();
            } else {
                win_key.fullscreen();
            }
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::Escape {
            if win_key.is_fullscreen() {
                win_key.unfullscreen();
            } else {
                win_key.close();
            }
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::space {
            if let Some(ref s) = *state_key.borrow() {
                let is_paused: bool = s.mpv.get_property("pause").unwrap_or(false);
                s.mpv.set_property("pause", !is_paused).ok();
            }
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::Left {
            if let Some(ref s) = *state_key.borrow() {
                if let Ok(time_pos) = s.mpv.get_property::<f64>("time-pos") {
                    s.mpv.set_property("time-pos", (time_pos - 10.0).max(0.0)).ok();
                }
            }
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::Right {
            if let Some(ref s) = *state_key.borrow() {
                if let Ok(time_pos) = s.mpv.get_property::<f64>("time-pos") {
                    if let Ok(duration) = s.mpv.get_property::<f64>("duration") {
                        s.mpv.set_property("time-pos", (time_pos + 10.0).min(duration)).ok();
                    }
                }
            }
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    window.add_controller(key_controller);

    window.present();
    window
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn show_video_player_window(parent_window: &impl IsA<gtk::Window>, file_path_str: &str, config: client_config::AppConfig) -> gtk::Window {
    let file_name = std::path::Path::new(file_path_str)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    super::style::ensure_loaded();

    let window = gtk::Window::builder()
        .default_width(800)
        .default_height(600)
        .modal(true)
        .transient_for(parent_window)
        .title(&format!("{} - {}", crate::i18n::tr("player.video_title"), file_name))
        .build();

    window.add_css_class("video-player-window");
    window.set_decorated(false);


    let file = gtk::gio::File::for_path(file_path_str);
    let video = gtk::Video::builder()
        .file(&file)
        .autoplay(true)
        .hexpand(true)
        .vexpand(true)
        .build();

    if let Some(child1) = video.first_child() {
        if let Some(child2) = child1.last_child() {
            child2.set_visible(false);
        }
    }

    let stream = video.media_stream().expect("No media stream in GtkVideo");

    let win_err = window.clone();
    let reported = Rc::new(Cell::new(false));
    stream.connect_error_notify(move |s| {
        let Some(err) = s.error() else {
            return;
        };
        if reported.replace(true) {
            return;
        }
        let dialog = adw::AlertDialog::builder()
            .heading(&*crate::i18n::tr("player.video_title"))
            .body(&err.to_string())
            .build();
        dialog.add_response("ok", "OK");
        dialog.set_default_response(Some("ok"));
        dialog.present(Some(&win_err));
    });

    let double_click_gesture = gtk::GestureClick::new();
    let win_fs = window.clone();
    double_click_gesture.connect_pressed(move |gesture, n_press, _, _| {
        if n_press == 2 {
            if win_fs.is_fullscreen() {
                win_fs.unfullscreen();
            } else {
                win_fs.fullscreen();
            }
            gesture.set_state(gtk::EventSequenceState::Claimed);
        }
    });
    video.add_controller(double_click_gesture);

    let handle = gtk::WindowHandle::builder()
        .child(&video)
        .build();

    let overlay = gtk::Overlay::builder()
        .child(&handle)
        .build();

    let close_btn = gtk::Button::builder()
        .css_classes(vec!["flat", "circular"])
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_top(16)
        .margin_end(16)
        .child(&super::load_icon("/com/icecommander/gtk/close-white.svg"))
        .build();
    close_btn.add_css_class("video-overlay-close-btn");
    overlay.add_overlay(&close_btn);

    let title_label = gtk::Label::builder()
        .label(&file_name)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_top(16)
        .margin_start(16)
        .margin_end(80)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    title_label.add_css_class("video-overlay-title");
    overlay.add_overlay(&title_label);



    let win_close = window.clone();
    close_btn.connect_clicked(move |_| {
        win_close.close();
    });

    let controls_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_top(8)
        .margin_bottom(8)
        .valign(gtk::Align::End)
        .halign(gtk::Align::Fill)
        .build();

    let play_btn = gtk::Button::builder()
        .css_classes(vec!["flat", "circular"])
        .child(&super::load_icon("/com/icecommander/gtk/play.svg"))
        .build();
    controls_box.append(&play_btn);

    let time_label = gtk::Label::new(Some("00:00"));
    controls_box.append(&time_label);

    let seek_scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .draw_value(false)
        .build();
    controls_box.append(&seek_scale);

    let duration_label = gtk::Label::new(Some("00:00"));
    controls_box.append(&duration_label);

    let vol_icon = gtk::Image::from_resource("/com/icecommander/gtk/high-volume.svg");
    vol_icon.set_pixel_size(16);
    controls_box.append(&vol_icon);

    let volume_scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .adjustment(&gtk::Adjustment::new(0.8, 0.0, 1.0, 0.05, 0.1, 0.0))
        .width_request(80)
        .draw_value(false)
        .build();
    controls_box.append(&volume_scale);

    let fullscreen_btn = gtk::Button::builder()
        .css_classes(vec!["flat", "circular"])
        .child(&super::load_icon("/com/icecommander/gtk/expand.svg"))
        .build();
    controls_box.append(&fullscreen_btn);

    controls_box.add_css_class("video-controls-box");

    overlay.add_overlay(&controls_box);
    window.set_child(Some(&overlay));

    let win_fs_btn = window.clone();
    fullscreen_btn.connect_clicked(move |_| {
        if win_fs_btn.is_fullscreen() {
            win_fs_btn.unfullscreen();
        } else {
            win_fs_btn.fullscreen();
        }
    });

    let stream_play = stream.clone();
    play_btn.connect_clicked(move |_| {
        let is_playing = stream_play.is_playing();
        stream_play.set_playing(!is_playing);
    });

    let stream_volume = stream.clone();
    stream_volume.set_volume(0.8);
    let vol_icon_stream = vol_icon.clone();
    volume_scale.connect_value_changed(move |scale| {
        let val = scale.value();
        stream_volume.set_volume(val);
        vol_icon_stream.set_resource(Some(if val >= 0.5 {
            "/com/icecommander/gtk/high-volume.svg"
        } else {
            "/com/icecommander/gtk/low-volume.svg"
        }));
    });

    let is_updating = Rc::new(Cell::new(false));

    let stream_seek = stream.clone();
    let is_updating_seek = is_updating.clone();
    seek_scale.connect_value_changed(move |scale| {
        if !is_updating_seek.get() {
            let val = scale.value();
            let us = (val * 1_000_000.0) as i64;
            stream_seek.seek(us);
        }
    });

    let stream_tick = stream.clone();
    let seek_scale_clone = seek_scale.clone();
    let time_label_clone = time_label.clone();
    let duration_label_clone = duration_label.clone();
    let play_btn_clone = play_btn.clone();
    let is_updating_clone = is_updating.clone();

    let tick_id = gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        let duration_us = stream_tick.duration();
        let position_us = stream_tick.timestamp();
        if duration_us > 0 {
            let duration = duration_us as f64 / 1_000_000.0;
            let time_pos = position_us as f64 / 1_000_000.0;

            is_updating_clone.set(true);
            seek_scale_clone.set_range(0.0, duration);
            seek_scale_clone.set_value(time_pos);

            time_label_clone.set_text(&format_time(time_pos));
            duration_label_clone.set_text(&format_time(duration));
            is_updating_clone.set(false);
        }

        let is_playing = stream_tick.is_playing();
        if is_playing {
            let img = super::load_icon("/com/icecommander/gtk/pause.svg");
            play_btn_clone.set_child(Some(&img));
        } else {
            let img = super::load_icon("/com/icecommander/gtk/play.svg");
            play_btn_clone.set_child(Some(&img));
        }

        gtk::glib::ControlFlow::Continue
    });

    let stream_close = stream.clone();
    let video_close = video.clone();
    let tick_id = std::cell::RefCell::new(Some(tick_id));
    window.connect_close_request(move |_| {
        if let Some(id) = tick_id.borrow_mut().take() {
            id.remove();
        }
        stream_close.set_playing(false);
        video_close.set_media_stream(gtk::MediaStream::NONE);
        crate::api::notify_viewer_closed();
        gtk::glib::Propagation::Proceed
    });

    let hide_timer = Rc::new(Cell::new(Option::<gtk::glib::SourceId>::None));
    let last_cursor_pos = Rc::new(Cell::new(Option::<(f64, f64)>::None));

    let motion_controller = gtk::EventControllerMotion::new();
    let win_motion = window.clone();
    let controls_box_motion = controls_box.clone();
    let close_btn_motion = close_btn.clone();
    let title_label_motion = title_label.clone();
    let hide_timer_motion = hide_timer.clone();
    let last_cursor_pos_motion = last_cursor_pos.clone();

    motion_controller.connect_motion(move |_, x, y| {
        if let Some((lx, ly)) = last_cursor_pos_motion.get() {
            if (x - lx).abs() < 1e-3 && (y - ly).abs() < 1e-3 {
                return;
            }
        }
        last_cursor_pos_motion.set(Some((x, y)));

        controls_box_motion.set_visible(true);
        close_btn_motion.set_visible(true);
        title_label_motion.set_visible(true);

        if let Some(source_id) = hide_timer_motion.replace(None) {
            source_id.remove();
        }

        if !win_motion.is_fullscreen() {
            let width = win_motion.width() as f64;
            let height = win_motion.height() as f64;
            let border = 8.0;
            let mut cursor_name = None;

            if x < border {
                if y < border {
                    cursor_name = Some("nw-resize");
                } else if y > height - border {
                    cursor_name = Some("sw-resize");
                } else {
                    cursor_name = Some("w-resize");
                }
            } else if x > width - border {
                if y < border {
                    cursor_name = Some("ne-resize");
                } else if y > height - border {
                    cursor_name = Some("se-resize");
                } else {
                    cursor_name = Some("e-resize");
                }
            } else if y < border {
                cursor_name = Some("n-resize");
            } else if y > height - border {
                cursor_name = Some("s-resize");
            }

            if let Some(name) = cursor_name {
                let cursor = gtk::gdk::Cursor::from_name(name, None);
                win_motion.set_cursor(cursor.as_ref());
            } else {
                win_motion.set_cursor(None);
            }
        } else {
            win_motion.set_cursor(None);
        }

        let win_timeout = win_motion.clone();
        let controls_timeout = controls_box_motion.clone();
        let close_btn_timeout = close_btn_motion.clone();
        let title_timeout = title_label_motion.clone();
        let hide_timer_timeout = hide_timer_motion.clone();
        let source_id = gtk::glib::timeout_add_local(
            std::time::Duration::from_millis(2500),
            move || {
                controls_timeout.set_visible(false);
                close_btn_timeout.set_visible(false);
                title_timeout.set_visible(false);
                let blank_cursor = gtk::gdk::Cursor::from_name("none", None);
                win_timeout.set_cursor(blank_cursor.as_ref());

                hide_timer_timeout.set(None);
                gtk::glib::ControlFlow::Break
            }
        );
        hide_timer_motion.set(Some(source_id));
    });
    motion_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    window.add_controller(motion_controller);

    let resize_gesture = gtk::GestureClick::new();
    resize_gesture.set_button(1);
    let win_resize = window.clone();
    resize_gesture.connect_pressed(move |gesture, n_press, x, y| {
        if win_resize.is_fullscreen() {
            return;
        }
        if n_press == 1 {
            let width = win_resize.width() as f64;
            let height = win_resize.height() as f64;
            let border = 8.0;
            let mut edge = None;

            if x < border {
                if y < border {
                    edge = Some(gtk::gdk::SurfaceEdge::NorthWest);
                } else if y > height - border {
                    edge = Some(gtk::gdk::SurfaceEdge::SouthWest);
                } else {
                    edge = Some(gtk::gdk::SurfaceEdge::West);
                }
            } else if x > width - border {
                if y < border {
                    edge = Some(gtk::gdk::SurfaceEdge::NorthEast);
                } else if y > height - border {
                    edge = Some(gtk::gdk::SurfaceEdge::SouthEast);
                } else {
                    edge = Some(gtk::gdk::SurfaceEdge::East);
                }
            } else if y < border {
                edge = Some(gtk::gdk::SurfaceEdge::North);
            } else if y > height - border {
                edge = Some(gtk::gdk::SurfaceEdge::South);
            }

            if let Some(edge) = edge {
                if let Some(surface) = win_resize.surface() {
                    if let Ok(toplevel) = surface.downcast::<gtk::gdk::Toplevel>() {
                        let device = gesture.device();
                        let sequence = gesture.current_sequence();
                        let event = gesture.last_event(sequence.as_ref());
                        let timestamp = event.map(|e| e.time()).unwrap_or(0);

                        toplevel.begin_resize(
                            edge,
                            device.as_ref(),
                            1,
                            x,
                            y,
                            timestamp,
                        );
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                    }
                }
            }
        }
    });
    resize_gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
    window.add_controller(resize_gesture);

    let controls_box_fs = controls_box.clone();
    let close_btn_fs = close_btn.clone();
    let title_label_fs = title_label.clone();
    let fullscreen_btn_clone = fullscreen_btn.clone();
    let hide_timer_fs = hide_timer.clone();
    window.connect_notify_local(Some("fullscreened"), move |win, _| {
        if win.is_fullscreen() {
            let img = super::load_icon("/com/icecommander/gtk/collapse.svg");
            fullscreen_btn_clone.set_child(Some(&img));

            let win_timeout = win.clone();
            let controls_timeout = controls_box_fs.clone();
            let close_btn_timeout = close_btn_fs.clone();
            let title_timeout = title_label_fs.clone();
            let hide_timer_timeout = hide_timer_fs.clone();

            if let Some(source_id) = hide_timer_fs.replace(None) {
                source_id.remove();
            }

            let source_id = gtk::glib::timeout_add_local(
                std::time::Duration::from_millis(2500),
                move || {
                    controls_timeout.set_visible(false);
                    close_btn_timeout.set_visible(false);
                    title_timeout.set_visible(false);
                    let blank_cursor = gtk::gdk::Cursor::from_name("none", None);
                    win_timeout.set_cursor(blank_cursor.as_ref());

                    hide_timer_timeout.set(None);
                    gtk::glib::ControlFlow::Break
                }
            );
            hide_timer_fs.set(Some(source_id));
        } else {
            let img = super::load_icon("/com/icecommander/gtk/expand.svg");
            fullscreen_btn_clone.set_child(Some(&img));

            if let Some(source_id) = hide_timer_fs.replace(None) {
                source_id.remove();
            }
            controls_box_fs.set_visible(true);
            close_btn_fs.set_visible(true);
            title_label_fs.set_visible(true);
            win.set_cursor(None);
        }
    });

    {
        let win_timeout = window.clone();
        let controls_timeout = controls_box.clone();
        let close_btn_timeout = close_btn.clone();
        let title_timeout = title_label.clone();
        let hide_timer_timeout = hide_timer.clone();
        let source_id = gtk::glib::timeout_add_local(
            std::time::Duration::from_millis(2500),
            move || {
                controls_timeout.set_visible(false);
                close_btn_timeout.set_visible(false);
                title_timeout.set_visible(false);
                let blank_cursor = gtk::gdk::Cursor::from_name("none", None);
                win_timeout.set_cursor(blank_cursor.as_ref());

                hide_timer_timeout.set(None);
                gtk::glib::ControlFlow::Break
            }
        );
        hide_timer.set(Some(source_id));
    }

    let key_controller = gtk::EventControllerKey::new();
    let win_key = window.clone();
    let stream_key = stream.clone();
    let config_key = config.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, state| {
        if let Some(action) = crate::hotkey::resolve_action(&config_key, keyval, state) {
            if action == "toggle_video_fullscreen" {
                if win_key.is_fullscreen() {
                    win_key.unfullscreen();
                } else {
                    win_key.fullscreen();
                }
                return gtk::glib::Propagation::Stop;
            }
        }

        let is_alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
        let is_enter = keyval == gtk::gdk::Key::Return || keyval == gtk::gdk::Key::KP_Enter;
        let is_f = keyval == gtk::gdk::Key::f || keyval == gtk::gdk::Key::F || keyval == gtk::gdk::Key::F11;

        if (is_alt && is_enter) || is_f {
            if win_key.is_fullscreen() {
                win_key.unfullscreen();
            } else {
                win_key.fullscreen();
            }
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::Escape {
            if win_key.is_fullscreen() {
                win_key.unfullscreen();
            } else {
                win_key.close();
            }
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::space {
            let is_playing = stream_key.is_playing();
            stream_key.set_playing(!is_playing);
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::Left {
            let current_pos_us = stream_key.timestamp();
            let new_pos_us = (current_pos_us - 10_000_000).max(0);
            stream_key.seek(new_pos_us);
            gtk::glib::Propagation::Stop
        } else if keyval == gtk::gdk::Key::Right {
            let current_pos_us = stream_key.timestamp();
            let duration_us = stream_key.duration();
            let new_pos_us = (current_pos_us + 10_000_000).min(duration_us);
            stream_key.seek(new_pos_us);
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    window.add_controller(key_controller);

    window.present();
    window
}

#[cfg(target_os = "linux")]
pub(super) fn prompt_codec_installation(
    parent_window: &impl IsA<gtk::Window>,
    file_path: &str,
    router: &std::rc::Rc<panel_router::PanelRouter>,
) {
    let dialog = adw::AlertDialog::builder()
        .heading(&*crate::i18n::tr("player.missing_codecs_title"))
        .body(&*crate::i18n::tr("player.missing_codecs_body"))
        .build();

    dialog.add_response("install", &*crate::i18n::tr("player.install_btn"));
    dialog.add_response("cancel", &*crate::i18n::tr("player.cancel_btn"));
    dialog.set_default_response(Some("install"));
    dialog.set_response_appearance("install", adw::ResponseAppearance::Suggested);

    let parent_win_clone = parent_window.clone();
    let file_path_clone = file_path.to_string();
    let router_clone = router.clone();

    dialog.connect_response(None, move |d, response| {
        d.close();
        if response == "install" {
            let parent_win_inner = parent_win_clone.clone();
            let path_inner = file_path_clone.clone();
            let router_inner_future = router_clone.clone();

            gtk::glib::spawn_future_local(async move {
                let progress_dlg = adw::AlertDialog::builder()
                    .heading(&*crate::i18n::tr("player.installing_codecs_title"))
                    .body(&*crate::i18n::tr("player.installing_codecs_body"))
                    .build();
                progress_dlg.present(Some(parent_win_inner.upcast_ref::<gtk::Window>()));

                let res = ic_platform::video_codecs::run_video_codecs_installer().await;
                progress_dlg.close();

                match res {
                    Ok(()) => {
                        ic_platform::video_codecs::update_gstreamer_registry();

                        let info_dlg = adw::AlertDialog::builder()
                            .heading(&*crate::i18n::tr("player.codecs_installed_title"))
                            .body(&*crate::i18n::tr("player.codecs_installed_body"))
                            .build();
                        info_dlg.add_response("open", &*crate::i18n::tr("player.open_video_btn"));
                        info_dlg.add_response("close", &*crate::i18n::tr("player.close_btn"));
                        info_dlg.set_default_response(Some("open"));
                        info_dlg.set_response_appearance("open", adw::ResponseAppearance::Suggested);

                        let parent_for_open = parent_win_inner.clone();
                        let path_for_open = path_inner.clone();
                        let router_for_open = router_inner_future.clone();
                        info_dlg.connect_response(None, move |d, response| {
                            d.close();
                            if response == "open" {
                                show_video_player_window(&parent_for_open, &path_for_open, router_for_open.config());
                            }
                        });
                        info_dlg.present(Some(parent_win_inner.upcast_ref::<gtk::Window>()));
                    }
                    Err(e) => {
                        let err_dlg = adw::AlertDialog::builder()
                            .heading(&*crate::i18n::tr("player.codecs_error_title"))
                            .body(&format!("{}", e))
                            .build();
                        err_dlg.add_response("close", &*crate::i18n::tr("player.close_btn"));
                        err_dlg.present(Some(parent_win_inner.upcast_ref::<gtk::Window>()));
                    }
                }
            });
        } else {
            let name = std::path::Path::new(&file_path_clone)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            crate::viewer::open_in_host(
                &parent_win_clone,
                file_path_clone.clone(),
                name,
                router_clone.clone(),
                false,
            );
        }
    });

    dialog.present(Some(parent_window.upcast_ref::<gtk::Window>()));
}
