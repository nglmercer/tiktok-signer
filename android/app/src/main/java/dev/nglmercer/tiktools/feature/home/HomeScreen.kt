package dev.nglmercer.tiktools.feature.home

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.*
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import dev.nglmercer.tiktools.core.model.Creator
import dev.nglmercer.tiktools.live.*
import dev.nglmercer.tiktools.tts.TtsEngine
import dev.nglmercer.tiktools.ui.components.*

sealed interface HomeAction {
    data class Connect(val username: String) : HomeAction

    data class SelectCreator(val creator: Creator) : HomeAction

    data object Disconnect : HomeAction

    data object Refresh : HomeAction

    data object Random : HomeAction

    data object Speech : HomeAction

    data object ToggleTts : HomeAction
}

@Composable
fun HomeScreen(
    state: HomeUiState,
    onAction: (HomeAction) -> Unit,
    error: String? = null,
    onDismiss: () -> Unit = {},
) {
    var username by rememberSaveable { mutableStateOf("") }
    LazyColumn(
        Modifier.fillMaxSize().testTag("home-screen"),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Text("TikTok LIVE Studio", style = MaterialTheme.typography.headlineLarge)
            LiveStatusBadge(state.connection)
        }
        item {
            ErrorBanner(error ?: (state.connection as? LiveSessionState.Failed)?.message, onDismiss)
        }
        val creator = state.connection.creator()
        if (creator != null) {
            item {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    CreatorAvatar(
                        creator.avatarUrl,
                        creator.nickname.ifEmpty { creator.uniqueId },
                        64.dp,
                    )
                    Column(Modifier.weight(1f)) {
                        Text("@${creator.uniqueId}", style = MaterialTheme.typography.titleLarge)
                        Text(
                            creator.title.ifBlank { creator.nickname },
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Text(
                            "${compact(if(state.stats.viewers>0) state.stats.viewers else creator.viewers)} viewers",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
            }
        }
        item {
            if (!state.connection.isActive) {
                OutlinedTextField(
                    username,
                    { username = it },
                    Modifier.fillMaxWidth(),
                    label = { Text("TikTok username") },
                    placeholder = { Text("@creator") },
                    singleLine = true,
                )
                Button(
                    onClick = { onAction(HomeAction.Connect(username)) },
                    enabled = username.isNotBlank(),
                    modifier =
                        Modifier.fillMaxWidth()
                            .padding(top = 8.dp)
                            .heightIn(min = 48.dp)
                            .testTag("connect-button"),
                ) {
                    Text("Connect")
                }
            } else
                OutlinedButton(
                    onClick = { onAction(HomeAction.Disconnect) },
                    modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
                ) {
                    Text(
                        if (state.connection is LiveSessionState.Connected) "Disconnect"
                        else "Cancel connection"
                    )
                }
        }
        if (state.connection.creator() != null || state.stats.events > 0) {
            item {
                HorizontalDivider()
                Row {
                    StatItem(compact(state.stats.events), "Events", Modifier.weight(1f))
                    StatItem(compact(state.stats.gifts), "Gifts", Modifier.weight(1f))
                    StatItem(compact(state.stats.likes), "Likes", Modifier.weight(1f))
                }
                Row {
                    StatItem(compact(state.stats.followers), "Followers", Modifier.weight(1f))
                    StatItem(
                        amount(state.stats.points),
                        state.preferences.rewards.currencyName,
                        Modifier.weight(2f),
                    )
                }
                HorizontalDivider()
            }
        }
        item {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(
                    Modifier.weight(1f)
                        .clickable { onAction(HomeAction.Speech) }
                        .padding(vertical = 12.dp)
                ) {
                    Text("Speech", style = MaterialTheme.typography.titleMedium)
                    Text(
                        when (state.preferences.engine) {
                            TtsEngine.OFF -> "Off"
                            TtsEngine.DEVICE -> "Device voice"
                            TtsEngine.SUPERTONIC -> "SuperTonic 3"
                        },
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                IconButton(onClick = { onAction(HomeAction.ToggleTts) }) {
                    Icon(
                        if (state.preferences.engine == TtsEngine.OFF) Icons.Outlined.VolumeOff
                        else Icons.Outlined.VolumeUp,
                        "Toggle speech",
                    )
                }
                IconButton(onClick = { onAction(HomeAction.Speech) }) {
                    Icon(Icons.Outlined.Tune, "Speech controls")
                }
            }
        }
        if (state.recent.isNotEmpty()) {
            item { SectionHeader("Recent activity") }
            items(state.recent) { EventRow(it, state.preferences.display) }
        }
        if (!state.connection.isActive) {
            if (state.preferences.recentCreators.isNotEmpty()) {
                item { SectionHeader("Recently watched") }
                items(state.preferences.recentCreators) { id ->
                    SettingItem("@$id", "Connect to check current LIVE status") {
                        onAction(HomeAction.Connect(id))
                    }
                }
            }
            item {
                SectionHeader("Live now") {
                    Row {
                        IconButton(
                            onClick = { onAction(HomeAction.Random) },
                            enabled = !state.feed.loading,
                        ) {
                            Icon(Icons.Outlined.Shuffle, "Connect to a random creator")
                        }
                        IconButton(
                            onClick = { onAction(HomeAction.Refresh) },
                            enabled = !state.feed.loading,
                        ) {
                            Icon(Icons.Outlined.Refresh, "Refresh live creators")
                        }
                    }
                }
            }
            if (state.feed.loading) item { LinearProgressIndicator(Modifier.fillMaxWidth()) }
            if (state.feed.error != null) item { ErrorBanner(state.feed.error) }
            if (!state.feed.loading && state.feed.rooms.isEmpty())
                item { EmptyState("No live creators to show", "Try refreshing the feed") }
            items(state.feed.rooms, key = { it.uniqueId }) { room ->
                Row(
                    Modifier.fillMaxWidth()
                        .clickable { onAction(HomeAction.SelectCreator(room)) }
                        .padding(vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    if (room.coverUrl.isNotBlank())
                        AsyncImage(
                            room.coverUrl,
                            null,
                            Modifier.size(72.dp),
                            contentScale = androidx.compose.ui.layout.ContentScale.Crop,
                        )
                    else CreatorAvatar(room.avatarUrl, room.uniqueId, 56.dp)
                    Column(Modifier.weight(1f)) {
                        Text("@${room.uniqueId}", style = MaterialTheme.typography.titleMedium)
                        Text(
                            room.title,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Text(
                            "${compact(room.viewers)} viewers",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    Icon(Icons.Outlined.ChevronRight, null)
                }
                HorizontalDivider()
            }
        }
    }
}
