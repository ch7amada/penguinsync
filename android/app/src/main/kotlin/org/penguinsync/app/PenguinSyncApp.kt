package org.penguinsync.app

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.os.Build
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import org.penguinsync.app.ui.ConnectionStatus
import org.penguinsync.app.ui.LogEntry
import org.penguinsync.app.ui.LogSeverity
import org.penguinsync.app.ui.reduce
import uniffi.penguinsync.ConnectionHandle
import uniffi.penguinsync.CoreEvent
import uniffi.penguinsync.CoreEventListener
import uniffi.penguinsync.PairedDevice
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
/// [PenguinSyncConnectionService] is started alongside the first successful
/// `pair()` call and owns keeping the process itself alive in the
/// background — confirmed live (docs/design.md §4.6): without it, a merely
/// backgrounded app (process still running, not killed) still got its
/// connection dropped and reconnected repeatedly, because a
/// foreground-service-less background process is a target for Android's
/// cached-process freezer. That service also owns the connected-device
/// notification now, since a foreground service must have one anyway.
class PenguinSyncApp : Application() {
    lateinit var core: PenguinSyncCore
        private set

    var connectionHandle: ConnectionHandle? = null
        private set

    /// Folded connection state, kept app-wide instead of inside whichever
    /// Activity happens to be alive. Updated unconditionally in
    /// [handleCoreEvent] — not only while a [uiListener] is attached — so a
    /// freshly (re)created `MainActivity` (e.g. reopened after being swiped
    /// from Recents while the foreground service kept the process itself
    /// alive, docs/design.md §4.6) can read the *real* current state instead
    /// of the [ConnectionStatus.NotPaired] a bare `mutableStateOf` default
    /// would otherwise show until the next event happened to arrive.
    var connectionStatus: ConnectionStatus = ConnectionStatus.NotPaired
        private set

    /// Set by whichever screen is currently visible, cleared when it stops.
    /// Read fresh on every event rather than captured at `pair()` time, so
    /// reassigning it (e.g. `MainActivity` recreated on rotation) re-routes
    /// future events without touching the underlying connection. Must work
    /// with this left `null` — that's the whole point of the QS tile and
    /// notification triggers. Raw [CoreEvent], not a formatted string: the
    /// UI now has both a Debug screen (wants a log line) and a Devices
    /// screen (wants structured connection status) reading the same stream,
    /// so formatting is their job, not this dispatcher's.
    var uiListener: ((CoreEvent) -> Unit)? = null

    /// Set by [PenguinSyncConnectionService] while it's alive, so it can
    /// keep its foreground notification current. Same "must work `null`"
    /// rule as [uiListener], though in practice the service is expected to
    /// outlive any single event once a connection exists.
    var serviceListener: ((CoreEvent) -> Unit)? = null

    private lateinit var prefs: SharedPreferences

    /// Whether to theme the app from the wallpaper (Material You) instead of
    /// PenguinSync's own palette. Every device this app runs on supports
    /// dynamic colour (minSdk 31), so this is purely a taste setting; the
    /// brand palette is the default.
    ///
    /// Plain `SharedPreferences` rather than the DataStore the design doc
    /// names as baseline (docs/design.md §4.6), and deliberately so: DataStore
    /// only hands out its value through a `Flow`, which Compose has to collect
    /// with some initial value, which means every cold start would visibly
    /// repaint from the brand palette to the user's wallpaper colours. A
    /// single boolean read synchronously before the first frame has no such
    /// problem. DataStore earns its place at M3, when there is per-device
    /// state worth the machinery.
    var useDynamicColor: Boolean by mutableStateOf(false)
        private set

    override fun onCreate() {
        super.onCreate()
        core = PenguinSyncCore(filesDir.absolutePath, Build.MODEL ?: "Android")
        prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
        useDynamicColor = prefs.getBoolean(KEY_DYNAMIC_COLOR, false)
        createNotificationChannel()
    }

    fun setDynamicColor(enabled: Boolean) {
        useDynamicColor = enabled
        prefs.edit { putBoolean(KEY_DYNAMIC_COLOR, enabled) }
    }

    /// Starts (or restarts) pairing. Cancelling whatever the previous
    /// attempt left running matches the FFI cancellation discipline
    /// (docs/design.md §4.2) — a second pairing attempt replaces the first,
    /// it doesn't run alongside it. Also starts the foreground service —
    /// safe to call every time, a second `startForegroundService` on an
    /// already-running service is a no-op beyond redelivering `onStartCommand`.
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
            ContextCompat.startForegroundService(
                this,
                Intent(this, PenguinSyncConnectionService::class.java),
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

    /// Devices screen's read of "who have I paired with" (docs/design.md
    /// §4.6, §9's four screens) — every persisted peer, connected or not.
    /// A thin pass-through; `core` already does the file read.
    fun pairedDevices(): List<PairedDevice> = core.listPairedDevices()

    private fun handleCoreEvent(event: CoreEvent) {
        // Writing is unrestricted from anywhere, foreground or not
        // (docs/design.md §3.1) — M1's write path, unchanged by M2.
        if (event is CoreEvent.ClipboardReceived) writeToClipboard(event.text)
        connectionStatus = connectionStatus.reduce(event)
        uiListener?.invoke(event)
        serviceListener?.invoke(event)
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
                description = "Shows the connection status and a manual clipboard-send action"
            }
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    /// Shared by the Debug screen's event log — one formatting rule, read by
    /// whichever screen is currently subscribed to [uiListener]. Returns a
    /// structured [LogEntry] rather than a bare string so the screen can
    /// colour a failure without pattern-matching the text back apart.
    fun describe(event: CoreEvent): LogEntry =
        when (event) {
            is CoreEvent.PeerHandshake ->
                LogEntry.now(
                    "connected to ${event.name} (${event.deviceId.take(16)}…)",
                    LogSeverity.GOOD,
                )
            is CoreEvent.Ponged -> LogEntry.now("ping ${event.rttMs} ms", LogSeverity.INFO)
            is CoreEvent.Disconnected ->
                LogEntry.now("disconnected: ${event.reason}", LogSeverity.BAD)
            is CoreEvent.Reconnecting ->
                LogEntry.now(
                    "reconnecting (attempt ${event.attempt}, in ${event.delayMs} ms)",
                    LogSeverity.WARN,
                )
            is CoreEvent.ClipboardReceived ->
                LogEntry.now("clipboard updated (${event.text.length} chars)", LogSeverity.INFO)
        }

    companion object {
        /// Shared with [PenguinSyncConnectionService], which owns the actual
        /// notification built on this channel.
        const val CHANNEL_ID = "penguinsync-connection"

        private const val PREFS_NAME = "penguinsync-ui"
        private const val KEY_DYNAMIC_COLOR = "dynamic_color"
    }
}
