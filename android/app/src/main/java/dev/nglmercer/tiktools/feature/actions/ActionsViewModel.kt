package dev.nglmercer.tiktools.feature.actions

import dev.nglmercer.tiktools.app.AppContainer
import dev.nglmercer.tiktools.core.model.LiveEvent
import dev.nglmercer.tiktools.data.actions.EventAction
import dev.nglmercer.tiktools.feature.StudioViewModel

class ActionsViewModel(private val app: AppContainer) : StudioViewModel() {
    val state = app.actions.state

    fun save(value: EventAction, onSaved: () -> Unit) = task {
        app.actions.save(value)
        onSaved()
    }

    fun delete(id: Long) = task { app.actions.delete(id) }

    fun enable(value: EventAction, enabled: Boolean) = task {
        app.actions.setEnabled(value, enabled)
    }

    fun test(value: EventAction) = task {
        val sample =
            app.events.history.value.firstOrNull { it.category in value.triggers }
                ?: LiveEvent(
                    value.triggers.firstOrNull() ?: LiveEvent.Category.CHAT,
                    "sample",
                    "Sample",
                    value.giftName.ifEmpty { "Test event" },
                    1,
                    1,
                )
        app.actions.test(value, sample)
    }
}
