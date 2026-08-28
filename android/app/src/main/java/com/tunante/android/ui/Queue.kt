package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
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
    onRemove: (Track) -> Unit,
    onPlay: (Track) -> Unit,
    onMove: (Int, Int) -> Unit,
    onClear: () -> Unit,
) {
    Column(Modifier.fillMaxSize().background(T.bgPrimary)) {
        Row(
            Modifier
                .fillMaxWidth()
                .background(T.bgSecondary)
                .heightIn(min = T.touchTarget)
                .padding(horizontal = T.gap),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Label("En cola", T.textPrimary, T.fontBody, maxLines = 1)
                Label(
                    if (tracks.size == 1) "1 pista" else "${tracks.size} pistas",
                    T.textSecondary,
                    T.fontSmall,
                )
            }
            if (tracks.isNotEmpty()) {
                Box(
                    Modifier
                        .heightIn(min = T.touchTarget)
                        .clickable(onClick = onClear)
                        .padding(horizontal = T.gap),
                    contentAlignment = Alignment.Center,
                ) { Label("Vaciar", T.accent, T.fontSmall) }
            }
        }
        Rule()

        if (tracks.isEmpty()) {
            EmptyNote(
                "No hay nada esperando",
                "Desliza una pista en la biblioteca para ponerla en cola.",
            )
            return@Column
        }

        LazyColumn(Modifier.fillMaxSize()) {
            itemsIndexed(tracks) { i, track ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = T.touchTarget)
                        .background(T.bgPrimary)
                        .padding(start = T.gap, top = 4.dp, bottom = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Label("${i + 1}", T.textMuted, T.fontSmall)
                    Spacer(Modifier.width(T.gap))
                    // Tapping jumps to it. The rest of the queue keeps its
                    // order — skipping to the third thing waiting should not
                    // throw away the first two.
                    Column(
                        Modifier
                            .weight(1f)
                            .heightIn(min = T.touchTarget)
                            .clickable { onPlay(track) },
                        verticalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
                    ) {
                        Label(
                            track.title.ifEmpty { track.path.substringAfterLast('/') },
                            T.textPrimary,
                            T.fontBody,
                            maxLines = 1,
                        )
                        if (track.artist.isNotEmpty()) {
                            Label(track.artist, T.textSecondary, T.fontSmall, maxLines = 1)
                        }
                    }
                    // Arrows rather than a drag: a drag inside a scrolling list
                    // needs a grab handle to be unambiguous, and a handle is the
                    // same width as these two put together. They are also the
                    // only reordering that works with one thumb.
                    Arrow("↑", i > 0) { onMove(i, i - 1) }
                    Arrow("↓", i < tracks.lastIndex) { onMove(i, i + 1) }
                    Arrow("✕", true) { onRemove(track) }
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
