package com.example.ttlsigner

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.example.ttlsigner.actions.ActionRunner
import com.example.ttlsigner.data.StudioDb
import com.example.ttlsigner.events.EventFilter
import com.example.ttlsigner.events.EventParser
import com.example.ttlsigner.events.LiveEvent
import com.example.ttlsigner.points.PointsRepository
import com.example.ttlsigner.tts.AndroidSpeaker
import com.example.ttlsigner.tts.NoopSpeaker
import com.example.ttlsigner.tts.Speaker
import com.example.ttlsigner.tts.SpeechText
import com.example.ttlsigner.tts.TtsEngine
import com.example.ttlsigner.tts.supertonic.SupertonicModels
import com.example.ttlsigner.tts.supertonic.SupertonicSpeaker
import kotlinx.coroutines.Job
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.random.Random
import kotlin.system.measureTimeMillis

/**
 * The tabs' shared session: room, feed, live stream, signer, guest cookies,
 * plus the studio pipeline — every live event is parsed once, then fanned out
 * to the reader, the points bank, the fetch-only actions, and the speaker.
 * Survives rotation, so a turn of the phone no longer drops the stream — the
 * fragments only render [StateFlow]s and call actions.
 *
 * Every step logs to the shared [Logger] buffer and to logcat. The screen may
 * show a full signed URL; logcat only ever gets its summary — see
 * [Logger.summarizeSignedUrl].
 */
class SessionViewModel(app: Application) : AndroidViewModel(app) {

    sealed interface LiveStatus {
        data object Idle : LiveStatus
        data class Connecting(val room: String) : LiveStatus
        data class Live(val room: String) : LiveStatus
    }

    data class FeedUiState(
        val rooms: List<Feed.LiveRoom> = emptyList(),
        val loading: Boolean = false,
        val error: String? = null,
    )

    data class BundleUiState(
        val version: String = BundleFetch.BUNDLE_VERSION,
        val shaShort: String = BundleFetch.BUNDLE_SHA256.take(16),
        val cache: BundleFetch.CacheInfo? = null,
    )

    companion object {
        const val MAX_EVENTS = 300
    }

    private val studioDb = StudioDb(app)
    val points = PointsRepository(app, studioDb)
    val actions = ActionRunner(app, studioDb)

    private var speaker: Speaker = NoopSpeaker()
    private var ttsWatchJob: Job? = null
    private val _ttsEngine = MutableStateFlow(AndroidSpeaker.engine(app))
    val ttsEngine: StateFlow<TtsEngine> = _ttsEngine.asStateFlow()

    data class SupertonicModelsUi(
        val ready: Boolean = false,
        val detail: String = "",
        val progress: Float? = null,
        val downloading: Boolean = false,
    )

    private val _supertonicModels = MutableStateFlow(SupertonicModelsUi())
    val supertonicModels: StateFlow<SupertonicModelsUi> = _supertonicModels.asStateFlow()

    private val _supertonicState =
        MutableStateFlow<SupertonicSpeaker.State>(SupertonicSpeaker.State.Idle)
    val supertonicState: StateFlow<SupertonicSpeaker.State> = _supertonicState.asStateFlow()

    private val _live = MutableStateFlow<LiveStatus>(LiveStatus.Idle)
    val live: StateFlow<LiveStatus> = _live.asStateFlow()

    private val _events = MutableStateFlow<List<LiveEvent>>(emptyList())
    private val _filter = MutableStateFlow(EventFilter())
    val filter: StateFlow<EventFilter> = _filter.asStateFlow()

    /** Events passing the current filter, oldest first. */
    val visibleEvents: StateFlow<List<LiveEvent>> =
        combine(_events, _filter) { list, filter -> list.filter(filter::matches) }
            .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

    private val _counts = MutableStateFlow<Map<LiveEvent.Category, Int>>(emptyMap())
    val counts: StateFlow<Map<LiveEvent.Category, Int>> = _counts.asStateFlow()

    private val _eventCount = MutableStateFlow(0)
    val eventCount: StateFlow<Int> = _eventCount.asStateFlow()

    private val _paused = MutableStateFlow(false)
    val paused: StateFlow<Boolean> = _paused.asStateFlow()

    private val _lastAward = MutableStateFlow<PointsRepository.Award?>(null)
    val lastAward: StateFlow<PointsRepository.Award?> = _lastAward.asStateFlow()

    private val _feed = MutableStateFlow(FeedUiState())
    val feed: StateFlow<FeedUiState> = _feed.asStateFlow()

    private val _busy = MutableStateFlow(false)
    val busy: StateFlow<Boolean> = _busy.asStateFlow()

