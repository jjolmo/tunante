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
    onOpenQueue: () -> Unit,
) {
    // Declared out here, not inside the Column: the picker it drives is drawn
    // on top of the whole screen, which is a sibling of the Column and not a
    // child of it.
    var adding by remember { mutableStateOf<Track?>(null) }

    Column(
        Modifier
            .fillMaxSize()
            .background(T.bgPrimary)
    ) {
        TopBar(view.tracks.size + view.folders.size, onScan, onPickFolders)

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
            if (state.hasSource) {
                NowPlaying(state, onTogglePlay, onNext, onPrev, onShuffle, onRepeat, onSleep,
                    onClearQueue, onSeek, onLoops, onFade, onOpenQueue)
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
                                onLongClick = { adding = track },
                            )
                        }
                    }
                }
            }
        }

        if (state.hasSource) {
            NowPlaying(state, onTogglePlay, onNext, onPrev, onShuffle, onRepeat, onSleep,
                    onClearQueue, onSeek, onLoops, onFade, onOpenQueue)
        }
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

@Composable
private fun TopBar(count: Int, onScan: () -> Unit, onPickFolders: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .background(T.bgSecondary)
            .padding(horizontal = T.gap),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(vertical = T.gap)) {
            Label("Tunante", color = T.textPrimary, size = T.fontTitle, weight = FontWeight.Medium)
            Label(
                if (count == 0) "sin biblioteca" else "$count elementos",
                color = T.textSecondary,
                size = T.fontSmall,
            )
        }
        // Tap rescans, long press chooses folders. Rescanning is the thing you
        // do repeatedly; picking where the music lives is the thing you do
        // once, and making the common action go through the rare one's screen
        // was a regression.
        WideTapTarget(onClick = onScan, onLongClick = onPickFolders) {
            Label("Escanear", color = T.accent, size = T.fontBody, maxLines = 1)
        }
    }
    Rule()
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

@Composable
private fun NowPlaying(
    state: PlayerState,
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
    onOpenQueue: () -> Unit,
) {
    Rule()
    Column(Modifier.fillMaxWidth().background(T.bgSecondary)) {
        Progress(state, onSeek)
        Row(
            Modifier.fillMaxWidth().padding(horizontal = T.gap, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Cover(state.path, T.touchTarget)
            Spacer(Modifier.width(T.gap))
            Column(Modifier.weight(1f)) {
                Label(
                    state.label,
                    color = T.textPrimary,
                    size = T.fontBody,
                    maxLines = 1,
                )
                Label(
                    "${mmss(state.positionMs)} / ${mmss(state.durationMs)}" +
                        if (state.artist.isNotEmpty()) "  ·  ${state.artist}" else "",
                    color = T.textSecondary,
                    size = T.fontSmall,
                    maxLines = 1,
                )
            }
            TapTarget(onPrev) { Glyph("◀◀") }
            TapTarget(onTogglePlay) { Glyph(if (state.playing) "❚❚" else "▶") }
            TapTarget(onNext) { Glyph("▶▶") }
        }
        Modes(state, onShuffle, onRepeat, onSleep, onLoops, onFade, onOpenQueue)
    }
}

/**
 * Shuffle, repeat and the sleep timer.
 *
 * A second row rather than more glyphs beside the transport: these are settings
 * you change rarely and the transport is what a thumb reaches for, so they do
 * not get to share its space.
 */
@Composable
private fun Modes(
    state: PlayerState,
    onShuffle: (Boolean) -> Unit,
    onRepeat: (Int) -> Unit,
    onSleep: (Int) -> Unit,
    onLoops: () -> Unit,
    onFade: () -> Unit,
    onOpenQueue: () -> Unit,
) {
    // Two rows. Loops and fade are not general playback settings — they are
    // what *makes* a length for music that has none, and they belong next to
    // each other rather than scattered among shuffle and repeat.
    Row(
        Modifier.fillMaxWidth().padding(horizontal = T.gap),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Mode(
            if (state.loops == 0) "Bucle ∞" else "Bucle ×${state.loops}",
            state.loops != 2,
            onLoops,
        )
        Mode(
            if (state.fadeSeconds == 0) "Sin fundido" else "Fundido ${state.fadeSeconds}s",
            state.fadeSeconds != 8,
            onFade,
        )
        Spacer(Modifier.weight(1f))
        if (state.queued > 0) {
            Mode("Cola (${state.queued})", true, onOpenQueue)
        }
    }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = T.gap).padding(bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Words, not symbols.
        //
        // These were glyphs and one of them rendered as an empty box: the moon
        // for the sleep timer is not in the system font, and Android does not
        // let an app assume one the way tunante-mini can, where the package
        // depends on DejaVu. A control nobody can read is worse than a wide one.
        Mode("Aleatorio", state.shuffle) { onShuffle(!state.shuffle) }
        Mode(
            when (state.repeat) {
                1 -> "Repetir todo"
                2 -> "Repetir una"
                else -> "Repetir"
            },
            state.repeat != 0,
        ) { onRepeat((state.repeat + 1) % 3) }
        Spacer(Modifier.weight(1f))
        // Cycles through the intervals mini offers, then back to off. A picker
        // for five choices costs more taps than tapping through them.
        val next = when (state.sleepMinutes) {
            0 -> 15
            in 1..15 -> 30
            in 16..30 -> 60
            else -> 0
        }
        Box(
            Modifier
                .heightIn(min = T.touchTarget)
                .clickable { onSleep(next) }
                .padding(horizontal = T.gap),
            contentAlignment = Alignment.Center,
        ) {
            Label(
                if (state.sleepMinutes > 0) "Apagar en ${state.sleepMinutes} min" else "Apagar",
                if (state.sleepMinutes > 0) T.accent else T.textMuted,
                T.fontSmall,
                maxLines = 1,
            )
        }
    }
}

