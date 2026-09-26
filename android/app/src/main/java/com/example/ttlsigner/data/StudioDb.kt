package com.example.ttlsigner.data

import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper

/**
 * The app's only database: viewer point balances, fetch-only event actions,
 * and their run log. Plain `SQLiteOpenHelper` on purpose — small, no
 * annotation processor, and every table stays readable in one file.
 *
 * Tables:
 * - `viewers(unique_id PK, nickname, points, level, updated_at)`
 * - `actions(id PK, name, enabled, triggers CSV, method, url, body, cooldown_secs)`
 * - `runs(id PK, action_id, at, status, ms, detail)` — capped ring log.
 */
class StudioDb(context: Context) : SQLiteOpenHelper(context, NAME, null, VERSION) {

    companion object {
        const val NAME = "tiktools-studio.db"
        const val VERSION = 1
        const val MAX_RUNS = 100
    }

    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL(
            """CREATE TABLE viewers(
              unique_id TEXT PRIMARY KEY,
              nickname TEXT NOT NULL DEFAULT '',
              points REAL NOT NULL DEFAULT 0,
              level INTEGER NOT NULL DEFAULT 1,
              updated_at INTEGER NOT NULL DEFAULT 0)""",
        )
        db.execSQL(
            """CREATE TABLE actions(
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              enabled INTEGER NOT NULL DEFAULT 1,
              triggers TEXT NOT NULL,
              method TEXT NOT NULL DEFAULT 'GET',
              url TEXT NOT NULL,
              body TEXT NOT NULL DEFAULT '',
              cooldown_secs INTEGER NOT NULL DEFAULT 0)""",
        )
        db.execSQL(
            """CREATE TABLE runs(
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              action_id INTEGER NOT NULL,
              at INTEGER NOT NULL,
              status TEXT NOT NULL,
              ms INTEGER NOT NULL DEFAULT 0,
              detail TEXT NOT NULL DEFAULT '')""",
        )
        db.execSQL("CREATE INDEX idx_viewers_points ON viewers(points DESC)")
        db.execSQL("CREATE INDEX idx_runs_at ON runs(at DESC)")
    }

    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        // Version 1 is the first schema; later versions migrate here.
    }

    // Viewers.

    data class Viewer(
        val uniqueId: String,
        val nickname: String,
        val points: Double,
        val level: Int,
        val updatedAt: Long,
    )

    fun upsertViewer(viewer: Viewer) {
        val values = ContentValues().apply {
            put("unique_id", viewer.uniqueId)
            put("nickname", viewer.nickname)
            put("points", viewer.points)
            put("level", viewer.level)
            put("updated_at", viewer.updatedAt)
        }
        writableDatabase.insertWithOnConflict(
            "viewers", null, values, SQLiteDatabase.CONFLICT_REPLACE,
        )
    }

    fun getViewer(uniqueId: String): Viewer? {
        readableDatabase.rawQuery(
            "SELECT unique_id,nickname,points,level,updated_at FROM viewers WHERE unique_id=?",
            arrayOf(uniqueId),
        ).use { c ->
            if (!c.moveToFirst()) return null
            return readViewer(c)
        }
    }

    fun leaderboard(limit: Int = 100): List<Viewer> {
        readableDatabase.rawQuery(
            "SELECT unique_id,nickname,points,level,updated_at FROM viewers " +
                "ORDER BY points DESC LIMIT ?",
            arrayOf(limit.coerceIn(1, 1000).toString()),
        ).use { c ->
            val out = ArrayList<Viewer>(c.count)
            while (c.moveToNext()) out.add(readViewer(c))
            return out
        }
    }

    fun viewerCount(): Int {
        readableDatabase.rawQuery("SELECT COUNT(*) FROM viewers", null).use { c ->
            return if (c.moveToFirst()) c.getInt(0) else 0
        }
    }

    fun resetViewers(uniqueId: String? = null) {
        if (uniqueId == null) {
            writableDatabase.execSQL("UPDATE viewers SET points=0, level=1")
        } else {
            writableDatabase.execSQL(
                "UPDATE viewers SET points=0, level=1 WHERE unique_id=?",
                arrayOf(uniqueId),
            )
        }
    }

    private fun readViewer(c: Cursor): Viewer = Viewer(
        uniqueId = c.getString(0),
        nickname = c.getString(1),
        points = c.getDouble(2),
        level = c.getInt(3),
        updatedAt = c.getLong(4),
    )

    // Actions.

    data class ActionRow(
        val id: Long,
        val name: String,
        val enabled: Boolean,
        val triggers: String,
        val method: String,
        val url: String,
        val body: String,
        val cooldownSecs: Long,
    )

    fun insertAction(row: ActionRow): Long {
        val values = ContentValues().apply {
            put("name", row.name)
            put("enabled", if (row.enabled) 1 else 0)
            put("triggers", row.triggers)
            put("method", row.method)
            put("url", row.url)
            put("body", row.body)
            put("cooldown_secs", row.cooldownSecs)
        }
        return writableDatabase.insert("actions", null, values)
    }

    fun updateAction(row: ActionRow) {
        val values = ContentValues().apply {
            put("name", row.name)
            put("enabled", if (row.enabled) 1 else 0)
            put("triggers", row.triggers)
            put("method", row.method)
            put("url", row.url)
            put("body", row.body)
            put("cooldown_secs", row.cooldownSecs)
        }
        writableDatabase.update("actions", values, "id=?", arrayOf(row.id.toString()))
    }

    fun deleteAction(id: Long) {
        writableDatabase.delete("actions", "id=?", arrayOf(id.toString()))
    }

    fun listActions(): List<ActionRow> {
        readableDatabase.rawQuery(
            "SELECT id,name,enabled,triggers,method,url,body,cooldown_secs " +
                "FROM actions ORDER BY id",
            null,
        ).use { c ->
            val out = ArrayList<ActionRow>(c.count)
            while (c.moveToNext()) {
                out.add(
                    ActionRow(
                        id = c.getLong(0),
                        name = c.getString(1),
                        enabled = c.getInt(2) != 0,
                        triggers = c.getString(3),
                        method = c.getString(4),
                        url = c.getString(5),
                        body = c.getString(6),
                        cooldownSecs = c.getLong(7),
                    ),
                )
            }
            return out
        }
    }

    // Run log (capped ring).

    data class RunRow(
        val id: Long,
        val actionId: Long,
        val at: Long,
        val status: String,
        val ms: Long,
        val detail: String,
    )

    fun insertRun(actionId: Long, at: Long, status: String, ms: Long, detail: String) {
        val values = ContentValues().apply {
            put("action_id", actionId)
            put("at", at)
            put("status", status)
            put("ms", ms)
            put("detail", detail.take(300))
        }
        val db = writableDatabase
        db.insert("runs", null, values)
        db.execSQL(
            "DELETE FROM runs WHERE id NOT IN " +
                "(SELECT id FROM runs ORDER BY id DESC LIMIT $MAX_RUNS)",
        )
    }

    fun recentRuns(limit: Int = 30): List<RunRow> {
        readableDatabase.rawQuery(
            "SELECT id,action_id,at,status,ms,detail FROM runs " +
                "ORDER BY id DESC LIMIT ?",
            arrayOf(limit.coerceIn(1, MAX_RUNS).toString()),
        ).use { c ->
            val out = ArrayList<RunRow>(c.count)
            while (c.moveToNext()) {
                out.add(
                    RunRow(
                        id = c.getLong(0),
                        actionId = c.getLong(1),
                        at = c.getLong(2),
                        status = c.getString(3),
                        ms = c.getLong(4),
                        detail = c.getString(5),
                    ),
                )
            }
            return out
        }
    }
}