    private val _taskError = MutableStateFlow<String?>(null)
    val taskError: StateFlow<String?> = _taskError.asStateFlow()

    /** The last handle a feed tap selected; Setup offers it as input. */
    private val _lastHandle = MutableStateFlow("")
    val lastHandle: StateFlow<String> = _lastHandle.asStateFlow()

    private var roomId: String? = null
    private var liveClient: LiveClient? = null
    private var guestSession = Feed.GuestSession()
    private val eventDeque = ArrayDeque<LiveEvent>()

    init {
        log("APP", "native: ${runCatching { RustSigner.version() }.getOrElse { "UNAVAILABLE: $it" }}")
        rebuildSpeaker()
        refreshSupertonicModels()
    }

    override fun onCleared() {
        speaker.shutdown()
        speaker = NoopSpeaker()
        // The worker is joined on its own thread: onCleared must return promptly,
        // and viewModelScope is already cancelled here. nDisconnect lands in ~1 s.
        val live = liveClient
        liveClient = null
        if (live != null) {
            Thread {
                try {
                    kotlinx.coroutines.runBlocking { live.disconnect() }
                } catch (e: Exception) {
                    Log.w(Logger.TAG, "disconnect on clear failed: ${e.message}")
                }
            }.start()
        }
    }

    private fun run(block: suspend () -> Unit) {
        _busy.value = true
        _taskError.value = null
        viewModelScope.launch {
            try {
                block()
            } catch (e: Exception) {
                _taskError.value = "${e.javaClass.simpleName}: ${e.message}"
                log("ERROR", "${e.javaClass.simpleName}: ${e.message}")
            } finally {
                _busy.value = false
            }
        }
    }

    /** Timestamped line to the shared buffer and to logcat. */
    private fun log(tag: String, msg: String, logcat: String = msg) {
        Logger.append(tag, msg)
        Log.d(Logger.TAG, Logger.line(tag, logcat))
    }

    // Reader: filter + pause + clear.

    fun setFilter(next: EventFilter) {
        _filter.value = next
    }

    fun toggleCategory(category: LiveEvent.Category) {
        _filter.value = _filter.value.toggle(category)
    }

    fun setPaused(paused: Boolean) {
        _paused.value = paused
    }

    fun clearEvents() {
        eventDeque.clear()
        _events.value = emptyList()
        _counts.value = emptyMap()
        _eventCount.value = 0
    }

    // TTS engines: off, the platform voice, or on-device SuperTonic 3.

    fun setTtsEngine(engine: TtsEngine) {
        val app = getApplication<Application>()
        AndroidSpeaker.setEngine(app, engine)
        _ttsEngine.value = engine
        rebuildSpeaker()
        log("TTS", "engine: ${engine.name.lowercase()}")
    }

    private fun rebuildSpeaker() {
        ttsWatchJob?.cancel()
        ttsWatchJob = null
        speaker.shutdown()
        val app = getApplication<Application>()
        speaker = when (_ttsEngine.value) {
            TtsEngine.OFF -> NoopSpeaker()
            TtsEngine.DEVICE -> AndroidSpeaker(app)
            TtsEngine.SUPERTONIC -> SupertonicSpeaker(app).also { current ->
                ttsWatchJob = viewModelScope.launch {
                    current.state.collect { _supertonicState.value = it }
                }
            }
        }
    }

    fun refreshSupertonicModels() {
        val app = getApplication<Application>()
        _supertonicModels.value = when (val status = SupertonicModels.status(app)) {
            is SupertonicModels.Status.Ready ->
                SupertonicModelsUi(ready = true, detail = "7 files on device · voice F1")
            is SupertonicModels.Status.Missing ->
                SupertonicModelsUi(ready = false, detail = "${status.files.size} files missing")
        }
    }

    /** Download the v3 model files once; progress lands in [supertonicModels]. */
    fun downloadSupertonicModels() = run {
        if (_supertonicModels.value.downloading) return@run
        log("TTS", "downloading SuperTonic 3 model files …")
        _supertonicModels.value = _supertonicModels.value.copy(downloading = true, progress = 0f)
        try {
            val app = getApplication<Application>()
            SupertonicModels.download(app) { progress ->
                _supertonicModels.value = _supertonicModels.value.copy(
                    downloading = true,
                    progress = progress.overall(),
                    detail = "${progress.fileName} (${progress.fileIndex + 1}/${progress.fileCount})",
                )
            }
            log("TTS", "SuperTonic 3 model ready")
        } finally {
            refreshSupertonicModels()
        }
    }

