package org.penguinsync.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.core.app.ActivityCompat

/// M0's pairing UI (paste the QR's URI — real camera scanning is later
/// platform combat, docs/design.md §9) plus M1's clipboard write path
/// (Linux -> Android, automatic — handled entirely in [PenguinSyncApp]) plus
/// M2's in-app manual send button (Android -> Linux). Connection state lives
/// in [PenguinSyncApp], not here, so it survives this Activity being
/// destroyed and recreated — the QS tile and notification action
/// ([ClipboardReadActivity]) need to reach the same live session this screen
/// does, whether or not this screen is even open.
class MainActivity : ComponentActivity() {
    private lateinit var app: PenguinSyncApp
    private val log = mutableStateListOf<String>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        app = application as PenguinSyncApp

        // Needed for the connected-device notification's "Send clipboard"
        // action to show up at all on API 33+ (docs/design.md §6.1's
        // Baseline tier); the QS tile and this screen's own button work
        // without it either way, so a denial here isn't fatal.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ActivityCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            ActivityCompat.requestPermissions(this, arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
        }

        setContent {
            MaterialTheme {
                PairingScreen(
                    fingerprint = app.core.deviceFingerprint(),
                    log = log,
                    onPair = ::startPairing,
                    onSendClipboard = ::sendClipboardNow,
                )
            }
        }
    }

    override fun onStart() {
        super.onStart()
        app.uiListener = { line -> runOnUiThread { log.add(0, line) } }
    }

    override fun onStop() {
        app.uiListener = null
        super.onStop()
    }

    private fun startPairing(qrUri: String) {
        app.startPairing(qrUri).onFailure { e -> log.add(0, "pair() failed: ${e.message}") }
    }

    /// This composable already has window focus by definition, so the
    /// in-app button reads and sends directly — no trampoline needed,
    /// unlike the QS tile and notification action (docs/design.md §6.1's
    /// Baseline tier; contrast [ClipboardReadActivity]).
    private fun sendClipboardNow() {
        when (val result = app.sendClipboardFromFocusedContext(this)) {
            is SendResult.Sent -> log.add(0, "→ clipboard sent to Linux")
            is SendResult.NothingToSend -> log.add(0, "  clipboard is empty or marked sensitive; nothing sent")
            is SendResult.Failed -> log.add(0, "✗ send failed: ${result.reason}")
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PairingScreen(
    fingerprint: String,
    log: List<String>,
    onPair: (String) -> Unit,
    onSendClipboard: () -> Unit,
) {
    var qrUri by remember { mutableStateOf("") }

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
            Row {
                Button(
                    onClick = { onPair(qrUri) },
                    enabled = qrUri.startsWith("penguinsync://"),
                ) { Text("Pair") }
                Spacer(Modifier.width(8.dp))
                // M2: manual, one-tap read of this device's own clipboard,
                // sent to Linux (docs/design.md §6.1's Baseline tier). The
                // QS tile and the connected-device notification's action do
                // the same thing from outside the app.
                OutlinedButton(onClick = onSendClipboard) { Text("Send clipboard to Linux") }
            }
            Spacer(Modifier.height(16.dp))
            Text("Events", style = MaterialTheme.typography.titleMedium)
            LazyColumn {
                items(log) { line -> Text(line, style = MaterialTheme.typography.bodySmall) }
            }
        }
    }
}
