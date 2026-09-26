package com.example.ttlsigner.actions

import com.example.ttlsigner.data.StudioDb
import com.example.ttlsigner.events.LiveEvent

/**
 * One fetch-only automation: when an event of a selected [triggers] category
 * lands, fire one HTTP fetch of [url] (GET) or [url]+[body] (POST).
 *
 * Deliberately fetch-only — no scripts, no plugins, no local execution. URL
 * and body are templates; see [Template] for the `{{placeholders}}`.
 */
data class EventAction(
    val id: Long = 0,
    val name: String = "",
    val enabled: Boolean = true,
    val triggers: Set<LiveEvent.Category> = setOf(LiveEvent.Category.CHAT),
    val method: Method = Method.GET,
    val url: String = "",
    val body: String = "",
    /** Per-action cooldown; 0 disables it. */
    val cooldownSecs: Long = 0,
) {
    enum class Method { GET, POST }

    /** Human trigger summary for the list row, e.g. "chat + gift". */
    fun triggerSummary(): String =
        triggers.sortedBy { it.ordinal }.joinToString(" + ") { it.name.lowercase() }

    fun toRow(): StudioDb.ActionRow = StudioDb.ActionRow(
        id = id,
        name = name,
        enabled = enabled,
        triggers = triggers.sortedBy { it.ordinal }.joinToString(",") { it.name },
        method = method.name,
        url = url,
        body = body,
        cooldownSecs = cooldownSecs.coerceAtLeast(0),
    )

    companion object {
        fun fromRow(row: StudioDb.ActionRow): EventAction = EventAction(
            id = row.id,
            name = row.name,
            enabled = row.enabled,
            triggers = row.triggers.split(",")
                .mapNotNull { runCatching { LiveEvent.Category.valueOf(it) }.getOrNull() }
                .toSet().ifEmpty { setOf(LiveEvent.Category.CHAT) },
            method = runCatching { Method.valueOf(row.method) }.getOrDefault(Method.GET),
            url = row.url,
            body = row.body,
            cooldownSecs = row.cooldownSecs.coerceAtLeast(0),
        )
    }
}

/**
 * `{{placeholder}}` substitution for action URLs and bodies. Pure and total:
 * unknown placeholders stay untouched so a typo is visible in the run log
 * instead of silently becoming empty.
 *
 * Supported: `{{user}}` `@handle`, `{{name}}` nickname-or-handle, `{{text}}`
 * (chat comment / gift name), `{{type}}` lowercase category, `{{count}}`,
 * `{{diamonds}}`.
 */
object Template {
    private val PLACEHOLDER = Regex("\\{\\{\\s*([a-zA-Z]+)\\s*\\}\\}")

    fun render(template: String, event: LiveEvent): String =
        PLACEHOLDER.replace(template) { match ->
            when (match.groupValues[1].lowercase()) {
                "user" -> "@${event.user}"
                "name" -> event.nickname.ifEmpty { event.user.ifEmpty { "-" } }
                "text" -> event.text
                "type" -> event.category.name.lowercase()
                "count" -> event.count.toString()
                "diamonds" -> event.diamonds.toString()
                else -> match.value
            }
        }
}

/** Pure trigger + cooldown matching, so the runner stays thin and testable. */
object ActionMatcher {
    fun matches(action: EventAction, event: LiveEvent, nowMs: Long, lastFiredMs: Long?): Boolean {
        if (!action.enabled) return false
        if (event.category !in action.triggers) return false
        if (action.url.isBlank()) return false
        val cooldownMs = action.cooldownSecs * 1000
        if (cooldownMs > 0 && lastFiredMs != null && nowMs - lastFiredMs < cooldownMs) return false
        return true
    }
}
