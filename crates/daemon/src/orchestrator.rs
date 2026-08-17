//! Bridges [`penguinsync_net::listener`] events to D-Bus state.
//!
//! Every incoming connection has already passed TLS (pinned key, or an open
//! pairing window — docs/design.md §7) before an event ever reaches here.
//! What's left is bookkeeping: recognize an already-paired device and flip
//! its `Connected` property, or walk a brand-new one through the
//! confirm-by-human step and persist the pin — plus, since M1, surfacing
//! each connection's send handle so the clipboard broadcaster
//! (`crate::clipboard`) has someone to send to, and since M4, turning file
//! transfer events into `Daemon1::transfer_progress`/`transfer_finished`
//! signals and a desktop notification on arrival (`crate::notify`).

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::mpsc;

use penguinsync_net::listener::ListenerEvent;
use penguinsync_net::session::{CloseReason, SessionEvent};
use penguinsync_protocol::message;

use crate::dbus::{self, Daemon1, Device1};
use crate::shared::{Shared, TransferDirection, TransferRecord};

pub async fn run(
    mut events: mpsc::UnboundedReceiver<ListenerEvent>,
    shared: Arc<Shared>,
    connection: zbus::Connection,
) {
    while let Some(event) = events.recv().await {
        match event {
            ListenerEvent::Connected { remote, handle } => {
                shared.remote_handles.lock().await.insert(remote, handle);
            }
            ListenerEvent::Session { remote, event } => {
                handle_session_event(&shared, &connection, remote, event).await;
            }
        }
    }
}

async fn handle_session_event(
    shared: &Arc<Shared>,
    connection: &zbus::Connection,
    remote: SocketAddr,
    event: SessionEvent,
) {
    match event {
        SessionEvent::PeerHandshake {
            device_id,
            name,
            pairing_token,
            ..
        } => {
            shared
                .remote_to_device
                .lock()
                .await
                .insert(remote, device_id);
            handle_handshake(shared, connection, remote, device_id, name, pairing_token).await;
        }
        SessionEvent::Ponged { rtt } => {
            tracing::debug!(?remote, ?rtt, "ping round trip");
        }
        SessionEvent::ClipboardReceived(clip) => {
            let sender = shared.remote_to_device.lock().await.get(&remote).copied();
            match sender {
                Some(sender) => crate::clipboard::handle_received(shared, sender, clip).await,
                // Shouldn't happen — the control stream only carries
                // Clipboard once a session is Ready, which is after the
                // handshake that populates this map — but a stray message
                // is dropped rather than panicking on the unwrap.
                None => tracing::warn!(
                    ?remote,
                    "clipboard message received before handshake; dropping"
                ),
            }
        }
        SessionEvent::TransferStarted {
            transfer_id,
            name,
            size,
        } => {
            shared.transfers.lock().await.insert(
                transfer_id,
                TransferRecord {
                    name: name.clone(),
                    direction: TransferDirection::Send,
                },
            );
            signal_ctx(shared, connection, remote)
                .emit_progress(transfer_id, &name, 0, size, TransferDirection::Send)
                .await;
        }
        SessionEvent::TransferOffered {
            transfer_id,
            name,
            size,
        } => {
            shared.transfers.lock().await.insert(
                transfer_id,
                TransferRecord {
                    name: name.clone(),
                    direction: TransferDirection::Receive,
                },
            );
            signal_ctx(shared, connection, remote)
                .emit_progress(transfer_id, &name, 0, size, TransferDirection::Receive)
                .await;
        }
        SessionEvent::TransferProgress {
            transfer_id,
            bytes,
            total,
        } => {
            let record = shared.transfers.lock().await.get(&transfer_id).cloned();
            let Some(record) = record else {
                // A progress tick for a transfer we never saw start/offered —
                // shouldn't happen, but nothing useful to report either.
                return;
            };
            signal_ctx(shared, connection, remote)
                .emit_progress(transfer_id, &record.name, bytes, total, record.direction)
                .await;
        }
        SessionEvent::TransferReceived {
            transfer_id,
            name,
            path,
            ok,
            error,
        } => {
            shared.transfers.lock().await.remove(&transfer_id);
            signal_ctx(shared, connection, remote)
                .emit_finished(
                    transfer_id,
                    &name,
                    ok,
                    error.as_deref(),
                    TransferDirection::Receive,
                )
                .await;
            if ok && let Some(path) = path {
                crate::notify::file_received(&name, &path).await;
            }
        }
        SessionEvent::TransferAcked {
            transfer_id,
            ok,
            error,
        } => {
            let record = shared.transfers.lock().await.remove(&transfer_id);
            let name = record.map(|r| r.name).unwrap_or_default();
            signal_ctx(shared, connection, remote)
                .emit_finished(
                    transfer_id,
                    &name,
                    ok,
                    error.as_deref(),
                    TransferDirection::Send,
                )
                .await;
        }
        SessionEvent::Closed(reason) => {
            shared.remote_handles.lock().await.remove(&remote);
            let device_id = shared.remote_to_device.lock().await.remove(&remote);
            if let Some(id) = device_id {
                shared.connected_devices.lock().await.remove(&id);
                tracing::info!(device = %message::short_fingerprint(&id), reason = ?close_reason_label(&reason), "device disconnected");
                dbus::set_connected(connection.object_server(), &id, false).await;
            }
        }
    }
}

