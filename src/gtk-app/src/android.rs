//! Android: GTK's runtime loads libice_commander_gtk.so and runs its `main()` on the GTK thread.

use std::ffi::{c_char, c_int};
use std::rc::Rc;

use gtk::{gdk, glib::translate::ToGlibPtr, prelude::*};
use jni::{
    errors::{Error, Result as JniResult},
    objects::{JObject, JValue},
    JNIEnv,
};

use crate::Platform;

extern "C" {
    fn gdk_android_display_get_env(display: *mut gdk::ffi::GdkDisplay) -> *mut jni::sys::JNIEnv;
    fn gdk_android_toplevel_get_activity(toplevel: *mut gdk::ffi::GdkSurface) -> jni::sys::jobject;
}

struct Android;

/// ndk-context panics when it is asked before `attach_services` has filled it in, and the drive
/// list is built while the application activates, long before the window is mapped.
static SERVICES_ATTACHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Platform for Android {
    fn mobile(&self) -> bool {
        true
    }

    /// Like Back on any Android app's first screen: the task goes behind, the app keeps running.
    fn put_away(&self, window: &gtk::Window) -> bool {
        with_activity(window, |env, activity| {
            env.call_method(activity, "moveTaskToBack", "(Z)Z", &[JValue::Bool(1)])?
                .z()
        })
        .unwrap_or_else(|e| {
            gtk::glib::g_warning!(
                "ice-commander",
                "cannot move the app to the background: {e}"
            );
            false
        })
    }

    fn attach_services(&self, window: &gtk::Window) {
        thread_local! {
            static ATTACHED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        }
        if ATTACHED.with(|done| done.get()) {
            return;
        }
        let attached = with_activity(window, |env, activity| {
            let context = env
                .call_method(
                    activity,
                    "getApplicationContext",
                    "()Landroid/content/Context;",
                    &[],
                )?
                .l()?;
            let context = env.new_global_ref(context)?;
            let vm = env.get_java_vm()?;
            // SAFETY: called once, with the process's VM and a global reference that is never
            // released, so both stay valid for as long as the process runs.
            unsafe {
                ndk_context::initialize_android_context(
                    vm.get_java_vm_pointer().cast(),
                    context.as_obj().as_raw().cast(),
                );
            }
            std::mem::forget(context);
            Ok(())
        });
        match attached {
            Ok(()) => {
                ATTACHED.with(|done| done.set(true));
                SERVICES_ATTACHED.store(true, std::sync::atomic::Ordering::Release);
            }
            Err(e) => gtk::glib::g_warning!("ice-commander", "cannot attach Android services: {e}"),
        }
    }

    fn keep_screen_on(&self, window: &gtk::Window, on: bool) {
        let kept = with_activity(window, |env, activity| {
            let application = env
                .call_method(
                    activity,
                    "getApplication",
                    "()Landroid/app/Application;",
                    &[],
                )?
                .l()?;
            env.call_method(
                &application,
                "keepScreenOn",
                "(Landroid/app/Activity;Z)V",
                &[JValue::Object(activity), JValue::Bool(u8::from(on))],
            )?;
            Ok(())
        });
        if let Err(e) = kept {
            gtk::glib::g_warning!("ice-commander", "cannot keep the screen on: {e}");
        }
    }

    fn paint_system_bars(&self, window: &gtk::Window, argb: i32) {
        let painted = with_activity(window, |env, activity| {
            let application = env
                .call_method(
                    activity,
                    "getApplication",
                    "()Landroid/app/Application;",
                    &[],
                )?
                .l()?;
            env.call_method(
                &application,
                "systemBars",
                "(Landroid/app/Activity;I)V",
                &[JValue::Object(activity), JValue::Int(argb)],
            )?;
            Ok(())
        });
        if let Err(e) = painted {
            gtk::glib::g_warning!("ice-commander", "cannot colour the system bars: {e}");
        }
    }

    fn ensure_storage_access(&self, window: &gtk::Window) {
        let app_id = self.app_id();
        let asked = with_activity(window, |env, activity| {
            let granted = env
                .call_static_method(
                    "android/os/Environment",
                    "isExternalStorageManager",
                    "()Z",
                    &[],
                )?
                .z()?;
            if granted {
                return Ok(());
            }
            let action =
                env.new_string("android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION")?;
            let package = env.new_string(format!("package:{app_id}"))?;
            let uri = env
                .call_static_method(
                    "android/net/Uri",
                    "parse",
                    "(Ljava/lang/String;)Landroid/net/Uri;",
                    &[JValue::Object(&package)],
                )?
                .l()?;
            let intent = env.new_object(
                "android/content/Intent",
                "(Ljava/lang/String;Landroid/net/Uri;)V",
                &[JValue::Object(&action), JValue::Object(&uri)],
            )?;
            env.call_method(
                activity,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[JValue::Object(&intent)],
            )?;
            Ok(())
        });
        if let Err(e) = asked {
            gtk::glib::g_warning!("ice-commander", "cannot ask for All-files access: {e}");
        }
    }
}

