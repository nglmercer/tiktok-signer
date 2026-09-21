package com.example.ttlsigner

import android.os.Bundle
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.ProgressBar
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.system.measureTimeMillis

/**
 * Example app for on-device signing through the reused Rust core:
 * `libttl_sign_mobile.so` over JNI. The flow:
 *
 * 1. Resolve a handle (or type a numeric room id to skip the network).
 * 2. Sign the socket URL through Rust and show the latency and the result.
 *
 * The signed URL is shown on screen (that is the point of the demo) but never
 * written to logcat — a signed URL is a replayable capability.
 */
class MainActivity : AppCompatActivity() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    private lateinit var handleInput: EditText
    private lateinit var output: TextView
    private lateinit var progress: ProgressBar
    private lateinit var buttons: List<Button>

    private var roomId: String? = null
    private var rustSigner: RustSigner? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        handleInput = findViewById(R.id.handleInput)
        output = findViewById(R.id.output)
        progress = findViewById(R.id.progress)
        val resolveButton: Button = findViewById(R.id.resolveButton)
        val signButton: Button = findViewById(R.id.signButton)
        buttons = listOf(resolveButton, signButton)

        append("native: ${runCatching { RustSigner.version() }.getOrElse { "UNAVAILABLE: $it" }}")

        resolveButton.setOnClickListener { run { resolve() } }
        signButton.setOnClickListener { run { sign() } }
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

    private fun describe(signed: String): String {
        val gnarly = signed.substringAfter("&X-Gnarly=", "")
        return if (gnarly.isEmpty()) "  NOT SIGNED: $signed"
        else "  ${signed.length} chars, X-Gnarly ${gnarly.length} chars\n  $signed"
    }
}
