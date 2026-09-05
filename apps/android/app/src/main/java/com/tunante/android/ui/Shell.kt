package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * Where the app can be.
 *
 * The same four tunante has, in the same order. This is the structure of
 * the app and not a menu: the library is one of four places, not the app with
 * three extras bolted to it, and the previous shape here — a library screen
 * with a title bar, a Scan button and two rows of chips under the player — was
 * what you get by adding each piece where there happened to be room.
 */
enum class Dest(val label: String, val icon: IconKind) {
    Playing("Sonando", IconKind.Playing),
    Queue("Lista", IconKind.Queue),
    Library("Biblioteca", IconKind.Library),
    Settings("Ajustes", IconKind.Settings),
}

@Composable
fun BottomNav(current: Dest, queued: Int, onGo: (Dest) -> Unit, vertical: Boolean = false) {
    // In landscape the four destinations stand in a rail down the left edge,
    // as the compact shell's TabSwitcher does with `vertical: true`; the bar
    // along the bottom is the portrait shape.
    if (vertical) {
        Row(Modifier.fillMaxHeight()) {
            Column(
                Modifier.fillMaxHeight().width(76.dp).background(T.bgSecondary).padding(vertical = 4.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                for (d in Dest.entries) NavItem(d, d == current, queued, Modifier.fillMaxWidth(), onGo)
            }
            Box(Modifier.fillMaxHeight().width(1.dp).background(T.border))
        }
        return
    }
    Rule()
    Row(
        Modifier.fillMaxWidth().background(T.bgSecondary).padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (d in Dest.entries) NavItem(d, d == current, queued, Modifier.weight(1f), onGo)
    }
}

@Composable
private fun NavItem(d: Dest, on: Boolean, queued: Int, modifier: Modifier, onGo: (Dest) -> Unit) {
    Column(
        modifier
            .padding(horizontal = 4.dp, vertical = 2.dp)
            .clip(RoundedCornerShape(T.radius))
            .background(if (on) T.bgSelected else Color.Transparent)
            .heightIn(min = 56.dp)
            .clickable { onGo(d) },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Box(contentAlignment = Alignment.TopEnd) {
            Icon(d.icon, if (on) T.textPrimary else T.textSecondary)
            // How many are waiting, where it is legible without opening the
            // list. mini has no badge, but mini is also never the app you left
            // in a pocket.
            if (d == Dest.Queue && queued > 0) {
                Label("$queued", T.accent, T.fontSmall)
            }
        }
        Spacer(Modifier.height(2.dp))
        Label(
            tr(d.label),
            if (on) T.textPrimary else T.textSecondary,
            T.fontSmall,
            maxLines = 1,
        )
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
            .heightIn(min = 66.dp)
            .padding(horizontal = T.gap),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Cover(state.path, 44.dp)
        Spacer(Modifier.width(T.gap))
        Column(Modifier.weight(1f)) {
            Label(
                if (state.hasSource) state.label else tr("Nada sonando"),
                if (state.hasSource) T.textPrimary else T.textMuted,
                T.fontBody,
                maxLines = 1,
            )
            if (state.hasSource && state.artist.isNotEmpty()) {
                Label(state.artist, T.textSecondary, T.fontSmall, maxLines = 1)
            }
        }
        StripButton("◀◀", T.textSecondary, 14.sp, onPrev)
        StripPlayPause(state.playing, onTogglePlay)
        StripButton("▶▶", T.textSecondary, 14.sp, onNext)
    }
}

/**
 * A glyph in the playing strip.
 *
 * The transport characters stay characters: unlike the shuffle and repeat
 * arrows, these are in every font on every Android there is — they were never
 * the ones that came out as empty boxes.
 */
@Composable
fun StripButton(
    glyph: String,
    color: Color,
    size: androidx.compose.ui.unit.TextUnit,
    onClick: () -> Unit,
) {
    Box(
        Modifier.width(T.touchTarget).heightIn(min = T.touchTarget).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { Label(glyph, color, size) }
}

/**
 * Play and pause in the strip: a glyph, not the filled disc.
 *
 * The disc belongs to the Playing screen, where it is the one thing a thumb
 * aims at. Here it would shout over the cover and the title, which are what
 * this bar is for. Pause is drawn rather than typed for the usual reason —
 * U+23F8 is not a character a phone can be relied on to have.
 */
@Composable
fun StripPlayPause(playing: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.width(T.touchTarget).heightIn(min = T.touchTarget).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        if (playing) {
            Row(horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                repeat(2) {
                    Box(Modifier.width(5.dp).height(20.dp).background(T.textPrimary))
                }
            }
        } else {
            Label("▶", T.textPrimary, 22.sp)
        }
    }
}

/**
 * The round control, transcribed from `RoundButton` in `tunante/ui/widgets.slint`.
 *
 * A circle that is invisible until it means something: transparent at rest,
 * [T.bgHover] while a finger is down, [T.bgSelected] when the thing it toggles
 * is on. That last state is the whole point — shuffle and repeat have to say
 * whether they are on from across a room, and a colour change on a bare glyph
 * did not.
 *
 * `size` is the circle; the content is centred in it and sized by the caller,
 * the same 0.38 ratio the Slint version uses for its glyph.
 */
@Composable
fun RoundButton(
    size: Dp,
    active: Boolean = false,
    onClick: () -> Unit,
    content: @Composable () -> Unit,
) {
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    Box(
        Modifier
            .size(size)
            .clip(CircleShape)
            .background(
                when {
                    active -> T.bgSelected
                    pressed -> T.bgHover
                    else -> Color.Transparent
                }
            )
            .clickable(interactionSource = interaction, indication = null, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { content() }
}

/** [RoundButton] carrying one of the transport glyphs. */
@Composable
fun RoundGlyph(size: Dp, glyph: String, active: Boolean = false, onClick: () -> Unit) =
    RoundButton(size, active, onClick) {
        Label(glyph, if (active) T.accent else T.textPrimary, (size.value * 0.38f).sp)
    }

/**
 * Play and pause, as the one control that is filled rather than outlined.
 *
 * The accent disc is what tells a thumb where to land without reading anything,
 * and it is the shape the Slint transport has always had. Play is the `▶`
 * glyph, which every Android has; pause is two drawn bars, because U+23F8 is
 * exactly the kind of character that came out as an empty box on a real phone.
 */
@Composable
fun PlayCircle(size: Dp, playing: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .size(size)
            .clip(CircleShape)
            .background(T.accent)
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        if (playing) {
            Row(horizontalArrangement = Arrangement.spacedBy(size * 0.09f)) {
                repeat(2) {
                    Box(
                        Modifier
                            .width(size * 0.088f)
                            .height(size * 0.353f)
                            .background(Color.White)
                    )
                }
            }
        } else {
            Label("▶", Color.White, (size.value * 0.38f).sp)
        }
    }
}
