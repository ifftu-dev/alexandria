package org.alexandria.node

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
import androidx.core.app.NotificationCompat

/**
 * Foreground service that holds the app's process alive so the Rust
 * libp2p task keeps running when the user backgrounds the activity.
 *
 * Without this, Android's Doze / battery optimisation suspends network
 * access for backgrounded apps and the libp2p swarm drops every peer.
 *
 * The service does no work of its own — its sole purpose is the
 * foreground-service notification, which signals to Android "this app
 * is doing user-visible work, do not kill its process". The libp2p
 * stack runs in the same process as MainActivity.
 */
class P2pForegroundService : Service() {

    override fun onCreate() {
        super.onCreate()
        ensureChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startInForeground()
        // START_STICKY: if Android kills the service under memory pressure,
        // it will be restarted with a null intent once memory is available.
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startInForeground() {
        val pendingIntent: PendingIntent? = packageManager
            .getLaunchIntentForPackage(packageName)
            ?.let { launchIntent ->
                PendingIntent.getActivity(
                    this,
                    0,
                    launchIntent,
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                )
            }

        val notification: Notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Alexandria")
            .setContentText("P2P network running in the background")
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setOngoing(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setContentIntent(pendingIntent)
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            // Android 14+ requires foregroundServiceType to be passed both
            // in the manifest declaration AND in startForeground.
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun ensureChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (nm.getNotificationChannel(CHANNEL_ID) != null) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Alexandria network",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Keeps the peer-to-peer network connected when the app is in the background"
            setShowBadge(false)
        }
        nm.createNotificationChannel(channel)
    }

    companion object {
        private const val CHANNEL_ID = "p2p_network"
        private const val NOTIFICATION_ID = 1001

        /**
         * Convenience starter — call once from MainActivity.onCreate.
         * Idempotent; Android collapses duplicate startService calls.
         */
        fun start(context: Context) {
            val intent = Intent(context, P2pForegroundService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, P2pForegroundService::class.java))
        }
    }
}
