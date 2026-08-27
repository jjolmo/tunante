package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
    Albums("Álbumes"),
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
) {
    Column(Modifier.fillMaxSize()) {
        if (open == null) {
            NewPlaylist(onCreate)
            if (playlists.isEmpty()) {
                EmptyNote("Ninguna lista todavía", "Créala arriba y añade pistas desde la biblioteca.")
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    items(playlists) { p -> PlaylistRow(p, { onOpen(p) }, { onDelete(p) }) }
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
            }
            Rule()
            if (tracks.isEmpty()) {
                EmptyNote("Lista vacía", "Añade pistas con ＋ desde la biblioteca.")
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    itemsIndexed(tracks) { i, t ->
                        SwipeRow("Quitar de la lista", { onRemove(open, t) }) {
                            TrackRow(
                                track = t,
                                selected = state.hasSource && t.path == state.path,
                                onClick = { onPlayIndex(i) },
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
                Label("Nueva lista…", T.textMuted, T.fontBody)
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
            Label("Crear", if (name.isBlank()) T.textMuted else T.accent, T.fontBody)
        }
    }
    Rule()
}

@Composable
private fun PlaylistRow(playlist: Playlist, onOpen: () -> Unit, onDelete: () -> Unit) {
    var confirming by remember(playlist.id) { mutableStateOf(false) }
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = T.touchTarget)
            .background(T.bgPrimary)
            .clickable(onClick = onOpen)
            .padding(horizontal = T.gap, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Label(playlist.name, T.textPrimary, T.fontBody, maxLines = 1)
            Label("${playlist.trackCount} pistas", T.textSecondary, T.fontSmall)
        }
        // Two taps to delete, and the second one says so. There is no undo
        // behind this, and a playlist is the one thing here the user made.
        Box(
            Modifier
                .heightIn(min = T.touchTarget)
                .clickable { if (confirming) onDelete() else confirming = true }
                .padding(horizontal = T.gap),
            contentAlignment = Alignment.Center,
        ) {
            Label(
                if (confirming) "¿Seguro?" else "✕",
                if (confirming) T.warningFg else T.textMuted,
                if (confirming) T.fontSmall else T.fontTitle,
            )
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
                    tab.label,
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
