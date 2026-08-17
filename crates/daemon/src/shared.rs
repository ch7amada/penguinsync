//! State shared between the D-Bus interfaces and the connection orchestrator.
//!
//! One `Arc<Shared>` is handed to both sides: the `Daemon1`/`Device1`
//! zbus interfaces (driven by TUI/CLI calls) and the task that reacts to
//! [`penguinsync_net::listener`] events (driven by the network). Neither
//! owns the other; this is the seam between them.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, oneshot};

use penguinsync_net::{ClipboardBackend, Identity, SessionHandle, TrustStore};
use penguinsync_protocol::DeviceId;
use penguinsync_protocol::pairing::PairingToken;

use crate::state::PersistedState;

/// Which side initiated a given `transfer_id` — the daemon needs to
/// remember this across a transfer's lifetime so `TransferProgress` events
/// (which don't carry a direction of their own — docs/protocol.md §6.4) can
/// be labelled correctly on the `TransferProgress`/`TransferFinished` D-Bus
/// signals (`crate::dbus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Send,
    Receive,
}

impl TransferDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            TransferDirection::Send => "send",
            TransferDirection::Receive => "receive",
        }
    }
}

/// Clipboard state shared between the watch-and-broadcast loop (Linux's own
/// clipboard changing) and the orchestrator's handling of an incoming
/// `Clipboard` message (Android's clipboard changing, M2) — both directions
/// go through the same echo-suppression hash, since a write from one side
/// triggers the other side's own change notification (docs/design.md §6.1).
#[derive(Default)]
pub struct ClipboardState {
    /// `None` until the GNOME extension is found (or if it never is —
    /// docs/design.md §4.4). Written once at startup, read on every incoming
    /// clipboard message.
    pub backend: Mutex<Option<Arc<dyn ClipboardBackend>>>,
    /// The most recent content hash sent or applied, in either direction.
    /// Only the immediately previous value is suppressed — see
    /// `crate::clipboard::should_broadcast`.
    pub last_hash: Mutex<Option<[u8; 32]>>,
}

pub struct Shared {
    pub identity: Identity,
    pub name: String,
    pub listen_addr: SocketAddr,
    pub trust: Arc<TrustStore>,
    pub state_path: PathBuf,
    pub state: Mutex<PersistedState>,

    /// The token from the most recent `StartPairing()` call, if its window
    /// hasn't closed yet. Single-slot: only one pairing attempt is ever in
    /// flight, matching the single QR code on screen.
    pub current_token: Mutex<Option<PairingToken>>,

    /// A confirmation the TUI hasn't answered yet, keyed by the connecting
    /// device's id. `confirm_pairing()` resolves it; the orchestrator awaits
    /// it after emitting `PairingRequested`.
    pub pending_confirmations: Mutex<HashMap<DeviceId, oneshot::Sender<bool>>>,

    /// Correlates a live session's remote address back to the device it
    /// turned out to be, so a later `Closed` event (which carries no device
    /// identity of its own) can find the right `Device1` object again.
    pub remote_to_device: Mutex<HashMap<SocketAddr, DeviceId>>,

    /// A connection's send handle, keyed by remote address from the moment
    /// it's accepted — before the handshake names which device it is. Once
    /// it does, [`crate::orchestrator`] promotes the entry into
    /// `connected_devices` below.
    pub remote_handles: Mutex<HashMap<SocketAddr, SessionHandle>>,

    /// Every currently connected, paired device's send handle, keyed by
    /// device id. What the clipboard broadcaster (docs/design.md §6.1) and
    /// `Device1::send_files` (docs/design.md §6.2) iterate to reach live
    /// connections.
    pub connected_devices: Mutex<HashMap<DeviceId, SessionHandle>>,

    pub clipboard: ClipboardState,

    /// Name and direction of every transfer currently in flight, keyed by
    /// `transfer_id` — populated on `TransferStarted`/`TransferOffered`,
    /// consulted by `TransferProgress` (which carries neither on the wire —
    /// docs/protocol.md §6.4) and removed on the terminal event
    /// (`TransferReceived`/`TransferAcked`), all in `crate::orchestrator`.
    pub transfers: Mutex<HashMap<u64, TransferRecord>>,
}

#[derive(Debug, Clone)]
pub struct TransferRecord {
    pub name: String,
    pub direction: TransferDirection,
}

impl Shared {
    pub async fn persist_state(&self) {
        let state = self.state.lock().await;
        if let Err(e) = state.save(&self.state_path) {
            tracing::error!(error = %e, path = %self.state_path.display(), "failed to save device state");
        }
    }
}
