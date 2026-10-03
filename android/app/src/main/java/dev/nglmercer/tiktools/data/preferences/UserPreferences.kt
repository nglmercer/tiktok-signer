package dev.nglmercer.tiktools.data.preferences

import dev.nglmercer.tiktools.core.model.EventDisplayConfig
import dev.nglmercer.tiktools.core.model.EventFilter
import dev.nglmercer.tiktools.data.rewards.PointsConfig
import dev.nglmercer.tiktools.tts.TtsEngine

enum class ThemeMode {
    SYSTEM,
    LIGHT,
    DARK,
}

data class SpeechPreferences(
    val chat: Boolean = true,
    val gifts: Boolean = true,
    val follows: Boolean = true,
    val shares: Boolean = true,
    val joins: Boolean = false,
)

data class UserPreferences(
    val engine: TtsEngine = TtsEngine.OFF,
    val speech: SpeechPreferences = SpeechPreferences(),
    val rewards: PointsConfig = PointsConfig(),
    val display: EventDisplayConfig = EventDisplayConfig(),
    val filter: EventFilter = EventFilter(),
    val reconnect: Boolean = true,
    val backgroundConnection: Boolean = true,
    val theme: ThemeMode = ThemeMode.SYSTEM,
    val recentCreators: List<String> = emptyList(),
)
