package dev.nglmercer.tiktools.live

import dev.nglmercer.tiktools.core.model.Creator
import dev.nglmercer.tiktools.core.nativebridge.RustSigner
import dev.nglmercer.tiktools.core.network.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

class DiscoveryRepository : DiscoveryGateway {
    private var guest = Feed.GuestSession()
    private val lock = Mutex()
    @Volatile private var names: List<String> = emptyList()

    private suspend fun agent(): String =
        try {
            withContext(Dispatchers.IO) { RustSigner.userAgent() }
        } catch (e: LinkageError) {
            throw IllegalStateException("LIVE engine unavailable on this device", e)
        }

    override suspend fun resolve(username: String): Creator {
        val handle = username.trim().removePrefix("@")
        require(handle.matches(Regex("[A-Za-z0-9._]{1,24}"))) { "Enter a TikTok username" }
        // Preserve the original app's numeric-room diagnostic/testing path.
        if (handle.matches(Regex("[0-9]+"))) return Creator("room:$handle", handle)
        return when (val result = Discovery.fetchRoomLookup(handle, agent())) {
            is Discovery.LookupResult.Found -> {
                check(result.room.isLive()) { "@$handle is offline" }
                Creator(
                    result.room.uniqueId,
                    result.room.roomId,
                    result.room.nickname,
                    result.room.title,
                )
            }
            is Discovery.LookupResult.NoRoom -> error("@$handle has no LIVE room")
            is Discovery.LookupResult.UserNotFound -> error("@$handle was not found")
            is Discovery.LookupResult.DecodeError -> error("Lookup failed: ${result.detail}")
        }
    }

    override suspend fun cookies(): String =
        lock.withLock {
            if (guest.isEmpty()) guest.bootstrap(agent())
            names = guest.cookieNames()
            guest.cookieHeader()
        }

    override suspend fun feed(): List<Creator> =
        lock.withLock {
            val rooms = Feed.fetchFeed("live", agent(), guest)
            names = guest.cookieNames()
            rooms.map {
                Creator(
                    it.uniqueId,
                    it.roomId,
                    it.nickname,
                    it.title,
                    it.avatarUrl,
                    it.coverUrl,
                    it.viewers,
                )
            }
        }

    override suspend fun resetGuest() =
        lock.withLock {
            guest = Feed.GuestSession()
            names = emptyList()
        }

    override fun cookieNames(): List<String> = names
}
