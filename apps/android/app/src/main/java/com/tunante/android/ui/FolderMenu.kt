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
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/**
 * What a row stands for, in the form the bridge wants it.
 *
 * A game is an album tag, a console is a set of extensions, and one game of one
 * console is a pair — none of the three is a path, and `nativeRowTracks` is the
 * only thing that knows how to read them back. Built here because this is where
 * the tab and the level are known; by the time the row reaches Rust the context
 * is gone.
 *
 * U+0001 joins the console to its directory because it is the one byte a path
 * cannot contain. tunante encodes the same pair the same way.
 */
fun rowKey(tab: Tab, here: String, folderPath: String): String = when {
    tab == Tab.Games -> "juego:$folderPath"
    tab == Tab.Consoles && here.isEmpty() -> "consola:$folderPath"
    tab == Tab.Consoles -> "$here$folderPath"
    else -> folderPath
}

/**
 * The long-press menu for a folder, album, game or console.
 *
 * Without it the only way to queue a game's soundtrack was to open it and swipe
 * every track, which for a rip of any size is not a way at all.
 *
 * The four actions and their wording are tunante's. The two "sólo esta
 * carpeta" ones appear only where they mean something different from the deep
 * ones: on a real directory. A game and a console are not directories and have
 * no subfolders to leave out, so offering the distinction there would be two
 * rows that do the same thing.
 */
@Composable
fun FolderMenu(
    name: String,
    isDirectory: Boolean,
    onEnqueue: (deep: Boolean) -> Unit,
    onAddToPlaylist: (deep: Boolean) -> Unit,
    onDismiss: () -> Unit,
) {
    Box(
        Modifier
            .fillMaxSize()
            .background(Color(0xCC000000))
            .clickable(onClick = onDismiss),
        contentAlignment = Alignment.BottomCenter,
    ) {
        // Its own clickable, swallowing taps: without it a press anywhere on
        // the sheet falls through to the scrim behind and closes it.
        Column(Modifier.fillMaxWidth().background(T.bgSecondary).clickable {}) {
            Row(Modifier.fillMaxWidth().padding(T.gap)) {
                Label(name, T.textPrimary, T.fontBody, maxLines = 1)
            }
            Rule()
            Action(
                { Label("≡", T.accent, T.fontBody) },
                if (isDirectory) tr("Añadir todo, con las subcarpetas") else tr("Añadir a la cola"),
            ) { onEnqueue(true) }
            if (isDirectory) {
                // Drawn, not typed. mini writes this one as `🗀` and can; here it
                // came out as an empty box on the first run, which is the same
                // trap the sleep-timer moon fell into. U+1F5C0 is not in the
                // system font. The three below it -- U+2261, U+266B, U+2715 --
                // are, and were checked on the screen rather than assumed.
                Action({ Icon(IconKind.Library, T.accent, size = 18.dp) },
                    tr("Añadir sólo esta carpeta")) { onEnqueue(false) }
            }
            Rule()
            Action(
                { Label("♫", T.accent, T.fontBody) },
                if (isDirectory) tr("Añadir carpeta y subcarpetas a lista") else tr("Añadir a una lista"),
            ) { onAddToPlaylist(true) }
            if (isDirectory) {
                Action({ Label("♫", T.accent, T.fontBody) }, tr("Añadir carpeta a lista")) {
                    onAddToPlaylist(false)
                }
            }
            Rule()
            Action({ Label("✕", T.accent, T.fontBody) }, tr("Cancelar"), onDismiss)
        }
    }
}

@Composable
private fun Action(mark: @Composable () -> Unit, label: String, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = T.touchTarget)
            .clickable(onClick = onClick)
            .padding(horizontal = T.gap, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.width(T.gap * 2), contentAlignment = Alignment.Center) { mark() }
        Spacer(Modifier.width(4.dp))
        Label(label, T.textPrimary, T.fontBody, maxLines = 1)
    }
}
