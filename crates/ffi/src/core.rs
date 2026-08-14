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
use penguinsync_net::session::{CloseReason, SessionEvent};
use penguinsync_net::{Endpoint, Identity, TrustStore};
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
        Ok(Arc::new(Self {
            runtime,
            identity,
            device_name,
            trust,
            peers_path,
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
            tx,
        ));
        self.runtime.spawn(forward_events(
            rx,
            listener,
            self.peers_path.clone(),
            payload.device_id,
            payload.name,
        ));

        Ok(Arc::new(ConnectionHandle { dial_task }))
    }
}

/// Forwards every network event to Kotlin, and persists the pin on the
/// first successful handshake — mirroring the daemon's "confirm, then
/// persist" step, but here the confirmation already happened when the human
/// scanned the QR.
async fn forward_events(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<DialerEvent>,
    listener: Arc<dyn CoreEventListener>,
    peers_path: PathBuf,
    device_id: penguinsync_protocol::DeviceId,
    name: String,
) {
    let mut persisted = false;
    while let Some(event) = rx.recv().await {
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
        listener.on_event(to_core_event(event));
    }
}

fn to_core_event(event: DialerEvent) -> CoreEvent {
    match event {
        DialerEvent::Reconnecting { attempt, delay } => CoreEvent::Reconnecting {
            attempt,
            delay_ms: delay.as_millis() as u64,
        },
        DialerEvent::Session(SessionEvent::PeerHandshake {
            device_id, name, ..
        }) => CoreEvent::PeerHandshake {
            device_id: message::to_hex(&device_id),
            name,
        },
        DialerEvent::Session(SessionEvent::Ponged { rtt }) => CoreEvent::Ponged {
            rtt_ms: rtt.as_millis() as u64,
        },
        DialerEvent::Session(SessionEvent::Closed(reason)) => CoreEvent::Disconnected {
            reason: describe_close(reason),
        },
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
