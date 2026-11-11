package com.icecommander.gtkapp;

import android.content.Context;
import android.net.wifi.WifiManager;
import android.util.Log;

/**
 * The Wi-Fi multicast lock for devices on the local network (a plugin's mDNS): without one most Wi-Fi
 * drivers drop multicast not addressed to this device, and with one they wake for all of it, which costs
 * battery. Held while a loaded plugin asks for it (platform_call "multicast", android.rs) and an activity
 * is started.
 *
 * <p>Ported from MagnetPlayer, without its watch's Wi-Fi request and the services it also holds the lock for.
 *
 * <p>Main thread only.
 */
final class LanMulticast {
	private static final String TAG = "ice-commander";

	private final Context app;
	private WifiManager.MulticastLock lock;
	private boolean wanted;
	private int started;

	/** app: the application's context, which a WifiManager does not leak. */
	LanMulticast(Context app) {
		this.app = app;
	}

	/** The plugins' wish (IceCommanderApplication.lanMulticast). */
	void wanted(boolean on) {
		wanted = on;
		update();
	}

	/** An activity started, or stopped. */
	void activity(boolean started) {
		this.started = Math.max(0, this.started + (started ? 1 : -1));
		update();
	}

	private void update() {
		boolean hold = wanted && started > 0;
		if (hold == (lock != null && lock.isHeld()))
			return;
		if (lock == null) {
			WifiManager wifi = app.getSystemService(WifiManager.class);
			// No Wi-Fi (a TV box on Ethernet): nothing filters, nothing to hold.
			if (wifi == null)
				return;
			lock = wifi.createMulticastLock("ice-commander-lan");
			lock.setReferenceCounted(false);
		}
		try {
			if (hold) {
				lock.acquire();
			} else {
				lock.release();
			}
			Log.i(TAG, "multicast lock " + (hold ? "on" : "off"));
		} catch (RuntimeException e) {
			Log.w(TAG, "multicast lock: " + e);
		}
	}
}
