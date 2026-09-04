package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

/** One row of the library, as the bridge hands it over. */
data class Track(
    val path: String,
    val title: String,
    val artist: String,
    val album: String,
    val durationMs: Long,
)

/** What the player is doing, polled from `nativeState`. */
data class PlayerState(
    val playing: Boolean = false,
    val hasSource: Boolean = false,
    val title: String = "",
    val artist: String = "",
    val album: String = "",
    val positionMs: Long = 0,
    val durationMs: Long = 0,
    val index: Int = -1,
    val queueLen: Int = 0,
    val path: String = "",
    val shuffle: Boolean = false,
    val repeat: Int = 0,
    val sleepMinutes: Int = 0,
    val queued: Int = 0,
    val queuedNext: String = "",
    val loops: Int = 2,
    val fadeSeconds: Int = 8,
) {
    /**
     * What to call the track on screen.
     *
     * The same fallback the list uses. Plenty of files carry no title tag —
     * anything that came out of Telegram, for one — and the row showed the file
     * name while the bar underneath showed a dash for the same track.
     */
    val label: String get() = title.ifEmpty { path.substringAfterLast('/') }.ifEmpty { "—" }
}

/**
 * The whole screen.
 *
 * A first pass, not the finished article: `tunante/ui/app.slint` is 2938
 * lines and has a folder tree, playlists, a search box, swipe-to-act rows and a
 * sleep timer. What is here is the spine — the library, the transport and the
 * now-playing bar — with the palette and the 48 dp touch targets already right,
 * so the rest is filling in rather than deciding.
 */
