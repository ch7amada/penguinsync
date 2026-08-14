//! Drives a real QUIC connection's control stream with
//! [`penguinsync_protocol::ConnectionMachine`].
//!
//! This is the seam between the sans-I/O protocol core and the socket: it
//! owns the control stream, decodes bytes into [`Message`]s, feeds them to
//! the machine, and carries out whatever [`Action`]s come back. Handshake
//! and keepalive drive themselves; clipboard is the one thing a caller
//! pushes in from outside, via [`Session::send_clipboard`].

use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use penguinsync_protocol::pairing::TokenBytes;
use penguinsync_protocol::{
    Action, Capability, Clip, ConnectionMachine, DeviceId, Event, LocalIdentity, RejectReason,
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
    /// The peer sent a clipboard update — broadcast, not targeted
    /// (docs/design.md §6.1).
    ClipboardReceived(Clip),
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

/// Something a caller wants this session to do, pushed in from outside the
/// driving task.
enum SessionCommand {
    SendClipboard(Clip),
}

/// A cheap, cloneable capability to talk to a running session from outside
/// whatever is draining its events — the daemon's clipboard orchestrator, or
/// the FFI core, holding on to it after the connection was reported so it
/// can push a clipboard update whenever the system clipboard changes, not
/// just at the moment the session was accepted.
#[derive(Clone)]
pub struct SessionHandle {
    connection: quinn::Connection,
    commands: mpsc::UnboundedSender<SessionCommand>,
}

impl SessionHandle {
    pub fn send_clipboard(&self, clip: Clip) {
        let _ = self.commands.send(SessionCommand::SendClipboard(clip));
    }

    pub fn remote_addr(&self) -> std::net::SocketAddr {
        self.connection.remote_address()
    }

    pub fn close(&self) {
        self.connection.close(0u32.into(), b"");
    }
}

/// A running control-stream session. Drop it (or call [`Session::close`]) to
/// tear down the connection; the driving task exits when the connection does.
pub struct Session {
    connection: quinn::Connection,
    events: mpsc::UnboundedReceiver<SessionEvent>,
    commands: mpsc::UnboundedSender<SessionCommand>,
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
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let conn_for_task = connection.clone();
        let machine = ConnectionMachine::new(local, keepalive_interval, pairing_token);
        tokio::spawn(drive(
            conn_for_task,
            machine,
            send,
            recv,
            events_tx,
            commands_rx,
        ));
        Self {
            connection,
            events: events_rx,
            commands: commands_tx,
        }
    }

    /// Wait for the next event. `None` once the driving task has exited —
    /// always preceded by a [`SessionEvent::Closed`].
    pub async fn next_event(&mut self) -> Option<SessionEvent> {
        self.events.recv().await
    }

    /// Queue a clipboard update to send on this session's control stream.
    /// Fire-and-forget: if the session has already closed, this is a no-op
    /// — there's no one to tell.
    pub fn send_clipboard(&self, clip: Clip) {
        let _ = self.commands.send(SessionCommand::SendClipboard(clip));
    }

    /// A cheap, cloneable handle a caller can hold on to after this
    /// `Session` is consumed by [`Session::drain`], to keep pushing
    /// clipboard updates for as long as the session stays connected.
    pub fn handle(&self) -> SessionHandle {
        SessionHandle {
            connection: self.connection.clone(),
            commands: self.commands.clone(),
        }
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
    mut machine: ConnectionMachine,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    tx: mpsc::UnboundedSender<SessionEvent>,
    mut commands: mpsc::UnboundedReceiver<SessionCommand>,
) {
    if !run_actions(machine.handle(Event::Started, rand::random), &mut send, &tx).await {
        return;
    }

    let mut ticker = tokio::time::interval(TICK_PERIOD);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Once the last `Session` handle is dropped, `commands` closes and
    // `.recv()` starts returning `None` on every poll. Stop polling it
    // rather than let that branch busy-spin for however long the
    // connection itself takes to wind down.
    let mut commands_open = true;

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
            cmd = commands.recv(), if commands_open => {
                match cmd {
                    Some(SessionCommand::SendClipboard(clip)) => {
                        if !run_actions(machine.handle(Event::SendClipboard(clip), rand::random), &mut send, &tx).await {
                            return;
                        }
                    }
                    None => commands_open = false,
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
            Action::PeerClipboard(clip) => {
                let _ = tx.send(SessionEvent::ClipboardReceived(clip));
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
