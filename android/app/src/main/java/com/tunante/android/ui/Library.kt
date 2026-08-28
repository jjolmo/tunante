package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.dp

/** A folder in the library tree, as `nativeBrowse` hands it over. */
data class Folder(
    val path: String,
    val name: String,
    val count: Int,
    /** A track inside it, to take the cover from. */
    val cover: String = "",
)

/**
 * What the library tab is showing.
 *
 * A search with text in it replaces the tree rather than filtering it, which is
 * what `tunante-mini` does too: when you are looking for a title you do not care
 * which folder it was in.
 */
data class LibraryView(
    val here: String = "",
    val folders: List<Folder> = emptyList(),
    val tracks: List<Track> = emptyList(),
    val query: String = "",
    /**
     * What to write in the breadcrumb, when `here` is not a path.
     *
     * The index tabs navigate by name — a console, or a game, which is an album
     * tag. Cutting one of those at its last `/` is a category error that only
     * shows itself on the rare name containing one, so it is decided here
     * rather than guessed at the point of drawing.
     */
    val label: String = "",
) {
    val searching: Boolean get() = query.isNotEmpty()

    val crumb: String get() = label.ifEmpty { here.substringAfterLast('/') }
}

/**
 * The breadcrumb.
 *
 * `tunante-mini` carries its own `◂` because Plasma Mobile has no back button
 * (see its HANDOVER.md). Android does have one, and it is wired to the same
 * action — but the affordance stays: a gesture you cannot see is not a way of
 * telling someone where they are.
 */
@Composable
fun Breadcrumb(view: LibraryView, onUp: () -> Unit) {
    if (view.here.isEmpty() && !view.searching) return
    Row(
        Modifier
            .fillMaxWidth()
            .background(T.bgTertiary)
            .heightIn(min = T.touchTarget)
            .clickable(onClick = onUp)
            .padding(horizontal = T.gap),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label("◂", T.accent, T.fontTitle)
        Spacer(Modifier.width(T.gap))
        Label(
            if (view.searching) "Buscando “${view.query}”" else view.crumb,
            T.textPrimary,
            T.fontBody,
            maxLines = 1,
        )
    }
    Rule()
}

@Composable
fun SearchBox(query: String, onQuery: (String) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .background(T.bgSecondary)
            .padding(horizontal = T.gap, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.weight(1f).heightIn(min = T.touchTarget), Alignment.CenterStart) {
            if (query.isEmpty()) {
                Label("Buscar…", T.textMuted, T.fontBody)
            }
            BasicTextField(
                value = query,
                onValueChange = onQuery,
                singleLine = true,
                textStyle = TextStyle(color = T.textPrimary, fontSize = T.fontBody),
                // Without this the caret is drawn in the platform's default
                // colour, which on this palette is nearly invisible.
                cursorBrush = SolidColor(T.accent),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        if (query.isNotEmpty()) {
            Box(
                Modifier
                    .heightIn(min = T.touchTarget)
                    .clickable { onQuery("") }
                    .padding(horizontal = T.gap),
                contentAlignment = Alignment.Center,
            ) { Label("✕", T.textSecondary, T.fontTitle) }
        }
    }
    Rule()
}

@Composable
fun FolderRow(folder: Folder, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = T.touchTarget)
            .background(T.bgPrimary)
            .clickable(onClick = onClick)
            .padding(horizontal = T.gap, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label("▸", T.textMuted, T.fontBody)
        Spacer(Modifier.width(T.gap))
        Column(Modifier.weight(1f)) {
            Label(folder.name, T.textPrimary, T.fontBody, maxLines = 1)
        }
        Label("${folder.count}", T.textMuted, T.fontSmall)
    }
    Rule()
}

/**
 * Folders as a grid of covers.
 *
 * Three columns upright, eight on its side — the same numbers `tunante-mini`
 * retiles to. They are not a ratio: a phone turned sideways is much wider than
 * it is tall, and a grid that only doubled would leave tiles the size of a
 * thumbnail's thumbnail.
 *
 * The cover shown for a folder is the cover of its first track, which for an
 * album folder is the album's, and for a console folder is whatever the first
 * game had. mini makes the same approximation.
 */
@Composable
fun FolderGrid(
    folders: List<Folder>,
    coverOf: (Folder) -> String,
    onOpen: (Folder) -> Unit,
) {
    val columns = if (LocalConfiguration.current.orientation ==
        android.content.res.Configuration.ORIENTATION_LANDSCAPE
    ) 8 else 3

    LazyVerticalGrid(
        columns = GridCells.Fixed(columns),
        modifier = Modifier.fillMaxSize().padding(horizontal = T.gap),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
        contentPadding = PaddingValues(vertical = T.gap),
    ) {
        items(folders, key = { it.path }) { folder ->
            Column(
                Modifier.clickable { onOpen(folder) },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                // Square, and sized by the column rather than by a fixed dp:
                // eight columns on a 1080-wide screen is 128 px a tile, and a
                // hardcoded size would either overflow or leave gaps.
                BoxWithConstraints {
                    Cover(coverOf(folder), maxWidth, maxSide = 256)
                }
                Label(folder.name, T.textPrimary, T.fontSmall, maxLines = 1)
            }
        }
    }
}
