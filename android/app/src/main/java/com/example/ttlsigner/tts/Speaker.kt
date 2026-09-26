package com.example.ttlsigner.tts

import android.content.Context
import android.speech.tts.TextToSpeech
import com.example.ttlsigner.events.LiveEvent
import java.util.Locale

/**
 * What the reader speaks for one event. Pure: chat reads the comment, gifts
 * and follows announce themselves, noisy categories (likes, room stats) stay
 * silent by default. Empty string means "say nothing".
 */
object SpeechText {
    fun forEvent(event: LiveEvent, speakJoins: Boolean = false): String {
        val who = event.nickname.ifEmpty { event.user }
        return when (event.category) {
            LiveEvent.Category.CHAT ->
                if (event.text.isBlank()) "" else "$who says: ${event.text.take(200)}"
            LiveEvent.Category.GIFT ->
                "$who sent ${event.text.ifEmpty { "a gift" }}"
            LiveEvent.Category.FOLLOW -> "$who followed"
            LiveEvent.Category.SHARE -> "$who shared the live"
            LiveEvent.Category.JOIN ->
                if (speakJoins && who.isNotEmpty()) "$who joined" else ""
            LiveEvent.Category.LIKE,
            LiveEvent.Category.MEMBER,
            LiveEvent.Category.ROOM,
            LiveEvent.Category.UNKNOWN -> ""
        }
    }
}

/**
 * TTS frontier: every component that may speak takes a [Speaker], so the
 * future full TTS feature (voices, per-event toggles, queue) plugs in without
 * touching the event pipeline. Today the app ships [NoopSpeaker] by default
 * and [AndroidSpeaker] behind the Setup toggle.
 */
interface Speaker {
    val enabled: Boolean
    fun speak(text: String)
    fun shutdown()
}

/** The selectable speech engines: off, the platform voice, SuperTonic 3. */
enum class TtsEngine {
    OFF,
    DEVICE,
    SUPERTONIC,
}

/** Silent speaker: keeps the pipeline wired while TTS is off. */
class NoopSpeaker : Speaker {
    override val enabled: Boolean = false
    override fun speak(text: String) = Unit
    override fun shutdown() = Unit
}

/** Thin `TextToSpeech` wrapper: lazy init, teardown on demand, no queue. */
class AndroidSpeaker(context: Context) : Speaker {
    private var tts: TextToSpeech? = null

    override val enabled: Boolean = true

    init {
        tts = TextToSpeech(context.applicationContext) { status ->
            if (status == TextToSpeech.SUCCESS) {
                tts?.language = Locale.getDefault()
            }
        }
    }

    override fun speak(text: String) {
        if (text.isBlank()) return
        tts?.speak(text, TextToSpeech.QUEUE_ADD, null, "tiktools-${System.currentTimeMillis()}")
    }

    override fun shutdown() {
        tts?.shutdown()
        tts = null
    }

    companion object {
        private const val PREFS = "tiktools_tts"
        private const val KEY_ENABLED = "enabled"
        private const val KEY_ENGINE = "engine"
        private const val KEY_JOINS = "joins"

        fun isEnabled(context: Context): Boolean =
            engine(context) != TtsEngine.OFF

        fun setEnabled(context: Context, enabled: Boolean) {
            setEngine(context, if (enabled) TtsEngine.DEVICE else TtsEngine.OFF)
        }

        /**
         * The selected engine. Upgrades the old boolean toggle once: an
         * enabled install becomes [TtsEngine.DEVICE].
         */
        fun engine(context: Context): TtsEngine {
            val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val stored = prefs.getString(KEY_ENGINE, null)
            if (stored != null) {
                return runCatching { TtsEngine.valueOf(stored) }.getOrDefault(TtsEngine.OFF)
            }
            val upgraded = if (prefs.getBoolean(KEY_ENABLED, false)) TtsEngine.DEVICE else TtsEngine.OFF
            prefs.edit().putString(KEY_ENGINE, upgraded.name).apply()
            return upgraded
        }

        fun setEngine(context: Context, engine: TtsEngine) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .edit().putString(KEY_ENGINE, engine.name).apply()
        }

        fun speakJoins(context: Context): Boolean =
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .getBoolean(KEY_JOINS, false)

        fun setSpeakJoins(context: Context, speak: Boolean) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .edit().putBoolean(KEY_JOINS, speak).apply()
        }
    }
}
