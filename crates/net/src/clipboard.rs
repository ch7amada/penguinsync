//! Pluggable clipboard backend abstraction (docs/design.md §4.4.1).
//!
//! v1 ships one backend — the GNOME Shell extension, implemented in
//! `penguinsync-daemon` (it needs `zbus`, which this crate deliberately does
//! not depend on). Android has no backend at all here: Kotlin owns clipboard
//! read/write directly (docs/design.md §4.6), and only pushes/receives
//! [`penguinsync_protocol::Clip`] through the FFI layer. An
//! `ext-data-control-v1` backend for other Wayland compositors is the v0.2
//! headline (docs/design.md §4.4.1) and would implement this same trait.

use std::pin::Pin;

use futures_core::Stream;

#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error("no clipboard content of that type")]
    NotAvailable,
    #[error("clipboard backend unavailable: {0}")]
    BackendUnavailable(String),
    #[error("clipboard backend error: {0}")]
    Other(String),
}

/// The clipboard changed. Carries only the MIME type — reading the new
/// content is a separate call, so a backend that can tell "it changed"
/// without transferring bytes yet (as GNOME's selection API does) doesn't
/// have to pay for a read nobody asked for.
#[derive(Debug, Clone)]
pub struct ClipChanged {
    pub mime: String,
}

/// A source (and sink) of clipboard content. Probed at startup — the daemon
/// must run fine with none available (docs/design.md §4.4): clipboard is
/// then reported unavailable, and file transfer and notification mirroring
/// don't need it.
#[async_trait::async_trait]
pub trait ClipboardBackend: Send + Sync {
    /// Stream of change notifications, already filtered to the clipboard
    /// selection (not primary/other X11-style selections).
    fn watch(&self) -> Pin<Box<dyn Stream<Item = ClipChanged> + Send>>;

    async fn read(&self, mime: &str) -> Result<Vec<u8>, ClipboardError>;

    /// Unused until M2 (Android → Linux clipboard write); part of the
    /// trait now so the M1 backend doesn't need a breaking change to grow
    /// it (docs/protocol.md's reserved-field philosophy, applied to traits).
    async fn write(&self, mime: &str, bytes: &[u8]) -> Result<(), ClipboardError>;
}
