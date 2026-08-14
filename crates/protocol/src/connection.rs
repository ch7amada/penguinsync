//! Per-connection state machine: handshake exchange and ping/pong keepalive.
//!
//! Sans-I/O: this drives itself from [`Event`]s and returns [`Action`]s. It
//! never sees a socket. `net` owns the real QUIC connection, feeds it bytes
//! it decoded and clock ticks, and carries out the actions (docs/design.md
//! §4.2, §8).
//!
//! What this type does *not* decide: whether a peer's cryptographic identity
//! is trusted. That decision needs the TLS-authenticated SPKI hash of the
//! peer, which only `net` has — see [`Action::PeerHandshake`]'s doc comment.

use std::time::{Duration, Instant};

use crate::PROTOCOL_VERSION;
use crate::clipboard::Clip;
use crate::message::{Capability, DeviceId, Handshake, Message, Ping, Pong};
use crate::pairing::TokenBytes;

/// Identity this side presents in its own [`Handshake`].
#[derive(Debug, Clone)]
pub struct LocalIdentity {
    pub device_id: DeviceId,
    pub name: String,
    pub capabilities: Vec<Capability>,
}

/// Input to the machine.
#[derive(Debug, Clone)]
pub enum Event {
    /// The QUIC connection is up; drive the handshake.
    Started,
    /// A fully decoded message arrived from the peer.
    Received(Message),
    /// Clock tick — drive keepalive. Call this roughly once a second; the
    /// machine decides internally whether it's actually time to ping.
    Tick(Instant),
    /// The caller (daemon/ffi) wants this clip sent to the peer. Ignored
    /// before the handshake completes — there's no one to send it to yet.
    SendClipboard(Clip),
}

/// Output of the machine. `net` executes these; none of them touch a socket
/// here, they only describe what should happen to one.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Write this message to the control stream.
    Send(Message),
    /// The peer's handshake arrived and its version matched. `net` should now
    /// cross-check `device_id` against the connection's TLS-authenticated
    /// peer key (and, for a new pairing, redeem `pairing_token`) before
    /// treating this as a trusted device — this machine has no TLS context
    /// and cannot make that call itself.
    PeerHandshake {
        device_id: DeviceId,
        name: String,
        capabilities: Vec<Capability>,
        pairing_token: Option<TokenBytes>,
    },
    /// A round trip completed.
    Ponged { rtt: Duration },
    /// The peer sent a clipboard update. Broadcast, not targeted — `net`
    /// doesn't need to do anything with identity here, unlike
    /// `PeerHandshake` (docs/design.md §6.1).
    PeerClipboard(Clip),
    /// No `Pong` arrived within the keepalive timeout. `net` should treat the
    /// connection as dead and let the reconnect loop take over.
    KeepaliveTimedOut,
    /// The peer's protocol version doesn't match ours. `net` should close the
    /// connection with a clear error surfaced in the UI (docs/design.md §5.4)
    /// rather than attempt anything else.
    Reject(RejectReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    VersionMismatch { ours: u16, theirs: u16 },
}

/// How long to wait for a `Pong` before declaring the connection dead.
/// A few multiples of the keepalive interval, so one lost packet doesn't
/// trigger a reconnect.
const PONG_TIMEOUT_MULTIPLIER: u32 = 3;

#[derive(Debug, PartialEq)]
enum Phase {
    AwaitingPeerHandshake,
    Ready,
}

#[derive(Debug)]
pub struct ConnectionMachine {
    phase: Phase,
    local: LocalIdentity,
    outgoing_pairing_token: Option<TokenBytes>,
    keepalive_interval: Duration,
    last_ping_sent: Option<Instant>,
    pending_ping: Option<(u64, Instant)>,
    next_nonce: u64,
}

impl ConnectionMachine {
    /// `outgoing_pairing_token` is `Some` only for the dialing side's very
    /// first connection after scanning a QR — it proves possession of the
    /// single-use token. Every reconnect of an already-paired device passes
    /// `None`.
    pub fn new(
        local: LocalIdentity,
        keepalive_interval: Duration,
        outgoing_pairing_token: Option<TokenBytes>,
    ) -> Self {
        Self {
            phase: Phase::AwaitingPeerHandshake,
            local,
            outgoing_pairing_token,
            keepalive_interval,
            last_ping_sent: None,
            pending_ping: None,
            next_nonce: 0,
        }
    }

