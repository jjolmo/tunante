package com.tunante.android

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.tunante.android.ui.Folder
import com.tunante.android.ui.LibraryView
import com.tunante.android.ui.DirListing
import com.tunante.android.ui.FolderPicker
import com.tunante.android.ui.PlayerState
import com.tunante.android.ui.Dest
import com.tunante.android.ui.Playlist
import com.tunante.android.ui.Tab
import com.tunante.android.ui.Track
import com.tunante.android.ui.TunanteApp
import com.tunante.android.ui.TunanteTheme
import com.tunante.android.ui.pollState
import org.json.JSONObject
import java.io.File
import kotlin.concurrent.thread

/**
 * The screen.
 *
 * Everything below the pixels is Rust, reached through [NativeBridge]: the
 * library database, the scan, the queue, the decoder. This class owns no state
 * of its own beyond what it is currently drawing — it asks, twice a second, the
 * same way [PlaybackService] does.
 */
class MainActivity : ComponentActivity() {

    private var view by mutableStateOf(LibraryView())
    private var tab by mutableStateOf(Tab.Library)
    private var playlists by mutableStateOf(emptyList<Playlist>())
    private var openPlaylist by mutableStateOf<Playlist?>(null)
    private var playlistTracks by mutableStateOf(emptyList<Track>())
    private var hasFiles by mutableStateOf(false)
    private var picking by mutableStateOf(false)
    private var listing by mutableStateOf(DirListing())
    private var roots by mutableStateOf(emptyList<String>())
    /**
     * Which of the four the app is showing.
     *
     * Starts on the library because that is where you go to put music on; mini
     * starts there too.
     */
    private var dest by mutableStateOf(Dest.Library)
    private var queue by mutableStateOf(emptyList<Track>())


    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val decoder = File(applicationInfo.nativeLibraryDir, "libtunante_decoder.so").absolutePath
        if (!NativeBridge.nativeInit(this, decoder)) {
            Log.e(TAG, "native init failed — see the lines above")
        }
        NativeBridge.nativeOpenDb(filesDir.absolutePath)

        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        }
        startForegroundService(Intent(this, PlaybackService::class.java))

        hasFiles = hasAllFiles()
        browse("")
        reloadPlaylists()
        reloadRoots()
        Log.i(TAG, "session: " + NativeBridge.nativeRestoreSession())

        setContent {
            TunanteTheme {
                val state = pollState { readState() }
                if (picking) {
                    FolderPicker(
                        listing = listing,
                        roots = roots,
                        onEnter = ::listDirs,
                        onUp = { listing.parent?.let(::listDirs) },
                        onToggleRoot = ::toggleRoot,
                        onDone = { picking = false; scan() },
                    )
                    return@TunanteTheme
                }
                TunanteApp(
                    dest = dest,
                    onDest = { d -> if (d == Dest.Queue) reloadQueue(); dest = d },
                    roots = roots,
                    queue = queue,
                    onQueueRemove = { NativeBridge.nativeDequeue(it.path); reloadQueue() },
                    onQueuePlay = { NativeBridge.nativePlayQueued(it.path); reloadQueue() },
                    onQueueMove = { f, t -> NativeBridge.nativeMoveInQueue(f, t); reloadQueue() },
                    onEnqueueRow = ::enqueueRow,
                    onAddRowToPlaylist = ::addRowToPlaylist,
                    onNewPlaylistWithRow = ::newPlaylistWithRow,
                    tab = tab,
                    onTab = ::switchTab,
                    playlists = playlists,
                    openPlaylist = openPlaylist,
                    playlistTracks = playlistTracks,
                    onOpenPlaylist = ::openPlaylist,
                    onClosePlaylist = { openPlaylist = null; playlistTracks = emptyList() },
                    onCreatePlaylist = ::createPlaylist,
                    onDeletePlaylist = ::deletePlaylist,
                    onPlayPlaylistIndex = ::playPlaylistAt,
                    onAddToPlaylist = ::addToPlaylist,
                    onNewPlaylistWith = ::newPlaylistWith,
                    onRenamePlaylist = ::renamePlaylist,
                    onMovePlaylist = ::movePlaylist,
                    onEnqueuePlaylist = ::enqueuePlaylist,
                    onEnqueueTrack = { NativeBridge.nativeEnqueue(it.path) },
                    onEnqueue = { NativeBridge.nativeEnqueue(it.path) },
                    onRemoveFromPlaylist = ::removeFromPlaylist,
                    view = view,
                    state = state,
                    hasAllFiles = hasFiles,
                    onGrantFiles = ::requestAllFiles,
                    onScan = ::rescanOrPick,
                    onPickFolders = ::openPicker,
                    onQuery = ::search,
                    onOpenFolder = ::openRow,
                    onUp = ::up,
                    onPlayIndex = ::playAt,
                    onTogglePlay = { NativeBridge.nativeTogglePlay() },
                    onNext = { NativeBridge.nativeNext() },
                    onPrev = { NativeBridge.nativePrev() },
                    onShuffle = { NativeBridge.nativeSetShuffle(it) },
                    onRepeat = { NativeBridge.nativeSetRepeat(it) },
                    onSleep = { NativeBridge.nativeSetSleepTimer(it) },
                    onClearQueue = { NativeBridge.nativeClearQueue() },
                    onSeek = { NativeBridge.nativeSeek(it) },
                    onLoops = { NativeBridge.nativeCycleLoops() },
                    onFade = { NativeBridge.nativeCycleFade() },
                )
            }
        }

        handleTestIntent()
    }

    /**
     * An intent arriving at an activity that is already running.
     *
     * Without this, being handed a file by a file manager while Tunante is open
     * does nothing at all — onCreate is where the intent was read, and onCreate
     * does not run again. It is also why every test hook needed `am start -S`,
     * which restarts the process and throws away the queue it was meant to be
     * testing.
     */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleTestIntent()
    }

    @Deprecated("The replacement, OnBackPressedDispatcher, needs androidx.activity's callback API; this activity has one back action and no fragments.")
    override fun onBackPressed() {
        // Anything Compose registered gets first refusal.
        //
        // Overriding this method at all takes back off the OnBackPressedDispatcher,
        // and a BackHandler inside the composition registers there. So the sheets
        // -- the folder menu, the playlist picker -- never saw a back press, and
        // it went straight to this activity's navigation while a menu covered the
        // whole screen. Deferring when a callback exists puts them back in line
        // without giving up the navigation below.
        if (onBackPressedDispatcher.hasEnabledCallbacks()) {
            @Suppress("DEPRECATION")
            super.onBackPressed()
            return
        }

        // Android has a back button where Plasma Mobile does not, so it gets
        // wired to the same action as the breadcrumb rather than closing the app
        // from whatever depth you happened to be at.
        // Back out of a destination to the library before it leaves the app:
        // the four are siblings, not a stack, so there is nothing else for
        // "back" to mean once you are somewhere other than where you started.
        if (picking) {
            picking = false
        } else if (tab == Tab.Playlists && openPlaylist != null) {
            openPlaylist = null
            playlistTracks = emptyList()
        } else if (tab != Tab.Library && view.here.isEmpty()) {
            switchTab(Tab.Library)
        } else if (view.searching || view.here.isNotEmpty()) {
            up()
        } else if (dest != Dest.Library) {
            dest = Dest.Library
        } else {
            @Suppress("DEPRECATION")
            super.onBackPressed()
        }
    }

    override fun onPause() {
        super.onPause()
        // The hook the service's five-second cadence cannot give us: this fires
        // at the moment the system is most likely to kill the process next.
        NativeBridge.nativeSaveSession()
    }

    override fun onResume() {
        super.onResume()
        // Coming back from the Settings page that grants all-files access is the
        // main reason this can have changed under us.
        hasFiles = hasAllFiles()
    }

    private fun readState(): PlayerState {
        val s = JSONObject(NativeBridge.nativeState())
        if (!s.optBoolean("ok", false)) return PlayerState()
        return PlayerState(
            playing = s.optBoolean("playing"),
            hasSource = s.optBoolean("hasSource"),
            title = s.optString("title"),
            artist = s.optString("artist"),
            positionMs = s.optLong("positionMs"),
            durationMs = s.optLong("durationMs"),
            index = s.optInt("index", -1),
            shuffle = s.optBoolean("shuffle"),
            repeat = s.optInt("repeat"),
            sleepMinutes = s.optInt("sleepMinutes"),
            loops = s.optInt("loops", 2),
            fadeSeconds = s.optInt("fadeSeconds", 8),
            queued = s.optInt("queued"),
            queuedNext = s.optString("queuedNext"),
            queueLen = s.optInt("queueLen"),
            path = s.optString("path"),
        )
    }

    private fun tracksFrom(array: org.json.JSONArray?): List<Track> {
        if (array == null) return emptyList()
        return (0 until array.length()).map { i ->
            val t = array.getJSONObject(i)
            Track(
                path = t.optString("path"),
                title = t.optString("title"),
                artist = t.optString("artist"),
                album = t.optString("album"),
                durationMs = t.optLong("duration_ms"),
            )
        }
    }

    private fun reloadPlaylists() {
        val s = JSONObject(NativeBridge.nativePlaylists())
        if (!s.optBoolean("ok", false)) return
        val a = s.optJSONArray("playlists") ?: return
        playlists = (0 until a.length()).map { i ->
            val p = a.getJSONObject(i)
            Playlist(p.optString("id"), p.optString("name"), p.optInt("track_count"))
        }
    }

    private fun openPlaylist(playlist: Playlist) {
        openPlaylist = playlist
        val s = JSONObject(NativeBridge.nativePlaylistTracks(playlist.id))
        playlistTracks = if (s.optBoolean("ok", false)) tracksFrom(s.optJSONArray("tracks")) else emptyList()
    }

    private fun createPlaylist(name: String) {
        NativeBridge.nativeCreatePlaylist(name)
        reloadPlaylists()
    }

    private fun deletePlaylist(playlist: Playlist) {
        NativeBridge.nativeDeletePlaylist(playlist.id)
        if (openPlaylist?.id == playlist.id) {
            openPlaylist = null
            playlistTracks = emptyList()
        }
        reloadPlaylists()
    }

    private fun addToPlaylist(playlist: Playlist, track: Track) {
        val paths = org.json.JSONArray().put(track.path)
        Log.i(TAG, "add: " + NativeBridge.nativeAddToPlaylist(playlist.id, paths.toString()))
        reloadPlaylists()
    }

    /**
     * The paths a library row stands for.
     *
     * Off the main thread by every caller: for a console this reads the whole
     * track table and filters it.
     */
    private fun rowPaths(row: String, deep: Boolean): org.json.JSONArray {
        val s = JSONObject(NativeBridge.nativeRowTracks(row, deep))
        val out = org.json.JSONArray()
        val tracks = s.optJSONArray("tracks") ?: return out
        for (i in 0 until tracks.length()) {
            out.put(tracks.getJSONObject(i).optString("path"))
        }
        return out
    }

    /** Long press on a folder, album, game or console: queue the lot. */
    private fun enqueueRow(row: String, deep: Boolean) {
        thread(name = "enqueue-row") {
            val paths = rowPaths(row, deep)
            if (paths.length() == 0) return@thread
            Log.i(TAG, "enqueue row: " + NativeBridge.nativeEnqueuePaths(paths.toString()))
        }
    }

    private fun addRowToPlaylist(playlist: Playlist, row: String, deep: Boolean) {
        thread(name = "add-row") {
            val paths = rowPaths(row, deep)
            if (paths.length() == 0) return@thread
            Log.i(TAG, "add row: " + NativeBridge.nativeAddToPlaylist(playlist.id, paths.toString()))
            runOnUiThread { reloadPlaylists() }
        }
    }

    private fun newPlaylistWithRow(name: String, row: String, deep: Boolean) {
        thread(name = "new-with-row") {
            val paths = rowPaths(row, deep)
            if (paths.length() == 0) return@thread
            val id = JSONObject(NativeBridge.nativeCreatePlaylist(name)).optString("id")
            if (id.isEmpty()) return@thread
            NativeBridge.nativeAddToPlaylist(id, paths.toString())
            runOnUiThread { reloadPlaylists() }
        }
    }

    private fun removeFromPlaylist(playlist: Playlist, track: Track) {
        NativeBridge.nativeRemoveFromPlaylist(playlist.id, track.path)
        openPlaylist(playlist)
        reloadPlaylists()
    }

    private fun reloadQueue() {
        val s = JSONObject(NativeBridge.nativeQueue())
        queue = if (s.optBoolean("ok", false)) tracksFrom(s.optJSONArray("tracks")) else emptyList()
    }

    /** A new playlist with this one track in it, without leaving the library. */
    private fun newPlaylistWith(name: String, track: Track) {
        val created = JSONObject(NativeBridge.nativeCreatePlaylist(name))
        val id = created.optString("id")
        if (id.isEmpty()) return
        NativeBridge.nativeAddToPlaylist(id, org.json.JSONArray().put(track.path).toString())
        reloadPlaylists()
    }

    private fun renamePlaylist(playlist: Playlist, name: String) {
        NativeBridge.nativeRenamePlaylist(playlist.id, name)
        reloadPlaylists()
        if (openPlaylist?.id == playlist.id) {
            openPlaylist = playlist.copy(name = name)
        }
    }

    /** Reorder by handing back the whole order, which is what the core wants. */
    private fun movePlaylist(from: Int, to: Int) {
        if (from !in playlists.indices || to !in playlists.indices) return
        val next = playlists.toMutableList()
        next.add(to, next.removeAt(from))
        NativeBridge.nativeReorderPlaylists(
            org.json.JSONArray().apply { next.forEach { put(it.id) } }.toString()
        )
        playlists = next
    }

    private fun enqueuePlaylist(playlist: Playlist) {
        Log.i(TAG, "enqueue playlist: " + NativeBridge.nativeEnqueuePlaylist(playlist.id))
    }

    private fun playPlaylistAt(index: Int) {
        val paths = org.json.JSONArray()
        playlistTracks.forEach { paths.put(it.path) }
        NativeBridge.nativePlayList(paths.toString(), index)
    }

    /**
     * The three library shapes share one screen: all of them are folders and
     * tracks, and only where the rows come from changes.
     */
    private fun switchTab(next: Tab) {
        tab = next
        when (next) {
            Tab.Library -> browse("")
            Tab.Albums -> load { NativeBridge.nativeAlbums() }
            Tab.Games -> load { NativeBridge.nativeGames("") }
            Tab.Consoles -> load { NativeBridge.nativeConsoles("") }
            Tab.Playlists -> reloadPlaylists()
        }
    }

    /**
     * Read a `{folders, tracks}` answer into the view.
     *
     * Off the main thread, always: every one of these reads the whole track
     * table to derive its rows, so on a real collection it is a full table read
     * plus the JSON for it — not something to do between two frames.
     */
    private fun load(here: String = "", label: String = "", call: () -> String) {
        thread(name = "load") {
            val s = JSONObject(call())
            if (!s.optBoolean("ok", false)) return@thread
            val folders = s.optJSONArray("folders")
            val next = LibraryView(
                here = here,
                label = label,
                folders = (0 until (folders?.length() ?: 0)).map { i ->
                    val f = folders!!.getJSONObject(i)
                    Folder(
                        f.optString("path"),
                        f.optString("name"),
                        f.optInt("count"),
                        f.optString("cover"),
                    )
                },
                tracks = tracksFrom(s.optJSONArray("tracks")),
                query = "",
            )
            runOnUiThread { view = next }
        }
    }

    /** Show one level of the tree. */
    private fun browse(folder: String) = load(folder) { NativeBridge.nativeBrowse(folder) }

    /** Opening a row means something different in each shape. */
    private fun openRow(path: String) {
        when (tab) {
            // The index tabs navigate by name, so the name is also the label.
            Tab.Games -> load(path, path) { NativeBridge.nativeGames(path) }
            // Consoles has three levels. At the top a row is a console and its
            // name is the key; below that a row is one of its directories, and
            // the key has to carry both -- a folder holding .spc rips and mp3s
            // shows up under two consoles, and only the pair says which one
            // was opened. The label stays the folder's own name.
            Tab.Consoles -> if (view.here.isEmpty()) {
                load(path, path) { NativeBridge.nativeConsoles(path) }
            } else {
                val key = view.here + CONSOLE_SEP + path
                load(key, path.substringAfterLast('/')) { NativeBridge.nativeConsoles(key) }
            }
            // An album row and a tree row are both directories.
            else -> browse(path)
        }
    }

    /**
     * Search replaces the tree rather than filtering it.
     *
     * When you are after a title you do not care which folder it was in, and a
     * filtered tree makes you walk down to find out.
     */
    private fun search(query: String) {
        if (query.isEmpty()) {
            browse(view.here)
            return
        }
        val s = JSONObject(NativeBridge.nativeSearch(query))
        view = view.copy(
            query = query,
            folders = emptyList(),
            tracks = if (s.optBoolean("ok", false)) tracksFrom(s.optJSONArray("tracks")) else emptyList(),
        )
    }

    /** Out of a search, or one folder up. */
    private fun up() {
        when {
            view.searching -> search("")
            // Consoles is the one index with a middle level, so leaving a game
            // goes back to its console rather than all the way out.
            tab == Tab.Consoles && view.here.contains(CONSOLE_SEP) -> {
                val console = view.here.substringBefore(CONSOLE_SEP)
                load(console, console) { NativeBridge.nativeConsoles(console) }
            }
            // In the other indexes there is one level to come back from, and it
            // is the index itself rather than a parent directory.
            tab != Tab.Library && view.here.isNotEmpty() -> switchTab(tab)
            view.here.isEmpty() -> Unit
            else -> browse(view.here.substringBeforeLast('/', ""))
        }
    }

    /**
     * Play what is on screen, starting where they tapped.
     *
     * The queue is exactly the list being shown — not `get_tracks_by_folder`,
     * which also matches subfolders and would put a different track under the
     * finger than the one it landed on.
     */
    private fun playAt(index: Int) {
        val paths = org.json.JSONArray()
        view.tracks.forEach { paths.put(it.path) }
        NativeBridge.nativePlayList(paths.toString(), index)
    }

    /** Empty means every folder the library is built from. */
    private fun scan(root: String = "") {
        thread(name = "scan") {
            val result = NativeBridge.nativeScan(root)
            Log.i(TAG, "scan: $result")
            runOnUiThread { switchTab(tab) }
        }
    }

    private fun reloadRoots() {
        val s = JSONObject(NativeBridge.nativeRoots())
        val a = s.optJSONArray("roots") ?: return
        roots = (0 until a.length()).map { a.getJSONObject(it).optString("path") }
    }

    /**
     * Rescan when there is something to rescan; otherwise ask where the music
     * is, because a scan with no roots has nothing to say.
     */
    private fun rescanOrPick() {
        if (roots.isEmpty()) openPicker() else scan()
    }

    private fun openPicker() {
        picking = true
        // Start where the roots are if there are any, so the common case is
        // "add another folder next to the one I already use".
        listDirs(roots.firstOrNull()?.substringBeforeLast('/') ?: "")
    }

    private fun listDirs(path: String) {
        thread(name = "dirs") {
            val s = JSONObject(NativeBridge.nativeListDirs(path))
            if (!s.optBoolean("ok", false)) return@thread
            val d = s.optJSONArray("dirs")
            val next = DirListing(
                here = s.optString("here"),
                parent = if (s.isNull("parent")) null else s.optString("parent"),
                dirs = (0 until (d?.length() ?: 0)).map { d!!.getString(it) },
            )
            runOnUiThread { listing = next }
        }
    }

    private fun toggleRoot(path: String, add: Boolean) {
        Log.i(TAG, "root: " + NativeBridge.nativeSetRoot(path, add))
        reloadRoots()
    }

    /**
     * Whether we can read the user's music by plain absolute path.
     *
     * Below API 30 there is nothing to ask about: the old READ_EXTERNAL_STORAGE
     * model still applies and paths just work.
     */
    private fun hasAllFiles(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.R || Environment.isExternalStorageManager()

    /**
     * Send the user to the system screen that grants it.
     *
     * There is no runtime dialog for this one — it is a Settings page with a
     * toggle. Deep-linked to our own entry so it is one tap rather than a scroll
     * through every installed app.
     */
    private fun requestAllFiles() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
        startActivity(
            Intent(
                Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                Uri.parse("package:$packageName"),
            )
        )
    }

    /**
     * The hooks the phases were tested through, kept because CI will need them.
     *
     *     adb shell am start -S -n com.tunante.android/.MainActivity --es scan /path
     *     adb shell am start -S -n com.tunante.android/.MainActivity --es playFolder /path
     *     adb shell am start -S -n com.tunante.android/.MainActivity --es play /path/to/file
     *
     * `-S` is not optional: without it, `am start` on a live activity delivers
     * onNewIntent and onCreate never runs, so the test appears to pass without
     * having done anything.
     */
    private fun handleTestIntent() {
        val i = intent ?: return
        i.getStringExtra("scan")?.let { scan(it) }
        i.getStringExtra("playFolder")?.let {
            Log.i(TAG, "playFolder: " + NativeBridge.nativePlayFolder(it, 0))
        }
        i.getStringExtra("play")?.let { NativeBridge.nativePlay(it) }
        i.getStringExtra("enqueue")?.let {
            Log.i(TAG, "enqueue: " + NativeBridge.nativeEnqueue(it))
        }
    }

    companion object {
        private const val TAG = "tunante"

        /**
         * Joins a console to one of its directories in a single row key.
         *
         * U+0001 because it is the one byte a path cannot contain, so the two
         * halves always split back apart cleanly. tunante-mini encodes the same
         * pair the same way.
         */
        private const val CONSOLE_SEP = '\u0001'
    }
}
