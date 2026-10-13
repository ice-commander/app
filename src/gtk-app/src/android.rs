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
            Ok(()) => ATTACHED.with(|done| done.set(true)),
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
