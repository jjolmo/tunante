package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.draw.clip
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp

/**
 * The Playing screen.
 *
 * Cover, name, seek bar, and one row of controls — shuffle, the transport,
 * repeat — laid out exactly as tunante's Transport lays them out. Shuffle
 * and repeat sit *in* that row rather than under it: they are things you do to
 * playback, and putting them on their own line as labelled chips was what made
 * the player read as a settings form.
 *
 * Loops, fade and the sleep timer are not here at all. They are options, they
 * live in Ajustes, and mini has always had them there.
 */
@Composable
fun PlayingScreen(
    state: PlayerState,
    onTogglePlay: () -> Unit,
    onNext: () -> Unit,
    onPrev: () -> Unit,
    onShuffle: (Boolean) -> Unit,
    onRepeat: (Int) -> Unit,
    onSeek: (Long) -> Unit,
) {
    if (!state.hasSource) {
        Column(Modifier.fillMaxSize().background(T.bgPrimary)) {
            EmptyNote(tr("No suena nada"), tr("Elige algo en la biblioteca."))
        }
        return
    }

    BoxWithConstraints(Modifier.fillMaxSize().background(T.bgPrimary)) {
        // The cover takes what is left after everything else, capped so it does
        // not swallow a tall screen. Measured from the box rather than asked of
        // the image, which is what keeps this out of the layout's own sizing.
        val side = minOf(maxWidth - T.gap * 2, maxHeight * 0.45f)
        Column(
            Modifier.fillMaxSize().padding(T.gap),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(Modifier.weight(1f))
            Cover(state.path, side)
            Spacer(Modifier.height(T.gap))
            Label(state.label, T.textPrimary, T.fontTitle, maxLines = 2)
            if (state.artist.isNotEmpty()) {
                Label(state.artist, T.textSecondary, T.fontBody, maxLines = 1)
            }
            if (state.album.isNotEmpty()) {
                Label(state.album, T.textMuted, T.fontSmall, maxLines = 1)
            }
            Spacer(Modifier.weight(1f))

            Seek(state, onSeek)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Label(mmss(state.positionMs), T.textSecondary, T.fontSmall)
                Label(mmss(state.durationMs), T.textSecondary, T.fontSmall)
            }
            Spacer(Modifier.height(T.gap))

            // Five controls at five sizes, exactly as widgets.slint lays the
            // Transport out: the outer pair small, the skips larger, and play
            // as a filled disc twice their weight. The sizes are what make the
            // row read as a hierarchy rather than five equal buttons.
            Row(
                Modifier.fillMaxWidth().padding(horizontal = T.gap),
                horizontalArrangement = Arrangement.SpaceAround,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RoundButton(46.dp, state.shuffle, { onShuffle(!state.shuffle) }) {
                    Icon(IconKind.Shuffle, if (state.shuffle) T.accent else T.textPrimary)
                }
                RoundGlyph(52.dp, "◀◀", onClick = onPrev)
                PlayCircle(68.dp, state.playing, onTogglePlay)
                RoundGlyph(52.dp, "▶▶", onClick = onNext)
                // One icon carrying three states rather than three icons: off,
                // the loop, and the loop with a dot for "this one". tunante
                // spells the third `↻¹`, a character Android may not have.
                RoundButton(46.dp, state.repeat != 0, { onRepeat((state.repeat + 1) % 3) }) {
                    Icon(
                        if (state.repeat == 2) IconKind.RepeatOne else IconKind.Repeat,
                        if (state.repeat != 0) T.accent else T.textPrimary,
                    )
                }
            }
        }
    }
}

/** Drag or tap anywhere on the line to move. */
@Composable
private fun Seek(state: PlayerState, onSeek: (Long) -> Unit) {
    val total = state.durationMs.coerceAtLeast(1)
    val done = (state.positionMs.toFloat() / total).coerceIn(0f, 1f)
    BoxWithConstraints(
        Modifier
            .fillMaxWidth()
            .heightIn(min = T.touchTarget)
            .padding(horizontal = T.gap * 1.5f),
        contentAlignment = Alignment.CenterStart,
    ) {
        val w = constraints.maxWidth.toFloat().coerceAtLeast(1f)
        val track = maxWidth
        Box(
            Modifier
                .fillMaxWidth()
                .heightIn(min = T.touchTarget)
                .pointerInput(total) {
                    detectHorizontalDragGestures { change, _ ->
                        onSeek((change.position.x / w * total).toLong().coerceIn(0, total))
                    }
                },
            contentAlignment = Alignment.CenterStart,
        ) {
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(T.bgTertiary),
            ) {
                Box(
                    Modifier
                        .fillMaxWidth(done)
                        .height(4.dp)
                        .clip(RoundedCornerShape(2.dp))
                        .background(T.accent),
                )
            }
            // The knob. Without it the bar says where you are but not that you
            // may move it; Slint's SeekBar has carried one from the start.
            Box(
                Modifier
                    .offset(x = track * done - 6.5.dp)
                    .size(13.dp)
                    .clip(CircleShape)
                    .background(T.accent),
            )
        }
    }
}
