package org.penguinsync.app

import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import uniffi.penguinsync.ConnectionHandle
import uniffi.penguinsync.CoreEvent
import uniffi.penguinsync.CoreEventListener
import uniffi.penguinsync.PenguinSyncCore

/// M0's entire Android UI: paste the QR's URI (real camera scanning is
/// later platform combat, docs/design.md §9's "isolated from platform
/// combat"), pair, watch the event log prove handshake/ping-pong/reconnect
/// are alive. No clipboard, no files, no notifications yet.
class MainActivity : ComponentActivity() {
    private lateinit var core: PenguinSyncCore
    private var handle: ConnectionHandle? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        core = PenguinSyncCore(filesDir.absolutePath, Build.MODEL ?: "Android")

        setContent {
            MaterialTheme {
                PairingScreen(
                    fingerprint = core.deviceFingerprint(),
                    onPair = ::startPairing,
                )
            }
        }
    }

    private fun startPairing(qrUri: String, onEvent: (String) -> Unit) {
        // Every long-lived operation gets an explicit cancel — starting a
        // new pairing attempt cancels whatever the previous one left
        // running (docs/design.md §4.2's FFI cancellation discipline).
        handle?.cancel()
        handle =
            try {
                core.pair(
                    qrUri,
                    object : CoreEventListener {
                        override fun onEvent(event: CoreEvent) {
                            runOnUiThread { onEvent(describe(event)) }
                        }
                    },
                )
            } catch (e: Exception) {
                onEvent("pair() failed: ${e.message}")
                null
            }
    }

    override fun onDestroy() {
        handle?.cancel()
        super.onDestroy()
    }

    private fun describe(event: CoreEvent): String =
        when (event) {
            is CoreEvent.PeerHandshake -> "✓ connected to ${event.name} (${event.deviceId.take(16)}…)"
            is CoreEvent.Ponged -> "  ping: ${event.rttMs} ms"
            is CoreEvent.Disconnected -> "✗ disconnected: ${event.reason}"
            is CoreEvent.Reconnecting -> "↻ reconnecting (attempt ${event.attempt}, in ${event.delayMs} ms)"
        }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PairingScreen(
    fingerprint: String,
    onPair: (String, (String) -> Unit) -> Unit,
) {
    var qrUri by remember { mutableStateOf("") }
    val log = remember { mutableStateListOf<String>() }

    Scaffold(topBar = { TopAppBar(title = { Text("PenguinSync") }) }) { padding ->
        Column(
            Modifier
                .padding(padding)
                .padding(16.dp)
                .fillMaxSize(),
        ) {
            Text("This device: $fingerprint", style = MaterialTheme.typography.bodyMedium)
            Spacer(Modifier.height(16.dp))
            Text(
                "Paste the pairing URI from the Linux TUI's QR code " +
                    "(camera scanning lands with a later milestone).",
                style = MaterialTheme.typography.bodySmall,
            )
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = qrUri,
                onValueChange = { qrUri = it },
                label = { Text("penguinsync://pair?...") },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = { onPair(qrUri) { line -> log.add(0, line) } },
                enabled = qrUri.startsWith("penguinsync://"),
            ) { Text("Pair") }
            Spacer(Modifier.height(16.dp))
            Text("Events", style = MaterialTheme.typography.titleMedium)
            LazyColumn {
                items(log) { line -> Text(line, style = MaterialTheme.typography.bodySmall) }
            }
        }
    }
}
