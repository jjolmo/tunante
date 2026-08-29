package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/** What `nativeListDirs` and `nativeRoots` hand back. */
data class DirListing(
    val here: String = "",
    val parent: String? = null,
    val dirs: List<String> = emptyList(),
)

/**
 * Choosing where the music is.
 *
 * A plain directory browser over real paths, the same as `tunante-mini`'s
 * `picker.rs`, and not Android's `ACTION_OPEN_DOCUMENT_TREE`. The document
 * picker hands back a `content://` URI with no path behind it, and every layer
 * under this — `walkdir`, the database, the C decoders that `fopen` by name —
 * deals in paths. With all-files access already granted there is nothing to buy
 * by going through it.
 */
@Composable
fun FolderPicker(
    listing: DirListing,
    roots: List<String>,
    onEnter: (String) -> Unit,
    onUp: () -> Unit,
    onToggleRoot: (String, Boolean) -> Unit,
    onDone: () -> Unit,
) {
    Column(Modifier.fillMaxSize().background(T.bgPrimary)) {
        Row(
            Modifier
                .fillMaxWidth()
                .background(T.bgSecondary)
                .heightIn(min = T.touchTarget)
                .padding(horizontal = T.gap),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f).padding(vertical = 8.dp)) {
                Label("Carpetas de música", T.textPrimary, T.fontBody, maxLines = 1)
                Label(listing.here, T.textSecondary, T.fontSmall, maxLines = 1)
            }
            Box(
                Modifier
                    .heightIn(min = T.touchTarget)
                    .clickable(onClick = onDone)
                    .padding(horizontal = T.gap),
                contentAlignment = Alignment.Center,
            ) { Label("Listo", T.accent, T.fontBody) }
        }
        Rule()

        val chosen = listing.here in roots
        Row(
            Modifier
                .fillMaxWidth()
                .background(if (chosen) T.bgSelected else T.bgTertiary)
                .heightIn(min = T.touchTarget)
                .clickable { onToggleRoot(listing.here, !chosen) }
                .padding(horizontal = T.gap),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Label(if (chosen) "✓" else "＋", T.accent, T.fontTitle)
            Spacer(Modifier.width(T.gap))
            Label(
                if (chosen) "Quitar esta carpeta de la biblioteca"
                else "Usar esta carpeta",
                T.textPrimary,
                T.fontBody,
                maxLines = 1,
            )
        }
        Rule()

        LazyColumn(Modifier.fillMaxSize()) {
            if (listing.parent != null) {
                item {
                    DirRow("◂  ..", T.accent) { onUp() }
                }
            }
            items(listing.dirs) { name ->
                // Marked here as well as on the row above, so a folder already
                // in the library is visible without walking into it.
                val isRoot = "${listing.here}/$name" in roots
                DirRow(if (isRoot) "▸  $name  ✓" else "▸  $name", T.textPrimary) {
                    onEnter("${listing.here}/$name")
                }
            }
        }
    }
}

@Composable
private fun DirRow(text: String, color: androidx.compose.ui.graphics.Color, onClick: () -> Unit) {
    Box(
        Modifier
            .fillMaxWidth()
            .heightIn(min = T.touchTarget)
            .clickable(onClick = onClick)
            .padding(horizontal = T.gap, vertical = 8.dp),
        contentAlignment = Alignment.CenterStart,
    ) { Label(text, color, T.fontBody, maxLines = 1) }
}
