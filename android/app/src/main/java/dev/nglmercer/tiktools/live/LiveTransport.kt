package dev.nglmercer.tiktools.live

import dev.nglmercer.tiktools.core.model.Creator

interface LiveConnection {
    suspend fun disconnect()
}

interface LiveTransport {
    interface Listener {
        fun onEvent(json: String)

        fun onState(kind: String, detail: String)
    }

    suspend fun connect(creator: Creator, cookies: String, listener: Listener): LiveConnection
}

interface DiscoveryGateway {
    suspend fun resolve(username: String): Creator

    suspend fun cookies(): String

    suspend fun feed(): List<Creator>

    suspend fun resetGuest()

    fun cookieNames(): List<String>
}

interface SessionKeepAlive {
    fun start(room: String, speech: Boolean)

    fun refresh(room: String, speech: Boolean)

    fun stop()
}
