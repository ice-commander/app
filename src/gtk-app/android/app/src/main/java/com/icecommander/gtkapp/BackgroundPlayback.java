package com.icecommander.gtkapp;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.graphics.drawable.Icon;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.Handler;
import android.os.Looper;
import android.os.PowerManager;
import android.os.SystemClock;
import android.util.Log;

/**
 * The system's media controls and background playback, driven from Rust (android.rs): a MediaSession
 * for the lock screen, the notification and headset buttons, audio focus, and for a session that goes
 * on hidden the mediaPlayback foreground service ({@link PlaybackService}) with a CPU wake lock.
 *
 * <p>Ported from MagnetPlayer, without its artwork: the player reads no tags, so there is no cover
 * to show.
 *
 * <p>The methods Rust calls only post to the main thread, never the reverse.
 */
public final class BackgroundPlayback {
	static {
		// GTK's runtime dlopens the library, which does not make its functions JNI natives.
		System.loadLibrary("ice_commander_gtk");
	}

	static final int PLAY = 1, PAUSE = 2, TOGGLE = 3, NEXT = 4, PREVIOUS = 5, SEEK_TO = 6, SEEK_BY = 7, STOP = 8;

	/** To the player on the GTK thread; arg: milliseconds for SEEK_TO and SEEK_BY. */
	static native void command(int command, long arg);

	static final int NOTIFICATION = 1;
	private static final String TAG = "ice-commander";
	private static final String CHANNEL = "playback";
	/** A paused session keeps the service this long, then lets it go. */
	private static final long PAUSED_KEEP_MS = 10 * 60_000;
	private static final long STOPPED_KEEP_MS = 30_000;
	/** Re-armed on every transport, at least every 300 s while playing; outlasts PAUSED_KEEP_MS. */
	private static final long LOCK_MS = PAUSED_KEEP_MS + 60_000;
	private static final long SKIP_MS = 10_000;

	private final Context app;
	private final Handler main = new Handler(Looper.getMainLooper());
	private final AudioManager audio;
	private final NotificationManager notifications;
	private final PowerManager.WakeLock cpu;
	private final AudioFocusRequest focus;
	/** The paused or stopped session's time is up: the notification stays, the service and CPU go. */
	private final Runnable letGo = this::letGo;
	private final BroadcastReceiver noisy = new BroadcastReceiver() {
		@Override
		public void onReceive(Context context, Intent intent) {
			if (playing)
				user(PAUSE, 0);
		}
	};

	private MediaSession session;
	/** Begun with background: the service and the wake lock follow the state. */
	private boolean background;
	/** The service was asked to start and has not been let go since. */
	private boolean serviceUp;
	/** The notification is posted, by the service or left behind by it. */
	private boolean posted;
	private boolean playing, hasFocus, resumeOnGain, noisyRegistered;
	private int state = PlaybackState.STATE_NONE;
	private long positionMs, positionAt;
	private float speed = 1f;
	private boolean hasPrevious, hasNext;
	private String title = "", artist, album;
	private long durationMs;
	private int track, tracks;

