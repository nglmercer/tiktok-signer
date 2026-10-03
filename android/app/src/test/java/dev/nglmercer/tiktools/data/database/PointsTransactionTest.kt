package dev.nglmercer.tiktools.data.database

import android.app.Application
import android.content.Context
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.preferencesDataStoreFile
import androidx.room.Room
import androidx.test.core.app.ApplicationProvider
import dev.nglmercer.tiktools.data.preferences.PreferencesRepository
import dev.nglmercer.tiktools.data.rewards.PointsRepository
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class)
class PointsTransactionTest {
    @Test
    fun simultaneousAdjustmentsRetainEveryDeltaAndResetClearsAttribution() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val db = Room.inMemoryDatabaseBuilder(context, TikToolsDatabase::class.java).build()
        try {
            val store =
                PreferenceDataStoreFactory.create(scope = scope) {
                    context.preferencesDataStoreFile(
                        "points-transaction-${java.util.UUID.randomUUID()}"
                    )
                }
            val points = PointsRepository(db, PreferencesRepository(context, scope, store), scope)
            coroutineScope {
                repeat(40) { launch(Dispatchers.Default) { points.adjust("creator", 2.5) } }
            }
            withContext(Dispatchers.IO) {
                assertEquals(100.0, db.viewers().find("creator")!!.points, 0.0)
                assertEquals(100.0, db.viewers().earning("creator", "ADJUSTMENT")!!.amount, 0.0)
            }
            val clamped = points.adjust("creator", -150.0)!!
            assertEquals(0.0, clamped.total, 0.0)
            assertEquals(-100.0, clamped.delta, 0.0)
            points.reset("creator")
            withContext(Dispatchers.IO) {
                assertEquals(0.0, db.viewers().find("creator")!!.points, 0.0)
                assertEquals(1, db.viewers().find("creator")!!.level)
                assertNull(db.viewers().earning("creator", "ADJUSTMENT"))
            }
        } finally {
            scope.cancel()
            db.close()
        }
    }
}
