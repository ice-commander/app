package com.icecommander.gtkapp;

import android.app.Activity;
import android.os.Environment;
import android.system.ErrnoException;
import android.system.Os;
import android.view.WindowManager;
import java.io.File;
import org.gtk.android.RuntimeApplication;

/** GTK's runtime plus the environment it does not set up itself (as of GTK 4.24). */
public class IceCommanderApplication extends RuntimeApplication {
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
		super.onCreate();
	}

	/** Called over JNI from the GTK thread; a window's flags belong to the activity's UI thread. */
	public void keepScreenOn(Activity activity, boolean on) {
		activity.runOnUiThread(() -> {
			if (on) {
				activity.getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
			} else {
				activity.getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
			}
		});
	}
}
