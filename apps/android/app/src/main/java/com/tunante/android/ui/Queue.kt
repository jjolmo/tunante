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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

/**
 * One row of the «Lista»: a track, whether it was prioritised by hand, whether
 * it is the last prioritised one (the rule under it says "the playlist resumes
 * here"), and whether it is the one sounding.
 */
data class ListRow(val track: Track, val queued: Boolean, val divider: Boolean, val now: Boolean)

/**
 * The playlist in real playback order: everything up to and including the
 * current track, then what was queued by hand, then the rest of the playlist.
 * The current row keeps its playlist index, so centring on it needs no maths.
 * Mirrors `push_now_playing` in apps/tunante/src/main.rs row for row.
 */
fun mergedRows(list: List<Track>, index: Int, queue: List<Track>): List<ListRow> {
    val split = (index + 1).coerceIn(0, list.size)
    val rows = ArrayList<ListRow>(list.size + queue.size)
    list.subList(0, split).forEachIndexed { i, t -> rows.add(ListRow(t, false, false, i == index)) }
    queue.forEachIndexed { k, t -> rows.add(ListRow(t, true, k == queue.lastIndex, false)) }
    list.subList(split, list.size).forEach { rows.add(ListRow(it, false, false, false)) }
    return rows
}

/**
 * The «Lista» screen: the tab that used to show the queue alone.
 *
 * What plays next is the queue first and the playlist after, so that is what
 * the screen shows, in that order, with a rule where the queue ends. Tapping a
 * row plays it. Swiping toggles priority: a playlist row goes into the queue,
 * a queued row comes out. The arrows reorder only the queued block — the
 * playlist's order is the folder's, not ours. The same list is the right
 * column of Playing in landscape, as it is in the compact shell.
 */
@Composable
fun ListScreen(
    list: List<Track>,
    index: Int,
    queue: List<Track>,
    onPlay: (Int) -> Unit,
    onToggle: (Int) -> Unit,
    onMove: (Int, Int) -> Unit,
    onClear: () -> Unit,
) {
    Column(Modifier.fillMaxSize().background(T.bgPrimary)) {
        NowList(list, index, queue, onPlay, onToggle, onMove, onClear, Modifier.fillMaxSize())
    }
}

@Composable
fun NowList(
    list: List<Track>,
    index: Int,
    queue: List<Track>,
    onPlay: (Int) -> Unit,
    onToggle: (Int) -> Unit,
    onMove: (Int, Int) -> Unit,
    onClear: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier.background(T.bgPrimary)) {
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
        if (queue.isNotEmpty()) {
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
                    else tr("Vaciar la cola ({})").replace("{}", "${queue.size}"),
                    if (confirming) Color.White else T.textSecondary,
                    T.fontBody,
                    maxLines = 1,
                )
            }
            Rule()
        }

        if (list.isEmpty() && queue.isEmpty()) {
            EmptyNote(tr("No suena nada"), tr("Elige algo en la biblioteca."))
            return@Column
        }

        val rows = mergedRows(list, index, queue)
        val cur = (index + 1).coerceIn(0, list.size)
        val listState = rememberLazyListState()
        // Centre on the sounding row whenever it moves — the compact shell's
        // `qcol.recenter()`. Measured from what is visible now; before the
        // first layout the estimate is a handful of rows.
        LaunchedEffect(index, queue.size, rows.size) {
            if (index < 0 || index >= rows.size) return@LaunchedEffect
            val visible = listState.layoutInfo.visibleItemsInfo.size.takeIf { it > 0 } ?: 6
            listState.animateScrollToItem((index - visible / 2).coerceAtLeast(0))
        }

        LazyColumn(Modifier.fillMaxSize(), state = listState) {
            itemsIndexed(rows, key = { i, r -> "$i:${r.track.path}" }) { i, row ->
                val track = row.track
                SwipeRow(
                    label = if (row.queued) tr("Quitar de la cola") else tr("A la cola"),
                    onSwiped = { onToggle(i) },
                    actionColor = if (row.queued) T.destructive else T.accent,
                    surfaceColor = if (row.now) T.bgSelected else T.bgPrimary,
                ) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = T.touchTarget + 8.dp)
                            .padding(start = T.gap, top = 4.dp, bottom = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        // ▶ on the one sounding, » on what was prioritised.
                        Box(Modifier.width(16.dp)) {
                            if (row.now) Label("▶", T.accent, T.fontSmall)
                            else if (row.queued) Label("»", T.textMuted, T.fontSmall)
                        }
                        Spacer(Modifier.width(T.gap))
                        Column(
                            Modifier
                                .weight(1f)
                                .heightIn(min = T.touchTarget)
                                .clickable { onPlay(i) },
                            verticalArrangement = Arrangement.Center,
                        ) {
                            Label(
                                track.title.ifEmpty { track.path.substringAfterLast('/') },
                                if (row.now) T.accent else T.textPrimary,
                                T.fontBody,
                                maxLines = 1,
                            )
                            val sub = track.artist.ifEmpty { track.album }
                            if (sub.isNotEmpty()) {
                                Label(sub, T.textSecondary, T.fontSmall, maxLines = 1)
                            }
                        }
                        if (row.queued) {
                            val q = i - cur
                            Arrow("↑", q > 0) { onMove(q, q - 1) }
                            Arrow("↓", q < queue.lastIndex) { onMove(q, q + 1) }
                        } else {
                            Spacer(Modifier.width(T.touchTarget * 2))
                        }
                    }
                }
                // The rule where the queue ends and the playlist resumes.
                if (row.divider) {
                    Box(Modifier.fillMaxWidth().height(2.dp).background(T.accent.copy(alpha = 0.6f)))
                } else {
                    Rule()
                }
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