    /** Speak one sample line through the selected engine. */
    fun testSpeech() {
        val current = speaker
        if (!current.enabled) {
            log("TTS", "pick an engine first")
            return
        }
        log("TTS", "test: speaking one line")
        current.speak("TikTools studio ready.")
    }

    fun speakJoins(): Boolean = AndroidSpeaker.speakJoins(getApplication())

    fun setSpeakJoins(speak: Boolean) {
        AndroidSpeaker.setSpeakJoins(getApplication(), speak)
    }

    // Direct login: a handle (or numeric room id) connects in one step —
    // resolve, sign, and open the stream with no intermediate screens.

    /** A numeric input is a room id for offline testing; anything else resolves over HTTPS. */
    private suspend fun currentRoomId(typed: String): String {
        roomId?.let { return it }
        val handle = typed.trim()
        if (handle.matches(Regex("\\d+"))) {
            roomId = handle
            log("RESOLVE", "using room id $handle as typed (no lookup)")
            return handle
        }
        return resolveNow(handle)
    }

    private suspend fun resolveNow(handle: String): String {
        val trimmed = handle.trim()
        require(trimmed.isNotEmpty()) { "type a handle or a numeric room id first" }
        log("RESOLVE", "looking up $trimmed …")
        val userAgent = withContext(Dispatchers.IO) { RustSigner.userAgent() }
        val outcome: Discovery.LookupResult
        val ms = measureTimeMillis { outcome = Discovery.fetchRoomLookup(trimmed, userAgent) }
        when (outcome) {
            is Discovery.LookupResult.Found -> {
                roomId = outcome.room.roomId
                log("RESOLVE", "${ms}ms: @${outcome.room.uniqueId} (${outcome.room.nickname})")
                log("RESOLVE", "room=${outcome.room.roomId} live=${outcome.room.isLive()} ${outcome.room.title}")
                return outcome.room.roomId
            }
            is Discovery.LookupResult.UserNotFound ->
                throw IllegalStateException("@${outcome.handle} does not resolve")
            is Discovery.LookupResult.NoRoom ->
                throw IllegalStateException("@${outcome.handle} has no live room")
            is Discovery.LookupResult.DecodeError ->
                throw IllegalStateException("lookup failed: ${outcome.detail}")
        }
    }

    // Feed + random.

    fun refreshFeed() = run {
        log("FEED", "fetching live rooms (guest session ${if (guestSession.isEmpty()) "new" else "cached"}) …")
        _feed.value = _feed.value.copy(loading = true, error = null)
        try {
            val userAgent = withContext(Dispatchers.IO) { RustSigner.userAgent() }
            val rooms: List<Feed.LiveRoom>
            val ms = measureTimeMillis { rooms = Feed.fetchFeed("live", userAgent, guestSession) }
            _feed.value = FeedUiState(rooms = rooms)
            log("FEED", "${ms}ms: ${rooms.size} rooms live (tap one to connect)")
        } catch (e: Exception) {
            _feed.value = _feed.value.copy(loading = false, error = "${e.javaClass.simpleName}: ${e.message}")
            throw e
        }
    }

    /** Feed tap: adopt the room and connect to it directly. */
    fun selectRoom(room: Feed.LiveRoom) = run {
        if (liveClient != null) {
            log("LIVE", "already connected; disconnect first")
            return@run
        }
        log("SELECT", "@${room.uniqueId} (${room.viewers} watching): ${room.title}")
        _lastHandle.value = "@${room.uniqueId}"
        roomId = null
        val id = resolveNow("@${room.uniqueId}")
        openStream(id)
    }

    /** A random room from the feed, fetching it first when empty, then connect. */
    fun connectRandom() = run {
        if (liveClient != null) {
            log("LIVE", "already connected; disconnect first")
            return@run
        }
        if (_feed.value.rooms.isEmpty()) refreshFeedNow()
        val rooms = _feed.value.rooms
        require(rooms.isNotEmpty()) { "the feed is empty, nothing to pick from" }
        val at = Random.nextInt(rooms.size)
        val room = rooms[at]
        log("RANDOM", "pick ${at + 1}/${rooms.size}: @${room.uniqueId}")
        _lastHandle.value = "@${room.uniqueId}"
        roomId = null
        val id = resolveNow("@${room.uniqueId}")
        openStream(id)
    }

    private suspend fun refreshFeedNow() {
        val userAgent = withContext(Dispatchers.IO) { RustSigner.userAgent() }
        _feed.value = FeedUiState(rooms = Feed.fetchFeed("live", userAgent, guestSession))
    }

    // Live stream.

