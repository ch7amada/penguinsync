package org.penguinsync.app.ui

import uniffi.penguinsync.CoreEvent

/// Structured connection state for the Devices screen (docs/design.md §4.6,
/// §9's four-screen layout) — derived from the same [CoreEvent] stream the
/// Debug screen renders as a flat log, but folded into "what am I looking
/// at right now" instead of "what just happened".
sealed interface ConnectionStatus {
    data object NotPaired : ConnectionStatus

    data class Connected(
        val name: String,
        val deviceId: String,
        val lastRttMs: ULong?,
    ) : ConnectionStatus

    data class Reconnecting(
        val attempt: UInt,
    ) : ConnectionStatus

    data class Disconnected(
        val reason: String,
    ) : ConnectionStatus
}

/// One event folded onto the previous status. `ClipboardReceived` and the
/// M4 transfer events leave status untouched — they're events on top of a
/// connection, not a connection-state change themselves.
fun ConnectionStatus.reduce(event: CoreEvent): ConnectionStatus =
    when (event) {
        is CoreEvent.PeerHandshake -> ConnectionStatus.Connected(event.name, event.deviceId, lastRttMs = null)
        is CoreEvent.Ponged ->
            if (this is ConnectionStatus.Connected) copy(lastRttMs = event.rttMs) else this
        is CoreEvent.Reconnecting -> ConnectionStatus.Reconnecting(event.attempt)
        is CoreEvent.Disconnected -> ConnectionStatus.Disconnected(event.reason)
        is CoreEvent.ClipboardReceived -> this
        is CoreEvent.TransferStarted -> this
        is CoreEvent.TransferOffered -> this
        is CoreEvent.TransferProgress -> this
        is CoreEvent.TransferReceived -> this
        is CoreEvent.TransferAcked -> this
    }

/// One line for the top app bar's subtitle. Shorter than
/// [notificationTitle]'s text on purpose — an app bar subtitle is read at a
/// glance, and the Devices screen's status card right below it carries the
/// detail.
fun ConnectionStatus.summary(): String =
    when (this) {
        is ConnectionStatus.NotPaired -> "No device paired"
        is ConnectionStatus.Connected -> lastRttMs?.let { "$name · $it ms" } ?: name
        is ConnectionStatus.Reconnecting -> "Reconnecting…"
        is ConnectionStatus.Disconnected -> "Disconnected"
    }

/// [PenguinSyncConnectionService]'s notification title/action off the same
/// folded status the Devices screen renders, instead of re-deriving it from
/// raw [CoreEvent]s itself — a service that only rebuilds its notification on
/// the next *new* event never recovers from the notification being swiped
/// away while idling on `Connected` (no more `PeerHandshake`/`Reconnecting`/
/// `Disconnected` due, just `Ponged`s the old per-event `when` ignored).
/// Rebuilding straight from [ConnectionStatus] on every `onStartCommand`
/// fixes that regardless of what event (if any) is next.
fun ConnectionStatus.notificationTitle(): Pair<String, Boolean> =
    when (this) {
        is ConnectionStatus.NotPaired -> "Connecting…" to false
        is ConnectionStatus.Connected -> "Connected to $name" to true
        is ConnectionStatus.Reconnecting -> "Reconnecting…" to false
        is ConnectionStatus.Disconnected -> "Disconnected: $reason" to false
    }
