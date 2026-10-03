package dev.nglmercer.tiktools.feature.settings

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.nglmercer.tiktools.data.preferences.*
import dev.nglmercer.tiktools.tts.TtsEngine
import dev.nglmercer.tiktools.ui.components.*

@Composable
fun SpeechControls(
    prefs: UserPreferences,
    onEngine: (TtsEngine) -> Unit,
    onSpeech: (SpeechPreferences) -> Unit,
    onRepeat: () -> Unit,
    onSkip: () -> Unit,
    canRepeat: Boolean,
) {
    for (engine in TtsEngine.entries) Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable { onEngine(engine) },
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
    ) {
        RadioButton(prefs.engine == engine, onClick = { onEngine(engine) })
        Text(
            when (engine) {
                TtsEngine.OFF -> "Off"
                TtsEngine.DEVICE -> "Device voice"
                TtsEngine.SUPERTONIC -> "SuperTonic 3"
            }
        )
    }
    if (prefs.engine != TtsEngine.OFF)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onRepeat, enabled = canRepeat) { Text("Repeat") }
            OutlinedButton(onClick = onSkip) { Text("Skip") }
        }
    SectionHeader("Announce")
    val p = prefs.speech
    SettingSwitch("Chat", p.chat, { onSpeech(p.copy(chat = it)) })
    SettingSwitch("Gifts", p.gifts, { onSpeech(p.copy(gifts = it)) })
    SettingSwitch("Follows", p.follows, { onSpeech(p.copy(follows = it)) })
    SettingSwitch("Shares", p.shares, { onSpeech(p.copy(shares = it)) })
    SettingSwitch("Joins", p.joins, { onSpeech(p.copy(joins = it)) })
}

@Composable
fun SpeechScreen(vm: SettingsViewModel) {
    val prefs = vm.preferences.collectAsStateWithLifecycle().value
    val model = vm.models.collectAsStateWithLifecycle().value
    val last = vm.lastSpoken.collectAsStateWithLifecycle().value
    val error = vm.error.collectAsStateWithLifecycle().value
    var deleting by remember { mutableStateOf(false) }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item {
            Text("Speech", style = MaterialTheme.typography.headlineMedium)
            ErrorBanner(error, vm::dismissError)
        }
        item { SpeechControls(prefs, vm::engine, vm::speech, vm::repeat, vm::skip, last != null) }
        item {
            HorizontalDivider()
            SectionHeader("SuperTonic 3")
            Text("High quality offline speech", style = MaterialTheme.typography.titleMedium)
            Text(
                "~210 MB download · runs entirely on your device · voice F1",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(model.detail)
            if (model.bytes > 0)
                Text(
                    "${amount(model.bytes/1048576.0)} MB on device",
                    style = MaterialTheme.typography.bodySmall,
                )
            ErrorBanner(model.error)
        }
        item {
            when {
                model.downloading -> {
                    LinearProgressIndicator(
                        progress = { model.progress ?: 0f },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Text("${((model.progress ?: 0f)*100).toInt()}%")
                    TextButton(onClick = vm::cancelDownload) { Text("Cancel download") }
                }
                model.ready ->
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(
                            onClick = vm::testVoice,
                            enabled = prefs.engine != TtsEngine.OFF,
                        ) {
                            Text("Test voice")
                        }
                        TextButton(onClick = { deleting = true }) { Text("Delete model") }
                    }
                else -> Button(onClick = vm::download) { Text("Download model") }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
    if (deleting)
        AlertDialog(
            onDismissRequest = { deleting = false },
            title = { Text("Delete the offline model?") },
            text = { Text("Speech will be turned off. You can download the model again later.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        vm.deleteModel()
                        deleting = false
                    }
                ) {
                    Text("Delete")
                }
            },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text("Cancel") } },
        )
}
