package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * Tunante's palette, transcribed from `tunante/ui/theme.slint`, which in
 * turn came from the desktop app's `app.css`. Three programs, one look.
 *
 * Same rule as there: **no literal colour anywhere else in the UI**. Re-theming
 * means editing this file and nothing else.
 *
 * Only the dark half is here. The phone build has never shipped light, and a
 * second palette nobody has looked at is a second palette nobody has checked.
 */
object T {
    val bgPrimary = Color(0xFF1E1E1E)
    val bgSecondary = Color(0xFF252526)
    val bgTertiary = Color(0xFF2D2D30)
    val bgHover = Color(0xFF3E3E42)
    val bgSelected = Color(0xFF094771)
    val textPrimary = Color(0xFFCCCCCC)
    val textSecondary = Color(0xFF969696)
    val textMuted = Color(0xFF5A5A5A)
    val accent = Color(0xFF007ACC)
    val accentHover = Color(0xFF1C97EA)
    val border = Color(0xFF3E3E42)

    /** Amber rather than red: the music is waiting, not lost. */
    val warningBg = Color(0xFF4A3A10)
    val warningFg = Color(0xFFF0C674)

    /**
     * Red, and only for what cannot be undone: the swipe that removes and the
     * bar that empties the queue. Kept apart from [warningFg] on purpose —
     * amber warns, this one destroys.
     */
    val destructive = Color(0xFFA33333)

    /** A finger is not a mouse pointer. Nothing interactive goes below this. */
    val touchTarget = 48.dp
    val gap = 12.dp
    val radius = 8.dp

    val fontTitle = 17.sp
    val fontBody = 15.sp
    val fontSmall = 13.sp
}

/**
 * `m:ss`, from milliseconds.
 *
 * Hours fold into the minutes rather than getting their own field: an hour-long
 * track is a rarity here, and a "1:02:03" that only sometimes appears makes the
 * row jump width as tracks change.
 */
fun mmss(ms: Long): String {
    if (ms <= 0) return "0:00"
    val total = ms / 1000
    return "${total / 60}:${(total % 60).toString().padStart(2, '0')}"
}

@Composable
fun TunanteTheme(content: @Composable () -> Unit) = content()

/**
 * Text that can only be styled from [T].
 *
 * Named `Label` rather than shadowing `Text`: the shadow made every call site
 * ambiguous to read, and this one exists precisely so a stray `fontSize = 14.sp`
 * has nowhere to go.
 */
@Composable
fun Label(
    text: String,
    color: Color,
    size: androidx.compose.ui.unit.TextUnit,
    weight: FontWeight = FontWeight.Normal,
    maxLines: Int = Int.MAX_VALUE,
) = androidx.compose.material3.Text(
    text = text,
    color = color,
    fontSize = size,
    fontWeight = weight,
    maxLines = maxLines,
    overflow = TextOverflow.Ellipsis,
)

/** The one-pixel line between rows. */
@Composable
fun Rule() = Box(
    Modifier
        .fillMaxWidth()
        .height(1.dp)
        .background(T.border)
)

/**
 * Translate a Spanish source string, through the same catalog the desktop
 * app reads, in Rust.
 *
 * Every visible literal in the Compose tree goes through this, as every one in
 * the Slint tree goes through `@tr`. Same source language, same `.po` files,
 * same `{}` placeholder — substitute it after translating, never before, or
 * the key stops matching. Memoised here so a list of two hundred rows is not
 * two hundred JNI crossings per frame.
 */
private val translated = java.util.concurrent.ConcurrentHashMap<String, String>()

fun tr(source: String): String =
    translated.getOrPut(source) { com.tunante.android.NativeBridge.nativeTr(source) }

/**
 * "1 pista", "4 pistas".
 *
 * A helper rather than the same ternary in three files, which is how the
 * playlist row came to say "1 pistas" while the queue two screens away said
 * "1 pista". Uses the same two msgids `library::pistas` does in the desktop app.
 */
fun pistas(n: Int): String =
    if (n == 1) tr("1 pista") else tr("{} pistas").replace("{}", "$n")
