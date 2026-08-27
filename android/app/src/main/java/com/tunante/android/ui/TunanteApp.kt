package com.tunante.android.ui

import androidx.compose.foundation.background
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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
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
                )
            }
            if (state.hasSource) {
                NowPlaying(state, onTogglePlay, onNext, onPrev, onShuffle, onRepeat, onSleep, onClearQueue)
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
            NowPlaying(state, onTogglePlay, onNext, onPrev, onShuffle, onRepeat, onSleep, onClearQueue)
        }
    }

    adding?.let { track ->
        PlaylistPicker(
            track = track,
            playlists = playlists,
            onPick = { p -> onAddToPlaylist(p, track); adding = null },
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
    onDismiss: () -> Unit,
) {
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
            if (playlists.isEmpty()) {
                Label("No hay listas. Crea una en la pestaña Listas.", T.textMuted, T.fontSmall)
            } else {
                for (p in playlists) {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = T.touchTarget)
                            .clickable { onPick(p) },
                        contentAlignment = Alignment.CenterStart,
                    ) { Label(p.name, T.accent, T.fontBody, maxLines = 1) }
                }
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
) {
    Rule()
    Column(Modifier.fillMaxWidth().background(T.bgSecondary)) {
        QueueStrip(state, onClearQueue)
        Progress(state)
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
        Modes(state, onShuffle, onRepeat, onSleep)
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
) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = T.gap).padding(bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Mode("⤨", state.shuffle) { onShuffle(!state.shuffle) }
        Mode(
            // "one" gets its own glyph rather than a badge: at this size a
            // superscript 1 is a smudge.
            if (state.repeat == 2) "↺¹" else "↻",
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
                if (state.sleepMinutes > 0) "⏾ ${state.sleepMinutes} min" else "⏾",
                if (state.sleepMinutes > 0) T.accent else T.textMuted,
                T.fontSmall,
            )
        }
    }
}

@Composable
private fun Mode(glyph: String, on: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.size(T.touchTarget).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Label(glyph, if (on) T.accent else T.textMuted, T.fontBody)
    }
}

/**
 * What is waiting, when anything is.
 *
 * Swiping a row queues it, and without this the track vanishes into somewhere
 * the screen never mentions again — a feature you cannot see is a feature that
 * looks like a bug the first time it plays something you forgot about.
 */
@Composable
private fun QueueStrip(state: PlayerState, onClear: () -> Unit) {
    if (state.queued == 0) return
    Rule()
    Row(
        Modifier
            .fillMaxWidth()
            .background(T.bgTertiary)
            .heightIn(min = T.touchTarget)
            .padding(horizontal = T.gap),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label("≡", T.accent, T.fontBody)
        Spacer(Modifier.width(T.gap))
        Label(
            if (state.queued == 1) "Siguiente: ${state.queuedNext}"
            else "Siguiente: ${state.queuedNext}  (+${state.queued - 1})",
            T.textSecondary,
            T.fontSmall,
            maxLines = 1,
        )
        Spacer(Modifier.weight(1f))
        Box(
            Modifier
                .heightIn(min = T.touchTarget)
                .clickable(onClick = onClear)
                .padding(start = T.gap),
            contentAlignment = Alignment.Center,
        ) { Label("Vaciar", T.accent, T.fontSmall) }
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
private fun Progress(state: PlayerState) {
    val fraction = if (state.durationMs > 0) {
        (state.positionMs.toFloat() / state.durationMs).coerceIn(0f, 1f)
    } else {
        0f
    }
    Box(Modifier.fillMaxWidth().height(2.dp).background(T.bgTertiary)) {
        Box(
            Modifier
                .fillMaxWidth(fraction)
                .height(2.dp)
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
