package com.tunante.android.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.dp

/**
 * The icons, drawn rather than typed.
 *
 * tunante writes these as glyphs — `⤨` for shuffle, `↻` for repeat, `☾`
 * for the sleep timer — and it can, because its package depends on DejaVu and
 * so the characters are always there. Android gives no such promise: the moon
 * came out as an empty box on a real phone, and the fix at the time was to
 * replace all of them with words. Words are what made the player look like a
 * form instead of a player.
 *
 * A path has no font to be missing from. It also lets one icon carry a state —
 * repeat-one is the repeat loop with a dot in it — which no single character
 * does at this size.
 *
 * Every one is drawn inside a 24×24 box and scaled, so they line up with each
 * other whatever size they are asked for.
 */
private val ICON = 24f

@Composable
fun Icon(
    kind: IconKind,
    color: Color,
    modifier: Modifier = Modifier,
    size: androidx.compose.ui.unit.Dp = 22.dp,
) {
    Canvas(modifier.size(size)) {
        val s = this.size.minDimension / ICON
        // Thin enough to read as a line drawing next to 15sp text, thick enough
        // not to disappear against the dark background.
        val w = 2f * s
        when (kind) {
            IconKind.Shuffle -> shuffle(color, s, w)
            IconKind.Repeat -> repeat(color, s, w, dot = false)
            IconKind.RepeatOne -> repeat(color, s, w, dot = true)
            IconKind.Playing -> playing(color, s)
            IconKind.Queue -> queue(color, s, w)
            IconKind.Library -> library(color, s, w)
            IconKind.Settings -> settings(color, s, w)
        }
    }
}

enum class IconKind { Shuffle, Repeat, RepeatOne, Playing, Queue, Library, Settings }

/**
 * An arrowhead, filled.
 *
 * Stroked as an open `V` these were blobs at 22 dp: two 2 px strokes meeting at
 * a point merge into one at that size, and the repeat arrow read as a notch in
 * a circle. A filled triangle has no join to close up.
 */
private fun DrawScope.head(
    c: Color,
    s: Float,
    tipX: Float,
    tipY: Float,
    dirDeg: Float,
    len: Float = 4f,
    half: Float = 2.6f,
) {
    val a = Math.toRadians(dirDeg.toDouble())
    val dx = Math.cos(a).toFloat()
    val dy = Math.sin(a).toFloat()
    val bx = tipX - len * dx
    val by = tipY - len * dy
    val p = Path().apply {
        moveTo(tipX * s, tipY * s)
        lineTo((bx - half * dy) * s, (by + half * dx) * s)
        lineTo((bx + half * dy) * s, (by - half * dx) * s)
        close()
    }
    drawPath(p, c)
}

/** Two arrows that cross and both come out pointing right. */
private fun DrawScope.shuffle(c: Color, s: Float, w: Float) {
    val stroke = Stroke(width = w, cap = androidx.compose.ui.graphics.StrokeCap.Round)
    val p = Path().apply {
        moveTo(3f * s, 7f * s); lineTo(7f * s, 7f * s)
        lineTo(15f * s, 17f * s); lineTo(18f * s, 17f * s)
        moveTo(3f * s, 17f * s); lineTo(7f * s, 17f * s)
        lineTo(15f * s, 7f * s); lineTo(18f * s, 7f * s)
    }
    drawPath(p, c, style = stroke)
    head(c, s, 21f, 7f, 0f)
    head(c, s, 21f, 17f, 0f)
}

/** A circle that comes back around, with a dot in it for "just this one". */
private fun DrawScope.repeat(c: Color, s: Float, w: Float, dot: Boolean) {
    val stroke = Stroke(width = w, cap = androidx.compose.ui.graphics.StrokeCap.Round)
    // The gap is at the top, where the arrowhead goes. A closed ring reads as a
    // progress spinner rather than as something that comes round again.
    drawArc(
        color = c,
        startAngle = 290f,
        sweepAngle = 300f,
        useCenter = false,
        topLeft = Offset(4f * s, 4f * s),
        size = Size(16f * s, 16f * s),
        style = stroke,
    )
    // In the gap rather than on top of the stroke, which is where the first
    // attempt put it: a triangle drawn over the end of a 2 px arc is a pinch in
    // the line, not an arrow. Tip and direction are the arc's own tangent
    // carried a little further round.
    head(c, s, 9.9f, 4.3f, -20f, len = 4.6f, half = 3f)
    if (dot) drawCircle(c, radius = 2.2f * s, center = Offset(12f * s, 12f * s))
}

/** A filled triangle: what is playing. */
private fun DrawScope.playing(c: Color, s: Float) {
    val p = Path().apply {
        moveTo(7f * s, 4f * s); lineTo(20f * s, 12f * s); lineTo(7f * s, 20f * s); close()
    }
    drawPath(p, c)
}

/** Stacked lines: what is waiting. */
private fun DrawScope.queue(c: Color, s: Float, w: Float) {
    for (y in listOf(6f, 12f, 18f)) {
        drawLine(
            c, Offset(4f * s, y * s), Offset(20f * s, y * s),
            strokeWidth = w, cap = androidx.compose.ui.graphics.StrokeCap.Round,
        )
    }
}

/** A folder: the library. */
private fun DrawScope.library(c: Color, s: Float, w: Float) {
    val stroke = Stroke(
        width = w,
        cap = androidx.compose.ui.graphics.StrokeCap.Round,
        join = androidx.compose.ui.graphics.StrokeJoin.Round,
    )
    val p = Path().apply {
        moveTo(3f * s, 19f * s)
        lineTo(3f * s, 6f * s)
        lineTo(10f * s, 6f * s)
        lineTo(12f * s, 9f * s)
        lineTo(21f * s, 9f * s)
        lineTo(21f * s, 19f * s)
        close()
    }
    drawPath(p, c, style = stroke)
}

/**
 * Sliders: the settings.
 *
 * Not a cog, which is what mini's `⚙` is and what this tried first. Drawn at
 * this size a cog is a small ring with six spokes around it, and a small ring
 * with six spokes around it is the brightness icon -- it read as one on the
 * phone, unmistakably. Three sliders at different settings cannot be mistaken
 * for anything else.
 */
private fun DrawScope.settings(c: Color, s: Float, w: Float) {
    val rows = listOf(7f to 15f, 12f to 8f, 17f to 17f)
    for ((y, knob) in rows) {
        drawLine(
            c, Offset(4f * s, y * s), Offset(20f * s, y * s),
            strokeWidth = w, cap = androidx.compose.ui.graphics.StrokeCap.Round,
        )
        drawCircle(c, radius = 2.4f * s, center = Offset(knob * s, y * s))
    }
}