/// The removable media the system knows of — an SD card, a stick — which nothing on the
/// filesystem lists, because /storage is closed to an app. The primary volume is left out: it
/// is $HOME, and the drive list already names it. Empty until `attach_services` has run.
pub fn storage_volumes() -> Vec<(String, String)> {
    if !SERVICES_ATTACHED.load(std::sync::atomic::Ordering::Acquire) {
        return Vec::new();
    }
    match ask_android_for_volumes() {
        Ok(volumes) => volumes,
        Err(e) => {
            gtk::glib::g_warning!("ice-commander", "cannot ask Android for its volumes: {e}");
            Vec::new()
        }
    }
}

fn ask_android_for_volumes() -> Result<Vec<(String, String)>, String> {
    let android = ndk_context::android_context();
    // SAFETY: both come from the context attach_services() handed to ndk-context, which holds
    // the process's VM and a global reference that is never released.
    let vm = unsafe { jni::JavaVM::from_raw(android.vm().cast()) }.map_err(|e| e.to_string())?;
    let context = unsafe { JObject::from_raw(android.context().cast()) };
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;

    let asked = env.with_local_frame(16, |env| -> JniResult<Vec<(String, String)>> {
        let service = env.new_string("storage")?;
        let manager = env
            .call_method(
                &context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&service)],
            )?
            .l()?;
        let volumes = env
            .call_method(&manager, "getStorageVolumes", "()Ljava/util/List;", &[])?
            .l()?;
        let count = env.call_method(&volumes, "size", "()I", &[])?.i()?;

        let mut found = Vec::new();
        for index in 0..count {
            let volume = env
                .call_method(
                    &volumes,
                    "get",
                    "(I)Ljava/lang/Object;",
                    &[JValue::Int(index)],
                )?
                .l()?;
            if env.call_method(&volume, "isPrimary", "()Z", &[])?.z()? {
                continue;
            }
            let directory = env
                .call_method(&volume, "getDirectory", "()Ljava/io/File;", &[])?
                .l()?;
            // Null while the card is unmounted or being ejected.
            if directory.is_null() {
                continue;
            }
            let path = env
                .call_method(&directory, "getAbsolutePath", "()Ljava/lang/String;", &[])?
                .l()?;
            let path: String = env.get_string((&path).into())?.into();
            let described = env
                .call_method(
                    &volume,
                    "getDescription",
                    "(Landroid/content/Context;)Ljava/lang/String;",
                    &[JValue::Object(&context)],
                )?
                .l()?;
            let name = if described.is_null() {
                path.clone()
            } else {
                env.get_string((&described).into())?.into()
            };
            found.push((path, name));
        }
        Ok(found)
    });
    // jni leaves a throw pending, and the next JNI call GTK makes on this thread would abort on
    // it. The exception lives on the thread, not in the frame, so clearing it here is in time.
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    asked.map_err(|e| e.to_string())
}

