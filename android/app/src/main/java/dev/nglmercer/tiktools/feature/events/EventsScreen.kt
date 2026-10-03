package dev.nglmercer.tiktools.feature.events

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.*
import androidx.compose.ui.platform.*
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import dev.nglmercer.tiktools.core.model.*
import dev.nglmercer.tiktools.ui.components.*
import org.json.JSONObject

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EventsScreen(
    state: EventsUiState,
    onFilter: (EventFilter) -> Unit,
    onDisplay: (EventDisplayConfig) -> Unit,
    onPause: (Boolean) -> Unit,
    onClear: () -> Unit,
) {
    var search by rememberSaveable { mutableStateOf(state.preferences.filter.query.isNotBlank()) }
    var menu by remember { mutableStateOf(false) }
    var advanced by rememberSaveable { mutableStateOf(false) }
    var density by rememberSaveable { mutableStateOf(false) }
    var clear by remember { mutableStateOf(false) }
    var detail by remember { mutableStateOf<LiveEvent?>(null) }
    val filter = state.preferences.filter
    val clipboard = LocalClipboardManager.current
    Column(Modifier.fillMaxSize().padding(horizontal = 16.dp).testTag("events-screen")) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Events", Modifier.weight(1f), style = MaterialTheme.typography.headlineMedium)
            IconButton(onClick = { search = !search }) {
                Icon(Icons.Outlined.Search, "Search events")
            }
            Box {
                IconButton(onClick = { menu = true }) {
                    Icon(Icons.Outlined.MoreVert, "More event options")
                }
                DropdownMenu(menu, { menu = false }) {
                    DropdownMenuItem(
                        text = { Text(if (state.paused) "Resume events" else "Pause events") },
                        onClick = {
                            menu = false
                            onPause(!state.paused)
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Display density") },
                        onClick = {
                            menu = false
                            density = true
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Advanced filters") },
                        onClick = {
                            menu = false
                            advanced = true
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Clear history") },
                        onClick = {
                            menu = false
                            clear = true
                        },
                    )
                }
            }
        }
        if (search)
            OutlinedTextField(
                filter.query,
                { onFilter(filter.copy(query = it)) },
                Modifier.fillMaxWidth(),
                label = { Text("Search user or message") },
                singleLine = true,
            )
        LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            item {
                FilterChip(
                    selected = filter.enabled.size == LiveEvent.Category.entries.size,
                    onClick = { onFilter(filter.selectAll()) },
                    label = { Text("All") },
                )
            }
            items(LiveEvent.Category.entries) { c ->
                FilterChip(
                    c in filter.enabled,
                    { onFilter(filter.copy(enabled = setOf(c))) },
                    label = { Text(c.name.lowercase().replaceFirstChar(Char::uppercase)) },
                    leadingIcon = { Icon(c.icon(), null, Modifier.size(16.dp)) },
                )
            }
        }
        if (state.paused)
            Text(
                "Timeline paused · rewards, actions and speech continue",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.primary,
            )
        LazyColumn(Modifier.weight(1f)) {
            if (state.events.isEmpty())
                item {
                    EmptyState("No matching events", "Connect from Home or adjust your filters")
                }
            items(state.events) { event ->
                EventRow(event, state.preferences.display) { detail = event }
                HorizontalDivider()
            }
        }
    }
    if (advanced)
        ModalBottomSheet(onDismissRequest = { advanced = false }) {
            Column(Modifier.padding(16.dp).navigationBarsPadding()) {
                SectionHeader("Advanced filters")
                Row {
                    TextButton(onClick = { onFilter(filter.selectAll()) }) { Text("Select all") }
                    TextButton(onClick = { onFilter(filter.clearSelection()) }) {
                        Text("Clear selection")
                    }
                }
                for (c in LiveEvent.Category.entries) SettingSwitch(
                    c.name.lowercase().replaceFirstChar(Char::uppercase),
                    c in filter.enabled,
                    { checked ->
                        onFilter(
                            filter.copy(
                                enabled = if (checked) filter.enabled + c else filter.enabled - c
                            )
                        )
                    },
                )
            }
        }
    if (density)
        ModalBottomSheet(onDismissRequest = { density = false }) {
            val d = state.preferences.display
            Column(Modifier.padding(16.dp).navigationBarsPadding()) {
                SectionHeader("Display density")
                SettingSwitch("Compact rows", d.compact, { onDisplay(d.copy(compact = it)) })
                SettingSwitch("Event icons", d.showBadge, { onDisplay(d.copy(showBadge = it)) })
                SettingSwitch("Timestamps", d.showTime, { onDisplay(d.copy(showTime = it)) })
                SettingSwitch(
                    "Single-line messages",
                    d.singleLine,
                    { onDisplay(d.copy(singleLine = it)) },
                )
            }
        }
    if (clear)
        AlertDialog(
            onDismissRequest = { clear = false },
            title = { Text("Clear event history?") },
            text = {
                Text("This clears the timeline, without changing rewards or session statistics.")
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        onClear()
                        clear = false
                    }
                ) {
                    Text("Clear")
                }
            },
            dismissButton = { TextButton(onClick = { clear = false }) { Text("Cancel") } },
        )
    detail?.let { event ->
        ModalBottomSheet(onDismissRequest = { detail = null }) {
            LazyColumn(contentPadding = PaddingValues(16.dp)) {
                item {
                    Text(
                        event.category.name.lowercase().replaceFirstChar(Char::uppercase),
                        style = MaterialTheme.typography.headlineMedium,
                    )
                    CreatorAvatar(event.avatarUrl, event.nickname)
                    Text(event.nickname.ifEmpty { event.user })
                    Text("@${event.user}")
                    Text(event.summary())
                    SettingItem("Received", eventTime(event.at))
                    SettingItem(
                        "Message ID",
                        runCatching { JSONObject(event.raw).optString("msg_id", "Not provided") }
                            .getOrDefault("Not provided"),
                    )
                    Button(onClick = { clipboard.setText(AnnotatedString(event.raw)) }) {
                        Text("Copy JSON")
                    }
                    Spacer(Modifier.height(24.dp))
                }
            }
        }
    }
}
