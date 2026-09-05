package com.tunante.android.ui

import android.graphics.BitmapFactory
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.Dp

/**
 * The console tile of the Consolas grid: the same art tabs.slint's ConsoleArt
 * draws. The home consoles are the Controllercons pack, rasterised from
 * the SVGs under `apps/tunante/ui/consoles/` into `assets/consoles/` tinted with the
 * same ink; the handhelds and the PC — for which the pack has nothing — are
 * the same rectangles ConsoleArt lays out, in hundredths of the side.
 */
val CONSOLE_PACK = setOf(
    "nes", "snes", "n64", "genesis", "mastersystem", "saturn", "dreamcast",
    "gamecube", "wii", "wiiu", "switch", "ps1", "ps2", "ps3", "ps4",
)
val CONSOLE_DRAWN = setOf("gameboy", "gbc", "gba", "nds", "n3ds", "psp", "psvita", "gamegear", "pc")

fun hasConsoleArt(id: String) = id in CONSOLE_PACK || id in CONSOLE_DRAWN

@Composable
fun ConsoleArt(id: String, side: Dp) {
    Box(
        Modifier.size(side).clip(RoundedCornerShape(T.radius)).background(T.bgTertiary),
        contentAlignment = Alignment.Center,
    ) {
        if (id in CONSOLE_PACK) {
            val ctx = LocalContext.current
            val bmp = remember(id) {
                runCatching { ctx.assets.open("consoles/$id.png").use { BitmapFactory.decodeStream(it) } }.getOrNull()
            }
            if (bmp != null) {
                Image(bmp.asImageBitmap(), contentDescription = id, modifier = Modifier.size(side * 0.9f))
            }
        } else {
            Canvas(Modifier.fillMaxSize()) { drawHandheld(id) }
        }
    }
}

/** `u` is a hundredth of the side, as in ConsoleArt; shapes are centred. */
private fun DrawScope.drawHandheld(id: String) {
    val u = size.width / 100f
    fun rect(x: Float, y: Float, w: Float, h: Float, color: Long, r: Float = 0f, ox: Float = 0f, oy: Float = 0f) {
        drawRoundRect(
            Color(color), Offset((ox + x) * u, (oy + y) * u), Size(w * u, h * u),
            CornerRadius(r * u, r * u),
        )
    }
    when (id) {
        "gameboy", "gbc" -> {
            val ox = (100 - 42) / 2f; val oy = (100 - 66) / 2f
            rect(0f, 0f, 42f, 66f, 0xFFB9B7A6, 4f, ox, oy)
            rect(7f, 6f, 28f, 25f, 0xFF6D7B3F, 2f, ox, oy)
            rect(8f, 40f, 14f, 5f, 0xFF3B3B40, 0f, ox, oy)
            rect(12.5f, 35.5f, 5f, 14f, 0xFF3B3B40, 0f, ox, oy)
            rect(26f, 42f, 7f, 7f, 0xFF93285F, 3.5f, ox, oy)
        }
        "gba" -> {
            val ox = (100 - 76) / 2f; val oy = (100 - 38) / 2f
            rect(0f, 0f, 76f, 38f, 0xFF4B3F9E, 12f, ox, oy)
            rect(24f, 8f, 28f, 22f, 0xFF6D7B3F, 2f, ox, oy)
            rect(6f, 16f, 13f, 5f, 0xFF2C245E, 0f, ox, oy)
            rect(10f, 12f, 5f, 13f, 0xFF2C245E, 0f, ox, oy)
            rect(58f, 18f, 7f, 7f, 0xFF2C245E, 3.5f, ox, oy)
            rect(66f, 12f, 7f, 7f, 0xFF2C245E, 3.5f, ox, oy)
        }
        "nds", "n3ds" -> {
            val ox = (100 - 52) / 2f; val oy = (100 - 64) / 2f
            rect(0f, 0f, 52f, 29f, 0xFFB8BCC4, 3f, ox, oy)
            rect(6f, 4f, 40f, 21f, 0xFF23262E, 1f, ox, oy)
            rect(0f, 31f, 52f, 4f, 0xFF7F838B, 0f, ox, oy)
            rect(0f, 35f, 52f, 29f, 0xFFB8BCC4, 3f, ox, oy)
            rect(6f, 39f, 40f, 21f, 0xFF23262E, 1f, ox, oy)
        }
        "psp", "psvita" -> {
            // Its four symbols, drawn: a triangle, a square, a circle, a cross.
            val ink = Color(0xFF6EA8FF); val s = 14f * u; val w = 3f * u
            val cx = size.width / 2; val cy = size.height / 2; val d = 22f * u
            val tri = androidx.compose.ui.graphics.Path().apply {
                moveTo(cx, cy - d - s / 2); lineTo(cx + s / 2, cy - d + s / 2); lineTo(cx - s / 2, cy - d + s / 2); close()
            }
            drawPath(tri, ink, style = androidx.compose.ui.graphics.drawscope.Stroke(w))
            drawRect(ink, Offset(cx - d - s / 2, cy - s / 2), Size(s, s), style = androidx.compose.ui.graphics.drawscope.Stroke(w))
            drawCircle(ink, s / 2, Offset(cx + d, cy), style = androidx.compose.ui.graphics.drawscope.Stroke(w))
            drawLine(ink, Offset(cx - s / 2, cy + d - s / 2), Offset(cx + s / 2, cy + d + s / 2), w)
            drawLine(ink, Offset(cx + s / 2, cy + d - s / 2), Offset(cx - s / 2, cy + d + s / 2), w)
        }
        "gamegear" -> {
            val ox = (100 - 74) / 2f; val oy = (100 - 32) / 2f
            rect(0f, 0f, 74f, 32f, 0xFF1F1F22, 14f, ox, oy)
            rect(9f, 12f, 19f, 7f, 0xFF4F4F57, 0f, ox, oy)
            rect(15f, 6f, 7f, 19f, 0xFF4F4F57, 0f, ox, oy)
            rect(42f, 13f, 8f, 8f, 0xFF4F4F57, 4f, ox, oy)
            rect(52f, 11f, 8f, 8f, 0xFF4F4F57, 4f, ox, oy)
            rect(62f, 9f, 8f, 8f, 0xFF4F4F57, 4f, ox, oy)
        }
        "pc" -> {
            val ox = (100 - 56) / 2f; val oy = (100 - 48) / 2f
            rect(4f, 2f, 48f, 34f, 0xFF4A4A55, 3f, ox, oy)
            rect(7f, 5f, 42f, 28f, 0xFF29B6F6, 1f, ox, oy)
            rect(25f, 36f, 6f, 6f, 0xFF4A4A55, 0f, ox, oy)
            rect(16f, 42f, 24f, 4f, 0xFF4A4A55, 2f, ox, oy)
        }
    }
}
