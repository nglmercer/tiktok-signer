package dev.nglmercer.tiktools.live

import dev.nglmercer.tiktools.core.model.Creator

sealed interface LiveSessionState {
    data object Disconnected : LiveSessionState

    data class Resolving(val username: String) : LiveSessionState

    data class Preparing(val creator: Creator) : LiveSessionState

    data class Connecting(val creator: Creator) : LiveSessionState

    data class Connected(val creator: Creator, val since: Long = System.currentTimeMillis()) :
        LiveSessionState

    data class Reconnecting(val creator: Creator, val attempt: Int) : LiveSessionState

    data class Failed(val message: String) : LiveSessionState
}

fun LiveSessionState.creator(): Creator? =
    when (this) {
        is LiveSessionState.Preparing -> creator
        is LiveSessionState.Connecting -> creator
        is LiveSessionState.Connected -> creator
        is LiveSessionState.Reconnecting -> creator
        else -> null
    }

val LiveSessionState.isActive: Boolean
    get() = this !is LiveSessionState.Disconnected && this !is LiveSessionState.Failed

fun LiveSessionState.label(): String =
    when (this) {
        LiveSessionState.Disconnected -> "Offline"
        is LiveSessionState.Resolving -> "Looking up @$username…"
        is LiveSessionState.Preparing -> "Preparing session…"
        is LiveSessionState.Connecting -> "Connecting to LIVE…"
        is LiveSessionState.Connected -> "LIVE"
        is LiveSessionState.Reconnecting -> "Reconnecting · attempt $attempt"
        is LiveSessionState.Failed -> "Connection failed"
    }
