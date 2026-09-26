package com.example.ttlsigner.actions

import android.content.Context
import com.example.ttlsigner.data.StudioDb
import com.example.ttlsigner.events.LiveEvent
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.system.measureTimeMillis

/**
 * Owns the fetch-only actions: CRUD over [StudioDb], per-action cooldowns,
 * and [fire] — match, render templates, fetch, log the run.
 *
 * The HTTP layer is a [Fetcher] so unit tests run without network; the app
 * uses [HttpFetcher] (plain `HttpURLConnection`, 10 s timeouts).
 */
class ActionRunner(context: Context, db: StudioDb, fetcher: Fetcher = HttpFetcher()) {

    interface Fetcher {
        data class Result(val ok: Boolean, val detail: String)
        suspend fun fetch(method: EventAction.Method, url: String, body: String): Result
    }

    class HttpFetcher(
        private val timeoutMs: Int = 10_000,
    ) : Fetcher {
        override suspend fun fetch(
            method: EventAction.Method,
            url: String,
            body: String,
        ): Fetcher.Result = withContext(Dispatchers.IO) {
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
                val snippet = try {
                    (if (code in 200..299) connection.inputStream else connection.errorStream)
                        ?.bufferedReader()?.use { it.readText().take(200) } ?: ""
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

    private val database = db
    private val http: Fetcher = fetcher
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val lastFired = mutableMapOf<Long, Long>()

    data class UiState(
        val actions: List<EventAction> = emptyList(),
        val runs: List<StudioDb.RunRow> = emptyList(),
    )

    private val _state = MutableStateFlow(UiState())
    val state: StateFlow<UiState> = _state.asStateFlow()

    init {
        refresh()
    }

    fun refresh() {
        scope.launch { reload() }
    }

    private suspend fun reload() = withContext(Dispatchers.IO) {
        _state.value = UiState(
            actions = database.listActions().map(EventAction::fromRow),
            runs = database.recentRuns(),
        )
    }

    fun save(action: EventAction) {
        scope.launch {
            withContext(Dispatchers.IO) {
                if (action.id == 0L) database.insertAction(action.toRow())
                else database.updateAction(action.toRow())
            }
            reload()
        }
    }

    fun delete(id: Long) {
        scope.launch {
            withContext(Dispatchers.IO) { database.deleteAction(id) }
            reload()
        }
    }

    fun setEnabled(action: EventAction, enabled: Boolean) {
        save(action.copy(enabled = enabled))
    }

    /**
     * Fire every action matching [event] (async; results land in the run log).
     * Called by the view model for each live event.
     */
    fun onEvent(event: LiveEvent) {
        val now = System.currentTimeMillis()
        val due = _state.value.actions.filter { action ->
            synchronized(lastFired) {
                ActionMatcher.matches(action, event, now, lastFired[action.id])
            }
        }
        if (due.isEmpty()) return
        scope.launch {
            for (action in due) fire(action, event)
            reload()
        }
    }

    /** Fire one action once, bypassing its cooldown (the list's test button). */
    fun test(action: EventAction, event: LiveEvent) {
        scope.launch {
            fire(action, event)
            reload()
        }
    }

    private suspend fun fire(action: EventAction, event: LiveEvent) {
        val url = Template.render(action.url, event)
        val body = Template.render(action.body, event)
        if (url.isBlank()) return
        synchronized(lastFired) { lastFired[action.id] = System.currentTimeMillis() }
        var result = Fetcher.Result(false, "not run")
        val ms = measureTimeMillis {
            result = try {
                http.fetch(action.method, url, body)
            } catch (e: Exception) {
                Fetcher.Result(false, "${e.javaClass.simpleName}: ${e.message}")
            }
        }
        withContext(Dispatchers.IO) {
            database.insertRun(
                actionId = action.id,
                at = System.currentTimeMillis(),
                status = if (result.ok) "ok" else "error",
                ms = ms,
                detail = "${event.category.name.lowercase()} @${event.user.ifEmpty { "-" }} → $url · ${result.detail}",
            )
        }
    }
}