fn signal_ctx<'a>(
    shared: &'a Shared,
    connection: &'a zbus::Connection,
    remote: SocketAddr,
) -> TransferSignalCtx<'a> {
    TransferSignalCtx {
        shared,
        connection,
        remote,
    }
}

/// This connection's device id, hex-encoded, or `String::new()` if the
/// handshake hasn't landed yet — shouldn't happen for a transfer event (they
/// only ever fire once a session is `Ready`), but the D-Bus signal still
/// needs *some* string rather than a panic.
async fn device_hex(shared: &Shared, remote: SocketAddr) -> String {
    shared
        .remote_to_device
        .lock()
        .await
        .get(&remote)
        .map(message::to_hex)
        .unwrap_or_default()
}

/// The three things every transfer-signal emitter needs just to find the
/// right `Daemon1` object and label the signal with the right device —
/// bundled so `emit_progress`/`emit_finished` don't each carry three
/// separate parameters on top of the transfer's own fields.
struct TransferSignalCtx<'a> {
    shared: &'a Shared,
    connection: &'a zbus::Connection,
    remote: SocketAddr,
}

impl TransferSignalCtx<'_> {
    async fn emit_progress(
        &self,
        transfer_id: u64,
        name: &str,
        bytes: u64,
        total: u64,
        direction: TransferDirection,
    ) {
        let Ok(iface) = self
            .connection
            .object_server()
            .interface::<_, Daemon1>(dbus::ROOT_PATH)
            .await
        else {
            return;
        };
        let _ = Daemon1::transfer_progress(
            iface.signal_emitter(),
            device_hex(self.shared, self.remote).await,
            transfer_id,
            name.to_string(),
            bytes,
            total,
            direction.as_str().to_string(),
        )
        .await;
    }

    async fn emit_finished(
        &self,
        transfer_id: u64,
        name: &str,
        ok: bool,
        error: Option<&str>,
        direction: TransferDirection,
    ) {
        let Ok(iface) = self
            .connection
            .object_server()
            .interface::<_, Daemon1>(dbus::ROOT_PATH)
            .await
        else {
            return;
        };
        let _ = Daemon1::transfer_finished(
            iface.signal_emitter(),
            device_hex(self.shared, self.remote).await,
            transfer_id,
            name.to_string(),
            ok,
            error.unwrap_or_default().to_string(),
            direction.as_str().to_string(),
        )
        .await;
    }
}

