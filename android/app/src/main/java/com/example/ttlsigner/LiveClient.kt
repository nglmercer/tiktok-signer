package com.example.ttlsigner

import android.content.Context
import android.os.Handler
import android.os.Looper
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Live event stream through the reused Rust core: sign, open the socket, decode
 * batches, push JSON. Callbacks arrive on the Rust worker thread and are posted to
 * the main thread before reaching [Listener], so UI code can touch views directly.
 */
class LiveClient private constructor(
    private var handle: Long,
    private val main: Handler,
    private val listener: Listener,
) {
    interface Listener {
        fun onEvent(json: String)
        fun onState(kind: String, detail: String)
    }

    private val callback = object : TtlEvents {
        override fun onEvent(json: String) {
            main.post { listener.onEvent(json) }
        }

        override fun onState(kind: String, detail: String) {
            main.post { listener.onState(kind, detail) }
        }
    }

    /** Stop the stream and wait for the worker. Blocks; call off the UI thread. */
    suspend fun disconnect() = withContext(Dispatchers.IO) {
        val live = handle
        handle = 0L
        if (live != 0L) TtlNative.nDisconnect(live)
    }

    companion object {
        /**
         * Open [roomId]'s event stream. Downloads (or reuses the cached) bundle and
         * signs every (re)connect with it; [cookieHeader] is the guest session the
         * handshake presents.
         */
        suspend fun connect(
            context: Context,
            roomId: String,
            cookieHeader: String,
            optionsJson: String = "{}",
            listener: Listener,
        ): LiveClient = withContext(Dispatchers.IO) {
            val bundle = BundleFetch.load(context.cacheDir)
            val main = Handler(Looper.getMainLooper())
            // The client is built first so its callback (bound to the listener) can
            // be handed to the native open; the handle lands right after.
            val client = LiveClient(0L, main, listener)
            val handle = TtlNative.nConnect(bundle, optionsJson, roomId, cookieHeader, client.callback)
            check(handle != 0L) { "the native live client failed to open" }
            client.setHandle(handle)
            client
        }
    }

    private fun setHandle(handle: Long) {
        this.handle = handle
    }
}