@Composable
private fun Mode(text: String, on: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .heightIn(min = T.touchTarget)
            .clickable(onClick = onClick)
            .padding(end = T.gap),
        contentAlignment = Alignment.Center,
    ) {
        Label(text, if (on) T.accent else T.textMuted, T.fontSmall, maxLines = 1)
    }
}

/**
 * The progress bar.
 *
 * Drawn from a wall clock, not from the audio device: the decoder pipe carries
 * no timing of its own. Good enough for a bar, and the same choice tunante-mini
 * makes.
 */
@Composable
private fun Progress(state: PlayerState, onSeek: (Long) -> Unit) {
    // Wider than it looks. Two pixels is right for the line and impossible for a
    // thumb, so the touch area is a 24 dp band with the line drawn inside it —
    // the same trick every media player uses and the reason this was unusable
    // when the bar was only what you could see.
    var dragging by remember { mutableStateOf<Float?>(null) }
    var width by remember { mutableFloatStateOf(1f) }

    val live = if (state.durationMs > 0) {
        (state.positionMs.toFloat() / state.durationMs).coerceIn(0f, 1f)
    } else {
        0f
    }
    // While a finger is down the bar follows the finger, not the clock: letting
    // the 500 ms tick fight the drag makes the handle jump backwards.
    val fraction = dragging ?: live

    fun seekTo(x: Float) {
        if (state.durationMs <= 0) return
        onSeek(((x / width).coerceIn(0f, 1f) * state.durationMs).toLong())
    }

    Box(
        Modifier
            .fillMaxWidth()
            .height(24.dp)
            .onSizeChanged { width = it.width.toFloat().coerceAtLeast(1f) }
            .pointerInput(state.durationMs) {
                detectHorizontalDragGestures(
                    onDragStart = { dragging = (it.x / width).coerceIn(0f, 1f) },
                    onDragEnd = {
                        dragging?.let { onSeek((it * state.durationMs).toLong()) }
                        dragging = null
                    },
                    onDragCancel = { dragging = null },
                ) { change, delta ->
                    change.consume()
                    dragging = ((dragging ?: live) + delta / width).coerceIn(0f, 1f)
                }
            }
            .pointerInput(state.durationMs) {
                // A tap jumps. Separate from the drag because a tap has no
                // movement for the drag detector to work with.
                detectTapGestures { seekTo(it.x) }
            },
        contentAlignment = Alignment.CenterStart,
    ) {
        Box(Modifier.fillMaxWidth().height(2.dp).background(T.bgTertiary))
        Box(
            Modifier
                .fillMaxWidth(fraction)
                .height(if (dragging != null) 4.dp else 2.dp)
                .background(T.accent)
        )
    }
}

@Composable
private fun Glyph(text: String) =
    Label(text, color = T.textPrimary, size = T.fontTitle)

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

/**
 * Like [TapTarget] but sized by its content.
 *
 * The square one is right for a glyph and wrong for a word: at 48 dp fixed,
 * "Escanear" wrapped to two lines and read as two buttons.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun WideTapTarget(
    onClick: () -> Unit,
    onLongClick: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    Box(
        Modifier
            .heightIn(min = T.touchTarget)
            .clip(RoundedCornerShape(T.radius))
            .combinedClickable(onClick = onClick, onLongClick = onLongClick)
            .padding(horizontal = T.gap),
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
