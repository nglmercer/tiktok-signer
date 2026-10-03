package dev.nglmercer.tiktools.app

import android.content.Context
import dev.nglmercer.tiktools.core.diagnostics.Logger
import dev.nglmercer.tiktools.core.nativebridge.RustSigner
import dev.nglmercer.tiktools.data.actions.ActionsRepository
import dev.nglmercer.tiktools.data.database.DatabaseFactory
import dev.nglmercer.tiktools.data.events.EventsRepository
import dev.nglmercer.tiktools.data.preferences.PreferencesRepository
import dev.nglmercer.tiktools.data.rewards.PointsRepository
import dev.nglmercer.tiktools.live.*
import dev.nglmercer.tiktools.tts.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

/** One container per application, independent of navigation and Activity recreation. */
class AppContainer(private val context: Context) {
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    val preferences = PreferencesRepository(context, scope)
    val database = DatabaseFactory.open(context)
    val points = PointsRepository(database, preferences, scope)
    val actions = ActionsRepository(database, scope)
    val events = EventsRepository()
    val tts = TtsRepository(context, preferences, scope)
    val discovery = DiscoveryRepository()
    val live =
        LiveSessionManager(
            discovery,
            NativeLiveTransport(context),
            object : SessionKeepAlive {
                override fun start(room: String, speech: Boolean) {
                    LiveService.start(context, room, speech)
                }

                override fun refresh(room: String, speech: Boolean) {
                    if (LiveService.running) LiveService.refresh(context, room, speech)
                    else LiveService.start(context, room, speech)
                }

                override fun stop() {
                    LiveService.stop(context)
                }
            },
            scope,
            { preferences.state.value.reconnect },
            { preferences.state.value.backgroundConnection },
            { preferences.state.value.engine != TtsEngine.OFF },
        )

    init {
        scope.launch(Dispatchers.IO) {
            Logger.append(
                "APP",
                "Native: ${runCatching { RustSigner.version() }.getOrElse { "Unavailable: ${it.message}" }}",
            )
        }
        scope.launch {
            live.events.collect { event ->
                events.append(event)
                actions.onEvent(event)
                tts.onEvent(event)
                // One consumer preserves reward order; Room transactions prevent lost adjustments.
                try {
                    points.award(event)?.let(events::awarded)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    Logger.append("REWARDS", e.message.orEmpty())
                }
            }
        }
        scope.launch { LiveService.disconnectRequests.collect { live.disconnect("Notification") } }
        scope.launch {
            preferences.values
                .map { it.engine to it.backgroundConnection }
                .distinctUntilChanged()
                .collect { live.refreshNotification() }
        }
    }

    suspend fun close() {
        live.close()
        tts.close()
        scope.cancel()
        withContext(Dispatchers.IO) { database.close() }
    }
}
