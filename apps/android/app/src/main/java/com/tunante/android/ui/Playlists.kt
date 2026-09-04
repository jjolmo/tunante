package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp

/** A playlist, as `nativePlaylists` hands it over. */
data class Playlist(val id: String, val name: String, val trackCount: Int)

/** Which tab is showing. The same four mini has. */
enum class Tab(val label: String) {
    /** Mirrors the disk. Honest, and it makes you walk down to a game whose
     *  name you already know — which is what the next two are for. */
    Library("Árbol"),
    Albums("Discos"),
    /** By the album tag rather than by folder: what the rip says it is from. */
    Games("Juegos"),
    Consoles("Consolas"),
    Playlists("Listas"),
}

/**
 * The playlists tab: the list of them, or the inside of one.
 *
 * `open` being null means "showing the list". Kept as one screen rather than two
 * because the transition is the whole interaction.
 */
@Composable
fun PlaylistsTab(
    playlists: List<Playlist>,
    open: Playlist?,
    tracks: List<Track>,
    state: PlayerState,
    onOpen: (Playlist) -> Unit,
    onBack: () -> Unit,
    onCreate: (String) -> Unit,
    onDelete: (Playlist) -> Unit,
    onPlayIndex: (Int) -> Unit,
    onRemove: (Playlist, Track) -> Unit,
    onRename: (Playlist, String) -> Unit,
    onMove: (Int, Int) -> Unit,
    onEnqueueAll: (Playlist) -> Unit,
    onEnqueueOne: (Track) -> Unit,
) {
    Column(Modifier.fillMaxSize()) {
        if (open == null) {
            NewPlaylist(onCreate)
            if (playlists.isEmpty()) {
                EmptyNote(tr("Ninguna lista todavía"), tr("Créala arriba y añade pistas desde la biblioteca."))
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    itemsIndexed(playlists) { i, p ->
                        PlaylistRow(
                            playlist = p,
                            canUp = i > 0,
                            canDown = i < playlists.lastIndex,
                            onOpen = { onOpen(p) },
                            onDelete = { onDelete(p) },
                            onRename = { name -> onRename(p, name) },
                            onUp = { onMove(i, i - 1) },
                            onDown = { onMove(i, i + 1) },
                        )
                    }
                }
            }
        } else {
            Row(
                Modifier
                    .fillMaxWidth()
                    .background(T.bgTertiary)
                    .heightIn(min = T.touchTarget)
                    .clickable(onClick = onBack)
                    .padding(horizontal = T.gap),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label("◂", T.accent, T.fontTitle)
                Spacer(Modifier.width(T.gap))
                Label(open.name, T.textPrimary, T.fontBody, maxLines = 1)
                Spacer(Modifier.weight(1f))
                if (tracks.isNotEmpty()) {
                    Box(
                        Modifier
                            .heightIn(min = T.touchTarget)
                            .clickable { onEnqueueAll(open) }
                            .padding(horizontal = T.gap),
                        contentAlignment = Alignment.Center,
                    ) { Label(tr("A la cola"), T.accent, T.fontSmall, maxLines = 1) }
                }
            }
            Rule()
            if (tracks.isEmpty()) {
                EmptyNote(tr("Lista vacía"), tr("Añade pistas con ＋ desde la biblioteca."))
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    itemsIndexed(tracks) { i, t ->
                        SwipeRow(tr("Quitar de la lista"), { onRemove(open, t) }) {
                            TrackRow(
                                track = t,
                                selected = state.hasSource && t.path == state.path,
                                onClick = { onPlayIndex(i) },
                                // The same verb the library gives a long press
                                // would be "add to playlist", which makes no
                                // sense inside one. Queueing is what is left.
                                onLongClick = { onEnqueueOne(t) },
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun NewPlaylist(onCreate: (String) -> Unit) {
    var name by remember { mutableStateOf("") }
    Row(
        Modifier
            .fillMaxWidth()
            .background(T.bgSecondary)
            .padding(horizontal = T.gap, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.weight(1f).heightIn(min = T.touchTarget), Alignment.CenterStart) {
            if (name.isEmpty()) {
                Label(tr("Nueva lista…"), T.textMuted, T.fontBody)
            }
            BasicTextField(
                value = name,
                onValueChange = { name = it },
                singleLine = true,
                textStyle = TextStyle(color = T.textPrimary, fontSize = T.fontBody),
                cursorBrush = SolidColor(T.accent),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Box(
            Modifier
                .heightIn(min = T.touchTarget)
                .clickable(enabled = name.isNotBlank()) {
                    onCreate(name.trim())
                    name = ""
                }
                .padding(horizontal = T.gap),
            contentAlignment = Alignment.Center,
        ) {
            // Greyed rather than hidden: a control that appears as you type
            // moves the row under the finger.
            Label(tr("Crear"), if (name.isBlank()) T.textMuted else T.accent, T.fontBody)
        }
    }
    Rule()
}

@Composable
private fun PlaylistRow(
    playlist: Playlist,
    canUp: Boolean,
    canDown: Boolean,
    onOpen: () -> Unit,
    onDelete: () -> Unit,
    onRename: (String) -> Unit,
    onUp: () -> Unit,
    onDown: () -> Unit,
) {
    var confirming by remember(playlist.id) { mutableStateOf(false) }
    var renaming by remember(playlist.id) { mutableStateOf<String?>(null) }

    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = T.touchTarget)
            .background(T.bgPrimary)
            .padding(start = T.gap, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val editing = renaming
        if (editing == null) {
            Column(
                Modifier.weight(1f).heightIn(min = T.touchTarget).clickable(onClick = onOpen),
                verticalArrangement = Arrangement.Center,
            ) {
                Label(playlist.name, T.textPrimary, T.fontBody, maxLines = 1)
                Label(pistas(playlist.trackCount), T.textSecondary, T.fontSmall)
            }
            // Renaming is behind its own control rather than a long press: a
            // long press on this row already means something in the library and
            // two different long presses is a quiz.
            Box(
                Modifier
                    .heightIn(min = T.touchTarget)
                    .clickable { renaming = playlist.name }
                    .padding(horizontal = 8.dp),
                contentAlignment = Alignment.Center,
            ) { Label(tr("Renombrar"), T.textMuted, T.fontSmall, maxLines = 1) }
            Box(Modifier.size(T.touchTarget).clickable(enabled = canUp, onClick = onUp),
                contentAlignment = Alignment.Center) {
                Label("↑", if (canUp) T.textPrimary else T.textMuted, T.fontBody)
            }
            Box(Modifier.size(T.touchTarget).clickable(enabled = canDown, onClick = onDown),
                contentAlignment = Alignment.Center) {
                Label("↓", if (canDown) T.textPrimary else T.textMuted, T.fontBody)
            }
            // Two taps to delete, and the second one says so. There is no undo
            // behind this, and a playlist is the one thing here the user made.
            Box(
                Modifier
                    .heightIn(min = T.touchTarget)
                    .clickable { if (confirming) onDelete() else confirming = true }
                    .padding(horizontal = 8.dp),
                contentAlignment = Alignment.Center,
            ) {
                Label(
                    if (confirming) tr("¿Seguro?") else "✕",
                    if (confirming) T.warningFg else T.textMuted,
                    if (confirming) T.fontSmall else T.fontTitle,
                )
            }
        } else {
            BasicTextField(
                value = editing,
                onValueChange = { renaming = it },
                singleLine = true,
                textStyle = TextStyle(color = T.textPrimary, fontSize = T.fontBody),
                cursorBrush = SolidColor(T.accent),
                modifier = Modifier.weight(1f),
            )
            Box(
                Modifier
                    .heightIn(min = T.touchTarget)
                    .clickable {
                        if (editing.isNotBlank()) onRename(editing.trim())
                        renaming = null
                    }
                    .padding(horizontal = T.gap),
                contentAlignment = Alignment.Center,
            ) { Label(tr("Guardar"), T.accent, T.fontSmall) }
            Box(
                Modifier
                    .heightIn(min = T.touchTarget)
                    .clickable { renaming = null }
                    .padding(horizontal = T.gap),
                contentAlignment = Alignment.Center,
            ) { Label(tr("Cancelar"), T.textMuted, T.fontSmall) }
        }
    }
    Rule()
}

@Composable
fun EmptyNote(title: String, detail: String) {
    Column(
        Modifier.fillMaxSize().padding(T.gap * 2),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Label(title, T.textSecondary, T.fontBody)
        Spacer(Modifier.height(T.gap))
        Label(detail, T.textMuted, T.fontSmall)
    }
}

/** The two-tab strip. */
@Composable
fun Tabs(current: Tab, onPick: (Tab) -> Unit) {
    Row(Modifier.fillMaxWidth().background(T.bgSecondary)) {
        for (tab in Tab.entries) {
            val chosen = tab == current
            Box(
                Modifier
                    .weight(1f)
                    .heightIn(min = T.touchTarget)
                    .background(if (chosen) T.bgTertiary else T.bgSecondary)
                    .clickable { onPick(tab) },
                contentAlignment = Alignment.Center,
            ) {
                Label(
                    tr(tab.label),
                    if (chosen) T.textPrimary else T.textSecondary,
                    // Four labels where there were two: the body size no longer
                    // fits "Consolas" on a narrow phone without truncating.
                    T.fontSmall,
                    maxLines = 1,
                )
            }
        }
    }
    // The underline is what says which one is live; the background alone is too
    // subtle at this palette's contrast.
    Row(Modifier.fillMaxWidth()) {
        for (tab in Tab.entries) {
            Box(
                Modifier
                    .weight(1f)
                    .height(2.dp)
                    .background(if (tab == current) T.accent else T.border)
            )
        }
    }
}
