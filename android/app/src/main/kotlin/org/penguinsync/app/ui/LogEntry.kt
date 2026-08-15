package org.penguinsync.app.ui

import android.text.format.DateFormat
import androidx.compose.runtime.Immutable

/// How a log line should read at a glance, without parsing its text.
///
/// The Debug screen is the only instrument there is when the phone won't
/// reconnect and isn't plugged into a laptop (docs/design.md §4.6) — so
/// "which of these forty lines is the failure" has to be answerable by
/// colour, not by reading. Severity is decided where the event is described
/// ([org.penguinsync.app.PenguinSyncApp.describe]) rather than sniffed back
/// out of the rendered string by the screen.
enum class LogSeverity {
    /// Routine traffic: pings, clipboard arrivals.
    INFO,

    /// Something worked: a handshake completed, a send went out.
    GOOD,

    /// Recoverable trouble: reconnect attempts.
    WARN,

    /// A failure the user may need to act on.
    BAD,
}

@Immutable
data class LogEntry(
    /// Wall-clock milliseconds. Stored raw and formatted at render time so
    /// the entry doesn't bake in a locale or a 12/24-hour choice the user is
    /// free to change under us.
    val at: Long,
    val text: String,
    val severity: LogSeverity,
) {
    fun formattedTime(is24Hour: Boolean): String =
        DateFormat.format(if (is24Hour) "HH:mm:ss" else "h:mm:ss a", at).toString()

    companion object {
        fun now(
            text: String,
            severity: LogSeverity = LogSeverity.INFO,
        ) = LogEntry(System.currentTimeMillis(), text, severity)
    }
}
