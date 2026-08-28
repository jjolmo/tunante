package com.tunante.android.ui

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Base64
import android.util.LruCache
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.Dp
import androidx.compose.foundation.Image
import androidx.compose.foundation.shape.RoundedCornerShape
import com.tunante.android.NativeBridge
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext

/**
 * Cover art, decoded once and kept.
 *
 * Bounded by bytes rather than by entry count: a 1600×1600 front cover and a
 * 200×200 thumbnail are both "one entry" and differ by sixty times the memory.
 * `tunante-mini` keeps forty entries at 224 px for the same job; this is the
 * same idea with the units that actually matter on a phone.
 */
private object ArtCache : LruCache<String, Bitmap>(12 * 1024 * 1024) {
    override fun sizeOf(key: String, value: Bitmap): Int = value.byteCount
}

/** Tracks that asked and had nothing, so we do not spawn a decoder per scroll. */
private val known = java.util.Collections.synchronizedSet(HashSet<String>())

/**
 * How many covers may be fetched at once.
 *
 * Not a nicety. `nativeArtwork` spawns a decoder process for embedded art and
 * waits up to five seconds, and `Dispatchers.IO` will happily run sixty-four
 * coroutines at a time — so flinging an eight-column grid would put dozens of
 * emulator processes on a phone simultaneously, each holding its console's RAM.
 *
 * Three is enough to keep a grid filling in visibly while leaving the cores for
 * the track that is actually playing.
 */
private val fetching = Semaphore(3)

/**
 * Forget every cached answer, after covers have been downloaded.
 *
 * Both halves matter, and the negative one matters more. [known] holds the
 * tracks that asked and had nothing — exactly the tracks a download run has
 * just given a cover to. Without this the new art stays invisible until the
 * app is restarted, which reads as the feature not having worked.
 */
fun forgetCachedArt() {
    ArtCache.evictAll()
    known.clear()
}

/**
 * Decode a `data:` URI, downsampling to roughly [maxSide] on its longest edge.
 *
 * Two passes on purpose: the first only reads the header, so a huge cover never
 * gets fully decoded just to be thrown away at a twelfth of the size.
 */
private fun decode(uri: String, maxSide: Int): Bitmap? {
    val comma = uri.indexOf(',')
    if (!uri.startsWith("data:") || comma < 0) return null
    val bytes = try {
        Base64.decode(uri.substring(comma + 1), Base64.DEFAULT)
    } catch (e: IllegalArgumentException) {
        return null
    }

    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    var sample = 1
    while (maxOf(bounds.outWidth, bounds.outHeight) / sample > maxSide) {
        sample *= 2
    }
    return BitmapFactory.decodeByteArray(
        bytes, 0, bytes.size,
        BitmapFactory.Options().apply { inSampleSize = sample },
    )
}

/**
 * The cover for a track, or a placeholder square.
 *
 * Always occupies its space, whether or not there is art: a list whose rows
 * change height as covers arrive is worse than one with no covers at all.
 */
@Composable
fun Cover(path: String, side: Dp, maxSide: Int = 256) {
    var bitmap by remember(path) { mutableStateOf(ArtCache.get(path)) }

    LaunchedEffect(path) {
        if (bitmap != null || path.isEmpty() || path in known) return@LaunchedEffect
        // Off the main thread: this can spawn a decoder process, and it runs
        // once per visible row.
        val decoded = withContext(Dispatchers.IO) {
            fetching.withPermit {
                // Re-checked inside the permit: by the time this one's turn
                // comes the row may have scrolled away and another pass may
                // already have answered for the same path.
                ArtCache.get(path) ?: run {
                    val uri = NativeBridge.nativeArtwork(path)
                    if (uri.isEmpty()) null else decode(uri, maxSide)
                }
            }
        }
        if (decoded == null) {
            known.add(path)
        } else {
            ArtCache.put(path, decoded)
            bitmap = decoded
        }
    }

    Box(
        Modifier
            .size(side)
            .clip(RoundedCornerShape(4.dp_))
            .background(T.bgTertiary),
        contentAlignment = Alignment.Center,
    ) {
        val b = bitmap
        if (b != null) {
            Image(
                bitmap = b.asImageBitmap(),
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.size(side),
            )
        } else {
            Label("♪", T.textMuted, T.fontBody)
        }
    }
}

/** Local shorthand so this file does not import `dp` just for one corner. */
private val Int.dp_: Dp get() = androidx.compose.ui.unit.Dp(this.toFloat())
