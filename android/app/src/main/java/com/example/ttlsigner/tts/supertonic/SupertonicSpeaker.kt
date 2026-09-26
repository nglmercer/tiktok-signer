package com.example.ttlsigner.tts.supertonic

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.util.Log
import com.example.ttlsigner.Logger
import com.example.ttlsigner.tts.Speaker
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * A [Speaker] that synthesizes with the on-device SuperTonic v3 engine and
 * plays the result through [AudioTrack]. The engine loads lazily on first
 * speech (sessions + style take seconds and ~1 GB RAM); until the model files
 * are downloaded, speech requests are dropped with a log line.
 *
 * Live-appropriate pacing: while an utterance is synthesizing or playing, new
 * requests are dropped instead of queued, so speech never lags the stream.
 */
class SupertonicSpeaker(context: Context) : Speaker {

    sealed interface State {
        data object Idle : State
        data object Loading : State
        data object Ready : State
        data class Speaking(val text: String) : State
        data class Error(val detail: String) : State
    }

    override val enabled: Boolean = true

    private val app = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val busy = AtomicBoolean(false)

    private val _state = MutableStateFlow<State>(State.Idle)
    val state: StateFlow<State> = _state.asStateFlow()

    private var engine: SupertonicEngine? = null
    private var style: SupertonicStyle? = null
    private val engineLock = Any()

    override fun speak(text: String) {
        if (text.isBlank()) return
        if (!busy.compareAndSet(false, true)) return
        _state.value = State.Speaking(text.take(80))
        scope.launch {
            try {
                val loaded = ensureEngine()
                if (loaded == null) {
                    Log.w(Logger.TAG, "supertonic: model not ready, speech dropped")
                    return@launch
                }
                val (eng, sty) = loaded
                val result = eng.synthesize(text.take(300), sty)
                play(result.wav, result.sampleRate)
                _state.value = State.Ready
            } catch (e: Exception) {
                Log.w(Logger.TAG, "supertonic speak failed: ${e.message}")
                _state.value = State.Error("${e.javaClass.simpleName}: ${e.message}")
            } finally {
                busy.set(false)
                if (_state.value is State.Speaking) _state.value = State.Ready
            }
        }
    }

    /** Preload the engine (the Setup test button); null when files are missing. */
    suspend fun warmup(): Boolean = withContext(Dispatchers.Default) {
        ensureEngine() != null
    }

    private suspend fun ensureEngine(): Pair<SupertonicEngine, SupertonicStyle>? =
        withContext(Dispatchers.IO) {
            synchronized(engineLock) {
                val ready = engine?.let { eng -> style?.let { sty -> eng to sty } }
                if (ready != null) return@withContext ready
            }
            if (SupertonicModels.status(app) !is SupertonicModels.Status.Ready) {
                return@withContext null
            }
            _state.value = State.Loading
            try {
                val eng = SupertonicEngine(SupertonicModels.dir(app))
                val sty = loadSupertonicStyle(listOf(SupertonicModels.styleFile(app)))
                synchronized(engineLock) {
                    engine = eng
                    style = sty
                }
                _state.value = State.Ready
                eng to sty
            } catch (e: Exception) {
                _state.value = State.Error("${e.javaClass.simpleName}: ${e.message}")
                throw e
            }
        }

    private fun play(wav: FloatArray, sampleRate: Int) {
        if (wav.isEmpty()) return
        val pcm = ByteArray(wav.size * 2)
        for (i in wav.indices) {
            val s = (wav[i].coerceIn(-1f, 1f) * 32767f).toInt()
            pcm[i * 2] = (s and 0xFF).toByte()
            pcm[i * 2 + 1] = ((s shr 8) and 0xFF).toByte()
        }
        val track = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .setSampleRate(sampleRate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .build(),
            )
            .setBufferSizeInBytes(pcm.size)
            .setTransferMode(AudioTrack.MODE_STATIC)
            .build()
        try {
            track.write(pcm, 0, pcm.size)
            track.play()
            // Static mode has no completion callback; poll the head position.
            val frames = wav.size
            var guard = 0
            while (track.playbackHeadPosition < frames && guard < 600) {
                Thread.sleep(100)
                guard++
            }
        } finally {
            track.release()
        }
    }

    override fun shutdown() {
        scope.cancel()
        synchronized(engineLock) {
            try {
                style?.close()
            } catch (e: Exception) {
                // Closing; nothing to do.
            }
            try {
                engine?.close()
            } catch (e: Exception) {
                // Closing; nothing to do.
            }
            style = null
            engine = null
        }
        busy.set(false)
    }
}
