//! Proves `PenguinSyncCore` — the actual object UniFFI exposes to Kotlin —
//! pairs and exchanges ping/pong over real loopback QUIC, against a bare
//! `net::listener` standing in for the Linux daemon. `crates/net/tests/
//! loopback.rs` already covers the transport in depth; this test's job is
//! the FFI object itself: constructor, QR parsing, event forwarding through
//! `Arc<dyn CoreEventListener>`, and `ConnectionHandle::cancel`
//! (docs/design.md §4.2, §9).

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use penguinsync::{CoreEvent, CoreEventListener, PenguinSyncCore};
use penguinsync_net::identity::Identity;
use penguinsync_net::tls::TrustStore;
use penguinsync_net::{endpoint::Endpoint, listener};
use penguinsync_protocol::pairing::{QrPayload, encode_qr_uri};
use penguinsync_protocol::{LocalIdentity, PROTOCOL_VERSION};

struct ChannelListener(std_mpsc::Sender<CoreEvent>);

impl CoreEventListener for ChannelListener {
    fn on_event(&self, event: CoreEvent) {
        let _ = self.0.send(event);
    }
}

fn tempdir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "penguinsync-ffi-test-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn recv_matching(
    rx: &std_mpsc::Receiver<CoreEvent>,
    mut predicate: impl FnMut(&CoreEvent) -> bool,
) -> CoreEvent {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let event = rx
            .recv_timeout(remaining)
            .expect("timed out waiting for expected event");
        if predicate(&event) {
            return event;
        }
    }
}

/// Stands in for `penguinsyncd`: a bare listener on loopback with a pairing
/// window open, using its own dedicated runtime/thread so it keeps running
/// independently of the FFI core's internal runtime under test.
fn spawn_fake_linux_daemon() -> (SocketAddr, penguinsync_protocol::DeviceId, Arc<TrustStore>) {
    let identity = Identity::generate().unwrap();
    let device_id = identity.device_id;
    let trust = Arc::new(TrustStore::new([]));
    trust.open_pairing_window(std::time::Instant::now() + Duration::from_secs(60));

    // quinn's `Endpoint` needs an active tokio runtime context to bind its
    // socket, so construction happens inside the thread's `block_on`, not
    // before it.
    let (ready_tx, ready_rx) = std_mpsc::channel();
    let trust_for_thread = trust.clone();
    std::thread::spawn(move || {
        let trust = trust_for_thread;
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let endpoint = Arc::new(
                Endpoint::listening(&identity, trust.clone(), "127.0.0.1:0".parse().unwrap())
                    .unwrap(),
            );
            let addr = endpoint.local_addr().unwrap();
            ready_tx.send(addr).unwrap();

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(listener::run(
                endpoint,
                LocalIdentity {
                    device_id,
                    name: "desk-fedora".into(),
                    capabilities: vec![],
                },
                Duration::from_millis(500),
                tx,
            ));
            // Pin whoever connects with a valid pairing window — the token
            // itself isn't re-validated here; that's the daemon's job
            // (`crates/daemon/src/orchestrator.rs`), already covered there.
            while let Some(ev) = rx.recv().await {
                if let listener::ListenerEvent {
                    event: penguinsync_net::session::SessionEvent::PeerHandshake { device_id, .. },
                    ..
                } = ev
                {
                    trust.pin(device_id);
                }
            }
        });
    });

    let addr = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("listener bound in time");
    (addr, device_id, trust)
}

#[test]
fn core_pairs_over_real_quic_and_receives_pongs() {
    let (linux_addr, linux_device_id, _linux_trust) = spawn_fake_linux_daemon();

    let core = PenguinSyncCore::new(
        tempdir("core").to_string_lossy().into_owned(),
        "pixel".to_string(),
    )
    .expect("core constructs");

    let qr = encode_qr_uri(&QrPayload {
        version: PROTOCOL_VERSION,
        device_id: linux_device_id,
        name: "desk-fedora".into(),
        addrs: vec![linux_addr],
        token: [0xCDu8; 16],
    });

    let (tx, rx) = std_mpsc::channel();
    let handle = core
        .pair(qr, Arc::new(ChannelListener(tx)))
        .expect("pair() accepts a well-formed QR");

    let event = recv_matching(&rx, |e| matches!(e, CoreEvent::PeerHandshake { .. }));
    match event {
        CoreEvent::PeerHandshake { device_id, name } => {
            assert_eq!(
                device_id,
                penguinsync_protocol::message::to_hex(&linux_device_id)
            );
            assert_eq!(name, "desk-fedora");
        }
        _ => unreachable!(),
    }

    recv_matching(&rx, |e| matches!(e, CoreEvent::Ponged { .. }));

    // The peer is now persisted — a fresh core reading the same data_dir
    // would restore the pin without a new QR scan.
    handle.cancel();
}

#[test]
fn pair_rejects_a_qr_with_no_candidate_address() {
    let core = PenguinSyncCore::new(
        tempdir("no-addr").to_string_lossy().into_owned(),
        "pixel".to_string(),
    )
    .unwrap();
    let qr = encode_qr_uri(&QrPayload {
        version: PROTOCOL_VERSION,
        device_id: [1u8; 32],
        name: "desk".into(),
        addrs: vec![],
        token: [0u8; 16],
    });
    let (tx, _rx) = std_mpsc::channel();
    assert!(core.pair(qr, Arc::new(ChannelListener(tx))).is_err());
}

#[test]
fn pair_rejects_a_protocol_version_mismatch() {
    let core = PenguinSyncCore::new(
        tempdir("bad-version").to_string_lossy().into_owned(),
        "pixel".to_string(),
    )
    .unwrap();
    let qr = encode_qr_uri(&QrPayload {
        version: PROTOCOL_VERSION + 1,
        device_id: [1u8; 32],
        name: "desk".into(),
        addrs: vec!["127.0.0.1:1".parse().unwrap()],
        token: [0u8; 16],
    });
    let (tx, _rx) = std_mpsc::channel();
    assert!(core.pair(qr, Arc::new(ChannelListener(tx))).is_err());
}
