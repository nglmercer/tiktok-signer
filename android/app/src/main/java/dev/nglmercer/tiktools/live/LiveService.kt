package dev.nglmercer.tiktools.live

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import dev.nglmercer.tiktools.R
import dev.nglmercer.tiktools.app.MainActivity
import dev.nglmercer.tiktools.core.diagnostics.Logger
import dev.nglmercer.tiktools.tts.TtsController

/**
 * Foreground keep-alive for the live session: while the stream is open, this service holds an
 * ongoing notification so the system ranks the process as user-visible and does not kill the socket
 * (or the speech) when the app goes to the background or the screen turns off.
 *
 * The [LiveSessionManager] owns the connection; the service owns nothing but the notification. It
 * is started on connect, refreshed on room/TTS changes, and stopped on disconnect. Repeat / skip
 * actions ride on the notification, and — like the in-app controls — appear only while a TTS engine
 * is enabled.
 */
class LiveService : Service() {

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        ensureChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_DISCONNECT -> {
                // From the notification button: ask the live session (if any)
                // to hang up, then release the hold either way.
                Log.d(Logger.TAG, Logger.line("LIVE", "disconnect requested from notification"))
                running = false
                disconnectRequests.tryEmit(Unit)
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_STOP -> {
                Log.d(Logger.TAG, Logger.line("LIVE", "background keep-alive stopped"))
                running = false
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_REPEAT -> {
                if (!TtsController.repeatLast()) {
                    Log.d(Logger.TAG, Logger.line("TTS", "repeat: nothing to replay"))
                }
                return START_STICKY
            }
            ACTION_SKIP -> {
                TtsController.skip()
                Log.d(Logger.TAG, Logger.line("TTS", "skip: current utterance stopped"))
                return START_STICKY
            }
            ACTION_START,
            ACTION_REFRESH -> {
                // A restart with a lost intent carries no state; only keep
                // running when the session still claims to be live.
                if (intent.action == ACTION_START || running) {
                    running = true
                    if (intent.hasExtra(EXTRA_ROOM)) {
                        room = intent.getStringExtra(EXTRA_ROOM).orEmpty()
                    }
                    if (intent.hasExtra(EXTRA_TTS)) {
                        ttsEnabled = intent.getBooleanExtra(EXTRA_TTS, false)
                    }
                    startOurselves()
                } else {
                    stopSelf()
                    return START_NOT_STICKY
                }
                return START_STICKY
            }
            else -> {
                // System restart after a process kill: statics are reset, so
                // `running` is false and this stops a zombie service.
                if (!running) {
                    stopSelf()
                    return START_NOT_STICKY
                }
                startOurselves()
                return START_STICKY
            }
        }
    }

    private fun startOurselves() {
        val notification = buildNotification(room, ttsEnabled)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
        // A refresh re-posts the same ongoing notification with fresh actions.
        notificationManager().notify(NOTIFICATION_ID, notification)
    }

    private fun ensureChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel =
            NotificationChannel(
                    CHANNEL_ID,
                    getString(R.string.live_channel_name),
                    NotificationManager.IMPORTANCE_LOW,
                )
                .apply {
                    description = getString(R.string.live_channel_desc)
                    setShowBadge(false)
                }
        notificationManager().createNotificationChannel(channel)
    }

    private fun notificationManager(): NotificationManager =
        getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

    private fun buildNotification(room: String, ttsOn: Boolean): Notification {
        val openApp =
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val text =
            if (room.isEmpty()) {
                getString(R.string.live_notification_connecting)
            } else {
                getString(R.string.live_notification_live, room)
            } +
                " · " +
                getString(
                    if (ttsOn) R.string.live_notification_tts_on
                    else R.string.live_notification_tts_off
                )
        val builder =
            NotificationCompat.Builder(this, CHANNEL_ID)
                .setSmallIcon(R.drawable.ic_launcher)
                .setContentTitle(getString(R.string.app_name))
                .setContentText(text)
                .setContentIntent(openApp)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setCategory(NotificationCompat.CATEGORY_TRANSPORT)
        if (ttsOn) {
            builder.addAction(
                R.drawable.ic_play,
                getString(R.string.repeat_speech),
                actionIntent(ACTION_REPEAT, 1),
            )
            builder.addAction(
                R.drawable.ic_close,
                getString(R.string.skip_speech),
                actionIntent(ACTION_SKIP, 2),
            )
        }
        builder.addAction(
            R.drawable.ic_delete,
            getString(R.string.disconnect),
            actionIntent(ACTION_DISCONNECT, 3),
        )
        return builder.build()
    }

    private fun actionIntent(action: String, code: Int): PendingIntent =
        PendingIntent.getService(
            this,
            code,
            Intent(this, LiveService::class.java).setAction(action),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

    companion object {
        const val CHANNEL_ID = "tiktools_live"
        const val NOTIFICATION_ID = 41

        const val ACTION_START = "com.example.ttlsigner.live.START"
        const val ACTION_STOP = "com.example.ttlsigner.live.STOP"
        const val ACTION_DISCONNECT = "com.example.ttlsigner.live.DISCONNECT"
        const val ACTION_REFRESH = "com.example.ttlsigner.live.REFRESH"
        const val ACTION_REPEAT = "com.example.ttlsigner.live.REPEAT"
        const val ACTION_SKIP = "com.example.ttlsigner.live.SKIP"

        private const val EXTRA_ROOM = "room"
        private const val EXTRA_TTS = "tts"

        /**
         * One shot per notification-tap disconnect. The session collects this and hangs up its
         * stream; when no session is alive there is nothing to hang up and the emission simply
         * expires.
         */
        val disconnectRequests =
            kotlinx.coroutines.flow.MutableSharedFlow<Unit>(extraBufferCapacity = 1)

        /** Last published state; doubles as the zombie check after a kill. */
        @Volatile
        var running: Boolean = false
            private set

        @Volatile private var room: String = ""
        @Volatile private var ttsEnabled: Boolean = false

        /** Hold the process while the stream is open. */
        fun start(context: Context, room: String, ttsEnabled: Boolean) {
            this.room = room
            this.ttsEnabled = ttsEnabled
            running = true
            ContextCompat.startForegroundService(
                context,
                Intent(context, LiveService::class.java)
                    .setAction(ACTION_START)
                    .putExtra(EXTRA_ROOM, room)
                    .putExtra(EXTRA_TTS, ttsEnabled),
            )
        }

        /** Republish the notification after a room/TTS change. No-op when idle. */
        fun refresh(context: Context, room: String, ttsEnabled: Boolean) {
            if (!running) return
            this.room = room
            this.ttsEnabled = ttsEnabled
            context.startService(
                Intent(context, LiveService::class.java)
                    .setAction(ACTION_REFRESH)
                    .putExtra(EXTRA_ROOM, room)
                    .putExtra(EXTRA_TTS, ttsEnabled)
            )
        }

        /** Release the process hold. Safe to call when not running. */
        fun stop(context: Context) {
            if (!running) return
            running = false
            context.startService(Intent(context, LiveService::class.java).setAction(ACTION_STOP))
        }
    }
}
