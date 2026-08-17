//! `PenguinSyncCore` — the entire FFI surface (docs/design.md §4.2).
//!
//! Deliberately narrow: one handle object, one callback interface, one
//! cancellable handle per long-lived operation. `protocol` never crosses the
//! boundary directly — everything here is plain data (`String`, primitives)
//! that `net`'s types get turned into.
//!
//! # Cancellation
//!
//! UniFFI does not propagate Kotlin coroutine cancellation into Rust. Every
//! long-lived operation ([`PenguinSyncCore::pair`]) returns a
//! [`ConnectionHandle`] with an explicit [`ConnectionHandle::cancel`], and
//! the Kotlin side's `callbackFlow` wrapper must end with
//! `awaitClose { handle.cancel() }` — getting this wrong leaks a tokio task
//! inside a process Android is trying to kill.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use penguinsync_net::reconnect::DialerEvent;
use penguinsync_net::session::{CloseReason, SessionEvent, SessionHandle};
use penguinsync_net::{Endpoint, FsSink, Identity, TransferSink, TrustStore};
use penguinsync_protocol::clipboard::{Clip, MIME_TEXT_PLAIN};
use penguinsync_protocol::pairing::decode_qr_uri;
use penguinsync_protocol::{LocalIdentity, PROTOCOL_VERSION, message};

use crate::state::PersistedPeers;

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum CoreError {
    #[error("loading device identity: {0}")]
    Identity(String),
    #[error("invalid pairing QR: {0}")]
    InvalidQr(String),
    #[error("network setup failed: {0}")]
    Network(String),
    #[error("clipboard content rejected: {0}")]
    InvalidClipboard(String),
    #[error("no device currently connected")]
    NotConnected,
}

/// One row of the Devices screen's paired-device list — read straight from
/// the persisted-peers file, not the live session, so it shows every device
/// ever paired with, connected or not (docs/design.md §4.6). `device_id` is
/// hex-encoded, matching [`CoreEvent::PeerHandshake`].
#[derive(uniffi::Record, Debug, Clone)]
pub struct PairedDevice {
    pub device_id: String,
    pub name: String,
}

#[derive(uniffi::Enum, Debug, Clone)]
pub enum CoreEvent {
    /// The peer's handshake arrived and versions matched. `device_id` is
    /// hex-encoded.
    PeerHandshake {
        device_id: String,
        name: String,
    },
    Ponged {
        rtt_ms: u64,
    },
    Disconnected {
        reason: String,
    },
    /// Retrying after `delay_ms` — the dialer's own backoff loop, driven
    /// entirely inside Rust (docs/design.md §5.3).
    Reconnecting {
        attempt: u32,
        delay_ms: u64,
    },
    /// Linux's clipboard changed. Write is unrestricted on Android — no
    /// permission needed, the M1 write path is just this event handler
    /// calling `ClipboardManager.setPrimaryClip` (docs/design.md §6.1,
    /// §3.1).
    ClipboardReceived {
        text: String,
    },
    /// This device just started sending a file — fired from
    /// [`PenguinSyncCore::send_file`]'s underlying
    /// [`SessionHandle::send_file`] (docs/design.md §6.2).
    TransferStarted {
        transfer_id: u64,
        name: String,
        size: u64,
    },
    /// The peer announced a file it's about to send us. Auto-accepted, no
    /// prompt — pairing is the trust decision (docs/design.md §6.2,
    /// docs/protocol.md §6.4).
    TransferOffered {
        transfer_id: u64,
        name: String,
        size: u64,
    },
    /// Cumulative bytes moved so far, either direction — `TransferStarted`
    /// vs. `TransferOffered` already told Kotlin which `transfer_id`s are
    /// sends and which are receives.
    TransferProgress {
        transfer_id: u64,
        bytes: u64,
        total: u64,
    },
    /// This device finished receiving a file — success or failure. On
    /// success `path` is where it landed under the app's downloads directory
    /// (`FsSink`, docs/design.md §6.2); on failure the partial file was
    /// discarded and `error` says why.
    TransferReceived {
        transfer_id: u64,
        name: String,
        path: Option<String>,
        ok: bool,
        error: Option<String>,
    },
    /// The peer's ack for a file this device sent.
    TransferAcked {
        transfer_id: u64,
        ok: bool,
        error: Option<String>,
    },
}

