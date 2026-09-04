package com.tunante.android.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntOffset
import kotlin.math.abs
import kotlin.math.roundToInt

/**
 * A row you can swipe sideways to act on, with a label showing underneath.
 *
 * The same gesture `tunante` puts on its list rows: swipe a library track
 * to queue it, swipe one inside a playlist to take it out. Either direction
 * does the same thing — asking which way you swiped is a quiz, not an
 * interface.
 *
 * # Why the drag is claimed rather than shared
 *
 * The row sits inside a vertically scrolling list, so both want the same
 * finger. `detectHorizontalDragGestures` only wins once the movement is more
 * horizontal than vertical, which is the behaviour you want: a diagonal flick
 * while scrolling a long list should scroll, not silently queue a track.
 */
@Composable
fun SwipeRow(
    label: String,
    onSwiped: () -> Unit,
    content: @Composable () -> Unit,
) {
    var offset by remember { mutableFloatStateOf(0f) }
    var width by remember { mutableFloatStateOf(1f) }
    var fired by remember { mutableStateOf(false) }

    // A third of the row. Far enough that a stray thumb does not trigger it,
    // near enough that it does not need a full sweep across a 6" screen.
    val threshold = width / 3f

    val settled by animateFloatAsState(offset, label = "swipe")

    LaunchedEffect(fired) {
        if (fired) {
            onSwiped()
            fired = false
        }
    }

    Box(
        Modifier
            .fillMaxWidth()
            .clipToBounds()
            .onSizeChanged { width = it.width.toFloat().coerceAtLeast(1f) }
    ) {
        // The action showing through from underneath. Drawn only while there is
        // something to show: at rest it would sit behind every row of a long
        // list for nothing.
        if (abs(settled) > 1f) {
            Box(
                Modifier
                    .matchParentSize()
                    .background(T.bgSelected)
                    .padding(horizontal = T.gap),
                contentAlignment = if (settled > 0) Alignment.CenterStart else Alignment.CenterEnd,
            ) {
                Label(label, T.textPrimary, T.fontSmall, maxLines = 1)
            }
        }

        Box(
            Modifier
                .offset { IntOffset(settled.roundToInt(), 0) }
                .pointerInput(Unit) {
                    detectHorizontalDragGestures(
                        onDragEnd = {
                            if (abs(offset) > threshold) {
                                fired = true
                            }
                            offset = 0f
                        },
                        onDragCancel = { offset = 0f },
                    ) { change, delta ->
                        change.consume()
                        offset += delta
                    }
                }
        ) { content() }
    }
}
