//! `org.penguinsync.Daemon1` — bus name and D-Bus surface (docs/design.md
//! §4.3).
//!
//! Root object `/org/penguinsync/Daemon` implements
//! `org.freedesktop.DBus.ObjectManager` plus this module's own `Daemon1`
//! interface; one `Device1` object per paired device lives underneath it at
//! `/org/penguinsync/Daemon/devices/<hex device id>`. Clients call
//! `GetManagedObjects()` then follow `InterfacesAdded`/`InterfacesRemoved` —
//! standard enough that the Nautilus extension (docs/design.md §4.5) is
//! mostly borrowed boilerplate on top of it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};
use zbus::{fdo, interface};

use penguinsync_protocol::pairing::{PairingToken, QrPayload, TOKEN_LEN, TOKEN_TTL, encode_qr_uri};
use penguinsync_protocol::{PROTOCOL_VERSION, message};

use crate::shared::Shared;

/// `file://` URI → local path. `url` (already a `penguinsync-protocol`
/// dependency, added here too) handles percent-decoding correctly —
/// Nautilus's `Gio.File.get_uri()` percent-encodes spaces and other
/// non-ASCII characters, and hand-rolled decoding is exactly the kind of
/// thing that silently mishandles a filename with an `%` in it.
fn uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

pub const BUS_NAME: &str = "org.penguinsync.Daemon1";
pub const ROOT_PATH: &str = "/org/penguinsync/Daemon";

pub fn device_path(id: &penguinsync_protocol::DeviceId) -> OwnedObjectPath {
    ObjectPath::try_from(format!("{ROOT_PATH}/devices/{}", message::to_hex(id)))
        .expect("hex device id is a valid path segment")
        .into()
}

/// The `Daemon1` interface. One instance, registered at [`ROOT_PATH`].
pub struct Daemon1 {
    pub shared: Arc<Shared>,
}

#[interface(name = "org.penguinsync.Daemon1")]
impl Daemon1 {
    /// Generates a pairing token, opens the 60 s trust window, and returns
    /// the QR payload (as a URI string, for the TUI to render) plus this
    /// device's fingerprint for the human to read alongside it.
    async fn start_pairing(&self) -> fdo::Result<(String, String)> {
        let random: [u8; TOKEN_LEN] = rand::random();
        let now = Instant::now();
        let token = PairingToken::issue(random, now);
        self.shared.trust.open_pairing_window(now + TOKEN_TTL);
        *self.shared.current_token.lock().await = Some(token);

        let qr = QrPayload {
            version: PROTOCOL_VERSION,
            device_id: self.shared.identity.device_id,
            name: self.shared.name.clone(),
            addrs: crate::net_addrs::candidate_addrs(self.shared.listen_addr),
            token: random,
        };
        let fingerprint = message::short_fingerprint(&self.shared.identity.device_id);
        Ok((encode_qr_uri(&qr), fingerprint))
    }

    /// Answers a pending `PairingRequested` signal. `device_id` is the hex
    /// id from that signal; unknown or already-answered ids are a no-op —
    /// the confirmation may simply have timed out already.
    async fn confirm_pairing(&self, device_id: String, accept: bool) -> fdo::Result<()> {
        let id = message::from_hex(&device_id)
            .ok_or_else(|| fdo::Error::InvalidArgs("malformed device id".into()))?;
        if let Some(tx) = self.shared.pending_confirmations.lock().await.remove(&id) {
            let _ = tx.send(accept);
        }
        Ok(())
    }

