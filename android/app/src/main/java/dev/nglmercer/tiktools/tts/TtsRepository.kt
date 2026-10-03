package dev.nglmercer.tiktools.tts

import android.content.Context
import dev.nglmercer.tiktools.core.diagnostics.Logger
import dev.nglmercer.tiktools.core.model.LiveEvent
import dev.nglmercer.tiktools.data.preferences.*
import dev.nglmercer.tiktools.tts.supertonic.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

class TtsRepository(
    private val context: Context,
    private val preferences: PreferencesRepository,
    private val scope: CoroutineScope,
) {
    data class ModelState(
        val ready: Boolean = false,
        val bytes: Long = 0,
        val progress: Float? = null,
        val detail: String = "",
        val downloading: Boolean = false,
        val error: String? = null,
    )

    private val _models = MutableStateFlow(ModelState())
    val models = _models.asStateFlow()
    private val _voice = MutableStateFlow<SupertonicSpeaker.State>(SupertonicSpeaker.State.Idle)
    val voice = _voice.asStateFlow()
    val lastSpoken = TtsController.lastSpoken
    private var speaker: Speaker = NoopSpeaker()
    private var engine = TtsEngine.OFF
    private var watcher: Job? = null
    private var download: Job? = null
    private val engineLock = kotlinx.coroutines.sync.Mutex()

    init {
        refreshModels()
        scope.launch {
            preferences.values
                .map { it.engine }
                .distinctUntilChanged()
                .collect { selected -> rebuild(selected) }
        }
    }

    private suspend fun rebuild(selected: TtsEngine) {
        engineLock.lock()
        try {
            watcher?.cancel()
            TtsController.attach(NoopSpeaker(), TtsEngine.OFF)
            val previous = speaker
            speaker = NoopSpeaker()
            withContext(Dispatchers.IO) { previous.shutdown() }
            engine = selected
            speaker =
                when (selected) {
                    TtsEngine.OFF -> NoopSpeaker()
                    TtsEngine.DEVICE -> AndroidSpeaker(context)
                    TtsEngine.SUPERTONIC -> SupertonicSpeaker(context)
                }
            (speaker as? SupertonicSpeaker)?.let { current ->
                watcher = scope.launch { current.state.collect { _voice.value = it } }
            }
            TtsController.attach(speaker, selected)
            Logger.append("TTS", "Engine ${selected.name.lowercase()}")
        } finally {
            engineLock.unlock()
        }
    }

    suspend fun setEngine(value: TtsEngine) {
        preferences.setEngine(value)
    }

    suspend fun setSpeech(value: SpeechPreferences) {
        preferences.setSpeech(value)
    }

    fun onEvent(event: LiveEvent) {
        val p = preferences.state.value.speech
        val enabled =
            when (event.category) {
                LiveEvent.Category.CHAT -> p.chat
                LiveEvent.Category.GIFT -> p.gifts
                LiveEvent.Category.FOLLOW -> p.follows
                LiveEvent.Category.SHARE -> p.shares
                LiveEvent.Category.JOIN -> p.joins
                else -> false
            }
        if (enabled) TtsController.speak(SpeechText.forEvent(event, p.joins))
    }

    fun repeat() {
        TtsController.repeatLast()
    }

    fun skip() {
        TtsController.skip()
    }

    fun testVoice() {
        TtsController.speak("TikTools Studio ready.")
    }

    fun refreshModels() {
        scope.launch(Dispatchers.IO) {
            val ready = SupertonicModels.status(context) is SupertonicModels.Status.Ready
            val bytes =
                SupertonicModels.dir(context)
                    .listFiles()
                    .orEmpty()
                    .filter { !it.name.endsWith(".part") }
                    .sumOf { it.length() }
            _models.value =
                _models.value.copy(
                    ready = ready,
                    bytes = bytes,
                    detail = if (ready) "7 files · voice F1" else "Download the offline voice model",
                )
        }
    }

    fun downloadModels() {
        if (download?.isActive == true) return
        _models.value = _models.value.copy(downloading = true, error = null, progress = 0f)
        download =
            scope.launch {
                try {
                    SupertonicModels.download(context) { p ->
                        _models.value =
                            _models.value.copy(
                                progress = p.overall(),
                                detail = "${p.fileName} · ${p.fileIndex+1}/${p.fileCount}",
                            )
                    }
                } catch (e: CancellationException) {
                    _models.value = _models.value.copy(detail = "Download canceled")
                    throw e
                } catch (e: Exception) {
                    _models.value = _models.value.copy(error = e.message ?: "Download failed")
                } finally {
                    _models.value = _models.value.copy(downloading = false, progress = null)
                    refreshModels()
                }
            }
    }

    fun cancelDownload() {
        download?.cancel()
    }

    suspend fun deleteModels() {
        check(download?.isActive != true) { "Cancel the download before deleting the model" }
        preferences.setEngine(TtsEngine.OFF)
        rebuild(TtsEngine.OFF)
        withContext(Dispatchers.IO) {
            check(
                !SupertonicModels.dir(context).exists() ||
                    SupertonicModels.dir(context).deleteRecursively()
            ) {
                "Could not delete all model files"
            }
        }
        refreshModels()
    }

    suspend fun close() {
        download?.cancelAndJoin()
        watcher?.cancelAndJoin()
        TtsController.attach(NoopSpeaker(), TtsEngine.OFF)
        withContext(Dispatchers.IO) { speaker.shutdown() }
    }
}
