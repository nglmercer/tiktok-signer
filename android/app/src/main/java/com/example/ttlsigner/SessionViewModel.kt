package com.example.ttlsigner

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.random.Random
import kotlin.system.measureTimeMillis

/**
 * The tabs' shared session: room, feed, live stream, signer, and guest cookies.
 * Survives rotation, so a turn of the phone no longer drops the stream — the
 * fragments only render [StateFlow]s and call actions.
 *
 * Every step logs to the shared [Logger] buffer (the Sign tab's console) and to
 * logcat. The screen may show a full signed URL; logcat only ever gets its
 * summary — see [Logger.summarizeSignedUrl].
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

    data class ResolveInfo(
        val handle: String,
        val roomId: String,
        val nickname: String,
        val title: String,
        val live: Boolean,
        val ms: Long,
    )

    data class SignResult(
        val room: String,
        val summary: String,
        val full: String,
        val ms: Long,
    )

    data class BundleUiState(
        val version: String = BundleFetch.BUNDLE_VERSION,
        val shaShort: String = BundleFetch.BUNDLE_SHA256.take(16),
        val cache: BundleFetch.CacheInfo? = null,
    )

    private val _live = MutableStateFlow<LiveStatus>(LiveStatus.Idle)
    val live: StateFlow<LiveStatus> = _live.asStateFlow()

    private val _events = MutableStateFlow<List<String>>(emptyList())
    val events: StateFlow<List<String>> = _events.asStateFlow()

    private val _eventCount = MutableStateFlow(0)
    val eventCount: StateFlow<Int> = _eventCount.asStateFlow()

    private val _feed = MutableStateFlow(FeedUiState())
    val feed: StateFlow<FeedUiState> = _feed.asStateFlow()

    private val _busy = MutableStateFlow(false)
    val busy: StateFlow<Boolean> = _busy.asStateFlow()

    private val _resolveInfo = MutableStateFlow<ResolveInfo?>(null)
    val resolveInfo: StateFlow<ResolveInfo?> = _resolveInfo.asStateFlow()

    private val _signResult = MutableStateFlow<SignResult?>(null)
    val signResult: StateFlow<SignResult?> = _signResult.asStateFlow()

    private val _taskError = MutableStateFlow<String?>(null)
    val taskError: StateFlow<String?> = _taskError.asStateFlow()

    /** The last handle a feed tap selected; the Sign tab offers it as input. */
    private val _lastHandle = MutableStateFlow("")
    val lastHandle: StateFlow<String> = _lastHandle.asStateFlow()

    private var roomId: String? = null
    private var rustSigner: RustSigner? = null
    private var liveClient: LiveClient? = null
    private var guestSession = Feed.GuestSession()
    private val eventLines = ArrayDeque<String>()

    init {
        log("APP", "native: ${runCatching { RustSigner.version() }.getOrElse { "UNAVAILABLE: $it" }}")
    }

    override fun onCleared() {
        rustSigner?.close()
        rustSigner = null
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

    // Resolve + sign.

    fun resolve(handle: String) = run { resolveNow(handle) }

    fun sign(handle: String) = run {
        val room = currentRoomId(handle)
        log("SIGN", "building socket URL for room $room …")
        val url = withContext(Dispatchers.IO) { RustSigner.socketUrl(room) }
        log("SIGN", "unsigned URL ready (${url.length} chars)")
        val signer = warmRust()
        log("SIGN", "signing (ws) …")
        val signed: String
        val ms = measureTimeMillis { signed = signer.sign(url) }
        val summary = Logger.summarizeSignedUrl(signed)
        log("SIGN", "signed in ${ms}ms: $summary", logcat = "signed in ${ms}ms: $summary")
        log("SIGN", describe(signed), logcat = summary)
        _signResult.value = SignResult(room, summary, signed, ms)
    }

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
                _resolveInfo.value = ResolveInfo(
                    handle = outcome.room.uniqueId,
                    roomId = outcome.room.roomId,
                    nickname = outcome.room.nickname,
                    title = outcome.room.title,
                    live = outcome.room.isLive(),
                    ms = ms,
                )
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

    private suspend fun warmRust(): RustSigner {
        rustSigner?.let { return it }
        log("SIGN", "opening signer (bundle download + parse, once) …")
        val ms = measureTimeMillis { rustSigner = RustSigner.open(getApplication(), "{}") }
        log("SIGN", "signer open in ${ms}ms")
        return rustSigner!!
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

    /** Feed tap: adopt the room, resolve it, then sign. */
    fun selectRoom(room: Feed.LiveRoom) = run {
        log("SELECT", "@${room.uniqueId} (${room.viewers} watching): ${room.title}")
        _lastHandle.value = "@${room.uniqueId}"
        roomId = null
        resolveNow("@${room.uniqueId}")
        signAfterResolve()
    }

    /** A random room from the feed, fetching it first when empty. */
    fun connectRandom() = run {
        if (_feed.value.rooms.isEmpty()) refreshFeedNow()
        val rooms = _feed.value.rooms
        require(rooms.isNotEmpty()) { "the feed is empty, nothing to pick from" }
        val at = Random.nextInt(rooms.size)
        val room = rooms[at]
        log("RANDOM", "pick ${at + 1}/${rooms.size}: @${room.uniqueId}")
        _lastHandle.value = "@${room.uniqueId}"
        roomId = null
        resolveNow("@${room.uniqueId}")
        signAfterResolve()
    }

    private suspend fun refreshFeedNow() {
        val userAgent = withContext(Dispatchers.IO) { RustSigner.userAgent() }
        _feed.value = FeedUiState(rooms = Feed.fetchFeed("live", userAgent, guestSession))
    }

    private suspend fun signAfterResolve() {
        val room = roomId ?: throw IllegalStateException("no room after resolve")
        log("SIGN", "building socket URL for room $room …")
        val url = withContext(Dispatchers.IO) { RustSigner.socketUrl(room) }
        log("SIGN", "unsigned URL ready (${url.length} chars)")
        val signer = warmRust()
        log("SIGN", "signing (ws) …")
        val signed: String
        val ms = measureTimeMillis { signed = signer.sign(url) }
        val summary = Logger.summarizeSignedUrl(signed)
        log("SIGN", "signed in ${ms}ms: $summary", logcat = "signed in ${ms}ms: $summary")
        log("SIGN", describe(signed), logcat = summary)
        _signResult.value = SignResult(room, summary, signed, ms)
    }

    // Live stream.

    /** Open the room's live event stream. Blank [typed] reuses the resolved room. */
    fun connect(typed: String) = run {
        if (liveClient != null) {
            log("LIVE", "already connected; disconnect first")
            return@run
        }
        val room = if (typed.trim().isEmpty()) currentRoomId("") else currentRoomId(typed)
        val userAgent = withContext(Dispatchers.IO) { RustSigner.userAgent() }
        if (guestSession.isEmpty()) {
            log("LIVE", "bootstrapping guest session …")
            guestSession.bootstrap(userAgent)
        }
        log("LIVE", "connecting to room $room …")
        _live.value = LiveStatus.Connecting(room)
        eventLines.clear()
        _events.value = emptyList()
        _eventCount.value = 0
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

    fun clearEvents() {
        eventLines.clear()
        _events.value = emptyList()
        _eventCount.value = 0
    }

    private val liveListener = object : LiveClient.Listener {
        override fun onEvent(json: String) {
            eventLines.addLast(EventFormat.line(json))
            while (eventLines.size > EventAdapter.MAX_LINES) eventLines.removeFirst()
            _events.value = eventLines.toList()
            _eventCount.value = _eventCount.value + 1
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

    /** Redownload the bundle; the warm signer is dropped so the next open parses it. */
    suspend fun redownloadBundle(): Long = withContext(Dispatchers.IO) {
        val app = getApplication<Application>()
        val text = BundleFetch.refresh(app.cacheDir)
        rustSigner?.close()
        rustSigner = null
        log("BUNDLE", "redownloaded ${text.length} chars, signer will reopen on next use")
        text.length.toLong()
    }

    /** Delete the cached bundle and drop the warm signer. Returns what was removed. */
    suspend fun clearCaches(): String = withContext(Dispatchers.IO) {
        val app = getApplication<Application>()
        val cache: File = BundleFetch.cacheFile(app.cacheDir)
        val hadBundle = cache.isFile && cache.delete()
        rustSigner?.close()
        rustSigner = null
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

    private fun describe(signed: String): String {
        val gnarly = signed.substringAfter("&X-Gnarly=", "")
        return if (gnarly.isEmpty()) "NOT SIGNED: $signed"
        else "${signed.length} chars, X-Gnarly ${gnarly.length} chars\n$signed"
    }
}
