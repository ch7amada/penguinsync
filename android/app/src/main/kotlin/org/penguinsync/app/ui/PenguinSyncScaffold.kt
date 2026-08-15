package org.penguinsync.app.ui

import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BugReport
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.outlined.BugReport
import androidx.compose.material.icons.outlined.Devices
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material.icons.outlined.QrCodeScanner
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.Scaffold
import androidx.compose.material3.ShortNavigationBar
import androidx.compose.material3.ShortNavigationBarItem
import androidx.compose.material3.ShortNavigationBarItemDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.TopAppBarState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
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
    val title: String,
    val selectedIcon: ImageVector,
    val icon: ImageVector,
) {
    DEVICES("devices", "Devices", "Devices", Icons.Filled.Devices, Icons.Outlined.Devices),
    PAIR("pair", "Pair", "Pair a device", Icons.Filled.QrCodeScanner, Icons.Outlined.QrCodeScanner),
    SETTINGS("settings", "Settings", "Settings", Icons.Filled.Settings, Icons.Outlined.Settings),
    DEBUG("debug", "Debug", "Debug", Icons.Filled.BugReport, Icons.Outlined.BugReport),
}

/// Top-level shell: the four screens the design doc specifies (docs/design.md
/// §4.6, §9 — "a simple `NavHost`, no nesting") behind a bottom nav bar.
/// State (connection status, paired devices, event log) is hoisted by the
/// caller and just handed down — this composable owns navigation only.
///
/// The app bar carries the screen's name and the screens never repeat it in
/// their body. That rule is why "PenguinSync" no longer appears twice on
/// every screen: the app's name is on the launcher and in the task switcher,
/// which is where a user looks for it — what they need *here* is which of the
/// four screens they're on, and a subtitle telling them the one thing that
/// changes underneath them (the connection).
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun PenguinSyncScaffold(
    fingerprint: String,
    connectionStatus: ConnectionStatus,
    pairedDevices: List<PairedDevice>,
    log: List<LogEntry>,
    dynamicColor: Boolean,
    onDynamicColorChange: (Boolean) -> Unit,
    onPair: (String) -> Unit,
    onSendClipboard: () -> Unit,
    onClearLog: () -> Unit,
) {
    val navController = rememberNavController()
    val backStackEntry by navController.currentBackStackEntryAsState()
    val currentDestination = backStackEntry?.destination
    val currentTab =
        AppTab.entries.firstOrNull { tab ->
            currentDestination?.hierarchy?.any { it.route == tab.route } == true
        } ?: AppTab.DEVICES

    // Keyed on the tab: a bar left collapsed by scrolling the Debug log would
    // otherwise still be collapsed on arriving at Settings, which has nothing
    // to scroll and therefore no way to expand it again.
    val topAppBarState = remember(currentTab) { TopAppBarState(-Float.MAX_VALUE, 0f, 0f) }
    val scrollBehavior = TopAppBarDefaults.exitUntilCollapsedScrollBehavior(topAppBarState)

    Scaffold(
        modifier = Modifier.nestedScroll(scrollBehavior.nestedScrollConnection),
        topBar = {
            MediumFlexibleTopAppBar(
                title = { Text(currentTab.title) },
                subtitle = {
                    val subtitle =
                        when (currentTab) {
                            AppTab.DEVICES -> connectionStatus.summary()
                            AppTab.PAIR -> "Scan the code shown on Linux"
                            AppTab.SETTINGS -> "Permissions and appearance"
                            AppTab.DEBUG -> "${log.size} events"
                        }
                    Text(subtitle)
                },
                actions = {
                    if (currentTab == AppTab.DEBUG && log.isNotEmpty()) {
                        IconButton(onClick = onClearLog) {
                            Icon(Icons.Outlined.DeleteSweep, contentDescription = "Clear log")
                        }
                    }
                },
                scrollBehavior = scrollBehavior,
            )
        },
        bottomBar = {
            ShortNavigationBar {
                AppTab.entries.forEach { tab ->
                    val selected = tab == currentTab
                    ShortNavigationBarItem(
                        // Blue, not the default secondaryContainer. In this
                        // palette secondary is a green a shade away from
                        // tertiary, and tertiary is what "connected" means —
                        // a permanently green pill under the nav bar spends
                        // that signal on decoration.
                        colors =
                            ShortNavigationBarItemDefaults.colors(
                                selectedIndicatorColor = MaterialTheme.colorScheme.primaryContainer,
                                selectedIconColor = MaterialTheme.colorScheme.onPrimaryContainer,
                                selectedTextColorTopIconPosition = MaterialTheme.colorScheme.onSurface,
                            ),
                        selected = selected,
                        onClick = {
                            navController.navigate(tab.route) {
                                popUpTo(navController.graph.findStartDestination().id) { saveState = true }
                                launchSingleTop = true
                                restoreState = true
                            }
                        },
                        icon = {
                            Icon(
                                if (selected) tab.selectedIcon else tab.icon,
                                contentDescription = tab.label,
                            )
                        },
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
                SettingsScreen(
                    fingerprint = fingerprint,
                    dynamicColor = dynamicColor,
                    onDynamicColorChange = onDynamicColorChange,
                )
            }
            composable(AppTab.DEBUG.route) {
                DebugScreen(log = log)
            }
        }
    }
}
