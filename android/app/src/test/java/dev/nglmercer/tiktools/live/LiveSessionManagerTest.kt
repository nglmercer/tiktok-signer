package dev.nglmercer.tiktools.live

import dev.nglmercer.tiktools.core.model.Creator
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import kotlinx.coroutines.test.*
import org.junit.Assert.*
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class LiveSessionManagerTest {
    private class Discovery : DiscoveryGateway {
        override suspend fun resolve(username: String) = Creator(username, "123")

        override suspend fun cookies() = "cookie=value"

        override suspend fun feed() = emptyList<Creator>()

        override suspend fun resetGuest() {}

        override fun cookieNames() = listOf("cookie")
    }

    private class Hold : SessionKeepAlive {
        var started = 0
        var stopped = 0

        override fun start(room: String, speech: Boolean) {
            started++
        }

        override fun refresh(room: String, speech: Boolean) {}

        override fun stop() {
            stopped++
        }
    }

    private class Socket(val gate: CompletableDeferred<Unit>? = null) : LiveTransport {
        lateinit var listener: LiveTransport.Listener
        var closed = 0

        override suspend fun connect(
            creator: Creator,
            cookies: String,
            listener: LiveTransport.Listener,
        ): LiveConnection {
            this.listener = listener
            if (gate != null) withContext(NonCancellable) { gate.await() }
            return object : LiveConnection {
                override suspend fun disconnect() {
                    closed++
                }
            }
        }
    }

    @Test
    fun onlyNativeOpenConfirmsLiveAndLateCallbacksAreIgnored() = runTest {
        val socket = Socket()
        val hold = Hold()
        val manager = LiveSessionManager(Discovery(), socket, hold, this)
        manager.connect("creator")
        runCurrent()
        assertTrue(manager.state.value is LiveSessionState.Connecting)
        socket.listener.onState("open", "123")
        runCurrent()
        assertTrue(manager.state.value is LiveSessionState.Connected)
        socket.listener.onState("decode_error", "bad event")
        runCurrent()
        assertTrue(manager.state.value is LiveSessionState.Connected)
        manager.disconnect()
        runCurrent()
        assertEquals(1, socket.closed)
        socket.listener.onState("open", "late")
        runCurrent()
        assertEquals(LiveSessionState.Disconnected, manager.state.value)
        manager.close()
        assertEquals(1, socket.closed)
    }

    @Test
    fun cancellationDuringNativeOpenReleasesReturnedHandle() = runTest {
        val gate = CompletableDeferred<Unit>()
        val socket = Socket(gate)
        val manager = LiveSessionManager(Discovery(), socket, Hold(), this)
        manager.connect("creator")
        runCurrent()
        manager.disconnect()
        gate.complete(Unit)
        runCurrent()
        assertEquals(1, socket.closed)
        assertEquals(LiveSessionState.Disconnected, manager.state.value)
        manager.close()
    }

    @Test
    fun reconnectPolicyCanStopTheNativeWorker() = runTest {
        val socket = Socket()
        val manager =
            LiveSessionManager(Discovery(), socket, Hold(), this, reconnectEnabled = { false })
        manager.connect("creator")
        runCurrent()
        socket.listener.onState("reconnecting", "network error")
        runCurrent()
        assertTrue(manager.state.value is LiveSessionState.Failed)
        assertEquals(1, socket.closed)
        manager.close()
    }

    @Test
    fun parsedEventsSurviveMalformedPayloadAndDisconnectStopsDelivery() = runTest {
        val socket = Socket()
        val manager = LiveSessionManager(Discovery(), socket, Hold(), this)
        val events = mutableListOf<dev.nglmercer.tiktools.core.model.LiveEvent>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            manager.events.collect(events::add)
        }
        manager.connect("creator")
        runCurrent()
        socket.listener.onEvent("{bad}")
        socket.listener.onEvent("{\"type\":\"chat\",\"comment\":\"hello\"}")
        runCurrent()
        assertEquals(2, events.size)
        assertEquals("hello", events.last().text)
        manager.disconnect()
        runCurrent()
        socket.listener.onEvent("{}")
        runCurrent()
        assertEquals(2, events.size)
        manager.close()
    }
}