/// Implemented in Kotlin, wrapped once per stream in a `callbackFlow`
/// (docs/design.md §4.2).
#[uniffi::export(with_foreign)]
pub trait CoreEventListener: Send + Sync {
    fn on_event(&self, event: CoreEvent);
}

/// The single handle object the whole FFI surface is built around: `start`
/// happens in the constructor (there is no separate daemon process on
/// Android — the app *is* the process), `pair` is the one long-lived
/// operation M0 needs.
#[derive(uniffi::Object)]
pub struct PenguinSyncCore {
    runtime: tokio::runtime::Runtime,
    identity: Identity,
    device_name: String,
    trust: Arc<TrustStore>,
    peers_path: PathBuf,
    /// The current session's send handle, if any (M2, docs/design.md §6.1's
    /// Baseline tier). Plain `std::sync::Mutex`, not `tokio::sync::Mutex` —
    /// [`PenguinSyncCore::send_clipboard`] is called directly from Kotlin,
    /// outside any tokio context, and only ever holds this lock long enough
    /// to clone or replace the handle, never across an `.await`.
    active_session: Arc<std::sync::Mutex<Option<SessionHandle>>>,
    /// Where files the peer sends us land. A pragmatic v1 destination — a
    /// `downloads` subdirectory under the app-private `data_dir` — not the
    /// public Downloads collection: proper `MediaStore`/Storage-Access-
    /// Framework integration needs a real device to get right and is
    /// deferred, the same way docs/design.md §10 flags other
    /// device-only-verifiable items.
    sink: Arc<dyn TransferSink>,
}