    pub fn handle(&mut self, event: Event, next_random_nonce: impl FnOnce() -> u64) -> Vec<Action> {
        match event {
            Event::Started => vec![Action::Send(Message::Handshake(Handshake {
                version: PROTOCOL_VERSION,
                device_id: self.local.device_id,
                name: self.local.name.clone(),
                capabilities: self.local.capabilities.clone(),
                pairing_token: self.outgoing_pairing_token,
            }))],
            Event::Received(msg) => self.on_received(msg),
            Event::Tick(now) => self.on_tick(now, next_random_nonce),
            Event::SendClipboard(clip) => {
                if self.phase == Phase::Ready {
                    vec![Action::Send(Message::Clipboard(clip))]
                } else {
                    vec![]
                }
            }
        }
    }

    fn on_received(&mut self, msg: Message) -> Vec<Action> {
        match (&self.phase, msg) {
            (Phase::AwaitingPeerHandshake, Message::Handshake(h)) => {
                if h.version != PROTOCOL_VERSION {
                    return vec![Action::Reject(RejectReason::VersionMismatch {
                        ours: PROTOCOL_VERSION,
                        theirs: h.version,
                    })];
                }
                self.phase = Phase::Ready;
                vec![Action::PeerHandshake {
                    device_id: h.device_id,
                    name: h.name,
                    capabilities: h.capabilities,
                    pairing_token: h.pairing_token,
                }]
            }
            (Phase::Ready, Message::Ping(Ping { nonce })) => {
                vec![Action::Send(Message::Pong(Pong { nonce }))]
            }
            (Phase::Ready, Message::Pong(Pong { nonce })) => {
                match self.pending_ping.take() {
                    Some((expected, sent_at)) if expected == nonce => {
                        vec![Action::Ponged {
                            rtt: sent_at.elapsed(),
                        }]
                    }
                    // Stale or unexpected nonce — leave any real pending ping
                    // in place rather than losing track of it.
                    other => {
                        self.pending_ping = other;
                        vec![]
                    }
                }
            }
            (Phase::Ready, Message::Clipboard(clip)) => vec![Action::PeerClipboard(clip)],
            // A handshake once Ready, or a ping/pong/clipboard before Ready,
            // is a protocol violation from a well-behaved peer. Ignore
            // rather than tear down the connection over a single stray
            // message.
            _ => vec![],
        }
    }

    fn on_tick(&mut self, now: Instant, next_random_nonce: impl FnOnce() -> u64) -> Vec<Action> {
        if self.phase != Phase::Ready {
            return vec![];
        }

        if let Some((_, sent_at)) = self.pending_ping {
            if now.duration_since(sent_at) >= self.keepalive_interval * PONG_TIMEOUT_MULTIPLIER {
                self.pending_ping = None;
                return vec![Action::KeepaliveTimedOut];
            }
            return vec![];
        }

        // The first tick after becoming Ready only establishes the baseline —
        // it does not fire an immediate ping. A handshake that just
        // completed is proof enough of life for one interval.
        let due = match self.last_ping_sent {
            None => {
                self.last_ping_sent = Some(now);
                false
            }
            Some(last) => now.duration_since(last) >= self.keepalive_interval,
        };
        if !due {
            return vec![];
        }

        let nonce = next_random_nonce();
        self.next_nonce = self.next_nonce.wrapping_add(1);
        self.last_ping_sent = Some(now);
        self.pending_ping = Some((nonce, now));
        vec![Action::Send(Message::Ping(Ping { nonce }))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(id: u8) -> LocalIdentity {
        LocalIdentity {
            device_id: [id; 32],
            name: format!("device-{id}"),
            capabilities: vec![],
        }
    }

    fn test_clip() -> Clip {
        Clip::new(crate::clipboard::MIME_TEXT_PLAIN, b"hello".to_vec()).unwrap()
    }

    fn peer_handshake(id: u8) -> Message {
        Message::Handshake(Handshake {
            version: PROTOCOL_VERSION,
            device_id: [id; 32],
            name: format!("device-{id}"),
            capabilities: vec![],
            pairing_token: None,
        })
    }

    #[test]
    fn started_sends_own_handshake() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        let actions = m.handle(Event::Started, || 0);
        assert_eq!(
            actions,
            vec![Action::Send(Message::Handshake(Handshake {
                version: PROTOCOL_VERSION,
                device_id: [1; 32],
                name: "device-1".into(),
                capabilities: vec![],
                pairing_token: None,
            }))]
        );
    }

    #[test]
    fn matching_handshake_becomes_ready() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        let actions = m.handle(Event::Received(peer_handshake(2)), || 0);
        assert_eq!(
            actions,
            vec![Action::PeerHandshake {
                device_id: [2; 32],
                name: "device-2".into(),
                capabilities: vec![],
                pairing_token: None,
            }]
        );
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        let bad = Message::Handshake(Handshake {
            version: PROTOCOL_VERSION + 1,
            device_id: [2; 32],
            name: "device-2".into(),
            capabilities: vec![],
            pairing_token: None,
        });
        assert_eq!(
            m.handle(Event::Received(bad), || 0),
            vec![Action::Reject(RejectReason::VersionMismatch {
                ours: PROTOCOL_VERSION,
                theirs: PROTOCOL_VERSION + 1,
            })]
        );
    }

