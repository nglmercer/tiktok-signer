package dev.nglmercer.tiktools.data.rewards

import dev.nglmercer.tiktools.core.model.LiveEvent
import dev.nglmercer.tiktools.data.database.*
import dev.nglmercer.tiktools.data.preferences.PreferencesRepository
import java.util.concurrent.Callable
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

class PointsRepository(
    private val db: TikToolsDatabase,
    private val preferences: PreferencesRepository,
    private val scope: CoroutineScope,
) {
    val leaderboard =
        db.viewers()
            .observeLeaderboard()
            .stateIn(scope, SharingStarted.WhileSubscribed(5000), emptyList())
    val count = db.viewers().observeCount().stateIn(scope, SharingStarted.WhileSubscribed(5000), 0)
    val config =
        preferences.state.map { it.rewards }.stateIn(scope, SharingStarted.Eagerly, PointsConfig())

    data class Award(val uniqueId: String, val delta: Double, val total: Double, val level: Int)

    suspend fun award(event: LiveEvent): Award? {
        val cfg = preferences.values.first().rewards
        val delta = PointsEngine.delta(event, cfg)
        if (delta == 0.0 || event.user.isBlank()) return null
        return apply(event.user, event.nickname, delta, event.category.name, cfg)
    }

    suspend fun adjust(uniqueId: String, amount: Double): Award? {
        require(amount.isFinite()) { "Enter a finite amount" }
        val id = uniqueId.trim().removePrefix("@")
        if (id.isBlank() || amount == 0.0) return null
        return apply(id, "", amount, "ADJUSTMENT", preferences.values.first().rewards)
    }

    private suspend fun apply(
        id: String,
        nickname: String,
        amount: Double,
        category: String,
        cfg: PointsConfig,
    ): Award =
        withContext(Dispatchers.IO) {
            db.runInTransaction(
                Callable {
                    val current = db.viewers().find(id)
                    val total = ((current?.points ?: 0.0) + amount).coerceAtLeast(0.0)
                    val delta = total - (current?.points ?: 0.0)
                    val level = PointsEngine.level(total, cfg)
                    db.viewers()
                        .upsert(
                            Viewer(
                                id,
                                nickname.ifEmpty { current?.nickname.orEmpty() },
                                total,
                                level,
                                System.currentTimeMillis(),
                            )
                        )
                    val previous = db.viewers().earning(id, category)?.amount ?: 0.0
                    db.viewers().upsertEarning(Earning(id, category, previous + delta))
                    Award(id, delta, total, level)
                }
            )
        }

    fun earnings(id: String): Flow<List<Earning>> = db.viewers().observeEarnings(id)

    suspend fun updateConfig(next: PointsConfig) = preferences.setRewards(next)

    suspend fun reset(id: String? = null) =
        withContext(Dispatchers.IO) {
            db.runInTransaction {
                db.viewers().reset(id)
                db.viewers().resetEarnings(id)
            }
        }
}
