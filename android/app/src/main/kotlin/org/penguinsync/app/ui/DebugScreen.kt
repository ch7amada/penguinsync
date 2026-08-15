package org.penguinsync.app.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp

/// Debug screen (docs/design.md §4.6, §9's four screens) — recent protocol
/// events, newest first. "Not optional polish": when the phone won't
/// reconnect and it isn't plugged into a laptop, this is the only
/// instrument there is (§4.6).
@Composable
fun DebugScreen(log: List<String>) {
    Column(
        Modifier
            .fillMaxSize()
            .padding(16.dp),
    ) {
        Text("Debug", style = MaterialTheme.typography.titleLarge)
        Text(
            "Recent protocol events, newest first.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))

        if (log.isEmpty()) {
            Text("No events yet.", style = MaterialTheme.typography.bodyMedium)
        } else {
            LazyColumn {
                items(log) { line ->
                    Text(
                        line,
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = FontFamily.Monospace,
                        modifier = Modifier.padding(vertical = 2.dp),
                    )
                }
            }
        }
    }
}
