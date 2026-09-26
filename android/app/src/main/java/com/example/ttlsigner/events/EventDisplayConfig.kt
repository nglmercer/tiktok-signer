package com.example.ttlsigner.events

import android.content.Context

/**
 * How event rows look: density, badge, timestamp, text lines. Defaults to
 * the minimalist row (compact, badge, time, single line); the reader's tune
 * button opens the options dialog and every change persists in
 * [SharedPreferences]. Pure data — the adapter applies it in `onBindViewHolder`.
 */
data class EventDisplayConfig(
    val compact: Boolean = true,
    val showBadge: Boolean = true,
    val showTime: Boolean = true,
    val singleLine: Boolean = true,
) {
    companion object {
        private const val PREFS = "tiktools_events_display"
        private const val KEY_COMPACT = "compact"
        private const val KEY_BADGE = "badge"
        private const val KEY_TIME = "time"
        private const val KEY_SINGLE_LINE = "single_line"

        fun load(context: Context): EventDisplayConfig {
            val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            // Defaults live in the data class: a fresh install renders the
            // minimalist row without writing anything.
            val fallback = EventDisplayConfig()
            return EventDisplayConfig(
                compact = prefs.getBoolean(KEY_COMPACT, fallback.compact),
                showBadge = prefs.getBoolean(KEY_BADGE, fallback.showBadge),
                showTime = prefs.getBoolean(KEY_TIME, fallback.showTime),
                singleLine = prefs.getBoolean(KEY_SINGLE_LINE, fallback.singleLine),
            )
        }

        fun save(context: Context, config: EventDisplayConfig) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putBoolean(KEY_COMPACT, config.compact)
                .putBoolean(KEY_BADGE, config.showBadge)
                .putBoolean(KEY_TIME, config.showTime)
                .putBoolean(KEY_SINGLE_LINE, config.singleLine)
                .apply()
        }
    }
}
