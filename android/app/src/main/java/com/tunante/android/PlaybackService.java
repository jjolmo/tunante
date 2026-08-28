package com.tunante.android;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.ServiceInfo;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;
import android.util.Log;

import org.json.JSONObject;

/**
 * Where playback actually lives.
 *
 * Three separate reasons this has to be a foreground service, not a thread in
 * the activity:
 *
 * <ol>
 *   <li>Since Android 17, an app with no visible activity <em>needs</em> one to
 *       play audio in the background. The failure is silent —
 *       {@code requestAudioFocus()} just returns {@code AUDIOFOCUS_REQUEST_FAILED}
 *       and nothing throws.</li>
 *   <li>It owns the 500 ms tick. In {@code tunante-mini} that clock is a
 *       {@code slint::Timer} on the UI thread, so advancing the queue, saving the
 *       session and the sleep timer all stop when the window does. Here it keeps
 *       running with the screen off, which is the point of a music player.</li>
 *   <li>It owns the media session, which is what puts controls on the lock
 *       screen and makes the buttons on a Bluetooth headset work.</li>
 * </ol>
 *
 * Deliberately built on the framework's own MediaSession rather than
 * androidx.media3: this app has no AndroidX dependencies at all, and adding one
 * to get a notification with three buttons is not a good trade.
 */
public class PlaybackService extends Service {

    private static final String TAG = "tunante";
    private static final String CHANNEL = "playback";
    private static final int NOTIFICATION_ID = 1;
    private static final long TICK_MS = 500;

    public static final String ACTION_START = "com.tunante.android.START";

    private MediaSession session;
    private AudioManager audio;
    private AudioFocusRequest focusRequest;
    private PowerManager.WakeLock wakeLock;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private boolean ticking;
    /** Ticks since the session was last written. */
    private int sinceSave;

