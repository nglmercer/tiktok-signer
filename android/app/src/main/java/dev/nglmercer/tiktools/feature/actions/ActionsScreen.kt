package dev.nglmercer.tiktools.feature.actions

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.*
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.nglmercer.tiktools.core.model.LiveEvent
import dev.nglmercer.tiktools.data.actions.*
import dev.nglmercer.tiktools.ui.components.*

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ActionsScreen(vm: ActionsViewModel) {
    val state = vm.state.collectAsStateWithLifecycle().value
    val error = vm.error.collectAsStateWithLifecycle().value
    var editId by rememberSaveable { mutableStateOf<Long?>(null) }
    var deleting by remember { mutableStateOf<EventAction?>(null) }
    LazyColumn(
        Modifier.fillMaxSize().padding(horizontal = 16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item {
            SectionHeader("Automations") {
                IconButton(
                    onClick = {
                        vm.dismissError()
                        editId = 0
                    }
                ) {
                    Icon(Icons.Outlined.Add, "New automation")
                }
            }
            ErrorBanner(error, vm::dismissError)
        }
        if (state.actions.isEmpty())
            item {
                EmptyState(
                    "No automations yet",
                    "Choose an event and the HTTP request it should trigger",
                    Icons.Outlined.Bolt,
                )
            }
        items(state.actions, key = { it.id }) { action ->
            val last = state.runs.firstOrNull { it.actionId == action.id }
            Row(
                Modifier.fillMaxWidth()
                    .clickable {
                        vm.dismissError()
                        editId = action.id
                    }
                    .padding(vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Icon(
                    Icons.Outlined.Bolt,
                    null,
                    tint =
                        if (action.enabled) MaterialTheme.colorScheme.primary
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Column(Modifier.weight(1f)) {
                    Text(action.name, style = MaterialTheme.typography.titleMedium)
                    Text(
                        "${action.triggerSummary()}${if(action.giftName.isNotBlank()) " · ${action.giftName}" else ""} · ${action.method}",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Text(
                        if (last == null) "No recent runs"
                        else "${last.status} · ${eventTime(last.at)}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                IconButton(onClick = { vm.test(action) }) {
                    Icon(Icons.Outlined.PlayArrow, "Test ${action.name}")
                }
                Switch(action.enabled, { vm.enable(action, it) })
            }
            HorizontalDivider()
        }
        item { SectionHeader("Execution history") }
        if (state.runs.isEmpty()) item { EmptyState("No executions yet") }
        items(state.runs, key = { it.id }) { run ->
            Column(Modifier.padding(vertical = 8.dp)) {
                Text(
                    "${eventTime(run.at)} · ${state.actions.firstOrNull { it.id==run.actionId }?.name ?: "Automation ${run.actionId}"} · ${run.status} · ${run.ms} ms",
                    style = MaterialTheme.typography.labelLarge,
                )
                Text(
                    run.detail,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
    editId?.let { id ->
        val original = state.actions.firstOrNull { it.id == id } ?: EventAction()
        ModalBottomSheet(onDismissRequest = { editId = null }) {
            AutomationEditor(
                original,
                error,
                onSave = { vm.save(it) { editId = null } },
                onTest = vm::test,
                onDelete =
                    if (id != 0L) {
                        { deleting = original }
                    } else null,
            )
        }
    }
    deleting?.let { action ->
        AlertDialog(
            onDismissRequest = { deleting = null },
            title = { Text("Delete ${action.name}?") },
            confirmButton = {
                TextButton(
                    onClick = {
                        vm.delete(action.id)
                        deleting = null
                        editId = null
                    }
                ) {
                    Text("Delete")
                }
            },
            dismissButton = { TextButton(onClick = { deleting = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun AutomationEditor(
    original: EventAction,
    error: String?,
    onSave: (EventAction) -> Unit,
    onTest: (EventAction) -> Unit,
    onDelete: (() -> Unit)?,
) {
    var name by rememberSaveable(original.id) { mutableStateOf(original.name) }
    var method by rememberSaveable(original.id) { mutableStateOf(original.method.name) }
    var url by rememberSaveable(original.id) { mutableStateOf(original.url) }
    var body by rememberSaveable(original.id) { mutableStateOf(original.body) }
    var gift by rememberSaveable(original.id) { mutableStateOf(original.giftName) }
    var cooldown by
        rememberSaveable(original.id) { mutableStateOf(original.cooldownSecs.toString()) }
    var categories by
        rememberSaveable(original.id) {
            mutableStateOf(original.triggers.map { it.name }.toTypedArray())
        }
    fun draft() =
        original.copy(
            name = name.trim(),
            method = EventAction.Method.valueOf(method),
            url = url.trim(),
            body = body,
            giftName = gift.trim(),
            cooldownSecs = cooldown.toLongOrNull() ?: -1,
            triggers = categories.map { LiveEvent.Category.valueOf(it) }.toSet(),
        )
    Column(
        Modifier.fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(16.dp)
            .imePadding()
            .navigationBarsPadding(),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            if (original.id == 0L) "New automation" else "Edit automation",
            style = MaterialTheme.typography.headlineMedium,
        )
        ErrorBanner(error)
        OutlinedTextField(
            name,
            { name = it },
            Modifier.fillMaxWidth(),
            label = { Text("Name") },
            singleLine = true,
        )
        SectionHeader("When")
        LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            items(LiveEvent.Category.entries) { c ->
                FilterChip(
                    c.name in categories,
                    {
                        categories =
                            if (c.name in categories)
                                categories.filter { it != c.name }.toTypedArray()
                            else categories + c.name
                    },
                    label = { Text(c.name.lowercase()) },
                    leadingIcon = { Icon(c.icon(), null, Modifier.size(16.dp)) },
                )
            }
        }
        if (LiveEvent.Category.GIFT.name in categories)
            OutlinedTextField(
                gift,
                { gift = it },
                Modifier.fillMaxWidth(),
                label = { Text("Gift name (optional)") },
                supportingText = { Text("Leave blank to match every gift") },
                singleLine = true,
            )
        SectionHeader("Do · HTTP request")
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            for (m in EventAction.Method.entries) FilterChip(
                method == m.name,
                { method = m.name },
                label = { Text(m.name) },
            )
        }
        OutlinedTextField(
            url,
            { url = it },
            Modifier.fillMaxWidth(),
            label = { Text("HTTP or HTTPS URL") },
            singleLine = true,
        )
        if (method == "POST")
            OutlinedTextField(
                body,
                { body = it },
                Modifier.fillMaxWidth(),
                label = { Text("JSON body") },
                minLines = 2,
                maxLines = 5,
            )
        Text(
            "Templates: {{user}}, {{name}}, {{text}}, {{type}}, {{count}}, {{diamonds}}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        OutlinedTextField(
            cooldown,
            { cooldown = it },
            label = { Text("Cooldown in seconds") },
            keyboardOptions =
                androidx.compose.foundation.text.KeyboardOptions(
                    keyboardType = androidx.compose.ui.text.input.KeyboardType.Number
                ),
            singleLine = true,
        )
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (onDelete != null)
                IconButton(onClick = onDelete) {
                    Icon(Icons.Outlined.DeleteOutline, "Delete automation")
                }
            OutlinedButton(
                onClick = { onTest(draft()) },
                enabled = url.isNotBlank() && categories.isNotEmpty(),
            ) {
                Text("Test")
            }
            Button(
                onClick = { onSave(draft()) },
                modifier = Modifier.weight(1f),
                enabled =
                    name.isNotBlank() &&
                        url.isNotBlank() &&
                        categories.isNotEmpty() &&
                        (cooldown.toLongOrNull() ?: -1) >= 0,
            ) {
                Text("Save")
            }
        }
    }
}