@Composable
fun TunanteApp(
    dest: Dest,
    onDest: (Dest) -> Unit,
    roots: List<String>,
    tab: Tab,
    onTab: (Tab) -> Unit,
    playlists: List<Playlist>,
    openPlaylist: Playlist?,
    playlistTracks: List<Track>,
    onOpenPlaylist: (Playlist) -> Unit,
    onClosePlaylist: () -> Unit,
    onCreatePlaylist: (String) -> Unit,
    onDeletePlaylist: (Playlist) -> Unit,
    onPlayPlaylistIndex: (Int) -> Unit,
    onAddToPlaylist: (Playlist, Track) -> Unit,
    onNewPlaylistWith: (String, Track) -> Unit,
    onRenamePlaylist: (Playlist, String) -> Unit,
    onMovePlaylist: (Int, Int) -> Unit,
    onEnqueuePlaylist: (Playlist) -> Unit,
    onEnqueueTrack: (Track) -> Unit,
    onEnqueue: (Track) -> Unit,
    onRemoveFromPlaylist: (Playlist, Track) -> Unit,
    view: LibraryView,
    state: PlayerState,
    hasAllFiles: Boolean,
    onGrantFiles: () -> Unit,
    onScan: () -> Unit,
    onDownloadCovers: () -> Unit,
    coverStatus: String,
    onPickFolders: () -> Unit,
    onQuery: (String) -> Unit,
    onOpenFolder: (String) -> Unit,
    onUp: () -> Unit,
    onPlayIndex: (Int) -> Unit,
    onTogglePlay: () -> Unit,
    onNext: () -> Unit,
    onPrev: () -> Unit,
    onShuffle: (Boolean) -> Unit,
    onRepeat: (Int) -> Unit,
    onSleep: (Int) -> Unit,
    onClearQueue: () -> Unit,
    onSeek: (Long) -> Unit,
    onLoops: () -> Unit,
    onFade: () -> Unit,
    onEnqueueRow: (String, Boolean) -> Unit,
    onAddRowToPlaylist: (Playlist, String, Boolean) -> Unit,
    onNewPlaylistWithRow: (String, String, Boolean) -> Unit,
    queue: List<Track>,
    onQueueRemove: (Track) -> Unit,
    onQueuePlay: (Track) -> Unit,
    onQueueMove: (Int, Int) -> Unit,
) {
    // Declared out here, not inside the Column: the picker it drives is drawn
    // on top of the whole screen, which is a sibling of the Column and not a
    // child of it.
    var adding by remember { mutableStateOf<Track?>(null) }
    // The row a long press landed on, and -- once an action is chosen -- the
    // row key and depth waiting for a playlist to be picked.
    var menuFor by remember { mutableStateOf<Folder?>(null) }
    // Key, depth, and the name to show. The name is carried rather than parsed
    // back out of the key: it was `key.substringAfterLast('/').substringAfter(':')`,
    // which happens to work and stops working on the first game whose own name
    // has a colon in it -- and "Final Fantasy Tactics A2: The Sealed Grimoire"
    // is sitting in the test library.
    var pendingRow by remember { mutableStateOf<Triple<String, Boolean, String>?>(null) }

    // A sheet covers the screen, so back belongs to it while it is up.
    androidx.activity.compose.BackHandler(
        enabled = adding != null || menuFor != null || pendingRow != null
    ) {
        pendingRow = null
        menuFor = null
        adding = null
    }

    Column(Modifier.fillMaxSize().background(T.bgPrimary)) {
        // Sideways changes destination, as it does in mini.
        //
        // Horizontal only, and on the container rather than on each screen: a
        // vertical one would eat the list scrolling, which is the most used
        // gesture in the app. It does not steal from the rows that swipe to
        // queue, or from the seek bar, because Compose offers a pointer event
        // to the deepest node first and this detector only sees what those
        // leave unconsumed.
        val slide = with(LocalDensity.current) { 60.dp.toPx() }
        Box(
            Modifier
                .weight(1f)
                .pointerInput(dest) {
                    var moved = 0f
                    detectHorizontalDragGestures(
                        onDragEnd = {
                            val order = Dest.entries
                            val i = order.indexOf(dest)
                            if (moved < -slide && i < order.lastIndex) onDest(order[i + 1])
                            if (moved > slide && i > 0) onDest(order[i - 1])
                            moved = 0f
                        },
                        onDragCancel = { moved = 0f },
                    ) { _, delta -> moved += delta }
                },
        ) {
            when (dest) {
                Dest.Playing ->
                    PlayingScreen(state, onTogglePlay, onNext, onPrev, onShuffle, onRepeat, onSeek)
                Dest.Queue -> QueueScreen(
                    tracks = queue,
                    nowPath = state.path,
                    onRemove = onQueueRemove,
                    onPlay = onQueuePlay,
                    onMove = onQueueMove,
                    onClear = onClearQueue,
                )
                Dest.Settings -> SettingsScreen(
                    state = state,
                    roots = roots,
                    onLoops = onLoops,
                    onFade = onFade,
                    onSleep = onSleep,
                    onScan = onScan,
                    onDownloadCovers = onDownloadCovers,
                    coverStatus = coverStatus,
                    onPickFolders = onPickFolders,
                )
                Dest.Library -> Library(
                    tab, onTab, playlists, openPlaylist, playlistTracks, onOpenPlaylist,
                    onClosePlaylist, onCreatePlaylist, onDeletePlaylist, onPlayPlaylistIndex,
                    onRenamePlaylist, onMovePlaylist, onEnqueuePlaylist, onEnqueueTrack,
                    onEnqueue, onRemoveFromPlaylist, view, state, hasAllFiles, onGrantFiles,
                    onQuery, onOpenFolder, onUp, onPlayIndex, { adding = it },
                    { menuFor = it },
                )
            }
        }

        // Under every destination except Playing, which is `root.tab != 0` in
        // app.slint. It used to be under Playing as well, on the argument that
        // the bar is where the thumb already is; the desktop tried that, found
        // the two transports were pushing each other off a short screen, and
        // dropped it. The screen that has a full transport does not need a
        // second one.
        if (dest != Dest.Playing) {
            MiniPlayer(state, onOpen = { onDest(Dest.Playing) }, onTogglePlay, onNext, onPrev)
        }
        BottomNav(dest, state.queued, onDest)
    }

    adding?.let { track ->
        PlaylistPicker(
            what = track.title.ifEmpty { track.path.substringAfterLast('/') },
            playlists = playlists,
            onPick = { p -> onAddToPlaylist(p, track); adding = null },
            onCreate = { name -> onNewPlaylistWith(name, track); adding = null },
            onDismiss = { adding = null },
        )
    }

    menuFor?.let { folder ->
        val key = rowKey(tab, view.here, folder.path)
        FolderMenu(
            name = folder.name,
            // Only a real directory has subfolders to include or leave out. A
            // game is an album tag and a console is a set of extensions.
            isDirectory = tab == Tab.Library || tab == Tab.Albums ||
                (tab == Tab.Consoles && view.here.isNotEmpty()),
            onEnqueue = { deep -> onEnqueueRow(key, deep); menuFor = null },
            onAddToPlaylist = { deep ->
                pendingRow = Triple(key, deep, folder.name)
                menuFor = null
            },
            onDismiss = { menuFor = null },
        )
    }

    pendingRow?.let { (key, deep, name) ->
        PlaylistPicker(
            what = name,
            playlists = playlists,
            onPick = { p -> onAddRowToPlaylist(p, key, deep); pendingRow = null },
            onCreate = { name -> onNewPlaylistWithRow(name, key, deep); pendingRow = null },
            onDismiss = { pendingRow = null },
        )
    }
}

