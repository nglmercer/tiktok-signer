package dev.nglmercer.tiktools.feature.settings

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.nglmercer.tiktools.data.preferences.ThemeMode
import dev.nglmercer.tiktools.tts.TtsEngine
import dev.nglmercer.tiktools.ui.components.*

@Composable
fun SettingsScreen(vm: SettingsViewModel, navigate: (String) -> Unit) {
    val p = vm.preferences.collectAsStateWithLifecycle().value
    val diagnostics = vm.diagnostics.collectAsStateWithLifecycle().value
    val error = vm.error.collectAsStateWithLifecycle().value
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(16.dp)) {
        item {
            Text("Settings", style = MaterialTheme.typography.headlineMedium)
            ErrorBanner(error, vm::dismissError)
        }
        item {
            SectionHeader("LIVE")
            SettingSwitch(
                "Automatic reconnect",
                p.reconnect,
                vm::reconnect,
                "Uses the native transport's retry policy",
            )
            SettingSwitch(
                "Background connection",
                p.backgroundConnection,
                vm::background,
                "Keep the LIVE session and speech running with a notification",
            )
            HorizontalDivider()
        }
        item {
            SectionHeader("Speech")
            SettingItem(
                "Engine",
                when (p.engine) {
                    TtsEngine.OFF -> "Off"
                    TtsEngine.DEVICE -> "Device voice"
                    TtsEngine.SUPERTONIC -> "SuperTonic 3"
                },
            ) {
                navigate("speech")
            }
            HorizontalDivider()
        }
        item {
            SectionHeader("Appearance")
            for (mode in ThemeMode.entries) Row(
                verticalAlignment = androidx.compose.ui.Alignment.CenterVertically
            ) {
                RadioButton(p.theme == mode, { vm.theme(mode) })
                Text(mode.name.lowercase().replaceFirstChar(Char::uppercase))
            }
            HorizontalDivider()
        }
        item {
            SectionHeader("Session")
            SettingItem("Guest session", diagnostics.guest)
            TextButton(onClick = vm::resetSession) { Text("Reset guest session") }
            HorizontalDivider()
        }
        item {
            SectionHeader("Signer")
            SettingItem("Engine", diagnostics.native)
            SettingItem("Bundle", diagnostics.bundle)
            TextButton(onClick = vm::redownloadBundle) { Text("Re-download bundle") }
            HorizontalDivider()
        }
        item {
            SectionHeader("Storage")
            TextButton(onClick = vm::clearBundle) { Text("Clear cached signing bundle") }
            TextButton(onClick = { navigate("speech") }) { Text("Manage offline speech model") }
            HorizontalDivider()
        }
        item {
            SectionHeader("Developer")
            SettingItem("Diagnostics", "Native version, connection, logs and export") {
                navigate("diagnostics")
            }
            HorizontalDivider()
        }
        item {
            SectionHeader("About")
            SettingItem("TikTools Studio", "1.2.0 · Android · Rust QuickJS core")
            Spacer(Modifier.height(24.dp))
        }
    }
}
