package dev.nglmercer.tiktools.data.database

import android.content.Context
import androidx.room.Room
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase

object DatabaseFactory {
    const val NAME = "tiktools-studio.db"
    /** Adopt the legacy v1 SQLite file, retaining balances, IDs, actions and execution history. */
    val MIGRATION_1_2 =
        object : Migration(1, 2) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL(
                    "CREATE TABLE viewers_new (unique_id TEXT NOT NULL PRIMARY KEY, nickname TEXT NOT NULL DEFAULT '', points REAL NOT NULL DEFAULT 0, level INTEGER NOT NULL DEFAULT 1, updated_at INTEGER NOT NULL DEFAULT 0)"
                )
                db.execSQL(
                    "INSERT INTO viewers_new SELECT unique_id,nickname,points,level,updated_at FROM viewers WHERE unique_id IS NOT NULL"
                )
                db.execSQL("DROP TABLE viewers")
                db.execSQL("ALTER TABLE viewers_new RENAME TO viewers")
                db.execSQL("CREATE INDEX idx_viewers_points ON viewers(points DESC)")
                db.execSQL(
                    "CREATE TABLE actions_new (id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, name TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, triggers TEXT NOT NULL, method TEXT NOT NULL DEFAULT 'GET', url TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', cooldown_secs INTEGER NOT NULL DEFAULT 0, gift_name TEXT NOT NULL DEFAULT '')"
                )
                db.execSQL(
                    "INSERT INTO actions_new(id,name,enabled,triggers,method,url,body,cooldown_secs) SELECT id,name,enabled,triggers,method,url,body,cooldown_secs FROM actions"
                )
                preserveSequence(db, "actions")
                db.execSQL("DROP TABLE actions")
                db.execSQL("ALTER TABLE actions_new RENAME TO actions")
                db.execSQL(
                    "CREATE TABLE runs_new (id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, action_id INTEGER NOT NULL, at INTEGER NOT NULL, status TEXT NOT NULL, ms INTEGER NOT NULL DEFAULT 0, detail TEXT NOT NULL DEFAULT '')"
                )
                db.execSQL("INSERT INTO runs_new SELECT id,action_id,at,status,ms,detail FROM runs")
                preserveSequence(db, "runs")
                db.execSQL("DROP TABLE runs")
                db.execSQL("ALTER TABLE runs_new RENAME TO runs")
                db.execSQL("CREATE INDEX idx_runs_at ON runs(at DESC)")
                db.execSQL(
                    "CREATE TABLE earnings (user_id TEXT NOT NULL, category TEXT NOT NULL, amount REAL NOT NULL, PRIMARY KEY(user_id,category))"
                )
            }

            private fun preserveSequence(db: SupportSQLiteDatabase, table: String) {
                // An entirely deleted table has no copied rows to create its new sequence entry.
                db.execSQL(
                    "INSERT INTO sqlite_sequence(name,seq) SELECT '${table}_new',0 WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name='${table}_new')"
                )
                db.execSQL(
                    "UPDATE sqlite_sequence SET seq=MAX(seq,COALESCE((SELECT seq FROM sqlite_sequence WHERE name='$table'),0)) WHERE name='${table}_new'"
                )
            }
        }

    fun open(context: Context): TikToolsDatabase =
        Room.databaseBuilder(context.applicationContext, TikToolsDatabase::class.java, NAME)
            .addMigrations(MIGRATION_1_2)
            .build()
}
