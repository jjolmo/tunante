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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
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
    actionColor: Color = T.accent,
    surfaceColor: Color = T.bgPrimary,
    enabled: Boolean = true,
    content: @Composable () -> Unit,
) {
    var offset by remember { mutableFloatStateOf(0f) }
    var width by remember { mutableFloatStateOf(1f) }
    var fired by remember { mutableStateOf(false) }

    // A fixed distance, as in `widgets.slint`, not a fraction of the row: the
    // gesture should feel the same on a phone and on a tablet, and a third of
    // a wide row is a sweep nobody finishes.
    val thresholdDp = 90.dp
    val threshold = with(LocalDensity.current) { thresholdDp.toPx() }

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
            // Fading in with the distance travelled tells you the swipe has
            // registered before it has fired, which is what stops a half
            // gesture feeling like a dead one.
            Box(
                Modifier
                    .matchParentSize()
                    .alpha((abs(settled) / threshold).coerceIn(0f, 1f))
                    .background(actionColor)
                    .padding(horizontal = T.gap * 2),
                contentAlignment = if (settled > 0) Alignment.CenterStart else Alignment.CenterEnd,
            ) {
                Label(label, Color.White, T.fontBody, maxLines = 1)
            }
        }

        Box(
            Modifier
                .offset { IntOffset(settled.roundToInt(), 0) }
                .background(surfaceColor)
                .pointerInput(enabled) {
                    if (!enabled) return@pointerInput
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
