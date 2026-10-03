package dev.nglmercer.tiktools.data.actions

import dev.nglmercer.tiktools.core.model.LiveEvent
import dev.nglmercer.tiktools.data.database.*

/**
 * One fetch-only automation: when an event of a selected [triggers] category lands, fire one HTTP
 * fetch of [url] (GET) or [url]+[body] (POST).
 *
 * Deliberately fetch-only — no scripts, no plugins, no local execution. URL and body are templates;
 * see [Template] for the `{{placeholders}}`.
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
    val giftName: String = "",
) {
    enum class Method {
        GET,
        POST,
    }

    /** Human trigger summary for the list row, e.g. "chat + gift". */
    fun triggerSummary(): String =
        triggers.sortedBy { it.ordinal }.joinToString(" + ") { it.name.lowercase() }

    val action: AutomationAction
        get() = AutomationAction.Http(method, url, body)

    fun toRow(): ActionRow =
        ActionRow(
            id,
            name,
            enabled,
            triggers.sortedBy { it.ordinal }.joinToString(",") { it.name },
            method.name,
            url,
            body,
            cooldownSecs.coerceAtLeast(0),
            giftName,
        )

    companion object {
        fun fromRow(row: ActionRow): EventAction =
            EventAction(
                id = row.id,
                name = row.name,
                enabled = row.enabled,
                triggers =
                    row.triggers
                        .split(",")
                        .mapNotNull { runCatching { LiveEvent.Category.valueOf(it) }.getOrNull() }
                        .toSet()
                        .ifEmpty { setOf(LiveEvent.Category.CHAT) },
                method = runCatching { Method.valueOf(row.method) }.getOrDefault(Method.GET),
                url = row.url,
                body = row.body,
                cooldownSecs = row.cooldownSecs.coerceAtLeast(0),
                giftName = row.giftName,
            )
    }
}

/**
 * `{{placeholder}}` substitution for action URLs and bodies. Pure and total: unknown placeholders
 * stay untouched so a typo is visible in the run log instead of silently becoming empty.
 *
 * Supported: `{{user}}` `@handle`, `{{name}}` nickname-or-handle, `{{text}}` (chat comment / gift
 * name), `{{type}}` lowercase category, `{{count}}`, `{{diamonds}}`.
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
        if (
            action.giftName.isNotBlank() &&
                (event.category != LiveEvent.Category.GIFT ||
                    !event.text.equals(action.giftName, ignoreCase = true))
        )
            return false
        val cooldownMs = action.cooldownSecs * 1000
        if (cooldownMs > 0 && lastFiredMs != null && nowMs - lastFiredMs < cooldownMs) return false
        return true
    }
}

sealed interface AutomationAction {
    data class Http(val method: EventAction.Method, val url: String, val body: String) :
        AutomationAction
}
