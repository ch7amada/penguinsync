package org.penguinsync.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.app.ActivityCompat
import org.penguinsync.app.ui.ConnectionStatus
import org.penguinsync.app.ui.LogEntry
import org.penguinsync.app.ui.LogSeverity
import org.penguinsync.app.ui.PenguinSyncScaffold
import org.penguinsync.app.ui.reduce
import org.penguinsync.app.ui.theme.PenguinSyncTheme
import uniffi.penguinsync.CoreEvent
import uniffi.penguinsync.PairedDevice

/// The app's four screens (docs/design.md §4.6, §9): Devices, Pair, Settings,
/// Debug, behind a bottom nav bar ([PenguinSyncScaffold]). Connection state
/// lives in [PenguinSyncApp], not here, so it survives this Activity being
/// destroyed and recreated — the QS tile and notification action
/// ([ClipboardReadActivity]) need to reach the same live session this screen
/// does, whether or not this screen is even open. This Activity's own state
/// (the event log, the folded [ConnectionStatus], the paired-device list) is
/// just a read of that stream, rebuilt fresh every time this screen starts.
class MainActivity : ComponentActivity() {
    private lateinit var app: PenguinSyncApp
    private val log = mutableStateListOf<LogEntry>()
    private var connectionStatus by mutableStateOf<ConnectionStatus>(ConnectionStatus.NotPaired)
    private var pairedDevices by mutableStateOf<List<PairedDevice>>(emptyList())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Before setContent: the system bars have to be transparent from the
        // very first frame, otherwise the window opens with an opaque bar in
        // the platform theme's colour and then repaints.
        enableEdgeToEdge()
        app = application as PenguinSyncApp

        // Needed for the connected-device notification's "Send clipboard"
        // action to show up at all on API 33+ (docs/design.md §6.1's
        // Baseline tier); the QS tile and this screen's own button work
        // without it either way, so a denial here isn't fatal. The Settings
        // screen offers the same request again for anyone who denies it here.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ActivityCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            ActivityCompat.requestPermissions(this, arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
        }

        setContent {
            PenguinSyncTheme(dynamicColor = app.useDynamicColor) {
                PenguinSyncScaffold(
                    fingerprint = app.core.deviceFingerprint(),
                    connectionStatus = connectionStatus,
                    pairedDevices = pairedDevices,
                    log = log,
                    dynamicColor = app.useDynamicColor,
                    onDynamicColorChange = app::setDynamicColor,
                    onPair = ::startPairing,
                    onSendClipboard = ::sendClipboardNow,
                    onClearLog = log::clear,
                )
            }
        }
    }

    override fun onStart() {
        super.onStart()
        pairedDevices = app.pairedDevices()
        // Read the real current state up front — this Activity instance is
        // fresh (e.g. reopened after being swiped from Recents while the
        // foreground service kept the process alive) and would otherwise
        // sit on the [ConnectionStatus.NotPaired] default until the next
        // event happened to arrive, even though the connection itself never
        // dropped (docs/design.md §4.6).
        connectionStatus = app.connectionStatus
        app.uiListener = { event -> runOnUiThread { onCoreEvent(event) } }
    }

    override fun onStop() {
        app.uiListener = null
        super.onStop()
    }

    private fun onCoreEvent(event: CoreEvent) {
        log.add(0, app.describe(event))
        connectionStatus = connectionStatus.reduce(event)
        // A fresh handshake means a peer was just persisted (or re-persisted)
        // to peers.json (docs/design.md §4.6) — cheap enough to just re-read.
        if (event is CoreEvent.PeerHandshake) pairedDevices = app.pairedDevices()
    }

    private fun startPairing(qrUri: String) {
        app.startPairing(qrUri).onFailure { e ->
            log.add(0, LogEntry.now("pair() failed: ${e.message}", LogSeverity.BAD))
        }
    }

    /// This is called from the Devices screen, which already has window
    /// focus by definition, so the in-app button reads and sends directly —
    /// no trampoline needed, unlike the QS tile and notification action
    /// (docs/design.md §6.1's Baseline tier; contrast [ClipboardReadActivity]).
    private fun sendClipboardNow() {
        val entry =
            when (val result = app.sendClipboardFromFocusedContext(this)) {
                is SendResult.Sent -> LogEntry.now("clipboard sent to Linux", LogSeverity.GOOD)
                is SendResult.NothingToSend ->
                    LogEntry.now(
                        "clipboard is empty or marked sensitive; nothing sent",
                        LogSeverity.INFO,
                    )
                is SendResult.Failed -> LogEntry.now("send failed: ${result.reason}", LogSeverity.BAD)
            }
        log.add(0, entry)
    }
}
