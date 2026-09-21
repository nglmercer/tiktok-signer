package com.example.ttlsigner

import android.os.Bundle
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.random.Random
import kotlin.system.measureTimeMillis

/**
 * Example app for on-device signing through the reused Rust core:
 * `libttl_sign_mobile.so` over JNI. The flow:
 *
 * 1. Resolve a handle (or type a numeric room id to skip the network).
 * 2. Sign the socket URL through Rust and show the latency and the result.
 * 3. Feed: list rooms broadcasting now, tap one to select and connect it.
 * 4. Random: pick a random room from the feed and connect it.
 *
 * The signed URL is shown on screen (that is the point of the demo) but never
 * written to logcat — a signed URL is a replayable capability.
 */
class MainActivity : AppCompatActivity() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    private lateinit var handleInput: EditText
    private lateinit var output: TextView
    private lateinit var outputScroll: ScrollView
    private lateinit var progress: ProgressBar
    private lateinit var feedAdapter: FeedAdapter
    private lateinit var buttons: List<Button>

    private var roomId: String? = null
    private var rustSigner: RustSigner? = null
    private var feedRooms: List<Feed.LiveRoom> = emptyList()
    private val guestSession = Feed.GuestSession()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        handleInput = findViewById(R.id.handleInput)
        output = findViewById(R.id.output)
        outputScroll = findViewById(R.id.outputScroll)
        progress = findViewById(R.id.progress)
        val resolveButton: Button = findViewById(R.id.resolveButton)
        val signButton: Button = findViewById(R.id.signButton)
        val feedButton: Button = findViewById(R.id.feedButton)
        val randomButton: Button = findViewById(R.id.randomButton)
        buttons = listOf(resolveButton, signButton, feedButton, randomButton)

        val feedList: RecyclerView = findViewById(R.id.feedList)
        feedList.layoutManager = LinearLayoutManager(this)
        feedAdapter = FeedAdapter { room -> run { select(room) } }
        feedList.adapter = feedAdapter

        append("native: ${runCatching { RustSigner.version() }.getOrElse { "UNAVAILABLE: $it" }}")

        resolveButton.setOnClickListener { run { resolve() } }
        signButton.setOnClickListener { run { sign() } }
        feedButton.setOnClickListener { run { loadFeed() } }
        randomButton.setOnClickListener { run { connectRandom() } }
    }

    override fun onDestroy() {
        scope.cancel()
        rustSigner?.close()
        rustSigner = null
        super.onDestroy()
    }

    private fun run(block: suspend () -> Unit) {
        setBusy(true)
        scope.launch {
            try {
                block()
            } catch (e: Exception) {
                append("ERROR: ${e.message}")
            } finally {
                setBusy(false)
            }
        }
    }

    private fun setBusy(busy: Boolean) {
        progress.visibility = if (busy) View.VISIBLE else View.GONE
        buttons.forEach { it.isEnabled = !busy }
    }

    private fun append(line: String) {
        output.append(line + "\n")
        // New output lands below the fold; follow it so the latest line is visible.
        outputScroll.post { outputScroll.fullScroll(View.FOCUS_DOWN) }
    }

    /** A numeric input is a room id for offline testing; anything else resolves over HTTPS. */
    private suspend fun currentRoomId(): String {
        roomId?.let { return it }
        val typed = handleInput.text.toString().trim()
        if (typed.matches(Regex("\\d+"))) {
            roomId = typed
            append("using room id $typed as typed (no lookup)")
            return typed
        }
        return resolve()
    }

    private suspend fun resolve(): String {
        val handle = handleInput.text.toString().trim()
        require(handle.isNotEmpty()) { "type a handle or a numeric room id first" }
        val userAgent = withContext(Dispatchers.IO) { RustSigner.userAgent() }
        val outcome: Discovery.LookupResult
        val ms = measureTimeMillis { outcome = Discovery.fetchRoomLookup(handle, userAgent) }
        when (outcome) {
            is Discovery.LookupResult.Found -> {
                roomId = outcome.room.roomId
                append("resolve ${ms}ms: @${outcome.room.uniqueId} (${outcome.room.nickname})")
                append("  room=${outcome.room.roomId} live=${outcome.room.isLive()} ${outcome.room.title}")
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
        val ms = measureTimeMillis { rustSigner = RustSigner.open(this, "{}") }
        append("open ${ms}ms (bundle download + 235 KB parse, once)")
        return rustSigner!!
    }

    private suspend fun sign() {
        val room = currentRoomId()
        val url = withContext(Dispatchers.IO) { RustSigner.socketUrl(room) }
        val signer = warmRust()
        val signed: String
        val ms = measureTimeMillis { signed = signer.sign(url) }
        append("sign ${ms}ms:")
        append(describe(signed))
    }

    /** Fetch the live feed and render it for selection. */
    private suspend fun loadFeed() {
        val userAgent = withContext(Dispatchers.IO) { RustSigner.userAgent() }
        val ms = measureTimeMillis { feedRooms = Feed.fetchFeed("live", userAgent, guestSession) }
        feedAdapter.submitList(feedRooms)
        append("feed ${ms}ms: ${feedRooms.size} rooms live (tap one to connect)")
    }

    /** Connect a feed room selected by tap: resolve it, then sign. */
    private suspend fun select(room: Feed.LiveRoom) {
        append("selected @${room.uniqueId} (${room.viewers} watching): ${room.title}")
        handleInput.setText("@${room.uniqueId}")
        roomId = null
        resolve()
        sign()
    }

    /** Connect a random room from the feed, fetching it first when empty. */
    private suspend fun connectRandom() {
        if (feedRooms.isEmpty()) loadFeed()
        require(feedRooms.isNotEmpty()) { "the feed is empty, nothing to pick from" }
        val at = Random.nextInt(feedRooms.size)
        val room = feedRooms[at]
        append("random pick ${at + 1}/${feedRooms.size}: @${room.uniqueId}")
        select(room)
    }

    private fun describe(signed: String): String {
        val gnarly = signed.substringAfter("&X-Gnarly=", "")
        return if (gnarly.isEmpty()) "  NOT SIGNED: $signed"
        else "  ${signed.length} chars, X-Gnarly ${gnarly.length} chars\n  $signed"
    }
}