/**
 * The library: the mode tabs and whatever level they are showing.
 *
 * Nothing above them any more. The title bar said "Tunante", which the launcher
 * icon has already said, and carried a Scan button that belongs in Ajustes --
 * it was in a title bar because Ajustes did not exist.
 */
@Composable
private fun Library(
    tab: Tab,
    onTab: (Tab) -> Unit,
    playlists: List<Playlist>,
    openPlaylist: Playlist?,
    playlistTracks: List<Track>,
    onOpenPlaylist: (Playlist) -> Unit,
    onClosePlaylist: () -> Unit,
    onCreatePlaylist: (String) -> Unit,
    onDeletePlaylist: (Playlist) -> Unit,
    onPlayPlaylistIndex: (Int) -> Unit,
    onRenamePlaylist: (Playlist, String) -> Unit,
    onMovePlaylist: (Int, Int) -> Unit,
    onEnqueuePlaylist: (Playlist) -> Unit,
    onEnqueueTrack: (Track) -> Unit,
    onEnqueue: (Track) -> Unit,
    onRemoveFromPlaylist: (Playlist, Track) -> Unit,
    view: LibraryView,
    state: PlayerState,
    hasAllFiles: Boolean,
    onGrantFiles: () -> Unit,
    onQuery: (String) -> Unit,
    onOpenFolder: (String) -> Unit,
    onUp: () -> Unit,
    onPlayIndex: (Int) -> Unit,
    onAdd: (Track) -> Unit,
    onFolderLongPress: (Folder) -> Unit,
) {
    Column(Modifier.fillMaxSize()) {
        if (!hasAllFiles) {
            PermissionBanner(onGrantFiles)
        }

        Tabs(tab, onTab)

        if (tab == Tab.Playlists) {
            Box(Modifier.weight(1f)) {
                PlaylistsTab(
                    playlists = playlists,
                    open = openPlaylist,
                    tracks = playlistTracks,
                    state = state,
                    onOpen = onOpenPlaylist,
                    onBack = onClosePlaylist,
                    onCreate = onCreatePlaylist,
                    onDelete = onDeletePlaylist,
                    onPlayIndex = onPlayPlaylistIndex,
                    onRemove = onRemoveFromPlaylist,
                    onRename = onRenamePlaylist,
                    onMove = onMovePlaylist,
                    onEnqueueAll = onEnqueuePlaylist,
                    onEnqueueOne = onEnqueueTrack,
                )
            }
            return@Column
        }

        // One box doing two jobs, saying which. Over the tree it searches the
        // whole library through the database; over an index it narrows what is
        // already on the screen, because an index has no deeper level to search
        // into. mini writes the same two placeholders for the same reason: the
        // field looks identical in both and hiding the difference would be
        // lying about which one you are getting.
        //
        // It used to appear only over the tree, which left no way at all to
        // find one album among four hundred.
        var filter by remember(tab, view.here) { mutableStateOf("") }
        if (tab == Tab.Library) {
            SearchBox(view.query, onQuery = onQuery)
        } else {
            SearchBox(filter, hint = tr("Filtrar lo que se ve…")) { filter = it }
        }
        Breadcrumb(view, onUp)

        val folders = if (tab == Tab.Library) view.folders
        else view.folders.filter { folds(it.name, filter) }
        val tracks = if (tab == Tab.Library) view.tracks
        else view.tracks.filter { folds(it.title.ifEmpty { it.path }, filter) }

        Box(Modifier.weight(1f)) {
            if (folders.isEmpty() && tracks.isEmpty()) {
                Empty(view)
            } else if (folders.isNotEmpty() && tracks.isEmpty() && !view.searching) {
                // A level that is only folders is a shelf, and a shelf is worth
                // showing as covers. A level with tracks in it is a track list,
                // and covers there would push the titles off the screen.
                FolderGrid(folders, coverOf = { it.cover }, onLongPress = onFolderLongPress) {
                    onOpenFolder(it.path)
                }
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    // Folders first, then what is loose in this one. The order
                    // matters for the index handed to onPlayIndex: it counts
                    // tracks only, so the queue and the list agree.
                    items(folders) { folder ->
                        FolderRow(
                            folder,
                            onClick = { onOpenFolder(folder.path) },
                            onLongClick = { onFolderLongPress(folder) },
                        )
                    }
                    items(tracks) { track ->
                        SwipeRow(tr("A la cola"), { onEnqueue(track) }) {
                            TrackRow(
                                track = track,
                                selected = state.hasSource && track.path == state.path,
                                // The index into the *unfiltered* list, always.
                                // A filtered list renumbers its rows, and the
                                // queue is built from what the level holds, so
                                // handing over the visible position would play
                                // a different track than the one pressed.
                                onClick = { onPlayIndex(view.tracks.indexOf(track)) },
                                // Long press is where "add to playlist" goes: a
                                // button on every row would compete with the row
                                // itself for a finger, and the row is what you
                                // press ninety-nine times out of a hundred.
                                onLongClick = { onAdd(track) },
                            )
                        }
                    }
                }
            }
        }

        // How much is on this level, where mini puts it: under the list, not in
        // a title bar above it.
        Rule()
        Row(Modifier.fillMaxWidth().padding(horizontal = T.gap, vertical = 6.dp)) {
            val n = folders.size + tracks.size
            Label(
                when (n) {
                    0 -> tr("sin biblioteca")
                    1 -> tr("1 elemento")
                    else -> tr("{} elementos").replace("{}", "$n")
                },
                T.textMuted,
                T.fontSmall,
            )
        }
    }
}

