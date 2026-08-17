package org.penguinsync.app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.OpenableColumns
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.app.ActivityCompat
import java.io.File
import java.io.IOException
import org.penguinsync.app.ui.ConnectionStatus
import org.penguinsync.app.ui.DeviceSendPickerDialog
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
///
/// M4 adds the app's other entry point: the share sheet (docs/design.md
/// §4.6, §6.2). `singleTop` means a share while this Activity is already on
/// screen redelivers through [onNewIntent] rather than a fresh [onCreate], so
/// both are wired to the same [handleShareIntent].
class MainActivity : ComponentActivity() {
    private lateinit var app: PenguinSyncApp
    private val log = mutableStateListOf<LogEntry>()
    private var connectionStatus by mutableStateOf<ConnectionStatus>(ConnectionStatus.NotPaired)
    private var pairedDevices by mutableStateOf<List<PairedDevice>>(emptyList())

    /// Non-null only while a shared/picked file is waiting on a
    /// [DeviceSendPickerDialog] pick — see [connectedDevices]' doc comment
    /// for why that dialog practically never renders today.
    private var pendingFileSend by mutableStateOf<List<String>?>(null)

    /// The in-app picker, the second required send affordance alongside the
    /// share sheet (docs/design.md §4.6, §6.2). `OpenMultipleDocuments`
    /// rather than `GetMultipleContents`: it hands back a persistable
    /// `content://` URI backed by the Storage Access Framework, the same
    /// kind of URI a share intent carries, so both paths join at
    /// [handleSharedUris].
    private val filePicker =
        registerForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
            if (uris.isNotEmpty()) handleSharedUris(uris)
        }

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
                    onSendFile = { filePicker.launch(arrayOf("*/*")) },
                    onClearLog = log::clear,
                )
                pendingFileSend?.let { paths ->
                    DeviceSendPickerDialog(
                        devices = connectedDevices(),
                        onPick = { sendFilesNow(paths); pendingFileSend = null },
                        onDismiss = { pendingFileSend = null },
                    )
                }
            }
        }

        handleShareIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleShareIntent(intent)
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
        // TransferProgress fires once per 64 KB chunk (crates/net/src/
        // transfer.rs's CHUNK_LEN) — logging every one would flood this flat
        // event log for anything but a tiny file. Every other event,
        // transfers included, is rare enough to log as-is.
        if (event !is CoreEvent.TransferProgress) log.add(0, app.describe(event))
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

    /// The share sheet's entry point (docs/design.md §4.6, §6.2). Only
    /// `ACTION_SEND`/`ACTION_SEND_MULTIPLE` carry a file to resolve; anything
    /// else (a plain launch, a notification tap) is a no-op here.
    private fun handleShareIntent(intent: Intent) {
        val uris: List<Uri> =
            when (intent.action) {
                Intent.ACTION_SEND -> extraStreamUri(intent)?.let { listOf(it) } ?: emptyList()
                Intent.ACTION_SEND_MULTIPLE -> extraStreamUris(intent)
                else -> emptyList()
            }
        if (uris.isNotEmpty()) handleSharedUris(uris)
    }

    @Suppress("DEPRECATION")
    private fun extraStreamUri(intent: Intent): Uri? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            intent.getParcelableExtra(Intent.EXTRA_STREAM)
        }

    @Suppress("DEPRECATION")
    private fun extraStreamUris(intent: Intent): List<Uri> =
        (
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM)
            }
        ) ?: emptyList()

    /// Common tail for both send surfaces (share sheet and in-app picker):
    /// resolve every `content://` URI to a real path Rust/UniFFI can open
    /// (docs/design.md §4.6's "Kotlin owns" list — SAF resolution is
    /// Kotlin's job, not Rust's), then either send immediately or defer to
    /// [DeviceSendPickerDialog], matching [connectedDevices]'s two possible
    /// outcomes today.
    private fun handleSharedUris(uris: List<Uri>) {
        val paths = uris.mapNotNull(::copyToCache)
        if (paths.isEmpty()) {
            log.add(0, LogEntry.now("couldn't read the shared file(s)", LogSeverity.BAD))
            return
        }
        val targets = connectedDevices()
        when {
            targets.isEmpty() -> log.add(0, LogEntry.now("no device connected; nothing sent", LogSeverity.WARN))
            targets.size == 1 -> sendFilesNow(paths)
            else -> pendingFileSend = paths
        }
    }

    private fun sendFilesNow(paths: List<String>) {
        paths.forEach { path ->
            when (val result = app.sendFile(path)) {
                is SendResult.Sent -> log.add(0, LogEntry.now("sending ${File(path).name}", LogSeverity.GOOD))
                is SendResult.Failed -> log.add(0, LogEntry.now("send failed: ${result.reason}", LogSeverity.BAD))
                // sendFile() never returns this — PenguinSyncApp.sendFile has
                // no "nothing to send" case, unlike the clipboard path.
                is SendResult.NothingToSend -> {}
            }
        }
    }

    /// Devices this app could plausibly target right now — in practice at
    /// most one: [PenguinSyncCore][uniffi.penguinsync.PenguinSyncCore] holds
    /// a single active session ([PenguinSyncApp.connectionHandle]), so a
    /// device *picker* has no real teeth until the core grows per-device
    /// targeting. Returned as a list (not an `Optional`) so
    /// [DeviceSendPickerDialog]'s multi-device path is real code exercised
    /// the day that lands, rather than a screen guessed at in advance.
    private fun connectedDevices(): List<PairedDevice> {
        val active = connectionStatus as? ConnectionStatus.Connected ?: return emptyList()
        return pairedDevices.filter { it.deviceId == active.deviceId }
    }

    /// Copies a shared/picked `content://` URI into a fresh subdirectory of
    /// the cache dir, under its original display name where the provider
    /// hands one back — `FsSink`'s peer-side name sanitisation
    /// (crates/net/src/transfer.rs) doesn't help here, this name is only
    /// ever read locally to open the file. A fresh subdirectory per URI
    /// avoids collisions between files that share a display name across
    /// separate shares. Returns `null` (rather than throwing) on any I/O
    /// failure — the caller already treats an empty result list as "nothing
    /// readable, tell the user".
    private fun copyToCache(uri: Uri): String? {
        val name = queryDisplayName(uri) ?: "shared_file"
        val dir = File(cacheDir, "share-in/${System.nanoTime()}").apply { mkdirs() }
        val dest = File(dir, name)
        return try {
            val input = contentResolver.openInputStream(uri) ?: return null
            input.use { stream -> dest.outputStream().use { output -> stream.copyTo(output) } }
            dest.absolutePath
        } catch (e: IOException) {
            null
        }
    }

    private fun queryDisplayName(uri: Uri): String? =
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (index >= 0 && cursor.moveToFirst()) cursor.getString(index) else null
        }
}
