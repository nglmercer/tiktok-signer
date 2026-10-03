package dev.nglmercer.tiktools.live

import android.content.Context
import dev.nglmercer.tiktools.core.model.Creator
import dev.nglmercer.tiktools.core.nativebridge.LiveClient
import kotlinx.coroutines.*

class NativeLiveTransport(private val context: Context) : LiveTransport {
    override suspend fun connect(
        creator: Creator,
        cookies: String,
        listener: LiveTransport.Listener,
    ): LiveConnection {
        // JNI open is blocking; cancellation must still release a handle returned after
        // cancellation.
        val client =
            try {
                withContext(NonCancellable) {
                    LiveClient.connect(
                        context,
                        creator.roomId,
                        cookies,
                        listener =
                            object : LiveClient.Listener {
                                override fun onEvent(json: String) = listener.onEvent(json)

                                override fun onState(kind: String, detail: String) =
                                    listener.onState(kind, detail)
                            },
                    )
                }
            } catch (e: LinkageError) {
                throw IllegalStateException("LIVE engine unavailable on this device", e)
            }
        try {
            currentCoroutineContext().ensureActive()
        } catch (e: CancellationException) {
            withContext(NonCancellable) { client.disconnect() }
            throw e
        }
        return object : LiveConnection {
            override suspend fun disconnect() {
                client.disconnect()
            }
        }
    }
}
