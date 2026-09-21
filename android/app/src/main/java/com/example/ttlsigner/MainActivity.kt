package com.example.ttlsigner

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Bundle
import android.util.Log
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
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
 * Every step is logged with a timestamp to the on-screen debug log and to
 * logcat (`adb logcat -s TtlSigner:D`). The screen may show a full signed URL;
 * logcat only ever gets its summary — see [Logger.summarizeSignedUrl].
 */
class MainActivity : AppCompatActivity() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    private lateinit var handleInput: EditText
    private lateinit var output: TextView
    private lateinit var outputScroll: ScrollView
    private lateinit var progress: ProgressBar
    private lateinit var feedAdapter: FeedAdapter
    private lateinit var feedTitle: TextView
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
        feedTitle = findViewById(R.id.feedTitle)
        val resolveButton: Button = findViewById(R.id.resolveButton)
        val signButton: Button = findViewById(R.id.signButton)
        val feedButton: Button = findViewById(R.id.feedButton)
        val randomButton: Button = findViewById(R.id.randomButton)
        buttons = listOf(resolveButton, signButton, feedButton, randomButton)

        val feedList: RecyclerView = findViewById(R.id.feedList)
        feedList.layoutManager = LinearLayoutManager(this)
        feedAdapter = FeedAdapter { room -> run { select(room) } }
        feedList.adapter = feedAdapter

        collapse(findViewById(R.id.feedHeader), feedList, findViewById(R.id.feedChevron))
        collapse(findViewById(R.id.logHeader), outputScroll, findViewById(R.id.logChevron))

        findViewById<Button>(R.id.copyButton).setOnClickListener { copyLog() }
        findViewById<Button>(R.id.clearButton).setOnClickListener { output.text = "" }

        log("APP", "native: ${runCatching { RustSigner.version() }.getOrElse { "UNAVAILABLE: $it" }}")

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

    /** A header that expands/collapses its content view, chevron included. */
    private fun collapse(header: LinearLayout, content: View, chevron: TextView) {
        header.setOnClickListener {
            val show = content.visibility != View.VISIBLE
            content.visibility = if (show) View.VISIBLE else View.GONE
            chevron.text = getString(if (show) R.string.expanded else R.string.collapsed)
        }
    }

    private fun run(block: suspend () -> Unit) {
        setBusy(true)
        scope.launch {
            try {
                block()
            } catch (e: Exception) {
                log("ERROR", "${e.javaClass.simpleName}: ${e.message}")
            } finally {
                setBusy(false)
            }
        }
    }

    private fun setBusy(busy: Boolean) {
        progress.visibility = if (busy) View.VISIBLE else View.GONE
        buttons.forEach { it.isEnabled = !busy }
    }

    /** Timestamped line to the screen and to logcat. Screen and logcat carry the same
     * text, except signed URLs: the screen shows them, logcat gets the summary. */
    private fun log(tag: String, msg: String, logcat: String = msg) {
        val line = Logger.line(tag, msg)
        output.append(line + "\n")
        Log.d(Logger.TAG, Logger.line(tag, logcat))
        // New output lands below the fold; follow it so the latest line is visible.
        outputScroll.post { outputScroll.fullScroll(View.FOCUS_DOWN) }
    }

    private fun copyLog() {
        val text = output.text.toString()
        val clip = ClipData.newPlainText("ttl-signer log", text)
        (getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(clip)
        Toast.makeText(this, "log copied (${text.length} chars)", Toast.LENGTH_SHORT).show()
    }

    /** A numeric input is a room id for offline testing; anything else resolves over HTTPS. */
    private suspend fun currentRoomId(): String {
        roomId?.let { return it }
        val typed = handleInput.text.toString().trim()
        if (typed.matches(Regex("\\d+"))) {
            roomId = typed
            log("RESOLVE", "using room id $typed as typed (no lookup)")
            return typed
        }
        return resolve()
    }

    private suspend fun resolve(): String {
        val handle = handleInput.text.toString().trim()
        require(handle.isNotEmpty()) { "type a handle or a numeric room id first" }
        log("RESOLVE", "looking up $handle …")
        val userAgent = withContext(Dispatchers.IO) { RustSigner.userAgent() }
        val outcome: Discovery.LookupResult
        val ms = measureTimeMillis { outcome = Discovery.fetchRoomLookup(handle, userAgent) }
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

    private suspend fun warmRust(): RustSigner {
        rustSigner?.let { return it }
        log("SIGN", "opening signer (bundle download + parse, once) …")
        val ms = measureTimeMillis { rustSigner = RustSigner.open(this, "{}") }
        log("SIGN", "signer open in ${ms}ms")
        return rustSigner!!
    }

    private suspend fun sign() {
        val room = currentRoomId()
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
    }

    /** Fetch the live feed and render it for selection. */
    private suspend fun loadFeed() {
        log("FEED", "fetching live rooms (guest session ${if (guestSession.isEmpty()) "new" else "cached"}) …")
        val userAgent = withContext(Dispatchers.IO) { RustSigner.userAgent() }
        val ms = measureTimeMillis { feedRooms = Feed.fetchFeed("live", userAgent, guestSession) }
        feedAdapter.submitList(feedRooms)
        feedTitle.text = "${getString(R.string.feed_label)} (${feedRooms.size})"
        log("FEED", "${ms}ms: ${feedRooms.size} rooms live (tap one to connect)")
    }

    /** Connect a feed room selected by tap: resolve it, then sign. */
    private suspend fun select(room: Feed.LiveRoom) {
        log("SELECT", "@${room.uniqueId} (${room.viewers} watching): ${room.title}")
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
        log("RANDOM", "pick ${at + 1}/${feedRooms.size}: @${room.uniqueId}")
        select(room)
    }

    private fun describe(signed: String): String {
        val gnarly = signed.substringAfter("&X-Gnarly=", "")
        return if (gnarly.isEmpty()) "NOT SIGNED: $signed"
        else "${signed.length} chars, X-Gnarly ${gnarly.length} chars\n$signed"
    }
}