async fn handle_handshake(
    shared: &Arc<Shared>,
    connection: &zbus::Connection,
    remote: SocketAddr,
    device_id: penguinsync_protocol::DeviceId,
    name: String,
    pairing_token: Option<penguinsync_protocol::pairing::TokenBytes>,
) {
    if shared.trust.is_paired(&device_id) {
        tracing::info!(device = %message::short_fingerprint(&device_id), %name, "device reconnected");
        mark_connected(shared, connection, remote, device_id, &name).await;
        return;
    }

    // Unpaired: this connection only got past TLS because a pairing window
    // was open. Redeem the token before ever prompting a human — a wrong or
    // stale token means this isn't the device the QR was shown to.
    let Some(presented) = pairing_token else {
        tracing::warn!(device = %message::short_fingerprint(&device_id), "unpaired peer connected with no pairing token; dropping");
        return;
    };
    let redeemed = {
        let mut token = shared.current_token.lock().await;
        match token.as_mut() {
            Some(t) => t.redeem(presented, std::time::Instant::now()).is_ok(),
            None => false,
        }
    };
    if !redeemed {
        tracing::warn!(device = %message::short_fingerprint(&device_id), "pairing token invalid or expired; dropping");
        return;
    }

    let fingerprint = message::short_fingerprint(&device_id);
    tracing::info!(device = %fingerprint, %name, "pairing request, awaiting confirmation");
    let _ = Daemon1::pairing_requested(
        &connection
            .object_server()
            .interface::<_, Daemon1>(dbus::ROOT_PATH)
            .await
            .expect("Daemon1 is always registered")
            .signal_emitter()
            .clone(),
        message::to_hex(&device_id),
        fingerprint,
        name.clone(),
    )
    .await;

    if !dbus::await_confirmation(shared, device_id).await {
        tracing::info!(device = %message::short_fingerprint(&device_id), "pairing rejected or timed out");
        return;
    }

    shared.trust.pin(device_id);
    shared.trust.close_pairing_window();
    {
        let mut state = shared.state.lock().await;
        state.upsert(&device_id, &name);
    }
    shared.persist_state().await;

    mark_connected(shared, connection, remote, device_id, &name).await;
    tracing::info!(device = %message::short_fingerprint(&device_id), "paired");
}

/// Common tail of both the reconnect and the fresh-pairing paths: create/
/// update the `Device1` object, flip `Connected`, and promote this
/// connection's send handle so the clipboard broadcaster can reach it.
async fn mark_connected(
    shared: &Arc<Shared>,
    connection: &zbus::Connection,
    remote: SocketAddr,
    device_id: penguinsync_protocol::DeviceId,
    name: &str,
) {
    ensure_device_object(shared, connection, device_id, name).await;
    dbus::set_connected(connection.object_server(), &device_id, true).await;
    if let Some(handle) = shared.remote_handles.lock().await.get(&remote).cloned() {
        shared
            .connected_devices
            .lock()
            .await
            .insert(device_id, handle);
    }
}

async fn ensure_device_object(
    shared: &Arc<Shared>,
    connection: &zbus::Connection,
    device_id: penguinsync_protocol::DeviceId,
    name: &str,
) {
    let path = dbus::device_path(&device_id);
    let server = connection.object_server();
    if server.interface::<_, Device1>(path.clone()).await.is_ok() {
        return;
    }
    let _ = server
        .at(
            path,
            Device1 {
                name: name.to_string(),
                device_id: message::to_hex(&device_id),
                connected: false,
                shared: shared.clone(),
            },
        )
        .await;
}

fn close_reason_label(reason: &CloseReason) -> &'static str {
    match reason {
        CloseReason::VersionMismatch { .. } => "version-mismatch",
        CloseReason::KeepaliveTimedOut => "keepalive-timed-out",
        CloseReason::ConnectionLost(_) => "connection-lost",
        CloseReason::FramingError(_) => "framing-error",
    }
}
