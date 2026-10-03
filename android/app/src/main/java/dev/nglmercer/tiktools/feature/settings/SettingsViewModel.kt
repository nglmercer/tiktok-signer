package dev.nglmercer.tiktools.feature.settings

import android.content.Context
import dev.nglmercer.tiktools.app.AppContainer
import dev.nglmercer.tiktools.core.diagnostics.Logger
import dev.nglmercer.tiktools.core.nativebridge.RustSigner
import dev.nglmercer.tiktools.core.network.BundleFetch
import dev.nglmercer.tiktools.data.preferences.*
import dev.nglmercer.tiktools.feature.StudioViewModel
import dev.nglmercer.tiktools.tts.TtsEngine
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

data class Diagnostics(
    val native: String = "Loading…",
    val bundle: String = "",
    val guest: String = "",
    val logs: String = "",
)

class SettingsViewModel(private val app: AppContainer, private val context: Context) :
    StudioViewModel() {
    val preferences = app.preferences.state
    val models = app.tts.models
    val voice = app.tts.voice
    val lastSpoken = app.tts.lastSpoken
    private val _diagnostics = MutableStateFlow(Diagnostics())
    val diagnostics = _diagnostics.asStateFlow()
    private val listener =
        object : Logger.Listener {
            override fun onLine(line: String) {
                _diagnostics.update { it.copy(logs = Logger.snapshot()) }
            }

            override fun onCleared() {
                _diagnostics.update { it.copy(logs = "") }
            }
        }

    init {
        Logger.addListener(listener)
        refreshDiagnostics()
    }

    override fun onCleared() {
        Logger.removeListener(listener)
    }

    fun engine(value: TtsEngine) = task { app.tts.setEngine(value) }

    fun speech(value: SpeechPreferences) = task { app.tts.setSpeech(value) }

    fun reconnect(value: Boolean) = task { app.preferences.setReconnect(value) }

    fun background(value: Boolean) = task { app.preferences.setBackground(value) }

    fun theme(value: ThemeMode) = task { app.preferences.setTheme(value) }

    fun repeat() {
        app.tts.repeat()
    }

    fun skip() {
        app.tts.skip()
    }

    fun testVoice() {
        app.tts.testVoice()
    }

    fun download() {
        app.tts.downloadModels()
    }

    fun cancelDownload() {
        app.tts.cancelDownload()
    }

    fun deleteModel() = task { app.tts.deleteModels() }

    fun resetSession() = task {
        app.discovery.resetGuest()
        refreshDiagnostics()
    }

    fun redownloadBundle() = task {
        withContext(Dispatchers.IO) { BundleFetch.refresh(context.cacheDir) }
        refreshDiagnostics()
    }

    fun clearBundle() = task {
        withContext(Dispatchers.IO) {
            val file = BundleFetch.cacheFile(context.cacheDir)
            check(!file.exists() || file.delete()) { "Could not remove cached bundle" }
        }
        refreshDiagnostics()
    }

    fun clearLogs() {
        Logger.clear()
    }

    fun refreshDiagnostics() = task {
        val info =
            withContext(Dispatchers.IO) {
                val cache = BundleFetch.describeCache(context.cacheDir)
                Diagnostics(
                    native =
                        runCatching { RustSigner.version() }
                            .getOrElse { "Unavailable: ${it.message}" },
                    bundle =
                        "${BundleFetch.BUNDLE_VERSION} · ${cache?.sizeBytes ?: 0} bytes · ${if(cache?.shaOk==true) "SHA256 verified" else "Not cached"}",
                    guest =
                        app.discovery.cookieNames().joinToString().ifEmpty { "No guest session" },
                    logs = Logger.snapshot(),
                )
            }
        _diagnostics.value = info
    }

    fun exportText(): String =
        "TikTools Studio 1.2\n${app.live.state.value}\nNative: ${diagnostics.value.native}\nBundle: ${diagnostics.value.bundle}\nGuest cookie names: ${diagnostics.value.guest}\n\n${diagnostics.value.logs}"
}
