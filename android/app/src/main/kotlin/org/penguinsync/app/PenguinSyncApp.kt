package org.penguinsync.app

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import uniffi.penguinsync.ConnectionHandle
import uniffi.penguinsync.CoreEvent
import uniffi.penguinsync.CoreEventListener
import uniffi.penguinsync.PenguinSyncCore

/// Outcome of trying to send this device's clipboard to Linux
/// ([PenguinSyncApp.sendClipboardFromFocusedContext]) — a `Boolean` isn't
/// enough because "nothing to send" (empty/sensitive clipboard) and "sent
/// but the peer isn't there" are different messages for the tap-triggered UI
/// to show.
sealed class SendResult {
    object Sent : SendResult()

    object NothingToSend : SendResult()

    data class Failed(val reason: String) : SendResult()
}

/// App-scoped connection state (docs/design.md §4.6). Deliberately outlives
/// any single Activity: the QS tile, the notification action, and
/// [ClipboardReadActivity] (M2, docs/design.md §9) all need to reach the
/// *same* live session `MainActivity` is showing, and a clipboard tap must
/// still work while the app is merely backgrounded — not killed — with no
/// Activity on screen at all. `PenguinSyncCore.pair()`'s `ConnectionHandle`
/// is therefore held here, not by an Activity, and is no longer cancelled
/// on any Activity's `onDestroy()`.
///
/// There is still no foreground service — that's separate, later work
/// (docs/design.md §4.6). If Android kills the process outright, the
/// connection dies with it, the same as before M2; this only buys survival
/// across an Activity being destroyed and recreated, or the user leaving
/// the app on screen.
class PenguinSyncApp : Application() {
    lateinit var core: PenguinSyncCore
        private set

    var connectionHandle: ConnectionHandle? = null
        private set

    /// Set by whichever screen is currently visible, cleared when it stops.
    /// Read fresh on every event rather than captured at `pair()` time, so
    /// reassigning it (e.g. `MainActivity` recreated on rotation) re-routes
    /// future events without touching the underlying connection. Must work
    /// with this left `null` — that's the whole point of the QS tile and
    /// notification triggers.
    var uiListener: ((String) -> Unit)? = null

    override fun onCreate() {
        super.onCreate()
        core = PenguinSyncCore(filesDir.absolutePath, Build.MODEL ?: "Android")
        createNotificationChannel()
    }

    /// Starts (or restarts) pairing. Cancelling whatever the previous
    /// attempt left running matches the FFI cancellation discipline
    /// (docs/design.md §4.2) — a second pairing attempt replaces the first,
    /// it doesn't run alongside it.
    fun startPairing(qrUri: String): Result<Unit> =
        runCatching {
            connectionHandle?.cancel()
            connectionHandle =
                core.pair(
                    qrUri,
                    object : CoreEventListener {
                        override fun onEvent(event: CoreEvent) = handleCoreEvent(event)
                    },
                )
        }

    /// M2's entire send path, from the clipboard side: called only once
    /// `context` is known to have window focus (docs/design.md §3.1) —
    /// `MainActivity`'s button already has it; [ClipboardReadActivity] waits
    /// for `onWindowFocusChanged(true)` first. `core.sendClipboard` handles
    /// the size cap and MIME tagging; this function's own job is the
    /// Android-specific parts Rust can't do: the actual read, and honoring
    /// `EXTRA_IS_SENSITIVE` (docs/design.md §6.1).
    fun sendClipboardFromFocusedContext(context: Context): SendResult {
        val manager = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        if (!manager.hasPrimaryClip()) return SendResult.NothingToSend
        val clip = manager.primaryClip ?: return SendResult.NothingToSend
        if (clip.itemCount == 0) return SendResult.NothingToSend

        // EXTRA_IS_SENSITIVE clips are never synced — a sync tool that
        // silently broadcasts password-manager clips across the LAN is a
        // security incident waiting to happen (docs/design.md §6.1). The
        // flag only exists from API 33; below that there's nothing to read,
        // so nothing to exclude on.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val sensitive =
                clip.description.extras?.getBoolean(ClipDescription.EXTRA_IS_SENSITIVE) == true
            if (sensitive) return SendResult.NothingToSend
        }

        val text = clip.getItemAt(0).coerceToText(context)?.toString()
        if (text.isNullOrEmpty()) return SendResult.NothingToSend

        return try {
            core.sendClipboard(text)
            SendResult.Sent
        } catch (e: Exception) {
            SendResult.Failed(e.message ?: e.toString())
        }
    }

    private fun handleCoreEvent(event: CoreEvent) {
        when (event) {
            // Writing is unrestricted from anywhere, foreground or not
            // (docs/design.md §3.1) — M1's write path, unchanged by M2.
            is CoreEvent.ClipboardReceived -> writeToClipboard(event.text)
            is CoreEvent.PeerHandshake -> showConnectedNotification(event.name)
            is CoreEvent.Disconnected -> cancelConnectedNotification()
            else -> {}
        }
        uiListener?.invoke(describe(event))
    }

    private fun writeToClipboard(text: String) {
        val manager = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        manager.setPrimaryClip(ClipData.newPlainText("PenguinSync", text))
    }

    private fun createNotificationChannel() {
        val channel =
            NotificationChannel(
                CHANNEL_ID,
                "PenguinSync connection",
                NotificationManager.IMPORTANCE_LOW,
            ).apply {
                description = "Shows the connected device and a manual clipboard-send action"
            }
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    /// The notification IS the M2 trigger (docs/design.md §6.1's Baseline
    /// row): an ongoing, low-priority notification with one action, "Send
    /// clipboard", that launches [ClipboardReadActivity]. Silently does
    /// nothing if POST_NOTIFICATIONS was never granted (or was revoked) —
    /// the QS tile and in-app button still work either way.
    private fun showConnectedNotification(deviceName: String) {
        val manager = NotificationManagerCompat.from(this)
        if (!manager.areNotificationsEnabled()) return

        val sendIntent = Intent(this, ClipboardReadActivity::class.java)
        val sendPendingIntent =
            PendingIntent.getActivity(
                this,
                0,
                sendIntent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )

        val notification =
            NotificationCompat.Builder(this, CHANNEL_ID)
                .setSmallIcon(R.drawable.ic_penguinsync_clipboard)
                .setContentTitle("Connected to $deviceName")
                .setContentText("Tap to send this phone's clipboard")
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .addAction(0, "Send clipboard", sendPendingIntent)
                .build()

        try {
            manager.notify(CONNECTED_NOTIFICATION_ID, notification)
        } catch (e: SecurityException) {
            // POST_NOTIFICATIONS revoked between the check above and here —
            // a narrow but real race. Not worth crashing the connection
            // path over a notification.
        }
    }

    private fun cancelConnectedNotification() {
        NotificationManagerCompat.from(this).cancel(CONNECTED_NOTIFICATION_ID)
    }

    private fun describe(event: CoreEvent): String =
        when (event) {
            is CoreEvent.PeerHandshake -> "✓ connected to ${event.name} (${event.deviceId.take(16)}…)"
            is CoreEvent.Ponged -> "  ping: ${event.rttMs} ms"
            is CoreEvent.Disconnected -> "✗ disconnected: ${event.reason}"
            is CoreEvent.Reconnecting -> "↻ reconnecting (attempt ${event.attempt}, in ${event.delayMs} ms)"
            is CoreEvent.ClipboardReceived -> "📋 clipboard updated (${event.text.length} chars)"
        }

    companion object {
        private const val CHANNEL_ID = "penguinsync-connection"
        private const val CONNECTED_NOTIFICATION_ID = 1
    }
}
