package dev.nglmercer.tiktools.feature.events

import androidx.lifecycle.viewModelScope
import dev.nglmercer.tiktools.app.AppContainer
import dev.nglmercer.tiktools.core.model.*
import dev.nglmercer.tiktools.data.preferences.UserPreferences
import dev.nglmercer.tiktools.feature.StudioViewModel
import kotlinx.coroutines.flow.*

data class EventsUiState(
    val events: List<LiveEvent> = emptyList(),
    val paused: Boolean = false,
    val preferences: UserPreferences = UserPreferences(),
)

class EventsViewModel(private val app: AppContainer) : StudioViewModel() {
    val state =
        combine(app.events.history, app.events.paused, app.preferences.state) { events, paused, p ->
                EventsUiState(events.filter(p.filter::matches), paused, p)
            }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), EventsUiState())

    fun filter(value: EventFilter) = task { app.preferences.setFilter(value) }

    fun display(value: EventDisplayConfig) = task { app.preferences.setDisplay(value) }

    fun pause(value: Boolean) {
        app.events.setPaused(value)
    }

    fun clear() {
        app.events.clearHistory()
    }
}