/**
 * Which playlist to put a track in.
 *
 * A scrim over the whole screen rather than a Material dialog: the dialog's own
 * surface colour fights this palette, and overriding it costs more than drawing
 * the box.
 */
@Composable
private fun PlaylistPicker(
    what: String,
    playlists: List<Playlist>,
    onPick: (Playlist) -> Unit,
    onCreate: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var name by remember { mutableStateOf("") }
    Box(
        Modifier
            .fillMaxSize()
            .background(androidx.compose.ui.graphics.Color(0xCC000000))
            .clickable(onClick = onDismiss),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .padding(T.gap * 2)
                .background(T.bgSecondary)
                .padding(vertical = T.gap),
        ) {
            Label(what, T.textPrimary, T.fontBody, maxLines = 1)
            Spacer(Modifier.height(T.gap))
            for (p in playlists) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = T.touchTarget)
                        .clickable { onPick(p) },
                    contentAlignment = Alignment.CenterStart,
                ) { Label(p.name, T.accent, T.fontBody, maxLines = 1) }
            }
            // A new list, from here. Going to the Listas tab to create one and
            // then coming back to find the track again is three screens for
            // something that is one intention.
            Rule()
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.weight(1f).heightIn(min = T.touchTarget), Alignment.CenterStart) {
                    if (name.isEmpty()) {
                        Label(tr("Lista nueva…"), T.textMuted, T.fontBody)
                    }
                    androidx.compose.foundation.text.BasicTextField(
                        value = name,
                        onValueChange = { name = it },
                        singleLine = true,
                        textStyle = androidx.compose.ui.text.TextStyle(
                            color = T.textPrimary, fontSize = T.fontBody,
                        ),
                        cursorBrush = androidx.compose.ui.graphics.SolidColor(T.accent),
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
                Box(
                    Modifier
                        .heightIn(min = T.touchTarget)
                        .clickable(enabled = name.isNotBlank()) { onCreate(name.trim()) }
                        .padding(horizontal = T.gap),
                    contentAlignment = Alignment.Center,
                ) { Label(tr("Crear"), if (name.isBlank()) T.textMuted else T.accent, T.fontBody) }
            }
        }
    }
}

