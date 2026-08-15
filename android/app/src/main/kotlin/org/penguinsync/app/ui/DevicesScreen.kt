package org.penguinsync.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import uniffi.penguinsync.PairedDevice

/// Devices screen — the app's landing tab (docs/design.md §4.6, §9's four
/// screens): this device's own identity, live connection status, the
/// paired-device list, and the one action everyone reaches for once
/// connected (send the clipboard now, M2's manual tier).
@Composable
fun DevicesScreen(
    fingerprint: String,
    status: ConnectionStatus,
    pairedDevices: List<PairedDevice>,
    onGoToPair: () -> Unit,
    onSendClipboard: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .padding(16.dp),
    ) {
        Text("This device", style = MaterialTheme.typography.titleSmall)
        Text(fingerprint, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.height(16.dp))

        StatusCard(status)

        if (status is ConnectionStatus.Connected) {
            Spacer(Modifier.height(8.dp))
            Button(onClick = onSendClipboard) { Text("Send clipboard to Linux") }
        }

        Spacer(Modifier.height(24.dp))
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text("Paired devices", style = MaterialTheme.typography.titleMedium)
            TextButton(onClick = onGoToPair) {
                Icon(Icons.Default.Add, contentDescription = null)
                Spacer(Modifier.width(4.dp))
                Text("Pair new")
            }
        }
        Spacer(Modifier.height(8.dp))

        if (pairedDevices.isEmpty()) {
            Text(
                "No devices paired yet. Tap \"Pair new\" and scan the QR code " +
                    "shown by `penguinsync` on Linux.",
                style = MaterialTheme.typography.bodySmall,
            )
        } else {
            LazyColumn {
                items(pairedDevices, key = { it.deviceId }) { device ->
                    val isActive = (status as? ConnectionStatus.Connected)?.deviceId == device.deviceId
                    DeviceRow(device, isActive)
                }
            }
        }
    }
}

@Composable
private fun StatusCard(status: ConnectionStatus) {
    val (dotColor, text) =
        when (status) {
            is ConnectionStatus.NotPaired ->
                MaterialTheme.colorScheme.outline to "Not connected"
            is ConnectionStatus.Connected -> {
                val rtt = status.lastRttMs?.let { " · ${it} ms" } ?: ""
                Color(0xFF2E7D32) to "Connected to ${status.name}$rtt"
            }
            is ConnectionStatus.Reconnecting ->
                Color(0xFFF9A825) to "Reconnecting… (attempt ${status.attempt})"
            is ConnectionStatus.Disconnected ->
                MaterialTheme.colorScheme.error to "Disconnected: ${status.reason}"
        }

    Row(
        Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(12.dp))
            .padding(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Spacer(
            Modifier
                .size(10.dp)
                .background(dotColor, CircleShape),
        )
        Spacer(Modifier.width(10.dp))
        Text(text, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun DeviceRow(
    device: PairedDevice,
    isActive: Boolean,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Column {
            Text(device.name, style = MaterialTheme.typography.bodyLarge)
            Text(
                device.deviceId.take(16) + "…",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (isActive) {
            Text(
                "Connected",
                style = MaterialTheme.typography.labelMedium,
                color = Color(0xFF2E7D32),
            )
        }
    }
}