    #[test]
    fn tick_pings_after_keepalive_interval_once_ready() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        m.handle(Event::Received(peer_handshake(2)), || 0);

        let t0 = Instant::now();
        assert!(m.handle(Event::Tick(t0), || 99).is_empty());

        let t1 = t0 + Duration::from_secs(20);
        assert_eq!(
            m.handle(Event::Tick(t1), || 99),
            vec![Action::Send(Message::Ping(Ping { nonce: 99 }))]
        );
    }

    #[test]
    fn ping_before_ready_is_ignored() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        assert!(m.handle(Event::Tick(Instant::now()), || 1).is_empty());
    }

    #[test]
    fn peer_ping_gets_echoed_as_pong() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        m.handle(Event::Received(peer_handshake(2)), || 0);
        let actions = m.handle(Event::Received(Message::Ping(Ping { nonce: 7 })), || 0);
        assert_eq!(
            actions,
            vec![Action::Send(Message::Pong(Pong { nonce: 7 }))]
        );
    }

    #[test]
    fn matching_pong_completes_round_trip() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        m.handle(Event::Received(peer_handshake(2)), || 0);
        let t0 = Instant::now();
        m.handle(Event::Tick(t0), || 5); // baseline
        m.handle(Event::Tick(t0 + Duration::from_secs(20)), || 5); // sends ping
        let actions = m.handle(Event::Received(Message::Pong(Pong { nonce: 5 })), || 0);
        assert!(matches!(actions.as_slice(), [Action::Ponged { .. }]));
    }

    #[test]
    fn mismatched_pong_nonce_is_ignored_and_ping_still_pending() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        m.handle(Event::Received(peer_handshake(2)), || 0);
        let t0 = Instant::now();
        m.handle(Event::Tick(t0), || 5); // baseline
        m.handle(Event::Tick(t0 + Duration::from_secs(20)), || 5); // sends ping
        let actions = m.handle(Event::Received(Message::Pong(Pong { nonce: 999 })), || 0);
        assert!(actions.is_empty());
        assert!(m.pending_ping.is_some());
    }

    #[test]
    fn missing_pong_times_out_after_three_intervals() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(10), None);
        m.handle(Event::Received(peer_handshake(2)), || 0);
        let t0 = Instant::now();
        m.handle(Event::Tick(t0), || 1); // baseline
        m.handle(Event::Tick(t0 + Duration::from_secs(10)), || 1); // sends ping

        assert!(
            m.handle(Event::Tick(t0 + Duration::from_secs(30)), || 1)
                .is_empty()
        );
        assert_eq!(
            m.handle(Event::Tick(t0 + Duration::from_secs(40)), || 1),
            vec![Action::KeepaliveTimedOut]
        );
    }

    #[test]
    fn send_clipboard_before_ready_is_dropped() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        assert!(m.handle(Event::SendClipboard(test_clip()), || 0).is_empty());
    }

    #[test]
    fn send_clipboard_once_ready_is_sent() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        m.handle(Event::Received(peer_handshake(2)), || 0);
        let clip = test_clip();
        assert_eq!(
            m.handle(Event::SendClipboard(clip.clone()), || 0),
            vec![Action::Send(Message::Clipboard(clip))]
        );
    }

    #[test]
    fn received_clipboard_once_ready_is_surfaced() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        m.handle(Event::Received(peer_handshake(2)), || 0);
        let clip = test_clip();
        assert_eq!(
            m.handle(Event::Received(Message::Clipboard(clip.clone())), || 0),
            vec![Action::PeerClipboard(clip)]
        );
    }

    #[test]
    fn received_clipboard_before_ready_is_ignored() {
        let mut m = ConnectionMachine::new(identity(1), Duration::from_secs(20), None);
        assert!(
            m.handle(Event::Received(Message::Clipboard(test_clip())), || 0)
                .is_empty()
        );
    }
}
