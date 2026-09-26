package com.example.ttlsigner.tts

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * App-scoped TTS front desk: the one place that knows the current [Speaker],
 * whether speech is on, and what was last spoken.
 *
 * The [SessionViewModel][com.example.ttlsigner.SessionViewModel] attaches its
 * speaker here on every engine switch and routes every utterance through
 * [speak], so the repeat control and the background notification always act on
 * the live speaker even when no fragment is observing.
 *
 * Pure Kotlin (no `Context`): unit-testable on the JVM with a fake [Speaker].
 */
object TtsController {

    private val lock = Any()

    private var speaker: Speaker = NoopSpeaker()
    private var engine: TtsEngine = TtsEngine.OFF

    private val _lastSpoken = MutableStateFlow<String?>(null)
    /** The last non-blank line handed to [speak], or null when none yet. */
    val lastSpoken: StateFlow<String?> = _lastSpoken.asStateFlow()

    /** Controls (repeat / skip) show only while a speech engine is selected. */
    fun controlsVisible(engine: TtsEngine): Boolean = engine != TtsEngine.OFF

    /** True while the attached engine speaks. */
    val enabled: Boolean
        get() = synchronized(lock) { engine != TtsEngine.OFF && speaker.enabled }

    /**
     * Swap the live speaker. Called by the session on every engine switch;
     * clears the remembered line so repeat never replays another engine's
     * voice.
     */
    fun attach(next: Speaker, nextEngine: TtsEngine) {
        synchronized(lock) {
            speaker = next
            engine = nextEngine
        }
        _lastSpoken.value = null
    }

    /** Speak [text] through the attached speaker; remembers non-blank lines. */
    fun speak(text: String) {
        val current = synchronized(lock) { speaker to engine }
        if (current.second == TtsEngine.OFF || !current.first.enabled) return
        if (text.isBlank()) return
        _lastSpoken.value = text
        current.first.speak(text)
    }

    /**
     * Repeat the last spoken line, stopping the current utterance first so a
     * busy engine replays instead of dropping. Returns false when there is
     * nothing to repeat or speech is off.
     */
    fun repeatLast(): Boolean {
        val current = synchronized(lock) { speaker to engine }
        if (current.second == TtsEngine.OFF || !current.first.enabled) return false
        val last = _lastSpoken.value
        if (last.isNullOrBlank()) return false
        current.first.stop()
        current.first.speak(last)
        return true
    }

    /** Skip the current utterance, if any. */
    fun skip() {
        synchronized(lock) { speaker }.stop()
    }

    /** Detach back to silence. Test-only; the session owns real shutdown. */
    fun resetForTest() {
        synchronized(lock) {
            speaker = NoopSpeaker()
            engine = TtsEngine.OFF
        }
        _lastSpoken.value = null
    }
}
