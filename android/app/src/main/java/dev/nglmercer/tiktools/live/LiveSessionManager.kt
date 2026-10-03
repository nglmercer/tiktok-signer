package dev.nglmercer.tiktools.live

import dev.nglmercer.tiktools.core.diagnostics.Logger
import dev.nglmercer.tiktools.core.model.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/** Application-scoped LIVE ownership; no rewards, reader filters, feed UI or speech preferences. */
class LiveSessionManager(
    private val discovery: DiscoveryGateway,
    private val transport: LiveTransport,
    private val keepAlive: SessionKeepAlive,
    private val scope: CoroutineScope,
    private val reconnectEnabled: () -> Boolean = { true },
    private val backgroundEnabled: () -> Boolean = { true },
    private val speechEnabled: () -> Boolean = { false },
) {
    private val _state = MutableStateFlow<LiveSessionState>(LiveSessionState.Disconnected)
    val state = _state.asStateFlow()
    private val _events = MutableSharedFlow<LiveEvent>(extraBufferCapacity = 512)
    val events = _events.asSharedFlow()
    private val lock = Mutex()
    private var connection: LiveConnection? = null
    private var operation: Job? = null
    @Volatile private var generation = 0L
    private var attempts = 0

    fun connect(username: String, preview: Creator? = null) {
        operation?.cancel()
        val token = ++generation
        attempts = 0
        _state.value = LiveSessionState.Resolving(username.removePrefix("@"))
        operation =
            scope.launch {
                try {
                    lock.withLock {
                        val old = connection
                        connection = null
                        withContext(NonCancellable) { old?.disconnect() }
                        ensureActive()
                        val found = discovery.resolve(username)
                        val creator =
                            if (preview?.roomId == found.roomId)
                                found.copy(
                                    avatarUrl = preview.avatarUrl,
                                    coverUrl = preview.coverUrl,
                                    viewers = preview.viewers,
                                    title = found.title.ifBlank { preview.title },
                                )
                            else found
                        if (token != generation) return@withLock
                        _state.value = LiveSessionState.Preparing(creator)
                        val cookies = discovery.cookies()
                        ensureActive()
                        if (token != generation) return@withLock
                        _state.value = LiveSessionState.Connecting(creator)
                        if (backgroundEnabled()) keepAlive.start(creator.roomId, speechEnabled())
                        val opened =
                            transport.connect(
                                creator,
                                cookies,
                                object : LiveTransport.Listener {
                                    override fun onEvent(json: String) {
                                        if (
                                            token == generation &&
                                                !_events.tryEmit(EventParser.parse(json))
                                        )
                                            Logger.append(
                                                "EVENTS",
                                                "Consumer overloaded; event dropped",
                                            )
                                    }

                                    override fun onState(kind: String, detail: String) {
                                        scope.launch {
                                            if (token != generation) return@launch
                                            Logger.append("LIVE", "$kind: $detail")
                                            when (kind) {
                                                "open" -> {
                                                    attempts = 0
                                                    _state.value =
                                                        LiveSessionState.Connected(creator)
                                                    refreshNotification()
                                                }
                                                "reconnecting" -> {
                                                    if (reconnectEnabled())
                                                        _state.value =
                                                            LiveSessionState.Reconnecting(
                                                                creator,
                                                                ++attempts,
                                                            )
                                                    else
                                                        disconnect(
                                                            "Reconnect disabled",
                                                            failed = true,
                                                        )
                                                }
                                                "closed",
                                                "error" ->
                                                    disconnect(
                                                        detail.ifBlank { "Stream closed" },
                                                        failed = true,
                                                    )
                                            }
                                        }
                                    }
                                },
                            )
                        if (token == generation && currentCoroutineContext().isActive)
                            connection = opened
                        else withContext(NonCancellable) { opened.disconnect() }
                    }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    if (token == generation) {
                        _state.value = LiveSessionState.Failed(e.message ?: "Connection failed")
                        keepAlive.stop()
                        Logger.append("ERROR", e.message.orEmpty())
                    }
                }
            }
    }

    fun disconnect(reason: String = "Disconnected", failed: Boolean = false) {
        operation?.cancel()
        ++generation
        _state.value =
            if (failed) LiveSessionState.Failed(reason) else LiveSessionState.Disconnected
        keepAlive.stop()
        operation =
            scope.launch {
                lock.withLock {
                    val old = connection
                    connection = null
                    withContext(NonCancellable) { old?.disconnect() }
                }
            }
    }

    fun refreshNotification() {
        val creator = state.value.creator() ?: return
        if (backgroundEnabled()) keepAlive.refresh(creator.roomId, speechEnabled())
        else keepAlive.stop()
    }

    fun onBackground() {
        if (!backgroundEnabled()) disconnect("Background connection disabled")
    }

    suspend fun close() {
        operation?.cancel()
        ++generation
        _state.value = LiveSessionState.Disconnected
        keepAlive.stop()
        lock.withLock {
            val old = connection
            connection = null
            withContext(NonCancellable) { old?.disconnect() }
        }
    }
}
