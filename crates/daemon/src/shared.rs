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

use penguinsync_net::{Identity, TrustStore};
use penguinsync_protocol::DeviceId;
use penguinsync_protocol::pairing::PairingToken;

use crate::state::PersistedState;

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
}

impl Shared {
    pub async fn persist_state(&self) {
        let state = self.state.lock().await;
        if let Err(e) = state.save(&self.state_path) {
            tracing::error!(error = %e, path = %self.state_path.display(), "failed to save device state");
        }
    }
}