/**
 * The one banner there is, in amber.
 *
 * Not decoration: without all-files access the scan finds literally nothing —
 * a directory it cannot read does not even list — so an empty library with no
 * explanation is the default first-run experience otherwise.
 */
@Composable
private fun PermissionBanner(onGrant: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .background(T.warningBg)
            .clickable(onClick = onGrant)
            .padding(T.gap),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Label(tr("Sin acceso a los ficheros"), color = T.warningFg, size = T.fontBody)
            Label(
                tr("Tunante necesita leer carpetas enteras: los formatos de consola no se indexan como audio."),
                color = T.warningFg,
                size = T.fontSmall,
            )
        }
        Label(tr("Dar acceso"), color = T.warningFg, size = T.fontBody, weight = FontWeight.Medium)
    }
}

@Composable
private fun Empty(view: LibraryView) = EmptyNote(
    if (view.searching) tr("Nada coincide") else tr("No hay nada todavía"),
    if (view.searching) tr("Prueba con otra cosa.")
    else tr("Pulsa Escanear para leer tu carpeta de música."),
)

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun TrackRow(
    track: Track,
    selected: Boolean,
    onClick: () -> Unit,
    onLongClick: (() -> Unit)? = null,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = T.touchTarget)
            .background(if (selected) T.bgSelected else T.bgPrimary)
            .combinedClickable(onClick = onClick, onLongClick = onLongClick)
            .padding(horizontal = T.gap, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Cover(track.path, 40.dp)
        Spacer(Modifier.width(T.gap))
        Column(Modifier.weight(1f)) {
            Label(
                track.title.ifEmpty { track.path.substringAfterLast('/') },
                color = T.textPrimary,
                size = T.fontBody,
                maxLines = 1,
            )
            val sub = listOf(track.artist, track.album).filter { it.isNotEmpty() }.joinToString(" — ")
            if (sub.isNotEmpty()) {
                Label(sub, color = T.textSecondary, size = T.fontSmall, maxLines = 1)
            }
        }
        Spacer(Modifier.width(T.gap))
        Label(mmss(track.durationMs), color = T.textMuted, size = T.fontSmall)
    }
    Rule()
}

/** Anything you can press is at least 48 dp, whatever is drawn inside it. */
@Composable
private fun TapTarget(onClick: () -> Unit, content: @Composable () -> Unit) {
    Box(
        Modifier
            .size(T.touchTarget)
            .clip(RoundedCornerShape(T.radius))
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { content() }
}

/** Poll the bridge for player state, at the same cadence as the service ticks. */
@Composable
fun pollState(read: () -> PlayerState): PlayerState {
    var state by remember { mutableStateOf(PlayerState()) }
    LaunchedEffect(Unit) {
        while (true) {
            state = read()
            delay(500)
        }
    }
    return state
}
