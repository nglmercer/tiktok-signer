package dev.nglmercer.tiktools.data.actions

import dev.nglmercer.tiktools.core.model.LiveEvent
import dev.nglmercer.tiktools.data.database.*
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL
import kotlin.system.measureTimeMillis
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

class ActionsRepository(
    private val db: TikToolsDatabase,
    private val scope: CoroutineScope,
    private val http: Fetcher = HttpFetcher(),
) {
    interface Fetcher {
        data class Result(val ok: Boolean, val detail: String)

        suspend fun fetch(method: EventAction.Method, url: String, body: String): Result
    }

    class HttpFetcher(private val timeoutMs: Int = 10_000) : Fetcher {
        override suspend fun fetch(
            method: EventAction.Method,
            url: String,
            body: String,
        ): Fetcher.Result =
            withContext(Dispatchers.IO) {
                val connection = URL(url).openConnection() as HttpURLConnection
                try {
                    connection.connectTimeout = timeoutMs
                    connection.readTimeout = timeoutMs
                    connection.setRequestProperty("User-Agent", "TikTools-Android/1.0")
                    if (method == EventAction.Method.POST) {
                        connection.requestMethod = "POST"
                        connection.doOutput = true
                        connection.setRequestProperty("Content-Type", "application/json")
                        connection.outputStream.bufferedWriter().use { it.write(body) }
                    }
                    val code = connection.responseCode
                    val snippet =
                        try {
                            (if (code in 200..299) connection.inputStream
                                else connection.errorStream)
                                ?.bufferedReader()
                                ?.use { it.readText().take(200) } ?: ""
                        } catch (e: Exception) {
                            ""
                        }
                    Fetcher.Result(code in 200..299, "HTTP $code $snippet".trim())
                } catch (e: Exception) {
                    Fetcher.Result(false, "${e.javaClass.simpleName}: ${e.message}")
                } finally {
                    connection.disconnect()
                }
            }
    }

    private val lastFired = mutableMapOf<Long, Long>()

    data class UiState(
        val actions: List<EventAction> = emptyList(),
        val runs: List<RunRow> = emptyList(),
    )

    val state =
        combine(db.automations().observeActions(), db.runs().observeRuns()) { actions, runs ->
                UiState(actions.map(EventAction::fromRow), runs)
            }
            .stateIn(scope, SharingStarted.Eagerly, UiState())

    suspend fun save(action: EventAction) =
        withContext(Dispatchers.IO) {
            require(action.name.isNotBlank()) { "Name is required" }
            require(action.triggers.isNotEmpty()) { "Select at least one trigger" }
            val url = URI(action.url)
            require(url.scheme in listOf("http", "https") && !url.host.isNullOrBlank()) {
                "Enter an HTTP or HTTPS URL"
            }
            require(action.cooldownSecs in 0..86400) {
                "Cooldown must be between 0 and 86400 seconds"
            }
            db.automations().save(action.toRow())
        }

    suspend fun delete(id: Long) = withContext(Dispatchers.IO) { db.automations().delete(id) }

    suspend fun setEnabled(action: EventAction, enabled: Boolean) {
        save(action.copy(enabled = enabled))
    }

    fun onEvent(event: LiveEvent) {
        val now = System.currentTimeMillis()
        val due =
            synchronized(lastFired) {
                state.value.actions.filter { action ->
                    ActionMatcher.matches(action, event, now, lastFired[action.id]).also { matches
                        ->
                        if (matches) lastFired[action.id] = now
                    }
                }
            }
        for (action in due) scope.launch(Dispatchers.IO) { fire(action, event) }
    }

    suspend fun test(action: EventAction, event: LiveEvent) {
        fire(action, event)
    }

    private suspend fun fire(action: EventAction, event: LiveEvent) =
        withContext(Dispatchers.IO) {
            val url = Template.render(action.url, event)
            val body = Template.render(action.body, event)
            var result = Fetcher.Result(false, "Not run")
            val ms = measureTimeMillis {
                result =
                    try {
                        http.fetch(action.method, url, body)
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        Fetcher.Result(false, e.message.orEmpty())
                    }
            }
            db.runInTransaction {
                db.runs()
                    .insert(
                        RunRow(
                            0,
                            action.id,
                            System.currentTimeMillis(),
                            if (result.ok) "ok" else "error",
                            ms,
                            "${event.category.name.lowercase()} @${event.user} → $url · ${result.detail}"
                                .take(300),
                        )
                    )
                db.runs().trim()
            }
        }
}