    /// Unpair is unilateral and immediate (docs/design.md §7): revokes the
    /// pin so the next connection attempt is rejected at the TLS layer, with
    /// no cooperation from the peer needed.
    async fn unpair(
        &self,
        device_id: String,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> fdo::Result<()> {
        let id = message::from_hex(&device_id)
            .ok_or_else(|| fdo::Error::InvalidArgs("malformed device id".into()))?;
        self.shared.trust.unpair(&id);
        {
            let mut state = self.shared.state.lock().await;
            state.remove(&id);
        }
        self.shared.persist_state().await;
        let _ = server.remove::<Device1, _>(device_path(&id)).await;
        Ok(())
    }

    /// A device connected with a token matching the open pairing window and
    /// is waiting for a human to confirm the fingerprint matches what's on
    /// its screen (docs/protocol.md §3.2 step 4).
    #[zbus(signal)]
    pub async fn pairing_requested(
        emitter: &SignalEmitter<'_>,
        device_id: String,
        fingerprint: String,
        name: String,
    ) -> zbus::Result<()>;

    /// A file transfer is under way — either direction, `direction` is
    /// `"send"` or `"receive"` (docs/design.md §6.2, §9's "TUI progress").
    /// `bytes`/`total` are cumulative, so a client can just render a bar.
    #[zbus(signal)]
    pub async fn transfer_progress(
        emitter: &SignalEmitter<'_>,
        device_id: String,
        transfer_id: u64,
        name: String,
        bytes: u64,
        total: u64,
        direction: String,
    ) -> zbus::Result<()>;

    /// A file transfer ended, successfully or not. `error` is empty when
    /// `ok` is true — D-Bus signals don't carry `Option`, and every other
    /// string field on this interface already uses "empty means absent"
    /// rather than pull in a variant wrapper for one field.
    #[zbus(signal)]
    pub async fn transfer_finished(
        emitter: &SignalEmitter<'_>,
        device_id: String,
        transfer_id: u64,
        name: String,
        ok: bool,
        error: String,
        direction: String,
    ) -> zbus::Result<()>;
}

/// One `Device1` object per paired device, at [`device_path`].
pub struct Device1 {
    pub name: String,
    pub device_id: String,
    pub connected: bool,
    /// Needed by [`Device1::send_files`] to reach this device's live
    /// [`penguinsync_net::SessionHandle`] — the other three fields are
    /// plain snapshots, this one is a way back into the running system.
    pub shared: Arc<Shared>,
}

#[interface(name = "org.penguinsync.Device1")]
impl Device1 {
    #[zbus(property)]
    async fn name(&self) -> String {
        self.name.clone()
    }

    #[zbus(property)]
    async fn device_id(&self) -> String {
        self.device_id.clone()
    }

    #[zbus(property)]
    async fn connected(&self) -> bool {
        self.connected
    }

    /// Sends each of `uris` (`file://…`, as Nautilus's `get_uri()` or the
    /// CLI's `send` verb produce them — docs/design.md §4.5, §6.2) to this
    /// device. Auto-accepted on arrival, no prompt — pairing is the trust
    /// decision (docs/protocol.md §6.4). Fire-and-forget per file: progress
    /// and outcome arrive as `Daemon1::transfer_progress`/`transfer_finished`
    /// signals, not as this call's return value, since a multi-file send
    /// outlives the D-Bus method call by a wide margin.
    async fn send_files(&self, uris: Vec<String>) -> fdo::Result<()> {
        let id = message::from_hex(&self.device_id)
            .ok_or_else(|| fdo::Error::InvalidArgs("malformed device id".into()))?;
        let handle = self.shared.connected_devices.lock().await.get(&id).cloned();
        let Some(handle) = handle else {
            return Err(fdo::Error::Failed(
                "device is not currently connected".into(),
            ));
        };
        for uri in uris {
            match uri_to_path(&uri) {
                Some(path) => {
                    handle.send_file(path);
                }
                None => {
                    tracing::warn!(%uri, "SendFiles given an unusable file:// URI; skipping");
                }
            }
        }
        Ok(())
    }
}

/// Flip a `Device1`'s `Connected` property and emit the property-changed
/// signal, if that object is currently registered. Called from the
/// orchestrator, outside of any D-Bus method dispatch, so it goes through
/// `ObjectServer::interface` rather than `&mut self`/`#[zbus(signal_emitter)]`
/// (docs comment on `ObjectServer::interface` in zbus covers exactly this
/// case).
pub async fn set_connected(
    server: &zbus::ObjectServer,
    device_id: &penguinsync_protocol::DeviceId,
    connected: bool,
) {
    let path = device_path(device_id);
    let Ok(iface_ref) = server.interface::<_, Device1>(path).await else {
        return;
    };
    let mut iface = iface_ref.get_mut().await;
    if iface.connected == connected {
        return;
    }
    iface.connected = connected;
    let _ = iface.connected_changed(iface_ref.signal_emitter()).await;
}

/// Wait for a pairing confirmation, or `false` if the window closes first —
/// a human who doesn't answer is the same as a human who says no.
pub async fn await_confirmation(
    shared: &Shared,
    device_id: penguinsync_protocol::DeviceId,
) -> bool {
    let (tx, rx) = tokio::sync::oneshot::channel();
    shared
        .pending_confirmations
        .lock()
        .await
        .insert(device_id, tx);
    tokio::time::timeout(TOKEN_TTL + Duration::from_secs(5), rx)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(false)
}
