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

    /** Play a collection from its first track, replacing the queue. Returns the player state. */
    public static native String nativePlayCollection(String pathsJson);

    /** The playlist (context) as {tracks, index}: what next/previous walk, and where we are. */
    public static native String nativeContext();

    /** Load the translation catalog for a language code, once. */
    public static native void nativeInitI18n(String lang);

    /** Translate a Spanish source string; returns it unchanged if there is no catalog entry. */
    public static native String nativeTr(String source);

    /**
     * Tell the Rust side where its cover cache goes. Mandatory: Rust cannot
     * discover getCacheDir() on its own, and without this every archive index
     * and every downloaded cover is silently re-fetched. Call right after
     * nativeOpenDb.
     */
    public static native boolean nativeSetCacheDir(String dir);

    /** Blocking and long. Call from a thread, poll nativeCoverProgress. */
    public static native String nativeDownloadCovers(boolean replaceExisting);

    public static native String nativeCoverProgress();
    public static native String nativeScanProgress();

    public static native void nativeCancelCovers();

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

    /**
     * The paths of everything a library row stands for.
     *
     * A row is not always a path: the index tabs build synthetic keys
     * ({@code juego:Nombre}, {@code consola:NES}, {@code NES\u0001/dir}) for
     * things the filesystem has no name for. The Rust side is the only place
     * that knows that encoding.
     *
     * {@code deep} only means anything for a real directory: whether to take
     * the subfolders too.
     */
    public static native String nativeRowTracks(String row, boolean deep);

    /** Put a batch at the end of the queue, in one crossing rather than N. */
    public static native String nativeEnqueuePaths(String pathsJson);

    public static native String nativeRemoveFromPlaylist(String id, String path);

    /** The folders the library is built from. */
    public static native String nativeRoots();

    /** Add a folder to scan, or take one away (which forgets its tracks). */
    public static native String nativeSetRoot(String path, boolean add);

    /** The directories directly inside {@code path}, for the folder picker. */
    public static native String nativeListDirs(String path);

    /** One row per folder that directly holds music. */
    public static native String nativeAlbums();

    /**
     * Empty {@code game} lists them; naming one lists its tracks.
     *
     * By the album tag, not by folder — see nativeAlbums for the other answer.
     */
    public static native String nativeGames(String game);

    /** Empty {@code console} lists the consoles; naming one lists its tracks. */
    public static native String nativeConsoles(String console);

    /** Empty the waiting list, leaving what is playing alone. */
    public static native void nativeClearQueue();

    /** 1 -> 2 -> 3 -> forever. */
    public static native void nativeCycleLoops();

    /** none -> 4 -> 8 -> 15 seconds. */
    public static native void nativeCycleFade();

    /** "Resume only if less than N hours passed": cycle 3 → 6 → 12 → 24 → 0 (always); returns the new value. */
    public static native int nativeCycleResumeHours();

    /** The current value of that setting (0 = always). */
    public static native int nativeResumeHours();

    /** Everything waiting, in order. */
    public static native String nativeQueue();

    /** Play something that was waiting, now, taking it out of the queue. */
    public static native String nativePlayQueued(String path);

    /** Take one track out of the waiting list, by path. */
    public static native void nativeDequeue(String path);

    public static native void nativeMoveInQueue(int from, int to);

    public static native String nativeRenamePlaylist(String id, String name);

    /** Store the playlists in this order. */
    public static native String nativeReorderPlaylists(String idsJson);

    /** Put a whole playlist in the waiting list. */
    public static native String nativeEnqueuePlaylist(String id);

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
     * In tunante this clock is a Slint timer on the UI thread, which is why
     * everything time-based there stops when the window does. Here it belongs to
     * the service, so the queue keeps advancing with the screen off.
     */
    public static native String nativeTick();

    /** The current state, without ticking the clock. */
    public static native String nativeState();

    public static native void nativeStop();

}
