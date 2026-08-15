package org.penguinsync.app.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ContentPaste
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.DevicesOther
import androidx.compose.material.icons.outlined.Link
import androidx.compose.material.icons.outlined.LinkOff
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import org.penguinsync.app.ui.theme.LocalStatusColors
import uniffi.penguinsync.PairedDevice

/// Devices screen — the app's landing tab (docs/design.md §4.6, §9's four
/// screens): live connection status, the paired-device list, and the one
/// action everyone reaches for once connected (send the clipboard now, M2's
/// manual tier).
///
/// This device's own fingerprint used to head this screen; it lives on
/// Settings now. It is identity, not status — you read it once while pairing
/// and never again, and it was competing for the top of the screen with the
/// thing that actually changes.
@Composable
fun DevicesScreen(
    status: ConnectionStatus,
    pairedDevices: List<PairedDevice>,
    onGoToPair: () -> Unit,
    onSendClipboard: () -> Unit,
) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item { StatusHero(status, onSendClipboard) }

        item {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Paired devices", style = MaterialTheme.typography.titleMedium)
                TextButton(onClick = onGoToPair) {
                    Icon(Icons.Default.Add, contentDescription = null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Pair new")
                }
            }
        }

        if (pairedDevices.isEmpty()) {
            item { EmptyDeviceList(onGoToPair) }
        } else {
            items(pairedDevices, key = { it.deviceId }) { device ->
                val isActive = (status as? ConnectionStatus.Connected)?.deviceId == device.deviceId
                DeviceCard(device, isActive)
            }
        }
    }
}

/// The one thing on this screen that changes on its own, sized accordingly.
///
/// Colour carries the state as much as the text does, and every one of those
/// colours is a theme role rather than a literal — which is what makes the
/// card legible in dark mode, something the old hardcoded `0xFF2E7D32` green
/// dot never was. The container is animated rather than swapped: a
/// reconnect that flickers amber for 300 ms should read as a wobble, not as
/// a different screen.
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun StatusHero(
    status: ConnectionStatus,
    onSendClipboard: () -> Unit,
) {
    val statusColors = LocalStatusColors.current
    val scheme = MaterialTheme.colorScheme

    val (container, onContainer) =
        when (status) {
            is ConnectionStatus.Connected ->
                statusColors.connectedContainer to statusColors.onConnectedContainer
            is ConnectionStatus.Reconnecting ->
                statusColors.warningContainer to statusColors.onWarningContainer
            is ConnectionStatus.Disconnected -> scheme.errorContainer to scheme.onErrorContainer
            is ConnectionStatus.NotPaired -> scheme.surfaceContainerHigh to scheme.onSurface
        }
    val animatedContainer by animateColorAsState(container, label = "statusContainer")
    val animatedOnContainer by animateColorAsState(onContainer, label = "statusOnContainer")

    val icon: ImageVector =
        when (status) {
            is ConnectionStatus.Connected -> Icons.Outlined.Link
            is ConnectionStatus.Reconnecting -> Icons.Outlined.Sync
            is ConnectionStatus.Disconnected -> Icons.Outlined.LinkOff
            is ConnectionStatus.NotPaired -> Icons.Outlined.DevicesOther
        }
    val headline =
        when (status) {
            is ConnectionStatus.Connected -> "Connected"
            is ConnectionStatus.Reconnecting -> "Reconnecting…"
            is ConnectionStatus.Disconnected -> "Disconnected"
            is ConnectionStatus.NotPaired -> "Not connected"
        }
    val detail =
        when (status) {
            is ConnectionStatus.Connected ->
                status.lastRttMs?.let { "${status.name} · round trip ${it} ms" } ?: status.name
            is ConnectionStatus.Reconnecting -> "Attempt ${status.attempt}"
            is ConnectionStatus.Disconnected -> status.reason
            is ConnectionStatus.NotPaired -> "Pair with a Linux desktop to start syncing"
        }

    Card(
        Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.extraLarge,
        colors = CardDefaults.cardColors(containerColor = animatedContainer),
    ) {
        Column(Modifier.padding(20.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier
                        .size(48.dp)
                        .background(animatedOnContainer.copy(alpha = 0.14f), CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    // A reconnect is the one state where the app is doing
                    // something the user can't see, so it gets the animated
                    // indicator rather than a static glyph — otherwise
                    // "retrying every few seconds" and "stalled forever" look
                    // exactly alike.
                    if (status is ConnectionStatus.Reconnecting) {
                        LoadingIndicator(
                            color = animatedOnContainer,
                            modifier = Modifier.size(30.dp),
                        )
                    } else {
                        Icon(
                            icon,
                            contentDescription = null,
                            tint = animatedOnContainer,
                            modifier = Modifier.size(26.dp),
                        )
                    }
                }
                Spacer(Modifier.width(16.dp))
                Column {
                    Text(
                        headline,
                        style = MaterialTheme.typography.headlineSmall,
                        color = animatedOnContainer,
                    )
                    Text(
                        detail,
                        style = MaterialTheme.typography.bodyMedium,
                        color = animatedOnContainer.copy(alpha = 0.8f),
                    )
                }
            }

            if (status is ConnectionStatus.Connected) {
                Spacer(Modifier.height(20.dp))
                Button(onClick = onSendClipboard, modifier = Modifier.fillMaxWidth()) {
                    Icon(Icons.Default.ContentPaste, contentDescription = null, Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text("Send clipboard to Linux")
                }
            }
        }
    }
}

@Composable
private fun DeviceCard(
    device: PairedDevice,
    isActive: Boolean,
) {
    val statusColors = LocalStatusColors.current
    Card(
        Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.large,
        colors =
            CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
            ),
    ) {
        ListItem(
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            leadingContent = {
                Icon(
                    Icons.Outlined.Computer,
                    contentDescription = null,
                    tint = if (isActive) statusColors.connected else statusColors.offline,
                )
            },
            supportingContent = {
                Text(
                    device.deviceId.take(16) + "…",
                    style = MaterialTheme.typography.bodySmall,
                )
            },
            trailingContent = {
                Text(
                    if (isActive) "Connected" else "Offline",
                    style = MaterialTheme.typography.labelMedium,
                    color = if (isActive) statusColors.connected else statusColors.offline,
                )
            },
        ) { Text(device.name) }
    }
}

@Composable
private fun EmptyDeviceList(onGoToPair: () -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(vertical = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            Icons.Outlined.DevicesOther,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(48.dp),
        )
        Spacer(Modifier.height(12.dp))
        Text(
            "No devices paired yet",
            style = MaterialTheme.typography.titleMedium,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            "Run penguinsync on Linux, press p, and scan the QR code it shows.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(16.dp))
        Button(onClick = onGoToPair) { Text("Pair a device") }
    }
}
