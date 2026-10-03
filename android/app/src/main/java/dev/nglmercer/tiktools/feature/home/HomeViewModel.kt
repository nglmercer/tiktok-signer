package dev.nglmercer.tiktools.feature.home

import androidx.lifecycle.viewModelScope
import dev.nglmercer.tiktools.app.AppContainer
import dev.nglmercer.tiktools.core.model.*
import dev.nglmercer.tiktools.data.events.LiveStats
import dev.nglmercer.tiktools.data.preferences.UserPreferences
import dev.nglmercer.tiktools.feature.StudioViewModel
import dev.nglmercer.tiktools.live.*
import kotlinx.coroutines.flow.*

data class FeedState(
    val rooms: List<Creator> = emptyList(),
    val loading: Boolean = false,
    val error: String? = null,
)

data class HomeUiState(
    val connection: LiveSessionState = LiveSessionState.Disconnected,
    val stats: LiveStats = LiveStats(),
    val recent: List<LiveEvent> = emptyList(),
    val preferences: UserPreferences = UserPreferences(),
    val feed: FeedState = FeedState(),
)

class HomeViewModel(private val app: AppContainer) : StudioViewModel() {
    private val feed = MutableStateFlow(FeedState())
    val state =
        combine(
                app.live.state,
                app.events.stats,
                app.events.history,
                app.preferences.state,
                feed,
            ) { session, stats, events, prefs, rooms ->
                HomeUiState(session, stats, events.take(8), prefs, rooms)
            }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), HomeUiState())

    init {
        refreshFeed()
    }

    fun refreshFeed() = task {
        if (feed.value.loading) return@task
        feed.value = feed.value.copy(loading = true, error = null)
        try {
            feed.value = FeedState(app.discovery.feed())
        } catch (e: Exception) {
            feed.value = feed.value.copy(loading = false, error = e.message)
            throw e
        }
    }

    fun connect(username: String, creator: Creator? = null) = task {
        require(username.trim().isNotBlank()) { "Enter a username" }
        app.preferences.values.first() // Wait for migration before selecting session policies.
        app.events.beginSession()
        app.live.connect(username, creator)
        if (!username.removePrefix("@").startsWith("room:"))
            app.preferences.rememberCreator(username.trim().removePrefix("@"))
    }

    fun random() = task {
        if (feed.value.rooms.isEmpty()) feed.value = FeedState(app.discovery.feed())
        val creator = feed.value.rooms.randomOrNull() ?: error("No creators are LIVE right now")
        connect(creator.uniqueId, creator)
    }

    fun disconnect() {
        app.live.disconnect()
    }

    fun toggleSpeech() = task {
        app.tts.setEngine(
            if (app.preferences.state.value.engine == dev.nglmercer.tiktools.tts.TtsEngine.OFF)
                dev.nglmercer.tiktools.tts.TtsEngine.DEVICE
            else dev.nglmercer.tiktools.tts.TtsEngine.OFF
        )
    }
}
