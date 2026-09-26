package com.example.ttlsigner.points

import android.content.Context
import com.example.ttlsigner.data.StudioDb
import com.example.ttlsigner.events.LiveEvent
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Point balances over [StudioDb], plus the editable [PointsConfig].
 * The view model calls [award] for every live event; the Points tab renders
 * [leaderboard] and edits [config]. All SQLite work runs on [Dispatchers.IO].
 */
class PointsRepository(context: Context, db: StudioDb = StudioDb(context)) {

    private val app = context.applicationContext
    private val database = db
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private val _leaderboard = MutableStateFlow<List<StudioDb.Viewer>>(emptyList())
    val leaderboard: StateFlow<List<StudioDb.Viewer>> = _leaderboard.asStateFlow()

    private val _config = MutableStateFlow(PointsConfig.load(app))
    val config: StateFlow<PointsConfig> = _config.asStateFlow()

    private val _count = MutableStateFlow(0)
    val count: StateFlow<Int> = _count.asStateFlow()

    init {
        refresh()
    }

    fun refresh() {
        scope.launch { reload() }
    }

    private suspend fun reload() = withContext(Dispatchers.IO) {
        _leaderboard.value = database.leaderboard()
        _count.value = database.viewerCount()
    }

    data class Award(
        val uniqueId: String,
        val delta: Double,
        val total: Double,
        val level: Int,
    )

    /**
     * Award points for one event per the current config. Returns the award, or
     * null when the event earns nothing (disabled rate, no user, …).
     */
    suspend fun award(event: LiveEvent): Award? = withContext(Dispatchers.IO) {
        val cfg = _config.value
        val delta = PointsEngine.delta(event, cfg)
        if (delta == 0.0 || event.user.isEmpty()) return@withContext null
        val current = database.getViewer(event.user)
        val total = ((current?.points ?: 0.0) + delta).coerceAtLeast(0.0)
        val level = PointsEngine.level(total, cfg)
        database.upsertViewer(
            StudioDb.Viewer(
                uniqueId = event.user,
                nickname = event.nickname.ifEmpty { current?.nickname ?: "" },
                points = total,
                level = level,
                updatedAt = System.currentTimeMillis(),
            ),
        )
        reload()
        Award(event.user, delta, total, level)
    }

    /** Manual adjustment from the Points tab; negative values are allowed. */
    suspend fun adjust(uniqueId: String, amount: Double): Award? =
        withContext(Dispatchers.IO) {
            val id = uniqueId.trim().trimStart('@')
            if (id.isEmpty() || amount == 0.0) return@withContext null
            val cfg = _config.value
            val current = database.getViewer(id)
            val total = ((current?.points ?: 0.0) + amount).coerceAtLeast(0.0)
            val level = PointsEngine.level(total, cfg)
            database.upsertViewer(
                StudioDb.Viewer(
                    uniqueId = id,
                    nickname = current?.nickname ?: "",
                    points = total,
                    level = level,
                    updatedAt = System.currentTimeMillis(),
                ),
            )
            reload()
            Award(id, amount, total, level)
        }

    fun updateConfig(next: PointsConfig) {
        val clean = next.normalize()
        PointsConfig.save(app, clean)
        _config.value = clean
    }

    fun reset(uniqueId: String? = null) {
        scope.launch {
            withContext(Dispatchers.IO) { database.resetViewers(uniqueId) }
            reload()
        }
    }
}