    /**
     * Direct login: a handle (or numeric room id) connects in one step.
     * Blank [typed] reuses the resolved room.
     */
    fun connect(typed: String) = run {
        if (liveClient != null) {
            log("LIVE", "already connected; disconnect first")
            return@run
        }
        val room = if (typed.trim().isEmpty()) currentRoomId("") else currentRoomId(typed)
        openStream(room)
    }

    /** Sign and open [room]'s live event stream. Shared by every login path. */
    private suspend fun openStream(room: String) {
        val userAgent = withContext(Dispatchers.IO) { RustSigner.userAgent() }
        if (guestSession.isEmpty()) {
            log("LIVE", "bootstrapping guest session …")
            guestSession.bootstrap(userAgent)
        }
        log("LIVE", "connecting to room $room …")
        _live.value = LiveStatus.Connecting(room)
        clearEvents()
        try {
            val app = getApplication<Application>()
            liveClient = LiveClient.connect(app, room, guestSession.cookieHeader(), listener = liveListener)
            _live.value = LiveStatus.Live(room)
        } catch (e: Exception) {
            _live.value = LiveStatus.Idle
            throw e
        }
    }

    /** Stop the live event stream. Safe to call when not connected. */
    fun disconnect(reason: String) = run {
        val live = liveClient
        liveClient = null
        _live.value = LiveStatus.Idle
        if (live == null) {
            log("LIVE", "not connected ($reason)")
            return@run
        }
        log("LIVE", "disconnecting ($reason) …")
        val ms = measureTimeMillis { live.disconnect() }
        log("LIVE", "disconnected in ${ms}ms")
    }

    private val liveListener = object : LiveClient.Listener {
        override fun onEvent(json: String) {
            // Runs on the main thread (LiveClient posts it there); parsing is
            // cheap, and the fan-out below hops to IO on its own.
            val event = EventParser.parse(json)
            _eventCount.value = _eventCount.value + 1
            _counts.value = _counts.value + (event.category to ((_counts.value[event.category] ?: 0) + 1))
            if (!_paused.value) {
                eventDeque.addLast(event)
                while (eventDeque.size > MAX_EVENTS) eventDeque.removeFirst()
                _events.value = eventDeque.toList()
            }
            viewModelScope.launch(Dispatchers.IO) {
                val award = points.award(event)
                if (award != null) _lastAward.value = award
            }
            actions.onEvent(event)
            if (speaker.enabled) {
                val text = SpeechText.forEvent(event, speakJoins())
                if (text.isNotEmpty()) speaker.speak(text)
            }
        }

        override fun onState(kind: String, detail: String) {
            log("LIVE", "$kind: $detail")
            // Terminal states release the client: the worker already exited, so this
            // join only frees the handle. Runs off the UI thread by construction.
            if (kind == "closed" || kind == "error") {
                val live = liveClient
                liveClient = null
                _live.value = LiveStatus.Idle
                if (live != null) viewModelScope.launch(Dispatchers.IO) { live.disconnect() }
            }
        }
    }

    // Settings.

    fun nativeVersion(): String =
        runCatching { RustSigner.version() }.getOrElse { "UNAVAILABLE: $it" }

    fun userAgent(): String =
        runCatching { RustSigner.userAgent() }.getOrElse { "UNAVAILABLE: $it" }

    suspend fun bundleState(): BundleUiState = withContext(Dispatchers.IO) {
        BundleUiState(cache = BundleFetch.describeCache(getApplication<Application>().cacheDir))
    }

    /** Redownload the bundle; the next stream open parses it. */
    suspend fun redownloadBundle(): Long = withContext(Dispatchers.IO) {
        val app = getApplication<Application>()
        val text = BundleFetch.refresh(app.cacheDir)
        log("BUNDLE", "redownloaded ${text.length} chars, next stream uses it")
        text.length.toLong()
    }

    /** Delete the cached bundle. Returns what was removed. */
    suspend fun clearCaches(): String = withContext(Dispatchers.IO) {
        val app = getApplication<Application>()
        val cache: File = BundleFetch.cacheFile(app.cacheDir)
        val hadBundle = cache.isFile && cache.delete()
        log("BUNDLE", "cache cleared (bundle was ${if (hadBundle) "present" else "absent"})")
        if (hadBundle) "bundle cache cleared" else "nothing cached"
    }

    fun guestCookieNames(): List<String> = guestSession.cookieNames()

    fun resetGuest() {
        guestSession = Feed.GuestSession()
        log("SESSION", "guest session reset; next feed load bootstraps again")
    }

    fun clearLog() {
        Logger.clear()
    }
}
