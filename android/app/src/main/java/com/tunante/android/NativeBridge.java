package com.tunante.android;

import android.content.Context;

/**
 * The whole Rust surface, for now.
 *
 * Phase 1 only proves the two risky things: that the app can exec the decoder
 * out of nativeLibraryDir, and that rodio reaches AAudio. The real surface —
 * library, queue, state — is phase 2, and will speak JSON rather than grow a
 * method per call.
 */
public final class NativeBridge {

    static {
        System.loadLibrary("tunante_android");
    }

    private NativeBridge() {}

    /**
     * @param context      handed to cpal via ndk_context; it needs a JavaVM and
     *                     a Context to enumerate audio devices at all.
     * @param decoderPath  absolute path to the decoder inside nativeLibraryDir.
     *                     Rust cannot work this out: current_exe() on Android
     *                     is /system/bin/app_process64.
     */
    public static native boolean nativeInit(Context context, String decoderPath);

    /**
     * Open the library database under {@code Context.getFilesDir()}.
     *
     * Every call below that returns a String returns JSON, with {@code ok:false}
     * and an {@code error} when it fails. Nothing throws across JNI: an
     * exception left pending turns the *next* JNI call into an abort, and one
     * shape of answer is far harder to get wrong than remembering to check.
     */
    public static native String nativeOpenDb(String dir);

    /** Blocking and long. Call from a thread, never from the main looper. */
    public static native String nativeScan(String root);

    /** Every track under a folder, or the whole library when {@code folder} is empty. */
    public static native String nativeTracks(String folder);

    /**
     * One level of the library tree: the folders directly under {@code parent}
     * and the tracks sitting in it. Empty {@code parent} asks for the roots.
     */
    public static native String nativeBrowse(String parent);

    /**
     * Cover art as a {@code data:} URI, or an empty string if there is none.
     *
     * Blocking — it can spawn a decoder — so call it off the main looper.
     */
    public static native String nativeArtwork(String path);

    /** Put back what was playing when the app last stopped, paused. */
    public static native String nativeRestoreSession();

    /** Write the session out. Cheap; called from the tick and from onPause. */
    public static native void nativeSaveSession();

    /** Minutes, or 0 to cancel. */
    public static native void nativeSetSleepTimer(int minutes);

    public static native void nativeSetShuffle(boolean on);

    /** 0 off, 1 all, 2 one. */
    public static native void nativeSetRepeat(int mode);

    /** Put a track next in line, without touching what is playing. */
    public static native String nativeEnqueue(String path);

    public static native String nativeRemoveFromPlaylist(String id, String path);

    /** The folders the library is built from. */
    public static native String nativeRoots();

    /** Add a folder to scan, or take one away (which forgets its tracks). */
    public static native String nativeSetRoot(String path, boolean add);

    /** The directories directly inside {@code path}, for the folder picker. */
    public static native String nativeListDirs(String path);

    /** One row per folder that directly holds music. */
    public static native String nativeAlbums();

    /** Empty {@code console} lists the consoles; naming one lists its tracks. */
    public static native String nativeConsoles(String console);

    /** Empty the waiting list, leaving what is playing alone. */
    public static native void nativeClearQueue();

    public static native String nativePlaylists();

    public static native String nativePlaylistTracks(String id);

    public static native String nativeCreatePlaylist(String name);

    public static native String nativeDeletePlaylist(String id);

    /**
     * Append tracks, named by path, to a playlist.
     *
     * By path because that is what the screen has; a track id is a UUID nobody
     * ever sees. Paths the library does not know are skipped, not invented.
     */
    public static native String nativeAddToPlaylist(String id, String pathsJson);

    /** Tracks matching {@code query}, across the whole library. */
    public static native String nativeSearch(String query);

    /**
     * Queue exactly these paths and start at {@code index}.
     *
     * {@code pathsJson} is a JSON array of strings. The general primitive, and
     * what the browser uses: nativePlayFolder matches subfolders too, so its
     * indices would not line up with a list of one folder's own tracks.
     */
    public static native String nativePlayList(String pathsJson, int index);

    public static native boolean nativePlay(String path);

    /** Load a folder's tracks as the queue and start at {@code index}. */
    public static native String nativePlayFolder(String folder, int index);

    public static native void nativeTogglePlay();
    public static native void nativePause();
    public static native void nativeResume();
    public static native void nativeNext();
    public static native void nativePrev();
    public static native void nativeSeek(long ms);

    /**
     * The heartbeat, driven by {@link PlaybackService} every 500 ms.
     *
     * In tunante-mini this clock is a Slint timer on the UI thread, which is why
     * everything time-based there stops when the window does. Here it belongs to
     * the service, so the queue keeps advancing with the screen off.
     */
    public static native String nativeTick();

    /** The current state, without ticking the clock. */
    public static native String nativeState();

    public static native void nativeStop();

    public static native boolean nativeIsPlaying();
}
