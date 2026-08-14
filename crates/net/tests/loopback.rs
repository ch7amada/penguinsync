//! Integration tests for M0: two real `net` instances talking real QUIC over
//! loopback (docs/design.md §4.2, §9). No sockets are mocked here — this is
//! the closest this repo can get to "pull the Wi-Fi, walk away, come back"
//! without an actual phone.
//!
//! Roles follow docs/protocol.md §4: Linux listens, Android dials.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;

use penguinsync_net::{
    endpoint::Endpoint, identity::Identity, listener, reconnect, session::SessionEvent,
    tls::TrustStore,
};
use penguinsync_protocol::LocalIdentity;

const KEEPALIVE: Duration = Duration::from_millis(500);
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

fn local_identity(device_id: penguinsync_protocol::DeviceId, name: &str) -> LocalIdentity {
    LocalIdentity {
        device_id,
        name: name.to_string(),
        capabilities: vec![],
    }
}

async fn next<T>(rx: &mut mpsc::UnboundedReceiver<T>) -> T {
    timeout(TEST_TIMEOUT, rx.recv())
        .await
        .expect("timed out waiting for event")
        .expect("channel closed unexpectedly")
}

/// Drain `rx` until a `PeerHandshake` arrives, returning its device_id.
/// Ignores `Reconnecting`/other events along the way.
async fn wait_for_dialer_handshake(
    rx: &mut mpsc::UnboundedReceiver<reconnect::DialerEvent>,
) -> penguinsync_protocol::DeviceId {
    loop {
        match next(rx).await {
            reconnect::DialerEvent::Session(SessionEvent::PeerHandshake { device_id, .. }) => {
                return device_id;
            }
            _ => continue,
        }
    }
}

async fn wait_for_listener_handshake(
    rx: &mut mpsc::UnboundedReceiver<listener::ListenerEvent>,
) -> penguinsync_protocol::DeviceId {
    loop {
        let ev = next(rx).await;
        if let SessionEvent::PeerHandshake { device_id, .. } = ev.event {
            return device_id;
        }
    }
}

async fn wait_for_dialer_pong(rx: &mut mpsc::UnboundedReceiver<reconnect::DialerEvent>) {
    loop {
        if let reconnect::DialerEvent::Session(SessionEvent::Ponged { .. }) = next(rx).await {
            return;
        }
    }
}

async fn wait_for_listener_pong(rx: &mut mpsc::UnboundedReceiver<listener::ListenerEvent>) {
    loop {
        if let SessionEvent::Ponged { .. } = next(rx).await.event {
            return;
        }
    }
}

