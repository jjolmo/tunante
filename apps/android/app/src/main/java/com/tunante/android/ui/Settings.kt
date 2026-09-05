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
    resumeHours: Int,
    onResumeHours: () -> Unit,
    onSleep: (Int) -> Unit,
    onScan: () -> Unit,
    onPickFolders: () -> Unit,
    onDownloadCovers: () -> Unit,
    coverStatus: String,
) {
    Column(Modifier.fillMaxSize().background(T.bgPrimary).verticalScroll(rememberScrollState())) {
        Heading(tr("Música de consola"))
        // What decides how long a track that never ends actually lasts.
        SettingRow(tr("Repeticiones del bucle"), "${state.loops}×", onClick = onLoops)
        SettingRow(
            tr("Fundido final"),
            if (state.fadeSeconds == 0) tr("sin fundido") else "${state.fadeSeconds} s",
            onClick = onFade,
        )
        // How long a saved position stays worth resuming. Past it the app still
        // reopens the list you were in, on the track you were on, but starts
        // clean instead of mid-song. Decided with cidwel: 6 h by default.
        SettingRow(
            tr("Reanudar si han pasado menos de"),
            if (resumeHours == 0) tr("siempre") else "$resumeHours h",
            onClick = onResumeHours,
        )

        Rule()
        Heading(tr("Biblioteca"))
        SettingRow(
            tr("Carpetas analizadas"),
            when (roots.size) {
                0 -> tr("ninguna")
                1 -> tr("1 carpeta")
                else -> tr("{} carpetas").replace("{}", "${roots.size}")
            },
        )
        SettingRow(tr("Añadir una carpeta"), "＋", onClick = onPickFolders)
        SettingRow(tr("Volver a analizar"), "↻", onClick = onScan)
        // Tapping again while it runs cancels it, which is why the value doubles
        // as the status line.
        SettingRow(
            tr("Descargar carátulas"),
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
            tr("Apagar la música en"),
            if (state.sleepMinutes > 0) "${state.sleepMinutes} min" else tr("desactivado"),
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
