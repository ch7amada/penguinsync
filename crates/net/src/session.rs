//! Drives a real QUIC connection's control stream with
//! [`penguinsync_protocol::ConnectionMachine`].
//!
//! This is the seam between the sans-I/O protocol core and the socket: it
//! owns the control stream, decodes bytes into [`Message`]s, feeds them to
//! the machine, and carries out whatever [`Action`]s come back. Nothing about
//! pairing/clipboard/file semantics lives here — just handshake and
//! keepalive, which is all M0 needs (docs/design.md §9).

use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use penguinsync_protocol::pairing::TokenBytes;
use penguinsync_protocol::{
    Action, Capability, ConnectionMachine, DeviceId, Event, LocalIdentity, RejectReason,
};

use crate::framing::{FramingError, read_message, write_message};

/// How often the driving task ticks the machine's keepalive clock. Not the
/// keepalive interval itself (docs/protocol.md §4 — that's ~20 s); this is
/// just the granularity at which the machine gets a chance to notice it's
/// due.
const TICK_PERIOD: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// The peer's handshake arrived and versions matched. See
    /// [`penguinsync_protocol::Action::PeerHandshake`] for what the caller
    /// still needs to check before trusting this.
    PeerHandshake {
        device_id: DeviceId,
        name: String,
        capabilities: Vec<Capability>,
        pairing_token: Option<TokenBytes>,
    },
    Ponged {
        rtt: Duration,
    },
    Closed(CloseReason),
}

#[derive(Debug, Clone)]
pub enum CloseReason {
    VersionMismatch { ours: u16, theirs: u16 },
    KeepaliveTimedOut,
    ConnectionLost(String),
    FramingError(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("opening control stream: {0}")]
    Open(#[from] quinn::ConnectionError),
    #[error("control stream write failed: {0}")]
    Write(#[from] quinn::WriteError),
}

/// A running control-stream session. Drop it (or call [`Session::close`]) to
/// tear down the connection; the driving task exits when the connection does.
pub struct Session {
    connection: quinn::Connection,
    events: mpsc::UnboundedReceiver<SessionEvent>,
}

impl Session {
    /// Dialer side (Android always dials — docs/design.md §5.3): opens the
    /// control stream and drives it.
    pub async fn open(
        connection: quinn::Connection,
        local: LocalIdentity,
        keepalive_interval: Duration,
        pairing_token: Option<TokenBytes>,
    ) -> Result<Self, SessionError> {
        let (send, recv) = connection.open_bi().await?;
        Ok(Self::spawn(
            connection,
            local,
            keepalive_interval,
            pairing_token,
            send,
            recv,
        ))
    }

    /// Listener side (Linux): accepts the peer-opened control stream and
    /// drives it.
    pub async fn accept(
        connection: quinn::Connection,
        local: LocalIdentity,
        keepalive_interval: Duration,
    ) -> Result<Self, SessionError> {
        let (send, recv) = connection.accept_bi().await?;
        Ok(Self::spawn(
            connection,
            local,
            keepalive_interval,
            None,
            send,
            recv,
        ))
    }

    fn spawn(
        connection: quinn::Connection,
        local: LocalIdentity,
        keepalive_interval: Duration,
        pairing_token: Option<TokenBytes>,
        send: quinn::SendStream,
        recv: quinn::RecvStream,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let conn_for_task = connection.clone();
        tokio::spawn(drive(
            conn_for_task,
            local,
            keepalive_interval,
            pairing_token,
            send,
            recv,
            tx,
        ));
        Self {
            connection,
            events: rx,
        }
    }

    /// Wait for the next event. `None` once the driving task has exited —
    /// always preceded by a [`SessionEvent::Closed`].
    pub async fn next_event(&mut self) -> Option<SessionEvent> {
        self.events.recv().await
    }

    pub fn remote_addr(&self) -> std::net::SocketAddr {
        self.connection.remote_address()
    }

    pub fn close(&self) {
        self.connection.close(0u32.into(), b"");
    }

    /// Drain every event until the session closes, calling `on_event` for
    /// each one (the terminal [`SessionEvent::Closed`] included). Returns the
    /// reason it closed, for a caller that wants it apart from the callback.
    pub async fn drain(mut self, on_event: impl Fn(SessionEvent)) -> Option<CloseReason> {
        while let Some(event) = self.next_event().await {
            let reason = match &event {
                SessionEvent::Closed(r) => Some(r.clone()),
                _ => None,
            };
            on_event(event);
            if reason.is_some() {
                return reason;
            }
        }
        None
    }
}

async fn drive(
    connection: quinn::Connection,
    local: LocalIdentity,
    keepalive_interval: Duration,
    pairing_token: Option<TokenBytes>,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    tx: mpsc::UnboundedSender<SessionEvent>,
) {
    let mut machine = ConnectionMachine::new(local, keepalive_interval, pairing_token);

    if !run_actions(machine.handle(Event::Started, rand::random), &mut send, &tx).await {
        return;
    }

    let mut ticker = tokio::time::interval(TICK_PERIOD);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            read = read_message(&mut recv) => {
                match read {
                    Ok(msg) => {
                        if !run_actions(machine.handle(Event::Received(msg), rand::random), &mut send, &tx).await {
                            return;
                        }
                    }
                    Err(FramingError::Read(e)) => {
                        let _ = tx.send(SessionEvent::Closed(CloseReason::ConnectionLost(e.to_string())));
                        return;
                    }
                    Err(e) => {
                        let _ = tx.send(SessionEvent::Closed(CloseReason::FramingError(e.to_string())));
                        connection.close(1u32.into(), b"framing error");
                        return;
                    }
                }
            }
            _ = ticker.tick() => {
                if !run_actions(machine.handle(Event::Tick(Instant::now()), rand::random), &mut send, &tx).await {
                    return;
                }
            }
            reason = connection.closed() => {
                let _ = tx.send(SessionEvent::Closed(CloseReason::ConnectionLost(reason.to_string())));
                return;
            }
        }
    }
}

/// Execute a batch of actions. Returns `false` if the session ended (a
/// terminal event was sent and the driving task should exit).
async fn run_actions(
    actions: Vec<Action>,
    send: &mut quinn::SendStream,
    tx: &mpsc::UnboundedSender<SessionEvent>,
) -> bool {
    for action in actions {
        match action {
            Action::Send(msg) => {
                if let Err(e) = write_message(send, &msg).await {
                    let _ = tx.send(SessionEvent::Closed(CloseReason::FramingError(
                        e.to_string(),
                    )));
                    return false;
                }
            }
            Action::PeerHandshake {
                device_id,
                name,
                capabilities,
                pairing_token,
            } => {
                let _ = tx.send(SessionEvent::PeerHandshake {
                    device_id,
                    name,
                    capabilities,
                    pairing_token,
                });
            }
            Action::Ponged { rtt } => {
                let _ = tx.send(SessionEvent::Ponged { rtt });
            }
            Action::KeepaliveTimedOut => {
                let _ = tx.send(SessionEvent::Closed(CloseReason::KeepaliveTimedOut));
                return false;
            }
            Action::Reject(RejectReason::VersionMismatch { ours, theirs }) => {
                let _ = tx.send(SessionEvent::Closed(CloseReason::VersionMismatch {
                    ours,
                    theirs,
                }));
                return false;
            }
        }
    }
    true
}