fn with_playback<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> JniResult<T>) -> Result<T, String> {
    if !SERVICES_ATTACHED.load(std::sync::atomic::Ordering::Acquire) {
        return Err("the Android context is not attached yet".to_string());
    }
    let android = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(android.vm().cast()) }.map_err(|e| e.to_string())?;
    let application = unsafe { JObject::from_raw(android.context().cast()) };
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;

    let called = env.with_local_frame(8, |env| -> JniResult<T> {
        let playback = env
            .call_method(
                &application,
                "playback",
                "()Lcom/icecommander/gtkapp/BackgroundPlayback;",
                &[],
            )?
            .l()?;
        f(env, &playback)
    });
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    called.map_err(|e| e.to_string())
}

fn told(what: &str, done: Result<(), String>) {
    if let Err(e) = done {
        gtk::glib::g_warning!("ice-commander", "cannot tell Android {what}: {e}");
    }
}

pub fn media_begin() {
    told(
        "a playback began",
        with_playback(|env, playback| {
            env.call_method(playback, "begin", "(Z)V", &[JValue::Bool(1)])?;
            Ok(())
        }),
    );
}

pub fn media_now_playing(title: &str, duration_ms: i64, track: i32, tracks: i32) {
    told(
        "what is playing",
        with_playback(|env, playback| {
            let title = env.new_string(title)?;
            let nothing = JObject::null();
            env.call_method(
                playback,
                "nowPlaying",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;JII)V",
                &[
                    JValue::Object(&title),
                    JValue::Object(&nothing),
                    JValue::Object(&nothing),
                    JValue::Long(duration_ms),
                    JValue::Int(track),
                    JValue::Int(tracks),
                ],
            )?;
            Ok(())
        }),
    );
}

pub fn media_transport(
    state: i32,
    position_ms: i64,
    speed: f32,
    has_previous: bool,
    has_next: bool,
) {
    told(
        "the transport",
        with_playback(|env, playback| {
            env.call_method(
                playback,
                "transport",
                "(IJFZZ)V",
                &[
                    JValue::Int(state),
                    JValue::Long(position_ms),
                    JValue::Float(speed),
                    JValue::Bool(u8::from(has_previous)),
                    JValue::Bool(u8::from(has_next)),
                ],
            )?;
            Ok(())
        }),
    );
}

pub fn media_end() {
    told(
        "the playback ended",
        with_playback(|env, playback| {
            env.call_method(playback, "end", "()V", &[])?;
            Ok(())
        }),
    );
}

#[no_mangle]
pub extern "C" fn Java_com_icecommander_gtkapp_BackgroundPlayback_command(
    _env: JNIEnv,
    _class: JObject,
    command: jni::sys::jint,
    arg: jni::sys::jlong,
) {
    gtk::glib::idle_add_once(move || crate::media::obey(command, arg));
}

fn with_activity<T>(
    window: &gtk::Window,
    f: impl FnOnce(&mut JNIEnv, &JObject) -> JniResult<T>,
) -> Result<T, String> {
    let surface = window.surface().ok_or("the window has no surface")?;
    let display = WidgetExt::display(window);
    // SAFETY: GTK keeps its thread attached to the JVM and returns that thread's env.
    let mut env =
        unsafe { JNIEnv::from_raw(gdk_android_display_get_env(display.to_glib_none().0)) }
            .map_err(|e| e.to_string())?;
    env.with_local_frame(8, |env| -> JniResult<T> {
        // SAFETY: a new local reference, released with this frame.
        let activity = unsafe {
            JObject::from_raw(gdk_android_toplevel_get_activity(surface.to_glib_none().0))
        };
        if activity.is_null() {
            return Err(Error::NullPtr("activity"));
        }
        let result = f(env, &activity);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        result
    })
    .map_err(|e| e.to_string())
}

#[no_mangle]
pub extern "C" fn main(_argc: c_int, _argv: *mut *mut c_char, _envp: *mut *mut c_char) -> c_int {
    // Android drops stderr; GLib's log handler reaches logcat.
    std::panic::set_hook(Box::new(|info| {
        gtk::glib::g_critical!("ice-commander", "{info}")
    }));
    let runtime = tokio::runtime::Runtime::new().expect("a Tokio runtime");
    let code = runtime.block_on(async { crate::run(Rc::new(Android)) });
    code.value()
}
