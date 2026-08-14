package org.penguinsync.app

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.getSystemService
import uniffi.penguinsync.CoreEvent

/// Foreground service, type `connectedDevice` (docs/design.md §4.6,
/// `AndroidManifest.xml`'s `FOREGROUND_SERVICE_CONNECTED_DEVICE` +
/// `CHANGE_WIFI_MULTICAST_STATE` pairing is exactly what the design doc
/// specifies for this type).
///
/// The entire reason this exists: confirmed live that a merely backgrounded
/// app — process still alive, not killed — got its connection dropped and
/// re-established over and over (`penguinsyncd`'s log showed repeated
/// `device disconnected` / `device reconnected` pairs the moment the app
/// left the foreground). A foreground-service-less background process is a
/// target for Android's cached-process freezer; holding a foreground
/// service keeps this process — and therefore the QUIC session's
/// keepalive/reconnect loop, which lives entirely inside Rust — actually
/// running while backgrounded. Also holds the `WifiLock` §5.3 calls for:
/// reconnect attempts need the radio awake too, not just an established
/// connection.
///
/// Started once, alongside the first successful `pair()` call
/// ([PenguinSyncApp.startPairing]), and never explicitly stopped —
/// mirrors the daemon's own always-on posture (§4.3). A future "unpair"
/// action would be the natural place to `stopSelf()`; there isn't one yet.
class PenguinSyncConnectionService : Service() {
    private var wifiLock: WifiManager.WifiLock? = null

    override fun onCreate() {
        super.onCreate()

        // Must happen within the OS's post-startForegroundService window,
        // before anything else that could plausibly be slow.
        ServiceCompat.startForeground(
            this,
            NOTIFICATION_ID,
            buildNotification("Connecting…", null),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
        )

        (application as PenguinSyncApp).serviceListener = ::onCoreEvent

        wifiLock =
            getSystemService<WifiManager>()
                ?.createWifiLock(WifiManager.WIFI_MODE_FULL_LOW_LATENCY, "penguinsync:connection")
                ?.apply {
                    setReferenceCounted(false)
                    acquire()
                }
    }

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int = START_STICKY

    override fun onDestroy() {
        (application as PenguinSyncApp).serviceListener = null
        wifiLock?.release()
        wifiLock = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun onCoreEvent(event: CoreEvent) {
        val (title, sendActionAvailable) =
            when (event) {
                is CoreEvent.PeerHandshake -> "Connected to ${event.name}" to true
                is CoreEvent.Reconnecting -> "Reconnecting…" to false
                is CoreEvent.Disconnected -> "Disconnected: ${event.reason}" to false
                else -> return
            }
        getSystemService(NotificationManager::class.java)
            .notify(NOTIFICATION_ID, buildNotification(title, if (sendActionAvailable) sendAction() else null))
    }

    /// Launches [ClipboardReadActivity] — the same manual-send trigger as
    /// the QS tile, from a notification action instead (docs/design.md
    /// §6.1's Baseline tier).
    private fun sendAction(): NotificationCompat.Action {
        val intent = Intent(this, ClipboardReadActivity::class.java)
        val pendingIntent =
            PendingIntent.getActivity(
                this,
                0,
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        return NotificationCompat.Action(0, "Send clipboard", pendingIntent)
    }

    private fun buildNotification(
        title: String,
        action: NotificationCompat.Action?,
    ): Notification {
        val builder =
            NotificationCompat.Builder(this, PenguinSyncApp.CHANNEL_ID)
                .setSmallIcon(R.drawable.ic_penguinsync_clipboard)
                .setContentTitle(title)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
        if (action != null) builder.addAction(action)
        return builder.build()
    }

    companion object {
        private const val NOTIFICATION_ID = 1
    }
}
