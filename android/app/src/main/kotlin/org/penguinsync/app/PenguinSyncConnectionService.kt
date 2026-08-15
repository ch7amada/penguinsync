package org.penguinsync.app

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.content.Context
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.core.content.getSystemService
import org.penguinsync.app.ui.notificationTitle
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

    /// Also reposts the notification off the app's real current
    /// [org.penguinsync.app.ui.ConnectionStatus] every time this fires — not
    /// just at `onCreate`. That's what makes the Settings screen's "Restore
    /// notification" button (docs/design.md §6.1) work: a swiped-away
    /// notification doesn't come back on its own from [onCoreEvent] alone,
    /// since idle traffic while `Connected` is just `Ponged`s, which that
    /// event-triggered path ignores. Redelivering `onStartCommand` here (via
    /// [restore]) is a cheap, always-correct way to force a repost.
    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        val (title, sendActionAvailable) = (application as PenguinSyncApp).connectionStatus.notificationTitle()
        getSystemService(NotificationManager::class.java)
            .notify(NOTIFICATION_ID, buildNotification(title, if (sendActionAvailable) sendAction() else null))
        return START_STICKY
    }

    override fun onDestroy() {
        (application as PenguinSyncApp).serviceListener = null
        wifiLock?.release()
        wifiLock = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun onCoreEvent(event: CoreEvent) {
        // Ponged/ClipboardReceived don't change the folded status (see
        // ConnectionStatus.reduce) — skip the repost, no point renotifying
        // with identical content on every keepalive.
        when (event) {
            is CoreEvent.PeerHandshake, is CoreEvent.Reconnecting, is CoreEvent.Disconnected -> {}
            else -> return
        }
        val (title, sendActionAvailable) = (application as PenguinSyncApp).connectionStatus.notificationTitle()
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
                // Brand blue, so the ongoing notification is recognisable as
                // this app's in a shade full of them. Fixed rather than read
                // from the Compose theme: a service has no composition, and
                // this notification exists precisely when no UI does.
                .setColor(ContextCompat.getColor(this, R.color.ic_launcher_background))
                .setColorized(false)
        if (action != null) builder.addAction(action)
        return builder.build()
    }

    companion object {
        private const val NOTIFICATION_ID = 1

        /// Settings screen's "Restore notification" button. Safe to call
        /// whether or not the service is already running: if it is, this
        /// just redelivers `onStartCommand`, which reposts the notification
        /// (see above); if pairing hasn't happened yet, it's a no-op beyond
        /// the service coming up idle at [org.penguinsync.app.ui.ConnectionStatus.NotPaired].
        fun restore(context: Context) {
            ContextCompat.startForegroundService(context, Intent(context, PenguinSyncConnectionService::class.java))
        }
    }
}
