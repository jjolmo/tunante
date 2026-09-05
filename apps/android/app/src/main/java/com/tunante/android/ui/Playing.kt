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
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.snapshotFlow
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
            // The cover and its three lines are a pager: the card follows the
            // finger 1:1 and the next (or previous) track's card slides in
            // beside it, so you see where you are going while you go. Release
            // past the middle and that card is the one that plays — exactly
            // what ▶▶ / ◀◀ would have given, because `prev`/`next` come from
            // the same queue walk. The seek bar and the transport stay below,
            // outside the pager: the bar has its own horizontal drag.
            val now = TrackCard(state.label, state.artist, state.album, state.path)
            // Pages are keyed by track path, so a neighbour that is the same
            // track as the current one (repeat-one, a two-track list wrapping)
            // would be a duplicate key — and there is nothing to page to there
            // anyway. Drop it rather than show the same card twice.
            val prev = state.prev?.takeIf { it.path != now.path }
            val next = state.next?.takeIf { it.path != now.path && it.path != prev?.path }
            val cards = listOfNotNull(prev, now, next)
            val current = if (prev != null) 1 else 0
            val pager = rememberPagerState(initialPage = current) { cards.size }
            // Whenever the track changes — by the pager, the buttons, or the
            // end of a song — re-centre on the new current card.
            LaunchedEffect(state.path, cards.size) {
                if (pager.currentPage != current) pager.scrollToPage(current)
            }
            // Settling on a neighbour is the gesture's verdict.
            LaunchedEffect(pager, current) {
                snapshotFlow { pager.settledPage }.collect { page ->
                    if (page > current) onNext() else if (page < current) onPrev()
                }
            }
            HorizontalPager(
                state = pager,
                modifier = Modifier.weight(1f).fillMaxWidth(),
                beyondViewportPageCount = 1,
                verticalAlignment = Alignment.CenterVertically,
                // Keyed by track, not by position. On release the next card
                // becomes the current one; with positional keys Compose threw
                // that card away and built a new one at index 1 — same cover,
                // loaded again, one blank frame: the flash on settle. Keyed,
                // the same card simply moves.
                key = { cards[it].path },
            ) { i ->
                val card = cards[i]
                Column(
                    Modifier.fillMaxWidth(),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Cover(card.path, side)
                    Spacer(Modifier.height(T.gap))
                    Label(card.title, T.textPrimary, T.fontTitle, maxLines = 2)
                    if (card.artist.isNotEmpty()) {
                        Label(card.artist, T.textSecondary, T.fontBody, maxLines = 1)
                    }
                    if (card.album.isNotEmpty()) {
                        Label(card.album, T.textMuted, T.fontSmall, maxLines = 1)
                    }
                }
            }

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
