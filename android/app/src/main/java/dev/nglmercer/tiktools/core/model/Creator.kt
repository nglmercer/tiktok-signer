package dev.nglmercer.tiktools.core.model

data class Creator(
    val uniqueId: String,
    val roomId: String,
    val nickname: String = "",
    val title: String = "",
    val avatarUrl: String = "",
    val coverUrl: String = "",
    val viewers: Long = 0,
)