/// Spawn a Linux-side listener on `addr` (0 = ephemeral port), returning its
/// bound address, its event receiver, the endpoint (to sever it later — see
/// the test below), and the accept-loop task handle.
async fn spawn_listener(
    identity: &Identity,
    trust: Arc<TrustStore>,
    addr: SocketAddr,
) -> (
    SocketAddr,
    mpsc::UnboundedReceiver<listener::ListenerEvent>,
    Arc<Endpoint>,
    tokio::task::JoinHandle<()>,
) {
    // Rebinding the exact port a just-closed endpoint used can transiently
    // race the OS/runtime releasing it (a test-only artifact of reusing one
    // process — a real daemon restart doesn't have this problem). Retry
    // briefly rather than require perfect teardown ordering.
    let endpoint = {
        let mut last_err = None;
        let mut bound = None;
        for _ in 0..20 {
            match Endpoint::listening(identity, trust.clone(), addr) {
                Ok(e) => {
                    bound = Some(e);
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
        Arc::new(bound.unwrap_or_else(|| panic!("could not bind {addr}: {last_err:?}")))
    };
    let bound = endpoint.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    let local = local_identity(identity.device_id, "desk-fedora");
    let handle = tokio::spawn({
        let endpoint = endpoint.clone();
        async move {
            listener::run(endpoint, local, KEEPALIVE, tx).await;
        }
    });
    (bound, rx, endpoint, handle)
}

#[tokio::test]
async fn pairs_connects_and_survives_a_dropped_connection() {
    let linux_identity = Identity::generate().unwrap();
    let android_identity = Identity::generate().unwrap();

    // --- Pairing: Android already pinned Linux's key from the QR; Linux
    // opens a pairing window (the QR is on screen) and has no pins yet.
    let linux_trust = Arc::new(TrustStore::new([]));
    linux_trust.open_pairing_window(std::time::Instant::now() + Duration::from_secs(60));
    let android_trust = Arc::new(TrustStore::new([linux_identity.device_id]));

    let (linux_addr, mut listener_rx, linux_endpoint, listener_handle) = spawn_listener(
        &linux_identity,
        linux_trust.clone(),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await;

    let android_endpoint =
        Arc::new(Endpoint::dialing(&android_identity, android_trust.clone()).unwrap());
    let (dialer_tx, mut dialer_rx) = mpsc::unbounded_channel();
    let android_local = local_identity(android_identity.device_id, "pixel");
    tokio::spawn(reconnect::run(
        android_endpoint,
        linux_addr,
        android_local,
        KEEPALIVE,
        Some([0xABu8; 16]),
        dialer_tx,
    ));

    // --- Both sides complete the handshake and see the right peer.
    let seen_by_linux = wait_for_listener_handshake(&mut listener_rx).await;
    assert_eq!(seen_by_linux, android_identity.device_id);
    let seen_by_android = wait_for_dialer_handshake(&mut dialer_rx).await;
    assert_eq!(seen_by_android, linux_identity.device_id);

    // Linux's TUI would now show the confirmation prompt; simulate the human
    // confirming it (docs/protocol.md §3.2 step 4).
    linux_trust.pin(android_identity.device_id);
    linux_trust.close_pairing_window();

    // --- A real round trip happens on a real QUIC stream, both directions.
    wait_for_listener_pong(&mut listener_rx).await;
    wait_for_dialer_pong(&mut dialer_rx).await;

    // --- Pull the Wi-Fi: sever the listener's socket outright. Aborting just
    // the accept-loop task isn't enough — the already-accepted connection's
    // own task would keep answering pings, since it's a separate task. This
    // is a graceful QUIC close rather than the silent packet loss a real
    // Wi-Fi drop causes, but it exercises the same reconnect path; the
    // silent-timeout arithmetic itself is covered by
    // `penguinsync_protocol::connection`'s unit tests.
    linux_endpoint.close();
    drop(linux_endpoint);
    listener_handle.abort();
    let _ = listener_handle.await;

    // Android's keepalive should notice within a few intervals and the
    // dialer should report the drop.
    loop {
        match next(&mut dialer_rx).await {
            reconnect::DialerEvent::Session(SessionEvent::Closed(_)) => break,
            _ => continue,
        }
    }

    // --- Wi-Fi comes back: a fresh listener on the *same* address, already
    // trusting Android from the pin above — no new pairing needed.
    let (relisten_addr, mut listener_rx2, _linux_endpoint2, _listener_handle2) =
        spawn_listener(&linux_identity, linux_trust, linux_addr).await;
    assert_eq!(
        relisten_addr, linux_addr,
        "reconnect must target the same cached address"
    );

    // The dialer's own backoff loop retries on its own — untouched, per
    // docs/design.md §9's M0 acceptance test.
    let reconnected_id = wait_for_listener_handshake(&mut listener_rx2).await;
    assert_eq!(reconnected_id, android_identity.device_id);
    let reconnected_from_android = wait_for_dialer_handshake(&mut dialer_rx).await;
    assert_eq!(reconnected_from_android, linux_identity.device_id);

    wait_for_listener_pong(&mut listener_rx2).await;
    wait_for_dialer_pong(&mut dialer_rx).await;
}

#[tokio::test]
async fn unpinned_peer_with_no_pairing_window_never_completes_the_handshake() {
    let linux_identity = Identity::generate().unwrap();
    let android_identity = Identity::generate().unwrap();

    // Linux has no pins and no open pairing window: a stranger on the LAN.
    let linux_trust = Arc::new(TrustStore::new([]));
    let (linux_addr, mut listener_rx, _linux_endpoint, _listener_handle) =
        spawn_listener(&linux_identity, linux_trust, "127.0.0.1:0".parse().unwrap()).await;

    // Android also never pinned Linux (no QR was ever scanned).
    let android_trust = Arc::new(TrustStore::new([]));
    let android_endpoint = Arc::new(Endpoint::dialing(&android_identity, android_trust).unwrap());
    let (dialer_tx, mut dialer_rx) = mpsc::unbounded_channel();
    let android_local = local_identity(android_identity.device_id, "stranger");
    tokio::spawn(reconnect::run(
        android_endpoint,
        linux_addr,
        android_local,
        KEEPALIVE,
        None,
        dialer_tx,
    ));

    // The dialer should keep failing to connect (TLS rejects both
    // directions) rather than ever reporting a handshake.
    let mut saw_reconnecting = false;
    for _ in 0..3 {
        match timeout(Duration::from_secs(5), dialer_rx.recv()).await {
            Ok(Some(reconnect::DialerEvent::Reconnecting { .. })) => saw_reconnecting = true,
            Ok(Some(reconnect::DialerEvent::Session(SessionEvent::PeerHandshake { .. }))) => {
                panic!("handshake must never complete without a pin or pairing window")
            }
            _ => {}
        }
    }
    assert!(
        saw_reconnecting,
        "dialer should be retrying, not succeeding"
    );
    assert!(
        timeout(Duration::from_millis(100), listener_rx.recv())
            .await
            .is_err(),
        "listener must never see a completed session"
    );
}
