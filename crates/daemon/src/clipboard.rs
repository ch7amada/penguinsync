//! GNOME Shell extension clipboard backend, and the watch-and-broadcast loop
//! that sits on top of it (docs/design.md §4.4, §6.1).
//!
//! The extension is optional. If it isn't installed or enabled,
//! [`GnomeClipboardBackend::probe`] returns `None` and the daemon simply
//! never starts the broadcast loop — clipboard is unavailable, everything
//! else (pairing, files once they land, notifications once they land) works
//! fine (docs/design.md §4.4, decision log #39).

use std::pin::Pin;
use std::sync::Arc;

use futures_util::StreamExt;
use futures_util::stream::Stream;

use penguinsync_net::{ClipChanged, ClipboardBackend, ClipboardError};
use penguinsync_protocol::clipboard::{Clip, MIME_TEXT_PLAIN};

use crate::shared::Shared;

/// Must match `gnome-extension/`'s advertised name/path exactly — see
/// `gnome-extension/README.md`.
const BUS_NAME: &str = "org.penguinsync.Clipboard";

#[zbus::proxy(
    interface = "org.penguinsync.Clipboard",
    default_service = "org.penguinsync.Clipboard",
    default_path = "/org/penguinsync/Clipboard"
)]
trait Clipboard {
    fn get_mimetypes(&self) -> zbus::Result<Vec<String>>;
    fn get_text(&self) -> zbus::Result<String>;
    fn set_text(&self, text: &str) -> zbus::Result<()>;
    fn get_value(&self, mimetype: &str) -> zbus::Result<Vec<u8>>;
    fn set_value(&self, value: &[u8], mimetype: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn owner_change(&self) -> zbus::Result<()>;
}

pub struct GnomeClipboardBackend {
    proxy: ClipboardProxy<'static>,
}

impl GnomeClipboardBackend {
    /// `None` if the extension isn't installed, isn't enabled, or hasn't
    /// claimed its bus name for any other reason. Never blocks on the
    /// extension showing up later — the daemon must start without it.
    pub async fn probe(connection: &zbus::Connection) -> Option<Self> {
        let dbus_proxy = zbus::fdo::DBusProxy::new(connection).await.ok()?;
        let owned = dbus_proxy
            .name_has_owner(BUS_NAME.try_into().ok()?)
            .await
            .ok()?;
        if !owned {
            return None;
        }
        let proxy = ClipboardProxy::new(connection).await.ok()?;
        Some(Self { proxy })
    }
}

#[async_trait::async_trait]
impl ClipboardBackend for GnomeClipboardBackend {
    fn watch(&self) -> Pin<Box<dyn Stream<Item = ClipChanged> + Send>> {
        let proxy = self.proxy.clone();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            let Ok(mut changes) = proxy.receive_owner_change().await else {
                tracing::warn!(
                    "could not subscribe to clipboard OwnerChange; clipboard sync stalled"
                );
                return;
            };
            // v1 only ever cares about text/plain (docs/design.md §6.1); the
            // extension's signal carries no mime itself, so this is the
            // whole of the translation.
            while changes.next().await.is_some() {
                if tx
                    .send(ClipChanged {
                        mime: MIME_TEXT_PLAIN.to_string(),
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
    }

    async fn read(&self, mime: &str) -> Result<Vec<u8>, ClipboardError> {
        if mime != MIME_TEXT_PLAIN {
            return Err(ClipboardError::NotAvailable);
        }
        self.proxy
            .get_text()
            .await
            .map(String::into_bytes)
            .map_err(|e| ClipboardError::Other(e.to_string()))
    }

    async fn write(&self, mime: &str, bytes: &[u8]) -> Result<(), ClipboardError> {
        if mime != MIME_TEXT_PLAIN {
            return Err(ClipboardError::NotAvailable);
        }
        let text = std::str::from_utf8(bytes).map_err(|e| ClipboardError::Other(e.to_string()))?;
        self.proxy
            .set_text(text)
            .await
            .map_err(|e| ClipboardError::Other(e.to_string()))
    }
}

/// Watches `backend` forever, broadcasting every genuine change (deduped by
/// content hash — an unmoved clipboard shouldn't cost a wire message even if
/// `OwnerChange` fires spuriously) to every currently connected, paired
/// device. Returns only if the backend's change stream ends, which in
/// practice means the extension disappeared (disabled, or GNOME Shell
/// restarted) — logged, not fatal to the daemon.
pub async fn watch_and_broadcast(backend: Arc<dyn ClipboardBackend>, shared: Arc<Shared>) {
    let mut changes = backend.watch();
    let mut last_hash: Option<[u8; 32]> = None;

    while let Some(change) = changes.next().await {
        if change.mime != MIME_TEXT_PLAIN {
            continue;
        }
        let bytes = match backend.read(&change.mime).await {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!(error = %e, "clipboard read failed after a change notification");
                continue;
            }
        };
        let clip = match Clip::new(change.mime, bytes) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(error = %e, "clipboard content rejected");
                continue;
            }
        };
        if !should_broadcast(&mut last_hash, clip.hash) {
            continue;
        }

        let handles: Vec<_> = shared
            .connected_devices
            .lock()
            .await
            .values()
            .cloned()
            .collect();
        let count = handles.len();
        for handle in handles {
            handle.send_clipboard(clip.clone());
        }
        tracing::info!(
            bytes = clip.content.len(),
            devices = count,
            "clipboard sent"
        );
    }
    tracing::warn!("clipboard change stream ended; clipboard sync stopped for this session");
}

/// `true` if `hash` is new since the last call — the one place the "did the
/// clipboard actually change" decision lives, so it's a plain unit test
/// rather than something only exercisable through a live D-Bus connection.
fn should_broadcast(last_hash: &mut Option<[u8; 32]>, hash: [u8; 32]) -> bool {
    if *last_hash == Some(hash) {
        false
    } else {
        *last_hash = Some(hash);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_hash_always_broadcasts() {
        let mut last = None;
        assert!(should_broadcast(&mut last, [1u8; 32]));
    }

    #[test]
    fn repeated_hash_is_suppressed() {
        let mut last = None;
        assert!(should_broadcast(&mut last, [1u8; 32]));
        assert!(!should_broadcast(&mut last, [1u8; 32]));
    }

    #[test]
    fn a_changed_hash_broadcasts_again() {
        let mut last = None;
        assert!(should_broadcast(&mut last, [1u8; 32]));
        assert!(should_broadcast(&mut last, [2u8; 32]));
    }

    #[test]
    fn reverting_to_a_prior_value_still_broadcasts() {
        // Only the *immediately previous* value is suppressed — this isn't
        // a full history of everything ever sent, just "did the very last
        // change notification turn out to be a no-op".
        let mut last = None;
        assert!(should_broadcast(&mut last, [1u8; 32]));
        assert!(should_broadcast(&mut last, [2u8; 32]));
        assert!(should_broadcast(&mut last, [1u8; 32]));
    }
}
