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
 * A first pass, not the finished article: `tunante-mini/ui/app.slint` is 2938
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
    queue: List<Track>,
    onQueueRemove: (Track) -> Unit,
    onQueuePlay: (Track) -> Unit,
    onQueueMove: (Int, Int) -> Unit,
) {
    // Declared out here, not inside the Column: the picker it drives is drawn
    // on top of the whole screen, which is a sibling of the Column and not a
    // child of it.
    var adding by remember { mutableStateOf<Track?>(null) }

    Column(Modifier.fillMaxSize().background(T.bgPrimary)) {
        Box(Modifier.weight(1f)) {
            when (dest) {
                Dest.Playing ->
                    PlayingScreen(state, onTogglePlay, onNext, onPrev, onShuffle, onRepeat, onSeek)
                Dest.Queue -> QueueScreen(
                    tracks = queue,
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
                    onPickFolders = onPickFolders,
                )
                Dest.Library -> Library(
                    tab, onTab, playlists, openPlaylist, playlistTracks, onOpenPlaylist,
                    onClosePlaylist, onCreatePlaylist, onDeletePlaylist, onPlayPlaylistIndex,
                    onRenamePlaylist, onMovePlaylist, onEnqueuePlaylist, onEnqueueTrack,
                    onEnqueue, onRemoveFromPlaylist, view, state, hasAllFiles, onGrantFiles,
                    onQuery, onOpenFolder, onUp, onPlayIndex, { adding = it },
                )
            }
        }

        // Under every destination, including Playing -- which is what mini
        // does. Two transports on one screen reads as a duplicate until the
        // phone is in a hand: the bar is where the thumb already is.
        MiniPlayer(state, onOpen = { onDest(Dest.Playing) }, onTogglePlay, onNext, onPrev)
        BottomNav(dest, state.queued, onDest)
    }

    adding?.let { track ->
        PlaylistPicker(
            track = track,
            playlists = playlists,
            onPick = { p -> onAddToPlaylist(p, track); adding = null },
            onCreate = { name -> onNewPlaylistWith(name, track); adding = null },
            onDismiss = { adding = null },
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

        // Search only makes sense over the library itself; the indexes are
        // already a way of finding something.
        if (tab == Tab.Library) {
            SearchBox(view.query, onQuery)
        }
        Breadcrumb(view, onUp)

        Box(Modifier.weight(1f)) {
            if (view.folders.isEmpty() && view.tracks.isEmpty()) {
                Empty(view)
            } else if (view.folders.isNotEmpty() && view.tracks.isEmpty() && !view.searching) {
                // A level that is only folders is a shelf, and a shelf is worth
                // showing as covers. A level with tracks in it is a track list,
                // and covers there would push the titles off the screen.
                FolderGrid(view.folders, coverOf = { it.cover }) { onOpenFolder(it.path) }
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    // Folders first, then what is loose in this one. The order
                    // matters for the index handed to onPlayIndex: it counts
                    // tracks only, so the queue and the list agree.
                    items(view.folders) { folder ->
                        FolderRow(folder) { onOpenFolder(folder.path) }
                    }
                    itemsIndexed(view.tracks) { i, track ->
                        SwipeRow("A la cola", { onEnqueue(track) }) {
                            TrackRow(
                                track = track,
                                selected = state.hasSource && track.path == state.path,
                                onClick = { onPlayIndex(i) },
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
            val n = view.folders.size + view.tracks.size
            Label(
                when (n) {
                    0 -> "sin biblioteca"
                    1 -> "1 elemento"
                    else -> "$n elementos"
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
    track: Track,
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
            Label(
                track.title.ifEmpty { track.path.substringAfterLast('/') },
                T.textPrimary, T.fontBody, maxLines = 1,
            )
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
                        Label("Lista nueva…", T.textMuted, T.fontBody)
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
                ) { Label("Crear", if (name.isBlank()) T.textMuted else T.accent, T.fontBody) }
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
            Label("Sin acceso a los ficheros", color = T.warningFg, size = T.fontBody)
            Label(
                "Tunante necesita leer carpetas enteras: los formatos de consola no se indexan como audio.",
                color = T.warningFg,
                size = T.fontSmall,
            )
        }
        Label("Dar acceso", color = T.warningFg, size = T.fontBody, weight = FontWeight.Medium)
    }
}

@Composable
private fun Empty(view: LibraryView) = EmptyNote(
    if (view.searching) "Nada coincide" else "No hay nada todavía",
    if (view.searching) "Prueba con otra cosa."
    else "Pulsa Escanear para leer tu carpeta de música.",
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