    /**
     * Headphones unplugged, or Bluetooth disconnected.
     *
     * Without this the audio keeps going and comes out of the phone's speaker,
     * which is the single rudest thing a music player can do.
     */
    private final BroadcastReceiver becomingNoisy = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            Log.i(TAG, "becoming noisy — pausing");
            NativeBridge.nativePause();
            update();
        }
    };

    private final AudioManager.OnAudioFocusChangeListener focusListener =
            new AudioManager.OnAudioFocusChangeListener() {
                @Override
                public void onAudioFocusChange(int change) {
                    switch (change) {
                        case AudioManager.AUDIOFOCUS_LOSS:
                        case AudioManager.AUDIOFOCUS_LOSS_TRANSIENT:
                            NativeBridge.nativePause();
                            break;
                        case AudioManager.AUDIOFOCUS_GAIN:
                            NativeBridge.nativeResume();
                            break;
                        default:
                            // AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK: the system
                            // ducks for us because we asked with
                            // setWillPauseWhenDucked(false).
                            break;
                    }
                    update();
                }
            };

    @Override
    public void onCreate() {
        super.onCreate();
        audio = (AudioManager) getSystemService(Context.AUDIO_SERVICE);

        session = new MediaSession(this, "tunante");
        session.setCallback(new MediaSession.Callback() {
            @Override
            public void onPlay() {
                requestFocus();
                NativeBridge.nativeResume();
                update();
            }

            @Override
            public void onPause() {
                NativeBridge.nativePause();
                update();
            }

            @Override
            public void onSkipToNext() {
                NativeBridge.nativeNext();
                update();
            }

            @Override
            public void onSkipToPrevious() {
                NativeBridge.nativePrev();
                update();
            }

            @Override
            public void onSeekTo(long pos) {
                NativeBridge.nativeSeek(pos);
                update();
            }

            @Override
            public void onStop() {
                NativeBridge.nativeStop();
                stop();
            }
        });
        session.setActive(true);

        PowerManager power = (PowerManager) getSystemService(Context.POWER_SERVICE);
        // The CPU only. A foreground service of type mediaPlayback is already
        // most of what keeps decoding alive; this covers the gap on devices that
        // are aggressive about dozing, and it is the direct equivalent of the
        // logind sleep inhibitor tunante-mini takes on postmarketOS.
        wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "tunante:playback");
        wakeLock.setReferenceCounted(false);

        registerReceiver(becomingNoisy,
                new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY));

        createChannel();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        startForegroundCompat(buildNotification(null));
        requestFocus();
        startTicking();
        return START_STICKY;
    }

    private void createChannel() {
        NotificationManager nm = getSystemService(NotificationManager.class);
        NotificationChannel channel = new NotificationChannel(
                CHANNEL, "Reproducción", NotificationManager.IMPORTANCE_LOW);
        channel.setShowBadge(false);
        nm.createNotificationChannel(channel);
    }

    private void startForegroundCompat(Notification n) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
        } else {
            startForeground(NOTIFICATION_ID, n);
        }
    }

    private void requestFocus() {
        AudioAttributes attrs = new AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                .build();
        focusRequest = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                .setAudioAttributes(attrs)
                .setWillPauseWhenDucked(false)
                .setOnAudioFocusChangeListener(focusListener, handler)
                .build();
        int result = audio.requestAudioFocus(focusRequest);
        // Worth logging rather than ignoring: this is exactly where an app that
        // forgot its foreground service finds out, and it finds out quietly.
        Log.i(TAG, "audio focus: "
                + (result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED ? "granted" : "DENIED"));
    }

    private void startTicking() {
        if (ticking) {
            return;
        }
        ticking = true;
        handler.post(new Runnable() {
            @Override
            public void run() {
                if (!ticking) {
                    return;
                }
                String json = NativeBridge.nativeTick();
                apply(json);
                // Every ten ticks, so five seconds. Not only on exit: this
                // process is killed by the system far more often than it is
                // closed, and a resume that only survives a clean exit is a
                // resume that rarely works.
                if (++sinceSave >= 10) {
                    sinceSave = 0;
                    NativeBridge.nativeSaveSession();
                }
                handler.postDelayed(this, TICK_MS);
            }
        });
    }

    /** Push the state Rust just reported into the session and the notification. */
    private void apply(String json) {
        try {
            JSONObject s = new JSONObject(json);
            if (!s.optBoolean("ok", false)) {
                return;
            }
            boolean playing = s.optBoolean("playing", false);
            long position = s.optLong("positionMs", 0);

            // Only when it actually moved. Pushing this every tick makes the
            // system's MediaSessionService log and re-broadcast a state change
            // twice a second forever, including while paused with nothing
            // happening at all.
            if (playing != lastPlaying || position != lastPosition) {
                lastPlaying = playing;
                lastPosition = position;
                session.setPlaybackState(new PlaybackState.Builder()
                    .setActions(PlaybackState.ACTION_PLAY
                            | PlaybackState.ACTION_PAUSE
                            | PlaybackState.ACTION_PLAY_PAUSE
                            | PlaybackState.ACTION_SKIP_TO_NEXT
                            | PlaybackState.ACTION_SKIP_TO_PREVIOUS
                            | PlaybackState.ACTION_SEEK_TO
                            | PlaybackState.ACTION_STOP)
                    .setState(playing ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED,
                            position, 1.0f)
                    .build());
            }

            // Metadata and the notification only when the track changed. At
            // twice a second, rebuilding them unconditionally is a lot of
            // garbage for a screen nobody is looking at.
            if (s.optBoolean("trackChanged", false) || !s.optString("title").equals(lastTitle)) {
                lastTitle = s.optString("title");
                // The evidence that the queue is advancing with nobody looking.
                // The duration is in here because it is not the file's: for
                // anything that loops it is what the decoder produces with the
                // loop and fade settings, and that is the number the bar and
                // the media session are drawn against.
                Log.i(TAG, "now playing [" + s.optInt("index", -1) + "/"
                        + s.optInt("queueLen", 0) + "] " + lastTitle
                        + " (" + s.optLong("durationMs", 0) + " ms)");
                session.setMetadata(new MediaMetadata.Builder()
                        .putString(MediaMetadata.METADATA_KEY_TITLE, s.optString("title"))
                        .putString(MediaMetadata.METADATA_KEY_ARTIST, s.optString("artist"))
                        .putString(MediaMetadata.METADATA_KEY_ALBUM, s.optString("album"))
                        .putLong(MediaMetadata.METADATA_KEY_DURATION, s.optLong("durationMs", 0))
                        .build());
                getSystemService(NotificationManager.class)
                        .notify(NOTIFICATION_ID, buildNotification(s));
            }

            if (playing) {
                if (!wakeLock.isHeld()) {
                    wakeLock.acquire();
                }
            } else if (wakeLock.isHeld()) {
                wakeLock.release();
            }
        } catch (Exception e) {
            Log.e(TAG, "applying state: " + json, e);
        }
    }

    private String lastTitle = "";
    private boolean lastPlaying;
    private long lastPosition = -1;

    private Notification buildNotification(JSONObject state) {
        String title = state == null ? "Tunante" : state.optString("title", "Tunante");
        String artist = state == null ? "" : state.optString("artist", "");

        PendingIntent open = PendingIntent.getActivity(this, 0,
                new Intent(this, MainActivity.class),
                PendingIntent.FLAG_IMMUTABLE);

        return new Notification.Builder(this, CHANNEL)
                .setContentTitle(title)
                .setContentText(artist)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentIntent(open)
                .setOngoing(true)
                .setStyle(new Notification.MediaStyle().setMediaSession(session.getSessionToken()))
                .build();
    }

    /** Called from the activity after every change it makes, so the two agree. */
    public void update() {
        apply(NativeBridge.nativeState());
    }

    private void stop() {
        ticking = false;
        if (wakeLock != null && wakeLock.isHeld()) {
            wakeLock.release();
        }
        stopForeground(true);
        stopSelf();
    }

    @Override
    public void onDestroy() {
        ticking = false;
        try {
            unregisterReceiver(becomingNoisy);
        } catch (IllegalArgumentException ignored) {
            // Never registered, because onCreate threw. Nothing to undo.
        }
        if (focusRequest != null) {
            audio.abandonAudioFocusRequest(focusRequest);
        }
        if (wakeLock != null && wakeLock.isHeld()) {
            wakeLock.release();
        }
        if (session != null) {
            session.release();
        }
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