#[uniffi::export]
impl PenguinSyncCore {
    /// `data_dir` must be an app-private directory Kotlin controls the
    /// lifetime of — this is where the device identity and pinned peer keys
    /// are written (docs/design.md §4.6).
    #[uniffi::constructor]
    pub fn new(data_dir: String, device_name: String) -> Result<Arc<Self>, CoreError> {
        let dir = PathBuf::from(data_dir);
        let identity =
            Identity::load_or_generate(&dir).map_err(|e| CoreError::Identity(e.to_string()))?;
        let peers_path = dir.join("peers.json");
        let peers = PersistedPeers::load(&peers_path);
        let trust = Arc::new(TrustStore::new(peers.device_ids()));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| CoreError::Network(e.to_string()))?;
        let sink: Arc<dyn TransferSink> = Arc::new(FsSink::new(dir.join("downloads")));
        Ok(Arc::new(Self {
            runtime,
            identity,
            device_name,
            trust,
            peers_path,
            active_session: Arc::new(std::sync::Mutex::new(None)),
            sink,
        }))
    }

    /// This device's fingerprint, for the pairing UI to show once Linux's
    /// TUI asks for confirmation (docs/protocol.md §3.2 step 4).
    pub fn device_fingerprint(&self) -> String {
        message::short_fingerprint(&self.identity.device_id)
    }

    /// `qr_uri` is exactly what the QR scanner (or a pasted-in fallback)
    /// read. Pins Linux's key immediately from the QR itself, dials the
    /// candidate addresses, and then keeps reconnecting forever — Android
    /// always dials (docs/design.md §5.3) — until `cancel()` is called on
    /// the returned handle.
    ///
    /// There is no separate confirmation step on this side: scanning the QR
    /// *is* the human action Android contributes to pairing. Linux still
    /// asks its own human to confirm the fingerprint before trusting back
    /// (docs/protocol.md §3.2).
    pub fn pair(
        &self,
        qr_uri: String,
        listener: Arc<dyn CoreEventListener>,
    ) -> Result<Arc<ConnectionHandle>, CoreError> {
        let payload = decode_qr_uri(&qr_uri).map_err(|e| CoreError::InvalidQr(e.to_string()))?;
        if payload.version != PROTOCOL_VERSION {
            return Err(CoreError::InvalidQr(format!(
                "protocol version mismatch: this app speaks {PROTOCOL_VERSION}, the QR is from {}",
                payload.version
            )));
        }
        let addr = *payload
            .addrs
            .first()
            .ok_or_else(|| CoreError::InvalidQr("QR carried no candidate address".into()))?;

        self.trust.pin(payload.device_id);

        // `pair()` is called directly from Kotlin, outside any tokio
        // context — but quinn's `Endpoint` binds its socket through the
        // *current* runtime handle even in its synchronous constructor.
        // `enter()` makes `self.runtime` that current runtime for the
        // duration of the call, without requiring an `async fn` here.
        let endpoint = {
            let _guard = self.runtime.enter();
            Endpoint::dialing(&self.identity, self.trust.clone())
                .map_err(|e| CoreError::Network(e.to_string()))?
        };
        let local = LocalIdentity {
            device_id: self.identity.device_id,
            name: self.device_name.clone(),
            capabilities: vec![],
        };

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let dial_task = self.runtime.spawn(penguinsync_net::reconnect::run(
            Arc::new(endpoint),
            addr,
            local,
            Duration::from_secs(20),
            Some(payload.token),
            self.sink.clone(),
            tx,
        ));
        self.runtime.spawn(forward_events(
            rx,
            listener,
            self.peers_path.clone(),
            payload.device_id,
            payload.name,
            self.active_session.clone(),
        ));

        Ok(Arc::new(ConnectionHandle { dial_task }))
    }

    /// Pushes this device's clipboard to Linux — the manual read tier
    /// (docs/design.md §6.1's Baseline row): Kotlin does the actual system
    /// clipboard read (it needs window focus, §3.1) and hands the resulting
    /// text here. Errs rather than silently dropping when nothing is
    /// connected, so the UI can tell the user the tap did nothing.
    pub fn send_clipboard(&self, text: String) -> Result<(), CoreError> {
        let clip = Clip::new(MIME_TEXT_PLAIN, text.into_bytes())
            .map_err(|e| CoreError::InvalidClipboard(e.to_string()))?;
        let handle = self
            .active_session
            .lock()
            .expect("active_session mutex poisoned")
            .clone();
        match handle {
            Some(handle) => {
                handle.send_clipboard(clip);
                Ok(())
            }
            None => Err(CoreError::NotConnected),
        }
    }

    /// Sends a local file to whatever device is currently connected
    /// (docs/design.md §6.2). `path` must already be a real filesystem path
    /// — resolving a Kotlin-side `content://` share URI down to one is
    /// Kotlin's job, matching how this project draws the Kotlin/Rust
    /// boundary (docs/design.md §4.6's "Kotlin owns" list). Fire-and-forget,
    /// like [`PenguinSyncCore::send_clipboard`]: the returned `JoinHandle` is
    /// dropped, and progress/outcome arrive as `CoreEvent::Transfer*` events
    /// instead. Errs rather than silently dropping when nothing is
    /// connected, so the UI can tell the user the tap did nothing.
    pub fn send_file(&self, path: String) -> Result<(), CoreError> {
        let handle = self
            .active_session
            .lock()
            .expect("active_session mutex poisoned")
            .clone();
        match handle {
            Some(handle) => {
                // Fire-and-forget: the `JoinHandle` is intentionally dropped,
                // not bound to `_` — that's what triggers clippy's
                // `let_underscore_future` (it can't tell an intentional
                // fire-and-forget apart from an accidentally-unawaited one).
                handle.send_file(PathBuf::from(path));
                Ok(())
            }
            None => Err(CoreError::NotConnected),
        }
    }

    /// Read-only snapshot for the Devices screen — every device ever paired
    /// with, from disk. Cheap enough to call on every recomposition trigger
    /// rather than caching in Kotlin: it's a small JSON file read, not a
    /// network round trip.
    pub fn list_paired_devices(&self) -> Vec<PairedDevice> {
        PersistedPeers::load(&self.peers_path)
            .entries()
            .map(|(device_id, name)| PairedDevice {
                device_id: device_id.to_string(),
                name: name.to_string(),
            })
            .collect()
    }
}

