package com.example.ttlsigner.events

/**
 * Reader filter: which [LiveEvent.Category] values stay visible, plus an
 * optional case-insensitive query matched against user, nickname, and text.
 *
 * Pure and UI-agnostic: the fragment owns a state of this, the adapter (or the
 * view model) applies [matches]. Defaults to everything visible.
 */
data class EventFilter(
    val enabled: Set<LiveEvent.Category> = LiveEvent.Category.values().toSet(),
    val query: String = "",
) {
    fun matches(event: LiveEvent): Boolean {
        if (event.category !in enabled) return false
        val q = query.trim()
        if (q.isEmpty()) return true
        return event.user.contains(q, ignoreCase = true) ||
            event.nickname.contains(q, ignoreCase = true) ||
            event.text.contains(q, ignoreCase = true)
    }

    /** Toggle one category; disabling the last visible one keeps it on. */
    fun toggle(category: LiveEvent.Category): EventFilter {
        val next = if (category in enabled) enabled - category else enabled + category
        if (next.isEmpty()) return this
        return copy(enabled = next)
    }

    fun showAll(): EventFilter =
        copy(enabled = LiveEvent.Category.values().toSet(), query = "")

    /** Check every category; the query is kept. */
    fun selectAll(): EventFilter =
        copy(enabled = LiveEvent.Category.values().toSet())

    /**
     * Uncheck every category: [matches] then hides everything and the reader
     * shows its empty state. Unlike [toggle] this allows the empty set — it
     * is only reachable through the explicit Clear button.
     */
    fun clearSelection(): EventFilter = copy(enabled = emptySet())
}
