package dev.nglmercer.tiktools.feature.settings

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.*
import androidx.compose.ui.platform.*
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.nglmercer.tiktools.ui.components.*
import kotlinx.coroutines.*

@Composable
fun DiagnosticsScreen(vm: SettingsViewModel) {
    val d = vm.diagnostics.collectAsStateWithLifecycle().value
    val error = vm.error.collectAsStateWithLifecycle().value
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val scope = rememberCoroutineScope()
    var exportError by remember { mutableStateOf<String?>(null) }
    val export =
        rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/plain")) {
            uri ->
            if (uri != null)
                scope.launch {
                    try {
                        withContext(Dispatchers.IO) {
                            checkNotNull(context.contentResolver.openOutputStream(uri))
                                .bufferedWriter()
                                .use { it.write(vm.exportText()) }
                        }
                    } catch (e: Exception) {
                        exportError = e.message
                    }
                }
        }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item {
            Text("Diagnostics", style = MaterialTheme.typography.headlineMedium)
            ErrorBanner(error ?: exportError) {
                vm.dismissError()
                exportError = null
            }
            SettingItem("Native", d.native)
            SettingItem("Bundle", d.bundle)
            SettingItem("Guest session", d.guest)
        }
        item {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = { clipboard.setText(AnnotatedString(vm.exportText())) }) {
                    Text("Copy diagnostics")
                }
                OutlinedButton(onClick = { export.launch("tiktools-diagnostics.txt") }) {
                    Text("Export logs")
                }
                TextButton(onClick = vm::clearLogs) { Text("Clear logs") }
                TextButton(onClick = vm::refreshDiagnostics) { Text("Refresh") }
            }
        }
        item {
            SectionHeader("Logs")
            SelectionContainer {
                Text(d.logs.ifBlank { "No logs" }, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}
