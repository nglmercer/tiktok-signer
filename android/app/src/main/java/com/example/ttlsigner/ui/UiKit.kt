package com.example.ttlsigner.ui

import android.content.Context
import androidx.recyclerview.widget.DividerItemDecoration
import androidx.recyclerview.widget.RecyclerView

/**
 * Shared minimalist helpers: the one place that decides how section titles,
 * count lines, and list dividers look, so every tab stays quiet in the same
 * way. The string helpers are pure and JVM-testable — see `UiKitTest`.
 */
object UiKit {

    /**
     * `("Events", 12)` → "Events · 12"; a null or zero count renders the bare
     * label, so empty sections don't shout "(0)".
     */
    fun sectionTitle(label: String, count: Int?): String =
        if (count == null || count <= 0) label else "$label · $count"

    /**
     * `[("chat", 3), ("gift", 1)]` → "chat 3 · gift 1". Callers pass pairs
     * already sorted; an empty list renders [emptyText].
     */
    fun countsLine(pairs: List<Pair<String, Int>>, emptyText: String): String =
        if (pairs.isEmpty()) emptyText
        else pairs.joinToString(" · ") { (label, count) -> "$label $count" }

    /** `12dp` → px on this display. */
    fun Context.dp(value: Int): Int =
        (value * resources.displayMetrics.density).toInt()

    /**
     * One hairline between rows, drawn from the theme divider so lists read
     * as a single quiet column instead of boxed cards.
     */
    fun RecyclerView.addMinimalDividers() {
        addItemDecoration(DividerItemDecoration(context, DividerItemDecoration.VERTICAL))
    }
}