/// Forwards every network event to Kotlin, and persists the pin on the
/// first successful handshake — mirroring the daemon's "confirm, then
/// persist" step, but here the confirmation already happened when the human
/// scanned the QR. Also keeps `active_session` current, so
/// [`PenguinSyncCore::send_clipboard`] always has (or knows it lacks) a live
/// handle, independent of whatever Kotlin is doing with events.
async fn forward_events(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<DialerEvent>,
    listener: Arc<dyn CoreEventListener>,
    peers_path: PathBuf,
    device_id: penguinsync_protocol::DeviceId,
    name: String,
    active_session: Arc<std::sync::Mutex<Option<SessionHandle>>>,
) {
    let mut persisted = false;
    while let Some(event) = rx.recv().await {
        match &event {
            DialerEvent::Connected(handle) => {
                *active_session
                    .lock()
                    .expect("active_session mutex poisoned") = Some(handle.clone());
            }
            DialerEvent::Session(SessionEvent::Closed(_)) => {
                *active_session
                    .lock()
                    .expect("active_session mutex poisoned") = None;
            }
            _ => {}
        }
        if !persisted
            && matches!(
                event,
                DialerEvent::Session(SessionEvent::PeerHandshake { .. })
            )
        {
            persisted = true;
            let mut peers = PersistedPeers::load(&peers_path);
            peers.upsert(&device_id, &name);
            if let Err(e) = peers.save(&peers_path) {
                tracing::warn!(error = %e, "failed to persist paired peer");
            }
        }
        if let Some(core_event) = to_core_event(event) {
            listener.on_event(core_event);
        }
    }
}

/// `None` for events Kotlin has no use for: `Connected` is a plumbing detail
/// (`PeerHandshake` already tells it "connected"), and a clipboard update
/// that fails to decode as UTF-8 text — which a well-behaved peer will
/// never send, `text/plain` is the only MIME v1 accepts — is dropped rather
/// than forwarded as garbage.
fn to_core_event(event: DialerEvent) -> Option<CoreEvent> {
    match event {
        DialerEvent::Connected(_) => None,
        DialerEvent::Reconnecting { attempt, delay } => Some(CoreEvent::Reconnecting {
            attempt,
            delay_ms: delay.as_millis() as u64,
        }),
        DialerEvent::Session(SessionEvent::PeerHandshake {
            device_id, name, ..
        }) => Some(CoreEvent::PeerHandshake {
            device_id: message::to_hex(&device_id),
            name,
        }),
        DialerEvent::Session(SessionEvent::ClipboardReceived(clip)) => {
            match String::from_utf8(clip.content) {
                Ok(text) => Some(CoreEvent::ClipboardReceived { text }),
                Err(e) => {
                    tracing::warn!(error = %e, "clipboard update was not valid UTF-8; dropped");
                    None
                }
            }
        }
        DialerEvent::Session(SessionEvent::Ponged { rtt }) => Some(CoreEvent::Ponged {
            rtt_ms: rtt.as_millis() as u64,
        }),
        DialerEvent::Session(SessionEvent::TransferStarted {
            transfer_id,
            name,
            size,
        }) => Some(CoreEvent::TransferStarted {
            transfer_id,
            name,
            size,
        }),
        DialerEvent::Session(SessionEvent::TransferOffered {
            transfer_id,
            name,
            size,
        }) => Some(CoreEvent::TransferOffered {
            transfer_id,
            name,
            size,
        }),
        DialerEvent::Session(SessionEvent::TransferProgress {
            transfer_id,
            bytes,
            total,
        }) => Some(CoreEvent::TransferProgress {
            transfer_id,
            bytes,
            total,
        }),
        DialerEvent::Session(SessionEvent::TransferReceived {
            transfer_id,
            name,
            path,
            ok,
            error,
        }) => Some(CoreEvent::TransferReceived {
            transfer_id,
            name,
            path: path.map(|p| p.to_string_lossy().into_owned()),
            ok,
            error,
        }),
        DialerEvent::Session(SessionEvent::TransferAcked {
            transfer_id,
            ok,
            error,
        }) => Some(CoreEvent::TransferAcked {
            transfer_id,
            ok,
            error,
        }),
        DialerEvent::Session(SessionEvent::Closed(reason)) => Some(CoreEvent::Disconnected {
            reason: describe_close(reason),
        }),
    }
}

fn describe_close(reason: CloseReason) -> String {
    match reason {
        CloseReason::VersionMismatch { ours, theirs } => {
            format!("protocol version mismatch (this app: {ours}, peer: {theirs})")
        }
        CloseReason::KeepaliveTimedOut => "connection timed out".to_string(),
        CloseReason::ConnectionLost(s) | CloseReason::FramingError(s) => s,
    }
}

/// A live `pair()` call. `cancel()` aborts the reconnect loop; the event
/// forwarder notices its channel close and exits right behind it.
#[derive(uniffi::Object)]
pub struct ConnectionHandle {
    dial_task: tokio::task::JoinHandle<()>,
}

#[uniffi::export]
impl ConnectionHandle {
    pub fn cancel(&self) {
        self.dial_task.abort();
    }
}
