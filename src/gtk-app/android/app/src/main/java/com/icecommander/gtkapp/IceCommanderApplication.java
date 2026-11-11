package com.icecommander.gtkapp;

import android.app.Activity;
import android.os.Bundle;
import android.os.Environment;
import android.system.ErrnoException;
import android.system.Os;
import android.view.Window;
import android.view.WindowManager;
import java.io.File;
import org.gtk.android.RuntimeApplication;
import org.gtk.android.ToplevelActivity;

/** GTK's runtime plus the environment it does not set up itself (as of GTK 4.24). */
public class IceCommanderApplication extends RuntimeApplication {
	/** The window keeps the screen on (keepScreenOn), main thread only. */
	private boolean screenOn;
	/** The colour of the system bars, or 0 before GTK has said; main thread only. */
	private int barColour;

	private BackgroundPlayback playback;
	private LanMulticast multicast;

	/** Built on first use, from whichever thread asks: the constructor only makes objects. */
	public synchronized BackgroundPlayback playback() {
		if (playback == null) {
			playback = new BackgroundPlayback(this);
		}
		return playback;
	}

	@Override
	public void onCreate() {
		try {
			// Not under filesDir: GTK's runtime replaces it whenever the APK's assets change.
			Os.setenv("XDG_CONFIG_HOME", getDir("config", MODE_PRIVATE).getPath(), true);
			Os.setenv("XDG_DATA_HOME", getDir("data", MODE_PRIVATE).getPath(), true);
			Os.setenv("XDG_CACHE_HOME", getCacheDir().getPath(), true);
			// Rust's temp_dir() falls back to /data/local/tmp, which an app cannot write.
			Os.setenv("TMPDIR", getCacheDir().getPath(), true);
			// Out of Auto Backup, and never on shared storage, which is mounted noexec.
			Os.setenv("IC_PLUGIN_DIR", new File(getNoBackupFilesDir(), "plugins").getPath(), true);
			// The user's files: "~" of a file manager on a phone.
			Os.setenv("HOME", Environment.getExternalStorageDirectory().getPath(), true);
			Os.setenv("FONTCONFIG_FILE", new File(getFilesDir(), "etc/fonts/fonts.conf").getPath(), true);
			// ADPF costs ~+56% CPU+GPU power while scrolling (android-rust-gtk4-adwaita benchmarks).
			Os.setenv("GDK_DISABLE", "adpf", true);
		} catch (ErrnoException e) {
			throw new RuntimeException(e);
		}
		multicast = new LanMulticast(this);
		super.onCreate();

		registerActivityLifecycleCallbacks(new ActivityLifecycleCallbacks() {
			@Override
			public void onActivityPostCreated(Activity activity, Bundle state) {
				// A fresh activity brings a new Android window whose flags start clear, including
				// the one GDK hands the kept window to after the system finished the old activity.
				if (screenOn && activity instanceof ToplevelActivity) {
					activity.getWindow()
							.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
				}
				if (barColour != 0 && activity instanceof ToplevelActivity) {
					paintBars(activity.getWindow(), barColour);
				}
			}

			@Override
			public void onActivityCreated(Activity activity, Bundle state) {}

			@Override
			public void onActivityStarted(Activity activity) {
				multicast.activity(true);
			}

			@Override
			public void onActivityResumed(Activity activity) {}

			@Override
			public void onActivityPaused(Activity activity) {}

			@Override
			public void onActivityStopped(Activity activity) {
				multicast.activity(false);
			}

			@Override
			public void onActivitySaveInstanceState(Activity activity, Bundle state) {}

			@Override
			public void onActivityDestroyed(Activity activity) {}
		});
	}

	/**
	 * Called over JNI from the GTK thread: the system bars take the colour of the window GTK is
	 * drawing, so the theme the user picked inside the application reaches the frame around it.
	 * The resources do this for the system's own light and dark; this is for the switch in the
	 * header, which Android never hears about.
	 */
	public void systemBars(Activity activity, int colour) {
		activity.runOnUiThread(() -> {
			barColour = colour;
			paintBars(activity.getWindow(), colour);
		});
	}

	private static void paintBars(Window window, int colour) {
		window.setNavigationBarColor(colour);
		window.setStatusBarColor(colour);
		// Or Android lays its own wash over both to keep them legible, which is the grey that
		// shows instead of the colour asked for.
		window.setNavigationBarContrastEnforced(false);
		window.setStatusBarContrastEnforced(false);
	}

	/** Called over JNI from the GTK thread; a window's flags belong to the activity's UI thread. */
	public void keepScreenOn(Activity activity, boolean on) {
		activity.runOnUiThread(() -> {
			screenOn = on;
			if (on) {
				activity.getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
			} else {
				activity.getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
			}
		});
	}

	/** Called over JNI from the GTK thread (android.rs): any loaded plugin asks for multicast, or none does. */
	public void lanMulticast(boolean on) {
		getMainExecutor().execute(() -> multicast.wanted(on));
	}
}
