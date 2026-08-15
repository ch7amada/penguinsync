package org.penguinsync.app.ui

import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.BatterySaver
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.Fingerprint
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material.icons.outlined.Palette
import androidx.compose.material.icons.outlined.Restore
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import org.penguinsync.app.PenguinSyncConnectionService

/// Settings screen (docs/design.md §4.6, §9's four screens). Only real,
/// working knobs live here — the per-device send/receive toggles and
/// notification allow-list the design doc describes arrive with M3/M5, once
/// there's a second device and a notification listener to toggle. What's
/// here today: this device's identity, the reliability prerequisites §4.6
/// lists as onboarding steps (battery-optimization exemption, notification
/// permission), and the one purely cosmetic choice worth offering.
@Composable
fun SettingsScreen(
    fingerprint: String,
    dynamicColor: Boolean,
    onDynamicColorChange: (Boolean) -> Unit,
) {
    val context = LocalContext.current

    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            // Extra at the bottom so the closing paragraph clears the
            // navigation bar instead of ending flush against it.
            .padding(start = 16.dp, top = 16.dp, end = 16.dp, bottom = 32.dp),
    ) {
        SettingsSection("This device") {
            ListItem(
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Outlined.Fingerprint, contentDescription = null) },
                supportingContent = {
                    Text(
                        fingerprint,
                        style = MaterialTheme.typography.bodyMedium,
                        fontFamily = FontFamily.Monospace,
                    )
                },
                trailingContent = {
                    IconButton(onClick = { copyFingerprint(context, fingerprint) }) {
                        Icon(Icons.Outlined.ContentCopy, contentDescription = "Copy fingerprint")
                    }
                },
            ) { Text("Fingerprint") }
        }

        Spacer(Modifier.height(16.dp))

        SettingsSection("Reliability") {
            BatteryOptimizationRow(context)
            HorizontalDivider(Modifier.padding(horizontal = 16.dp))
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                NotificationPermissionRow(context)
                HorizontalDivider(Modifier.padding(horizontal = 16.dp))
            }
            RestoreNotificationRow(context)
        }

        Spacer(Modifier.height(16.dp))

        SettingsSection("Appearance") {
            ListItem(
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Outlined.Palette, contentDescription = null) },
                supportingContent = {
                    Text(
                        if (dynamicColor) {
                            "Following your wallpaper"
                        } else {
                            "Using PenguinSync's own palette"
                        },
                    )
                },
                trailingContent = {
                    Switch(checked = dynamicColor, onCheckedChange = onDynamicColorChange)
                },
            ) { Text("Use device colours") }
        }

        Spacer(Modifier.height(24.dp))
        Text(
            "PenguinSync — clipboard sync, manual tier (docs/design.md M2). " +
                "File transfer and notification mirroring aren't implemented yet.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/// A titled group of rows in one card. Grouping is the whole reason the
/// screen reads as settings rather than as a list: "Fix" next to a battery
/// warning means something different from "Fix" floating on its own.
@Composable
private fun SettingsSection(
    title: String,
    content: @Composable () -> Unit,
) {
    Text(
        title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(start = 4.dp, bottom = 8.dp),
    )
    Card(
        Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.large,
        colors =
            CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
            ),
    ) {
        Column(Modifier.padding(vertical = 4.dp)) { content() }
    }
}

private fun copyFingerprint(
    context: Context,
    fingerprint: String,
) {
    val manager = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    manager.setPrimaryClip(ClipData.newPlainText("PenguinSync fingerprint", fingerprint))
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
        icon = Icons.Outlined.BatterySaver,
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
        icon = Icons.Outlined.Notifications,
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
        icon = Icons.Outlined.Restore,
        title = "Connection notification",
        subtitle = "Swiped it away by accident? Bring it back.",
        actionLabel = "Restore",
        onAction = { PenguinSyncConnectionService.restore(context) },
    )
}

@Composable
private fun SettingRow(
    icon: ImageVector,
    title: String,
    subtitle: String,
    actionLabel: String?,
    onAction: () -> Unit,
) {
    ListItem(
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        leadingContent = { Icon(icon, contentDescription = null) },
        supportingContent = { Text(subtitle) },
        trailingContent =
            actionLabel?.let { label ->
                { TextButton(onClick = onAction) { Text(label) } }
            },
    ) { Text(title) }
}
