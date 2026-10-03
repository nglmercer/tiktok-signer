package dev.nglmercer.tiktools.feature.rewards

import dev.nglmercer.tiktools.app.AppContainer
import dev.nglmercer.tiktools.data.rewards.PointsConfig
import dev.nglmercer.tiktools.feature.StudioViewModel

class RewardsViewModel(private val app: AppContainer) : StudioViewModel() {
    val leaderboard = app.points.leaderboard
    val config = app.points.config

    fun earnings(id: String) = app.points.earnings(id)

    fun adjust(id: String, amount: Double) = task { app.points.adjust(id, amount) }

    fun saveRates(value: PointsConfig, onSaved: () -> Unit) = task {
        app.points.updateConfig(value)
        onSaved()
    }

    fun reset(id: String? = null) = task { app.points.reset(id) }
}
