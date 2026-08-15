package org.penguinsync.app.ui

import android.text.format.DateFormat
import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.foundation.background
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import org.penguinsync.app.ui.theme.LocalStatusColors

/// Debug screen (docs/design.md §4.6, §9's four screens) — recent protocol
/// events, newest first. "Not optional polish": when the phone won't
/// reconnect and it isn't plugged into a laptop, this is the only
/// instrument there is (§4.6).
///
/// Which is exactly why the lines are timestamped and colour-coded now. A
/// flat monospace wall answers "what happened" but not "when did it start
/// going wrong", and the second question is the one you actually have while
/// staring at a phone that won't reconnect.
@Composable
fun DebugScreen(log: List<LogEntry>) {
    val is24Hour = DateFormat.is24HourFormat(LocalContext.current)

    if (log.isEmpty()) {
        EmptyLog()
        return
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        items(log) { entry -> LogRow(entry, is24Hour) }
    }
}

@Composable
private fun LogRow(
    entry: LogEntry,
    is24Hour: Boolean,
) {
    val statusColors = LocalStatusColors.current
    val accent =
        when (entry.severity) {
            LogSeverity.GOOD -> statusColors.connected
            LogSeverity.WARN -> statusColors.warning
            LogSeverity.BAD -> MaterialTheme.colorScheme.error
            LogSeverity.INFO -> MaterialTheme.colorScheme.outline
        }

    Row(
        Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Spacer(
            Modifier
                .size(8.dp)
                .background(accent, CircleShape),
        )
        Spacer(Modifier.width(10.dp))
        Text(
            entry.formattedTime(is24Hour),
            style = MaterialTheme.typography.labelSmall,
            fontFamily = FontFamily.Monospace,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.width(10.dp))
        Text(
            entry.text,
            style = MaterialTheme.typography.bodySmall,
            fontFamily = FontFamily.Monospace,
            color =
                if (entry.severity == LogSeverity.INFO) {
                    MaterialTheme.colorScheme.onSurfaceVariant
                } else {
                    MaterialTheme.colorScheme.onSurface
                },
        )
    }
}

@Composable
private fun EmptyLog() {
    Column(
        Modifier
            .fillMaxSize()
            .padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(
            Icons.Outlined.Terminal,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(48.dp),
        )
        Spacer(Modifier.height(12.dp))
        Text("No events yet", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(4.dp))
        Text(
            "Protocol events show up here as they happen, newest first.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
    }
}
