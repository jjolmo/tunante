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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/** What `nativeListDirs` and `nativeRoots` hand back. */
data class DirListing(
    val here: String = "",
    val parent: String? = null,
    val dirs: List<String> = emptyList(),
)

/** One of the usual places music lives on a phone: a name and the path behind it. */
data class Suggestion(val label: String, val path: String)

/**
 * Choosing where the music is — and, on the first run, the screen the app
 * opens with.
 *
 * Top to bottom: the file-access permission if it is still missing (nothing
 * below works without it); the usual places — Music, Downloads, the SD card,
 * the whole internal storage — each a tick away; and under them a plain
 * directory browser for anything else. Real paths, as `tunante`'s `picker.rs`,
 * not `ACTION_OPEN_DOCUMENT_TREE`: the document picker hands back a
 * `content://` URI with no path behind it, and every layer under this —
 * `walkdir`, the database, the C decoders that `fopen` by name — deals in
 * paths.
 */
@Composable
fun FolderPicker(
    listing: DirListing,
    roots: List<String>,
    suggestions: List<Suggestion>,
    hasAllFiles: Boolean,
    firstRun: Boolean,
    onGrantFiles: () -> Unit,
    onEnter: (String) -> Unit,
    onUp: () -> Unit,
    onToggleRoot: (String, Boolean) -> Unit,
    onDone: () -> Unit,
    onSkip: () -> Unit,
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
                Label(
                    if (firstRun) tr("¿Dónde está tu música?") else tr("Carpetas de música"),
                    T.textPrimary, T.fontTitle, maxLines = 1,
                )
                Label(
                    if (roots.isEmpty()) tr("Marca al menos una carpeta")
                    else tr("{} carpeta(s) elegidas").replace("{}", "${roots.size}"),
                    T.textSecondary, T.fontSmall, maxLines = 1,
                )
            }
            // Scan what is ticked. Greyed, not hidden, with nothing ticked.
            Box(
                Modifier
                    .heightIn(min = T.touchTarget)
                    .clickable(enabled = roots.isNotEmpty(), onClick = onDone)
                    .padding(horizontal = T.gap),
                contentAlignment = Alignment.Center,
            ) { Label(tr("Analizar"), if (roots.isNotEmpty()) T.accent else T.textMuted, T.fontBody) }
        }
        Rule()

        if (!hasAllFiles) {
            PermissionBanner(onGrantFiles)
            Rule()
        }

        LazyColumn(Modifier.weight(1f)) {
            if (suggestions.isNotEmpty()) {
                item { SectionLabel(tr("Lugares habituales")) }
                items(suggestions) { s ->
                    val on = s.path in roots
                    PickRow(on, s.label, s.path) { onToggleRoot(s.path, !on) }
                }
                item { Rule() }
            }
            item { SectionLabel(tr("Otras carpetas")) }
            item {
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
                    Column(Modifier.weight(1f)) {
                        Label(
                            if (chosen) tr("Quitar esta carpeta de la biblioteca") else tr("Usar esta carpeta"),
                            T.textPrimary, T.fontBody, maxLines = 1,
                        )
                        Label(listing.here, T.textSecondary, T.fontSmall, maxLines = 1)
                    }
                }
            }
            if (listing.parent != null) {
                item { DirRow("◂  ..", T.accent) { onUp() } }
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

        // The first run can be left for later: an empty library is a place to
        // start. Later, the picker is opened from Ajustes and closes with back.
        if (firstRun) {
            Rule()
            Box(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = T.touchTarget)
                    .clickable(onClick = onSkip),
                contentAlignment = Alignment.Center,
            ) { Label(tr("Ahora no"), T.textSecondary, T.fontBody) }
        }
    }
}

@Composable
private fun SectionLabel(text: String) {
    Box(Modifier.fillMaxWidth().padding(start = T.gap, top = 14.dp, bottom = 4.dp)) {
        Label(text, T.textMuted, T.fontSmall)
    }
}

/** A suggested place: tick, name, and the path under it. */
@Composable
private fun PickRow(on: Boolean, label: String, path: String, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .background(if (on) T.bgSelected else Color.Transparent)
            .heightIn(min = T.touchTarget)
            .clickable(onClick = onClick)
            .padding(horizontal = T.gap, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(if (on) "✓" else "＋", if (on) T.accent else T.textMuted, T.fontTitle)
        Spacer(Modifier.width(T.gap))
        Column(Modifier.weight(1f)) {
            Label(label, T.textPrimary, T.fontBody, maxLines = 1)
            Label(path, T.textSecondary, T.fontSmall, maxLines = 1)
        }
    }
}

@Composable
private fun DirRow(text: String, color: Color, onClick: () -> Unit) {
    Box(
        Modifier
            .fillMaxWidth()
            .heightIn(min = T.touchTarget)
            .clickable(onClick = onClick)
            .padding(horizontal = T.gap, vertical = 8.dp),
        contentAlignment = Alignment.CenterStart,
    ) { Label(text, color, T.fontBody, maxLines = 1) }
}
