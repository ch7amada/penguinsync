package org.penguinsync.app.ui

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import org.penguinsync.app.PenguinSyncConnectionService

/// Settings screen (docs/design.md §4.6, §9's four screens). Only real,
/// working knobs live here — the per-device send/receive toggles and
/// notification allow-list the design doc describes arrive with M3/M5, once
/// there's a second device and a notification listener to toggle. What's
/// here today: the reliability prerequisites §4.6 lists as onboarding steps
/// (battery-optimization exemption, notification permission) that M0–M2
/// never actually asked for outside a best-effort prompt in `MainActivity`.
@Composable
fun SettingsScreen(fingerprint: String) {
    val context = LocalContext.current

    Column(
        Modifier
            .fillMaxSize()
            .padding(16.dp),
    ) {
        Text("Settings", style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(16.dp))

        Text("This device", style = MaterialTheme.typography.titleSmall)
        Text(fingerprint, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.height(24.dp))

        BatteryOptimizationRow(context)
        HorizontalDivider(Modifier.padding(vertical = 12.dp))
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            NotificationPermissionRow(context)
            HorizontalDivider(Modifier.padding(vertical = 12.dp))
        }
        RestoreNotificationRow(context)
        HorizontalDivider(Modifier.padding(vertical = 12.dp))

        Spacer(Modifier.height(12.dp))
        Text(
            "PenguinSync — clipboard sync, manual tier (docs/design.md M2). " +
                "File transfer and notification mirroring aren't implemented yet.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun BatteryOptimizationRow(context: Context) {
    var exempt by
        remember {
            mutableStateOf(
                context.getSystemService(PowerManager::class.java)
                    .isIgnoringBatteryOptimizations(context.packageName),
            )
        }
    val launcher =
        rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
            exempt =
                context.getSystemService(PowerManager::class.java)
                    .isIgnoringBatteryOptimizations(context.packageName)
        }

    SettingRow(
        title = "Background reliability",
        subtitle =
            if (exempt) {
                "Exempt from battery optimization"
            } else {
                "Battery optimization can silently drop the connection while backgrounded"
            },
        actionLabel = if (exempt) null else "Fix",
        onAction = {
            launcher.launch(
                Intent(
                    Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                    "package:${context.packageName}".toUri(),
                ),
            )
        },
    )
}

@Composable
private fun NotificationPermissionRow(context: Context) {
    var granted by
        remember {
            mutableStateOf(
                ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
                    PackageManager.PERMISSION_GRANTED,
            )
        }
    val launcher =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { result ->
            granted = result
        }

    SettingRow(
        title = "Notifications",
        subtitle =
            if (granted) {
                "Enabled"
            } else {
                "Needed for the connected-device notification's \"Send clipboard\" action"
            },
        actionLabel = if (granted) null else "Grant",
        onAction = { launcher.launch(Manifest.permission.POST_NOTIFICATIONS) },
    )
}

/// Swiping away the ongoing connection notification (some launchers/OEMs
/// allow it despite `setOngoing(true)`) doesn't bring it back on its own —
/// [org.penguinsync.app.PenguinSyncConnectionService] only reposts it from a
/// live [uniffi.penguinsync.CoreEvent], and idling on an already-`Connected`
/// status produces nothing but `Ponged`s, which it deliberately ignores. No
/// way to detect "is the notification currently showing" from here, so this
/// is a plain always-available action rather than a conditional row like the
/// two above.
@Composable
private fun RestoreNotificationRow(context: Context) {
    SettingRow(
        title = "Connection notification",
        subtitle = "Swiped it away by accident? Bring it back.",
        actionLabel = "Restore",
        onAction = { PenguinSyncConnectionService.restore(context) },
    )
}

@Composable
private fun SettingRow(
    title: String,
    subtitle: String,
    actionLabel: String?,
    onAction: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(subtitle, style = MaterialTheme.typography.bodySmall)
        }
        if (actionLabel != null) {
            Spacer(Modifier.width(8.dp))
            TextButton(onClick = onAction) { Text(actionLabel) }
        }
    }
}
