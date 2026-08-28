package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.dp

/**
 * Where the app can be.
 *
 * The same four tunante-mini has, in the same order. This is the structure of
 * the app and not a menu: the library is one of four places, not the app with
 * three extras bolted to it, and the previous shape here — a library screen
 * with a title bar, a Scan button and two rows of chips under the player — was
 * what you get by adding each piece where there happened to be room.
 */
enum class Dest(val label: String, val icon: IconKind) {
    Playing("Sonando", IconKind.Playing),
    Queue("Cola", IconKind.Queue),
    Library("Biblioteca", IconKind.Library),
    Settings("Ajustes", IconKind.Settings),
}

@Composable
fun BottomNav(current: Dest, queued: Int, onGo: (Dest) -> Unit) {
    Rule()
    Row(
        Modifier.fillMaxWidth().background(T.bgSecondary).padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (d in Dest.entries) {
            val on = d == current
            Column(
                Modifier
                    .weight(1f)
                    .heightIn(min = 56.dp)
                    .clickable { onGo(d) },
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Box(contentAlignment = Alignment.TopEnd) {
                    Icon(d.icon, if (on) T.accent else T.textMuted)
                    // How many are waiting, where it is legible without opening
                    // the queue. mini has no badge, but mini is also never the
                    // app you left in a pocket.
                    if (d == Dest.Queue && queued > 0) {
                        Label("$queued", T.accent, T.fontSmall)
                    }
                }
                Spacer(Modifier.height(2.dp))
                Label(d.label, if (on) T.accent else T.textMuted, T.fontSmall, maxLines = 1)
            }
        }
    }
}

/**
 * The bar that says what is playing, under every destination.
 *
 * Cover, name, and the three buttons a thumb reaches for. Shuffle, repeat and
 * the seek bar are not here: they are on the Playing screen, one tap away, and
 * this bar is what you glance at rather than what you operate.
 */
@Composable
fun MiniPlayer(
    state: PlayerState,
    onOpen: () -> Unit,
    onTogglePlay: () -> Unit,
    onNext: () -> Unit,
    onPrev: () -> Unit,
) {
    // Out of the way when there is no room for it, which is mini's rule
    // (`height < 300px`) in Android's units. In landscape the tabs, the search
    // box, the count strip, this bar and the four destinations leave the
    // library itself a single row of covers. The bar is the one of those that
    // repeats something already reachable: the destinations below it include
    // Sonando.
    if (LocalConfiguration.current.screenHeightDp < 420) return

    Rule()
    // Progress along the top edge, a hairline rather than a slider: this is
    // where you glance at how far in you are from any destination, and mini
    // has had it since it had a mini-player. Dragging it belongs on the seek
    // bar of the Playing screen, which is one tap away.
    val done = if (state.durationMs > 0) {
        (state.positionMs.toFloat() / state.durationMs).coerceIn(0f, 1f)
    } else {
        0f
    }
    Box(Modifier.fillMaxWidth().height(2.dp).background(T.bgTertiary)) {
        Box(Modifier.fillMaxWidth(done).height(2.dp).background(T.accent))
    }
    Row(
        Modifier
            .fillMaxWidth()
            .background(T.bgSecondary)
            .clickable(onClick = onOpen)
            .padding(horizontal = T.gap, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Cover(state.path, 40.dp)
        Spacer(Modifier.width(T.gap))
        Column(Modifier.weight(1f)) {
            Label(
                if (state.hasSource) state.label else "Nada sonando",
                if (state.hasSource) T.textPrimary else T.textMuted,
                T.fontBody,
                maxLines = 1,
            )
            if (state.hasSource) {
                Label(
                    "${mmss(state.positionMs)} / ${mmss(state.durationMs)}" +
                        if (state.artist.isNotEmpty()) "  ·  ${state.artist}" else "",
                    T.textSecondary,
                    T.fontSmall,
                    maxLines = 1,
                )
            }
        }
        TransportButton("◀◀", onPrev)
        TransportButton(if (state.playing) "❚❚" else "▶", onTogglePlay)
        TransportButton("▶▶", onNext)
    }
}

/**
 * The transport glyphs stay characters.
 *
 * Unlike the shuffle and repeat arrows, these four are in every font on every
 * Android there is — they were never the ones that came out as empty boxes.
 */
@Composable
fun TransportButton(glyph: String, onClick: () -> Unit) {
    Box(
        Modifier.width(T.touchTarget).heightIn(min = T.touchTarget).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { Label(glyph, T.textPrimary, T.fontTitle) }
}
