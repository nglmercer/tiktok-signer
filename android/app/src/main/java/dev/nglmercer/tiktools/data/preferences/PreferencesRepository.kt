package dev.nglmercer.tiktools.data.preferences

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.SharedPreferencesMigration
import androidx.datastore.preferences.core.*
import androidx.datastore.preferences.preferencesDataStore
import dev.nglmercer.tiktools.core.model.*
import dev.nglmercer.tiktools.data.rewards.PointsConfig
import dev.nglmercer.tiktools.tts.TtsEngine
import java.io.IOException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.*

private val Context.studioPreferences by
    preferencesDataStore(
        name = "studio",
        produceMigrations = { context ->
            listOf("tiktools_tts", "tiktools_points", "tiktools_events_display").map {
                SharedPreferencesMigration(context, it)
            }
        },
    )

class PreferencesRepository(
    context: Context,
    scope: CoroutineScope,
    private val store: DataStore<Preferences> = context.applicationContext.studioPreferences,
) {
    val values =
        store.data
            .catch { if (it is IOException) emit(emptyPreferences()) else throw it }
            .map(::decode)
    val state = values.stateIn(scope, SharingStarted.Eagerly, UserPreferences())

    suspend fun setEngine(engine: TtsEngine) =
        store.edit { it[stringPreferencesKey("engine")] = engine.name }

    suspend fun setSpeech(p: SpeechPreferences) =
        store.edit {
            it[booleanPreferencesKey("speech_chat")] = p.chat
            it[booleanPreferencesKey("speech_gifts")] = p.gifts
            it[booleanPreferencesKey("speech_follows")] = p.follows
            it[booleanPreferencesKey("speech_shares")] = p.shares
            it[booleanPreferencesKey("joins")] = p.joins
        }

    suspend fun setRewards(p: PointsConfig) =
        store.edit { it[stringPreferencesKey("config")] = PointsConfig.encode(p.normalize()) }

    suspend fun setDisplay(p: EventDisplayConfig) =
        store.edit {
            it[booleanPreferencesKey("compact")] = p.compact
            it[booleanPreferencesKey("badge")] = p.showBadge
            it[booleanPreferencesKey("time")] = p.showTime
            it[booleanPreferencesKey("single_line")] = p.singleLine
        }

    suspend fun setFilter(filter: EventFilter) =
        store.edit {
            it[stringSetPreferencesKey("filter_categories")] =
                filter.enabled.map { c -> c.name }.toSet()
            it[stringPreferencesKey("filter_query")] = filter.query
        }

    suspend fun setReconnect(enabled: Boolean) =
        store.edit { it[booleanPreferencesKey("reconnect")] = enabled }

    suspend fun setBackground(enabled: Boolean) =
        store.edit { it[booleanPreferencesKey("background")] = enabled }

    suspend fun setTheme(mode: ThemeMode) =
        store.edit { it[stringPreferencesKey("theme")] = mode.name }

    suspend fun rememberCreator(id: String) =
        store.edit {
            val previous =
                it[stringPreferencesKey("recent_creators")]
                    .orEmpty()
                    .split(",")
                    .filter(String::isNotBlank)
            it[stringPreferencesKey("recent_creators")] =
                (listOf(id) + previous.filter { name -> name != id }).take(8).joinToString(",")
        }

    companion object {
        internal fun decode(p: Preferences): UserPreferences {
            fun bool(name: String, fallback: Boolean) = p[booleanPreferencesKey(name)] ?: fallback
            val engine =
                p[stringPreferencesKey("engine")]?.let {
                    runCatching { TtsEngine.valueOf(it) }.getOrNull()
                } ?: if (bool("enabled", false)) TtsEngine.DEVICE else TtsEngine.OFF
            return UserPreferences(
                engine = engine,
                speech =
                    SpeechPreferences(
                        bool("speech_chat", true),
                        bool("speech_gifts", true),
                        bool("speech_follows", true),
                        bool("speech_shares", true),
                        bool("joins", false),
                    ),
                rewards =
                    p[stringPreferencesKey("config")]?.let {
                        runCatching { PointsConfig.decode(it).normalize() }.getOrNull()
                    } ?: PointsConfig(),
                display =
                    EventDisplayConfig(
                        bool("compact", true),
                        bool("badge", true),
                        bool("time", true),
                        bool("single_line", true),
                    ),
                filter =
                    EventFilter(
                        p[stringSetPreferencesKey("filter_categories")]
                            ?.mapNotNull {
                                runCatching { LiveEvent.Category.valueOf(it) }.getOrNull()
                            }
                            ?.toSet() ?: LiveEvent.Category.entries.toSet(),
                        p[stringPreferencesKey("filter_query")].orEmpty(),
                    ),
                reconnect = bool("reconnect", true),
                backgroundConnection = bool("background", true),
                theme =
                    p[stringPreferencesKey("theme")]?.let {
                        runCatching { ThemeMode.valueOf(it) }.getOrNull()
                    } ?: ThemeMode.SYSTEM,
                recentCreators =
                    p[stringPreferencesKey("recent_creators")]
                        .orEmpty()
                        .split(",")
                        .filter(String::isNotBlank),
            )
        }
    }
}
