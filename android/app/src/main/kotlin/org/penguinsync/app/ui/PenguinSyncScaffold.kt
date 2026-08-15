package org.penguinsync.app.ui

import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BugReport
import androidx.compose.material.icons.filled.PhoneAndroid
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import uniffi.penguinsync.PairedDevice

private enum class AppTab(
    val route: String,
    val label: String,
    val icon: ImageVector,
) {
    DEVICES("devices", "Devices", Icons.Default.PhoneAndroid),
    PAIR("pair", "Pair", Icons.Default.QrCodeScanner),
    SETTINGS("settings", "Settings", Icons.Default.Settings),
    DEBUG("debug", "Debug", Icons.Default.BugReport),
}

/// Top-level shell: the four screens the design doc specifies (docs/design.md
/// §4.6, §9 — "a simple `NavHost`, no nesting") behind a bottom nav bar.
/// State (connection status, paired devices, event log) is hoisted by the
/// caller and just handed down — this composable owns navigation only.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PenguinSyncScaffold(
    fingerprint: String,
    connectionStatus: ConnectionStatus,
    pairedDevices: List<PairedDevice>,
    log: List<String>,
    onPair: (String) -> Unit,
    onSendClipboard: () -> Unit,
) {
    val navController = rememberNavController()

    Scaffold(
        topBar = { TopAppBar(title = { Text("PenguinSync") }) },
        bottomBar = {
            NavigationBar {
                val backStackEntry by navController.currentBackStackEntryAsState()
                val currentDestination = backStackEntry?.destination
                AppTab.entries.forEach { tab ->
                    NavigationBarItem(
                        selected = currentDestination?.hierarchy?.any { it.route == tab.route } == true,
                        onClick = {
                            navController.navigate(tab.route) {
                                popUpTo(navController.graph.findStartDestination().id) { saveState = true }
                                launchSingleTop = true
                                restoreState = true
                            }
                        },
                        icon = { Icon(tab.icon, contentDescription = tab.label) },
                        label = { Text(tab.label) },
                    )
                }
            }
        },
    ) { padding ->
        NavHost(
            navController = navController,
            startDestination = AppTab.DEVICES.route,
            modifier = Modifier.padding(padding),
        ) {
            composable(AppTab.DEVICES.route) {
                DevicesScreen(
                    fingerprint = fingerprint,
                    status = connectionStatus,
                    pairedDevices = pairedDevices,
                    onGoToPair = {
                        navController.navigate(AppTab.PAIR.route) { launchSingleTop = true }
                    },
                    onSendClipboard = onSendClipboard,
                )
            }
            composable(AppTab.PAIR.route) {
                PairScreen(
                    fingerprint = fingerprint,
                    onPair = { uri ->
                        onPair(uri)
                        // A decoded/submitted QR is the point of no return —
                        // pairing has already started. Jumping to Devices
                        // immediately is the confirmation: without it the
                        // camera preview just keeps rendering with no visible
                        // change, and a successful scan looks identical to a
                        // failed one.
                        navController.navigate(AppTab.DEVICES.route) {
                            popUpTo(navController.graph.findStartDestination().id) { saveState = true }
                            launchSingleTop = true
                        }
                    },
                )
            }
            composable(AppTab.SETTINGS.route) {
                SettingsScreen(fingerprint = fingerprint)
            }
            composable(AppTab.DEBUG.route) {
                DebugScreen(log = log)
            }
        }
    }
}
