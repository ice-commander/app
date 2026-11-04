package com.icecommander.gtkapp;

import android.app.Service;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.IBinder;
import android.util.Log;

/**
 * The mediaPlayback foreground service: while it runs, a hidden app keeps its CPU and its sound.
 * {@link BackgroundPlayback} starts and stops it; the notification's buttons come here.
 */
public final class PlaybackService extends Service {
	static final String ACTION_PLAY = "com.icecommander.gtkapp.action.PLAY";
	static final String ACTION_PAUSE = "com.icecommander.gtkapp.action.PAUSE";
	static final String ACTION_NEXT = "com.icecommander.gtkapp.action.NEXT";
	static final String ACTION_PREVIOUS = "com.icecommander.gtkapp.action.PREVIOUS";
	static final String ACTION_STOP = "com.icecommander.gtkapp.action.STOP";

	/** Main thread only, as are the fields. */
	static PlaybackService running;

	boolean foreground;
	/** BackgroundPlayback let it go, rather than the system taking it down. */
	private boolean stopping;

	@Override
	public void onCreate() {
		super.onCreate();
		running = this;
	}

	@Override
	public int onStartCommand(Intent intent, int flags, int startId) {
		stopping = false;
		BackgroundPlayback playback = ((IceCommanderApplication) getApplication()).playback();
		// First of all: a service started with startForegroundService has a few seconds for it.
		try {
			startForeground(BackgroundPlayback.NOTIFICATION, playback.notification(),
					ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
			foreground = true;
		} catch (IllegalStateException e) {
			// ForegroundServiceStartNotAllowedException: started from the background with no exemption.
			Log.w("ice-commander", "the playback service cannot go foreground", e);
		}
		playback.onStarted(this, intent != null ? intent.getAction() : null);
		// A restarted process has no session to play.
		return START_NOT_STICKY;
	}

	/** remove: the notification goes with it; else it stays behind. */
	void stop(boolean remove) {
		stopForeground(remove ? STOP_FOREGROUND_REMOVE : STOP_FOREGROUND_DETACH);
		foreground = false;
		stopping = true;
		stopSelf();
	}

	@Override
	public void onDestroy() {
		if (running == this)
			running = null;
		foreground = false;
		if (!stopping)
			((IceCommanderApplication) getApplication()).playback().onServiceGone();
		super.onDestroy();
	}

	@Override
	public IBinder onBind(Intent intent) {
		return null;
	}
}