	/** On any thread: it only builds objects. */
	BackgroundPlayback(Context app) {
		this.app = app;
		audio = app.getSystemService(AudioManager.class);
		notifications = app.getSystemService(NotificationManager.class);
		cpu = app.getSystemService(PowerManager.class)
				.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "icecommander:playback");
		cpu.setReferenceCounted(false);
		focus = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
				.setAudioAttributes(new AudioAttributes.Builder()
						.setUsage(AudioAttributes.USAGE_MEDIA)
						.setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
						.build())
				.setAcceptsDelayedFocusGain(true)
				// Since Android 8 the system ducks us itself.
				.setWillPauseWhenDucked(false)
				.setOnAudioFocusChangeListener(this::onFocusChange, main)
				.build();
		NotificationChannel channel = new NotificationChannel(
				CHANNEL, app.getString(R.string.playback_channel), NotificationManager.IMPORTANCE_LOW);
		channel.setShowBadge(false);
		notifications.createNotificationChannel(channel);
	}

	// Rust, on the GTK thread.

	/** "(Z)V": a playback the user started while the window is visible; background: it goes on hidden. */
	public void begin(boolean background) {
		main.post(() -> onBegin(background));
	}

	/** "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;JII)V": null and 0 for unknown. */
	public void nowPlaying(String title, String artist, String album, long durationMs, int track, int tracks) {
		main.post(() -> {
			this.title = title != null ? title : "";
			this.artist = artist;
			this.album = album;
			this.durationMs = durationMs;
			this.track = track;
			this.tracks = tracks;
			publishMetadata();
			publishState();
			updateNotification();
		});
	}

	/** "(IJFZZ)V": a PlaybackState state code, the position in ms, the speed, whether previous and next exist. */
	public void transport(int state, long positionMs, float speed, boolean hasPrevious, boolean hasNext) {
		main.post(() -> onTransport(state, positionMs, speed, hasPrevious, hasNext));
	}

	/** "()V": the session is over. */
	public void end() {
		main.post(this::onEnd);
	}

	// Main thread.

	private void onBegin(boolean background) {
		if (session == null) {
			session = new MediaSession(app, "IceCommander");
			session.setCallback(new MediaSession.Callback() {
				@Override
				public void onPlay() {
					user(PLAY, 0);
				}

				@Override
				public void onPause() {
					user(PAUSE, 0);
				}

				@Override
				public void onStop() {
					user(STOP, 0);
				}

				@Override
				public void onSkipToNext() {
					user(NEXT, 0);
				}

				@Override
				public void onSkipToPrevious() {
					user(PREVIOUS, 0);
				}

				@Override
				public void onSeekTo(long ms) {
					user(SEEK_TO, ms);
				}

				@Override
				public void onFastForward() {
					user(SEEK_BY, SKIP_MS);
				}

				@Override
				public void onRewind() {
					user(SEEK_BY, -SKIP_MS);
				}
			}, main);
			session.setSessionActivity(openApp());
			publishMetadata();
			publishState();
		}
		session.setActive(true);
		if (!noisyRegistered) {
			// Only at run time: the manifest cannot declare it.
			app.registerReceiver(noisy, new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), null, main,
					Context.RECEIVER_NOT_EXPORTED);
			noisyRegistered = true;
		}
		this.background = background;
		if (background) {
			// Now, while the window is visible: from the background Android may refuse it.
			startService();
			keep();
		} else {
			main.removeCallbacks(letGo);
			stopService(true);
			cpu.release();
		}
	}

	private void onTransport(int state, long positionMs, float speed, boolean hasPrevious, boolean hasNext) {
		this.state = state;
		this.positionMs = positionMs;
		this.speed = speed;
		positionAt = SystemClock.elapsedRealtime();
		this.hasPrevious = hasPrevious;
		this.hasNext = hasNext;
		playing = state == PlaybackState.STATE_PLAYING || state == PlaybackState.STATE_BUFFERING;
		if (session == null)
			return;
		publishState();
		if (playing && !hasFocus)
			requestFocus();
		if (background)
			keep();
		updateNotification();
	}

	private void onEnd() {
		main.removeCallbacks(letGo);
		stopService(true);
		cpu.release();
		audio.abandonAudioFocusRequest(focus);
		hasFocus = resumeOnGain = false;
		if (noisyRegistered) {
			app.unregisterReceiver(noisy);
			noisyRegistered = false;
		}
		if (session != null) {
			session.setActive(false);
			session.release();
			session = null;
		}
		background = playing = hasPrevious = hasNext = false;
		state = PlaybackState.STATE_NONE;
		positionMs = durationMs = 0;
		title = "";
		artist = album = null;
		track = tracks = 0;
	}

	/** A background session keeps the service and the CPU while it plays, and for a while after. */
	private void keep() {
		main.removeCallbacks(letGo);
		if (playing || state == PlaybackState.STATE_NONE) {
			cpu.acquire(LOCK_MS);
			startService();
		} else if (serviceUp) {
			// Held so that letGo runs on time: the main looper's clock stops while the CPU sleeps.
			cpu.acquire(LOCK_MS);
			main.postDelayed(letGo, state == PlaybackState.STATE_STOPPED ? STOPPED_KEEP_MS : PAUSED_KEEP_MS);
		}
	}

	private void letGo() {
		stopService(false);
		cpu.release();
	}

	private void startService() {
		PlaybackService running = PlaybackService.running;
		if (serviceUp && running != null && running.foreground)
			return;
		try {
			app.startForegroundService(new Intent(app, PlaybackService.class));
			serviceUp = true;
		} catch (IllegalStateException e) {
			// ForegroundServiceStartNotAllowedException: hidden, with no exemption.
			Log.w(TAG, "cannot start the playback service", e);
		}
	}

	/** remove: the notification goes too; else it stays behind, dismissible, with its buttons. */
	private void stopService(boolean remove) {
		serviceUp = false;
		PlaybackService running = PlaybackService.running;
		if (running != null)
			running.stop(remove);
		if (remove && posted) {
			notifications.cancel(NOTIFICATION);
			posted = false;
		}
	}

	/** PlaybackService.onStartCommand, after its startForeground; action: a notification button. */
	void onStarted(PlaybackService service, String action) {
		if (service.foreground) {
			posted = true;
			serviceUp = true;
		}
		if (session == null || !background) {
			// A notification left from a process that is gone, or the session ended meanwhile.
			stopService(true);
			return;
		}
		if (!service.foreground)
			stopService(false);
		if (action == null)
			return;
		switch (action) {
			case PlaybackService.ACTION_PLAY -> user(PLAY, 0);
			case PlaybackService.ACTION_PAUSE -> user(PAUSE, 0);
			case PlaybackService.ACTION_NEXT -> user(NEXT, 0);
			case PlaybackService.ACTION_PREVIOUS -> user(PREVIOUS, 0);
			case PlaybackService.ACTION_STOP -> {
				stopService(true);
				user(STOP, 0);
			}
			default -> {
			}
		}
	}

	/** PlaybackService.onDestroy when nobody let it go: stopWithTask, the task was swiped away. */
	void onServiceGone() {
		serviceUp = false;
		main.removeCallbacks(letGo);
		cpu.release();
		if (session != null)
			user(STOP, 0);
	}

	/** From the user: a pause or a stop ends any wait to resume after a call. */
	private void user(int command, long arg) {
		if (command == PLAY || command == PAUSE || command == STOP)
			resumeOnGain = false;
		command(command, arg);
	}

	private void requestFocus() {
		int result = audio.requestAudioFocus(focus);
		hasFocus = result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED;
		if (result == AudioManager.AUDIOFOCUS_REQUEST_DELAYED) {
			resumeOnGain = true;
			command(PAUSE, 0);
		} else if (result == AudioManager.AUDIOFOCUS_REQUEST_FAILED) {
			command(PAUSE, 0);
		}
	}

	private void onFocusChange(int change) {
		switch (change) {
			case AudioManager.AUDIOFOCUS_GAIN -> {
				hasFocus = true;
				if (resumeOnGain) {
					resumeOnGain = false;
					command(PLAY, 0);
				}
			}
			case AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
				hasFocus = false;
				if (playing) {
					resumeOnGain = true;
					command(PAUSE, 0);
				}
			}
			case AudioManager.AUDIOFOCUS_LOSS -> {
				hasFocus = resumeOnGain = false;
				audio.abandonAudioFocusRequest(focus);
				command(PAUSE, 0);
			}
			default -> {
			}
		}
	}

	private void publishState() {
		if (session == null)
			return;
		long actions = PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PAUSE | PlaybackState.ACTION_PLAY_PAUSE
				| PlaybackState.ACTION_STOP | PlaybackState.ACTION_FAST_FORWARD | PlaybackState.ACTION_REWIND
				| (durationMs > 0 ? PlaybackState.ACTION_SEEK_TO : 0)
				| (hasPrevious ? PlaybackState.ACTION_SKIP_TO_PREVIOUS : 0)
				| (hasNext ? PlaybackState.ACTION_SKIP_TO_NEXT : 0);
		session.setPlaybackState(new PlaybackState.Builder()
				.setActions(actions)
				.setState(state, positionMs, state == PlaybackState.STATE_PLAYING ? speed : 0f, positionAt)
				.build());
	}

	private void publishMetadata() {
		if (session == null)
			return;
		MediaMetadata.Builder metadata = new MediaMetadata.Builder()
				.putString(MediaMetadata.METADATA_KEY_TITLE, title)
				.putString(MediaMetadata.METADATA_KEY_ARTIST, artist)
				.putString(MediaMetadata.METADATA_KEY_ALBUM, album);
		if (durationMs > 0)
			metadata.putLong(MediaMetadata.METADATA_KEY_DURATION, durationMs);
		if (track > 0)
			metadata.putLong(MediaMetadata.METADATA_KEY_TRACK_NUMBER, track);
		if (tracks > 0)
			metadata.putLong(MediaMetadata.METADATA_KEY_NUM_TRACKS, tracks);
		session.setMetadata(metadata.build());
	}

	private void updateNotification() {
		if (posted)
			notifications.notify(NOTIFICATION, notification());
	}

	/** Also for PlaybackService's startForeground. Android 13+ takes the buttons from the PlaybackState. */
	Notification notification() {
		Notification.MediaStyle style = new Notification.MediaStyle().setShowActionsInCompactView(0, 1, 2);
		if (session != null)
			style.setMediaSession(session.getSessionToken());
		return new Notification.Builder(app, CHANNEL)
				.setSmallIcon(R.drawable.ic_notification)
				.setContentTitle(title.isEmpty() ? app.getString(R.string.app_name) : title)
				.setContentText(artist)
				.setContentIntent(openApp())
				.setDeleteIntent(service(PlaybackService.ACTION_STOP))
				.setVisibility(Notification.VISIBILITY_PUBLIC)
				.setCategory(Notification.CATEGORY_TRANSPORT)
				.setOnlyAlertOnce(true)
				.setShowWhen(false)
				.addAction(action(android.R.drawable.ic_media_previous, R.string.previous,
						PlaybackService.ACTION_PREVIOUS))
				.addAction(playing
						? action(android.R.drawable.ic_media_pause, R.string.pause, PlaybackService.ACTION_PAUSE)
						: action(android.R.drawable.ic_media_play, R.string.play, PlaybackService.ACTION_PLAY))
				.addAction(action(android.R.drawable.ic_media_next, R.string.next, PlaybackService.ACTION_NEXT))
				.setStyle(style)
				.build();
	}

	private Notification.Action action(int icon, int label, String action) {
		return new Notification.Action.Builder(Icon.createWithResource(app, icon), app.getString(label),
				service(action)).build();
	}

	/** A notification button may start a foreground service from the background. */
	private PendingIntent service(String action) {
		return PendingIntent.getForegroundService(app, 0, new Intent(action).setClass(app, PlaybackService.class),
				PendingIntent.FLAG_IMMUTABLE);
	}

	/** As the launcher does: the running task comes to the front. */
	private PendingIntent openApp() {
		Intent intent = app.getPackageManager().getLaunchIntentForPackage(app.getPackageName());
		intent.setFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED);
		return PendingIntent.getActivity(app, 0, intent, PendingIntent.FLAG_IMMUTABLE);
	}
}
