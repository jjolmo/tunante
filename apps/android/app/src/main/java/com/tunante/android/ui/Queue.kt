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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import kotlinx.coroutines.delay
import androidx.compose.ui.unit.dp

/**
 * What is waiting, and the only screen where its order can be changed.
 *
 * The queue is a layer over the folder you were listening to: emptying it does
 * not stop the music, and taking one track out does not disturb the rest.
 */
@Composable
fun QueueScreen(
    tracks: List<Track>,
    nowPath: String = "",
    onRemove: (Track) -> Unit,
    onPlay: (Track) -> Unit,
    onMove: (Int, Int) -> Unit,
    onClear: () -> Unit,
) {
    Column(Modifier.fillMaxSize().background(T.bgPrimary)) {
        // Emptying the queue cannot be undone, so it is asked once. The
        // question times out rather than staying armed: a red bar left across
        // the top of the screen is a trap for the next thumb.
        var confirming by remember { mutableStateOf(false) }
        LaunchedEffect(confirming) {
            if (confirming) {
                delay(4000)
                confirming = false
            }
        }
        if (tracks.isNotEmpty()) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .background(if (confirming) T.destructive else T.bgSecondary)
                    .heightIn(min = T.touchTarget)
                    .clickable {
                        if (confirming) {
                            confirming = false
                            onClear()
                        } else {
                            confirming = true
                        }
                    }
                    .padding(horizontal = T.gap),
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label(
                    if (confirming) "✕" else "🗑",
                    if (confirming) Color.White else T.textSecondary,
                    T.fontBody,
                )
                Spacer(Modifier.width(8.dp))
                Label(
                    if (confirming) tr("¿Seguro? Toca otra vez")
                    else tr("Vaciar la cola ({})").replace("{}", "${tracks.size}"),
                    if (confirming) Color.White else T.textSecondary,
                    T.fontBody,
                    maxLines = 1,
                )
            }
            Rule()
        }

        if (tracks.isEmpty()) {
            EmptyNote(
                tr("No hay nada esperando"),
                tr("Desliza una pista en la biblioteca para ponerla en cola."),
            )
            return@Column
        }

        LazyColumn(Modifier.fillMaxSize()) {
            itemsIndexed(tracks) { i, track ->
                val sounding = track.path == nowPath
                // Swipe to take it out, the same gesture and the same red the
                // library rows and tabs.slint's queue use. The arrows stay:
                // they are the only reordering that works with one thumb.
                SwipeRow(
                    label = tr("Quitar"),
                    onSwiped = { onRemove(track) },
                    actionColor = T.destructive,
                    surfaceColor = if (sounding) T.bgSelected else T.bgPrimary,
                ) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = T.touchTarget + 8.dp)
                            .padding(start = T.gap, top = 4.dp, bottom = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        // The marker, not the index: which one is sounding is
                        // what you look for in a queue.
                        Box(Modifier.width(16.dp)) {
                            if (sounding) Label("▶", T.accent, T.fontSmall)
                        }
                        Spacer(Modifier.width(T.gap))
                        // Tapping jumps to it. The rest of the queue keeps its
                        // order — skipping to the third thing waiting should not
                        // throw away the first two.
                        Column(
                            Modifier
                                .weight(1f)
                                .heightIn(min = T.touchTarget)
                                .clickable { onPlay(track) },
                            verticalArrangement = Arrangement.Center,
                        ) {
                            Label(
                                track.title.ifEmpty { track.path.substringAfterLast('/') },
                                if (sounding) T.accent else T.textPrimary,
                                T.fontBody,
                                maxLines = 1,
                            )
                            if (track.artist.isNotEmpty()) {
                                Label(track.artist, T.textSecondary, T.fontSmall, maxLines = 1)
                            }
                        }
                        Arrow("↑", i > 0) { onMove(i, i - 1) }
                        Arrow("↓", i < tracks.lastIndex) { onMove(i, i + 1) }
                    }
                }
                Rule()
            }
        }
    }
}

@Composable
private fun Arrow(glyph: String, enabled: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.size(T.touchTarget).clickable(enabled = enabled, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        // Greyed rather than absent: a control that disappears at the ends of
        // the list moves every other control under the finger.
        Label(glyph, if (enabled) T.textPrimary else T.textMuted, T.fontBody)
    }
}
