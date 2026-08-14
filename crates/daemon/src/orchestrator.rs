//! Bridges [`penguinsync_net::listener`] events to D-Bus state.
//!
//! Every incoming connection has already passed TLS (pinned key, or an open
//! pairing window — docs/design.md §7) before an event ever reaches here.
//! What's left is bookkeeping: recognize an already-paired device and flip
//! its `Connected` property, or walk a brand-new one through the
//! confirm-by-human step and persist the pin.

use std::sync::Arc;

use tokio::sync::mpsc;

use penguinsync_net::listener::ListenerEvent;
use penguinsync_net::session::{CloseReason, SessionEvent};
use penguinsync_protocol::message;

use crate::dbus::{self, Daemon1, Device1};
use crate::shared::Shared;

pub async fn run(
    mut events: mpsc::UnboundedReceiver<ListenerEvent>,
    shared: Arc<Shared>,
    connection: zbus::Connection,
) {
    while let Some(ListenerEvent { remote, event }) = events.recv().await {
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
                handle_handshake(&shared, &connection, device_id, name, pairing_token).await;
            }
            SessionEvent::Ponged { rtt } => {
                tracing::debug!(?remote, ?rtt, "ping round trip");
            }
            SessionEvent::Closed(reason) => {
                let device_id = shared.remote_to_device.lock().await.remove(&remote);
                if let Some(id) = device_id {
                    tracing::info!(device = %message::short_fingerprint(&id), reason = ?close_reason_label(&reason), "device disconnected");
                    dbus::set_connected(connection.object_server(), &id, false).await;
                }
            }
        }
    }
}

async fn handle_handshake(
    shared: &Arc<Shared>,
    connection: &zbus::Connection,
    device_id: penguinsync_protocol::DeviceId,
    name: String,
    pairing_token: Option<penguinsync_protocol::pairing::TokenBytes>,
) {
    if shared.trust.is_paired(&device_id) {
        tracing::info!(device = %message::short_fingerprint(&device_id), %name, "device reconnected");
        ensure_device_object(shared, connection, device_id, &name).await;
        dbus::set_connected(connection.object_server(), &device_id, true).await;
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

    ensure_device_object(shared, connection, device_id, &name).await;
    dbus::set_connected(connection.object_server(), &device_id, true).await;
    tracing::info!(device = %message::short_fingerprint(&device_id), "paired");
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
            },
        )
        .await;
    let _ = shared; // reserved: future milestones persist per-device settings here too
}

fn close_reason_label(reason: &CloseReason) -> &'static str {
    match reason {
        CloseReason::VersionMismatch { .. } => "version-mismatch",
        CloseReason::KeepaliveTimedOut => "keepalive-timed-out",
        CloseReason::ConnectionLost(_) => "connection-lost",
        CloseReason::FramingError(_) => "framing-error",
    }
}
