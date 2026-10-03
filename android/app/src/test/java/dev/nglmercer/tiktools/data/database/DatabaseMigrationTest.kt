package dev.nglmercer.tiktools.data.database

import android.app.Application
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class)
class DatabaseMigrationTest {
    @Test
    fun legacyBalancesAutomationIdsAndHistorySurviveRoomAdoption() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        context.deleteDatabase(DatabaseFactory.NAME)
        context.openOrCreateDatabase(DatabaseFactory.NAME, Context.MODE_PRIVATE, null).use { db ->
            db.execSQL(
                "CREATE TABLE viewers(unique_id TEXT PRIMARY KEY,nickname TEXT NOT NULL DEFAULT '',points REAL NOT NULL DEFAULT 0,level INTEGER NOT NULL DEFAULT 1,updated_at INTEGER NOT NULL DEFAULT 0)"
            )
            db.execSQL(
                "CREATE TABLE actions(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL,enabled INTEGER NOT NULL DEFAULT 1,triggers TEXT NOT NULL,method TEXT NOT NULL DEFAULT 'GET',url TEXT NOT NULL,body TEXT NOT NULL DEFAULT '',cooldown_secs INTEGER NOT NULL DEFAULT 0)"
            )
            db.execSQL(
                "CREATE TABLE runs(id INTEGER PRIMARY KEY AUTOINCREMENT,action_id INTEGER NOT NULL,at INTEGER NOT NULL,status TEXT NOT NULL,ms INTEGER NOT NULL DEFAULT 0,detail TEXT NOT NULL DEFAULT '')"
            )
            db.execSQL("CREATE INDEX idx_viewers_points ON viewers(points DESC)")
            db.execSQL("CREATE INDEX idx_runs_at ON runs(at DESC)")
            db.execSQL("INSERT INTO viewers VALUES('creator','Creator',1250.5,13,42)")
            db.execSQL(
                "INSERT INTO actions VALUES(41,'Rose',1,'GIFT','POST','https://example.test','{}',5)"
            )
            db.execSQL(
                "INSERT INTO actions VALUES(90,'Deleted',1,'CHAT','GET','https://example.test','',0)"
            )
            db.execSQL("DELETE FROM actions WHERE id=90")
            db.execSQL("INSERT INTO runs VALUES(21,41,42,'ok',20,'old run')")
            db.version = 1
        }
        val room = DatabaseFactory.open(context)
        try {
            withContext(Dispatchers.IO) {
                assertEquals(1250.5, room.viewers().find("creator").points, 0.0)
            }
            val action = room.automations().observeActions().first().single()
            assertEquals(41L, action.id)
            assertEquals("", action.giftName)
            assertEquals("POST", action.method)
            assertEquals("old run", room.runs().observeRuns().first().single().detail)
            val id =
                withContext(Dispatchers.IO) {
                    room
                        .automations()
                        .save(
                            ActionRow(
                                0,
                                "New",
                                true,
                                "CHAT",
                                "GET",
                                "https://example.test",
                                "",
                                0,
                                "",
                            )
                        )
                }
            assertTrue("Migration must not reuse previously allocated IDs", id > 90)
        } finally {
            room.close()
            context.deleteDatabase(DatabaseFactory.NAME)
        }
    }
}
