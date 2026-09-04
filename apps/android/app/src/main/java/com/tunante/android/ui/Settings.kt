package com.tunante.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * Ajustes, with the same rows and the same words as tunante's.
 *
 * This screen not existing is why the player looked the way it did: loops and
 * fade had nowhere to go, so they became chips under the transport, and Scan
 * had nowhere to go, so it became a button in a title bar. Both are options
 * you set once and forget, which is exactly what a settings screen is for.
 */
@Composable
fun SettingsScreen(
    state: PlayerState,
    roots: List<String>,
    onLoops: () -> Unit,
    onFade: () -> Unit,
    onSleep: (Int) -> Unit,
    onScan: () -> Unit,
    onPickFolders: () -> Unit,
    onDownloadCovers: () -> Unit,
    coverStatus: String,
) {
    Column(Modifier.fillMaxSize().background(T.bgPrimary).verticalScroll(rememberScrollState())) {
        Heading("Música de consola")
        // What decides how long a track that never ends actually lasts.
        SettingRow("Repeticiones del bucle", "${state.loops}×", onClick = onLoops)
        SettingRow(
            "Fundido final",
            if (state.fadeSeconds == 0) "sin fundido" else "${state.fadeSeconds} s",
            onClick = onFade,
        )

        Rule()
        Heading("Biblioteca")
        SettingRow(
            "Carpetas analizadas",
            when (roots.size) {
                0 -> "ninguna"
                1 -> "1 carpeta"
                else -> "${roots.size} carpetas"
            },
        )
        SettingRow("Añadir una carpeta", "＋", onClick = onPickFolders)
        SettingRow("Volver a analizar", "↻", onClick = onScan)
        // Tapping again while it runs cancels it, which is why the value doubles
        // as the status line.
        SettingRow(
            "Descargar carátulas",
            if (coverStatus.isEmpty()) "⬇" else coverStatus,
            highlighted = coverStatus.isNotEmpty(),
            onClick = onDownloadCovers,
        )

        Rule()
        // Named for what it does rather than for the verb, which on its own —
        // "Apagar" — did not say apagar *qué*. The interval cycles through the
        // ones mini offers: a picker for four choices costs more taps than
        // tapping through them.
        SettingRow(
            "Apagar la música en",
            if (state.sleepMinutes > 0) "${state.sleepMinutes} min" else "desactivado",
            highlighted = state.sleepMinutes > 0,
        ) {
            onSleep(
                when (state.sleepMinutes) {
                    0 -> 15
                    in 1..15 -> 30
                    in 16..30 -> 60
                    else -> 0
                }
            )
        }
    }
}

@Composable
private fun Heading(text: String) =
    Row(Modifier.padding(start = T.gap, top = T.gap, bottom = 4.dp)) {
        Label(text, T.textSecondary, T.fontSmall)
    }

/** Label on the left, what it is set to on the right. */
@Composable
private fun SettingRow(
    label: String,
    value: String,
    highlighted: Boolean = false,
    onClick: (() -> Unit)? = null,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = T.touchTarget)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = T.gap, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(label, if (onClick != null) T.textPrimary else T.textSecondary, T.fontBody)
        Spacer(Modifier.weight(1f))
        Spacer(Modifier.width(T.gap))
        Label(value, if (highlighted) T.accent else T.textSecondary, T.fontBody, maxLines = 1)
    }
}
