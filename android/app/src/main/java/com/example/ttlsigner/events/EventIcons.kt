package com.example.ttlsigner.events

import com.example.ttlsigner.R

/** Category → vector drawable for the reader's filter chips. */
object EventIcons {
    fun res(category: LiveEvent.Category): Int = when (category) {
        LiveEvent.Category.CHAT -> R.drawable.ic_ev_chat
        LiveEvent.Category.GIFT -> R.drawable.ic_ev_gift
        LiveEvent.Category.LIKE -> R.drawable.ic_ev_like
        LiveEvent.Category.FOLLOW -> R.drawable.ic_ev_follow
        LiveEvent.Category.SHARE -> R.drawable.ic_ev_share
        LiveEvent.Category.JOIN -> R.drawable.ic_ev_join
        LiveEvent.Category.MEMBER -> R.drawable.ic_ev_member
        LiveEvent.Category.ROOM -> R.drawable.ic_ev_room
        LiveEvent.Category.UNKNOWN -> R.drawable.ic_ev_unknown
    }
}
