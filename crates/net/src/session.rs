//! Drives a real QUIC connection's control stream with
//! [`penguinsync_protocol::ConnectionMachine`].
//!
//! This is the seam between the sans-I/O protocol core and the socket: it
//! owns the control stream, decodes bytes into [`Message`]s, feeds them to
//! the machine, and carries out whatever [`Action`]s come back. Handshake
//! and keepalive drive themselves; clipboard and file transfer are things a
//! caller pushes in from outside, via [`Session::send_clipboard`] and
//! [`SessionHandle::send_file`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};

use penguinsync_protocol::pairing::TokenBytes;
use penguinsync_protocol::{
    Action, Capability, Clip, ConnectionMachine, DeviceId, Event, LocalIdentity, RejectReason,
    TransferMeta,
};

use crate::framing::{FramingError, read_message, write_message};
use crate::transfer::{TransferError, TransferSink};

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
    /// This side just started sending a file — fired from
    /// [`SessionHandle::send_file`], not from the control-stream drive loop,
    /// since it's this side's own action, not something the peer told us.
    TransferStarted {
        transfer_id: u64,
        name: String,
        size: u64,
    },
    /// The peer announced a file it's about to send us, and the matching
    /// payload stream has been correlated with it (docs/protocol.md §6.4 —
    /// the two arrive as independent streams, so this fires once both
    /// halves are known, not the instant the control message arrives).
    TransferOffered {
        transfer_id: u64,
        name: String,
        size: u64,
    },
    /// Cumulative bytes moved so far, for either direction — the caller
    /// already knows (from `TransferStarted` vs. `TransferOffered`) which
    /// `transfer_id`s are sends and which are receives.
    TransferProgress {
        transfer_id: u64,
        bytes: u64,
        total: u64,
    },
    /// This side finished receiving a file — success or failure. On success
    /// `path` is where it landed (docs/design.md §6.2: never overwrites,
    /// `(1)`-suffixed on collision); on failure the partial file was
    /// discarded and `error` says why.
    TransferReceived {
        transfer_id: u64,
        name: String,
        path: Option<PathBuf>,
        ok: bool,
        error: Option<String>,
    },
    /// The peer's ack for a file *we* sent — the receiving side's
    /// `TransferReceived` outcome, echoed back over the control stream.
    TransferAcked {
        transfer_id: u64,
        ok: bool,
        error: Option<String>,
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

/// Something a caller wants this session to do, pushed in from outside the
/// driving task.
enum SessionCommand {
    Clipboard(Clip),
    /// Announce a file on the control stream. The payload stream itself is
    /// opened directly on the connection by [`SessionHandle::send_file`],
    /// not funneled through here — only the single-writer control stream
    /// needs serializing through the machine.
    TransferOffer(TransferMeta),
    /// The result of a transfer this side just finished receiving, to ack
    /// back to the sender.
    TransferComplete {
        transfer_id: u64,
        ok: bool,
        error: Option<String>,
    },
}

/// Matches a `TransferOffer` (arriving on the control stream) with its
/// payload's unidirectional stream (arriving independently) by
/// `transfer_id`, whichever gets there first (docs/protocol.md §6.4).
#[derive(Default)]
struct TransferCoordinator {
    slots: StdMutex<HashMap<u64, Slot>>,
}

enum Slot {
    Meta(TransferMeta),
    Waiting(oneshot::Sender<TransferMeta>),
}

impl TransferCoordinator {
    fn announce(&self, meta: TransferMeta) {
        let mut slots = self.slots.lock().expect("transfer coordinator poisoned");
        match slots.remove(&meta.transfer_id) {
            Some(Slot::Waiting(tx)) => {
                let _ = tx.send(meta);
            }
            _ => {
                slots.insert(meta.transfer_id, Slot::Meta(meta));
            }
        }
    }

    /// Waits for `announce(meta)` with a matching `transfer_id`, however
    /// long that takes — the control stream is reliable and ordered, so a
    /// well-behaved peer's offer always arrives eventually.
    async fn await_meta(&self, transfer_id: u64) -> TransferMeta {
        let existing = {
            let mut slots = self.slots.lock().expect("transfer coordinator poisoned");
            match slots.remove(&transfer_id) {
                Some(Slot::Meta(meta)) => Some(meta),
                other => {
                    if let Some(slot) = other {
                        slots.insert(transfer_id, slot);
                    }
                    None
                }
            }
        };
        if let Some(meta) = existing {
            return meta;
        }
        let (tx, rx) = oneshot::channel();
        {
            let mut slots = self.slots.lock().expect("transfer coordinator poisoned");
            slots.insert(transfer_id, Slot::Waiting(tx));
        }
        rx.await
            .expect("announce() always fulfills a waiting slot before the session ends")
    }
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
    events: mpsc::UnboundedSender<SessionEvent>,
}

impl SessionHandle {
    pub fn send_clipboard(&self, clip: Clip) {
        let _ = self.commands.send(SessionCommand::Clipboard(clip));
    }

    /// Sends `path` to the peer: hashes and stats it, announces it on the
    /// control stream, then streams the content on its own fresh
    /// unidirectional stream (docs/protocol.md §6.4). Runs on a detached
    /// task — progress and the outcome arrive as [`SessionEvent`]s
    /// (`TransferStarted`/`TransferProgress` from here directly, the final
    /// `TransferAcked` once the peer's ack comes back over the control
    /// stream), not through the returned handle; it exists so a caller that
    /// cares about a hard failure (couldn't even open the file) can still
    /// observe one.
    pub fn send_file(&self, path: PathBuf) -> tokio::task::JoinHandle<Result<(), TransferError>> {
        let connection = self.connection.clone();
        let commands = self.commands.clone();
        let events = self.events.clone();
        tokio::spawn(async move {
            let transfer_id: u64 = rand::random();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();
            let stat = crate::transfer::hash_and_stat(&path).await?;

            let _ = events.send(SessionEvent::TransferStarted {
                transfer_id,
                name: name.clone(),
                size: stat.size,
            });
            let _ = commands.send(SessionCommand::TransferOffer(TransferMeta {
                transfer_id,
                name,
                size: stat.size,
                offset: 0,
                hash: stat.hash,
            }));

            let mut send = connection.open_uni().await?;
            crate::transfer::write_header(&mut send, transfer_id, 0).await?;
            let total = stat.size;
            crate::transfer::send_file_stream(&path, &mut send, |bytes| {
                let _ = events.send(SessionEvent::TransferProgress {
                    transfer_id,
                    bytes,
                    total,
                });
            })
            .await?;
            send.finish()?;
            Ok(())
        })
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
    events_tx: mpsc::UnboundedSender<SessionEvent>,
    commands: mpsc::UnboundedSender<SessionCommand>,
}

impl Session {
    /// Dialer side (Android always dials — docs/design.md §5.3): opens the
    /// control stream and drives it. `sink` receives whatever files the peer
    /// sends us (docs/protocol.md §6.4).
    pub async fn open(
        connection: quinn::Connection,
        local: LocalIdentity,
        keepalive_interval: Duration,
        pairing_token: Option<TokenBytes>,
        sink: Arc<dyn TransferSink>,
    ) -> Result<Self, SessionError> {
        let (send, recv) = connection.open_bi().await?;
        Ok(Self::spawn(
            connection,
            local,
            keepalive_interval,
            pairing_token,
            send,
            recv,
            sink,
        ))
    }

    /// Listener side (Linux): accepts the peer-opened control stream and
    /// drives it. `sink` receives whatever files the peer sends us
    /// (docs/protocol.md §6.4).
    pub async fn accept(
        connection: quinn::Connection,
        local: LocalIdentity,
        keepalive_interval: Duration,
        sink: Arc<dyn TransferSink>,
    ) -> Result<Self, SessionError> {
        let (send, recv) = connection.accept_bi().await?;
        Ok(Self::spawn(
            connection,
            local,
            keepalive_interval,
            None,
            send,
            recv,
            sink,
        ))
    }

    fn spawn(
        connection: quinn::Connection,
        local: LocalIdentity,
        keepalive_interval: Duration,
        pairing_token: Option<TokenBytes>,
        send: quinn::SendStream,
        recv: quinn::RecvStream,
        sink: Arc<dyn TransferSink>,
    ) -> Self {
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let coordinator = Arc::new(TransferCoordinator::default());
        let conn_for_task = connection.clone();
        let machine = ConnectionMachine::new(local, keepalive_interval, pairing_token);
        tokio::spawn(drive(
            conn_for_task,
            machine,
            send,
            recv,
            events_tx.clone(),
            commands_rx,
            coordinator.clone(),
        ));
        tokio::spawn(accept_transfers(
            connection.clone(),
            sink,
            coordinator,
            events_tx.clone(),
            commands_tx.clone(),
        ));
        Self {
            connection,
            events: events_rx,
            events_tx,
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
        let _ = self.commands.send(SessionCommand::Clipboard(clip));
    }

    /// See [`SessionHandle::send_file`] — same behaviour, callable before
    /// [`Session::drain`] consumes this into a handle.
    pub fn send_file(&self, path: PathBuf) -> tokio::task::JoinHandle<Result<(), TransferError>> {
        self.handle().send_file(path)
    }

    /// A cheap, cloneable handle a caller can hold on to after this
    /// `Session` is consumed by [`Session::drain`], to keep pushing
    /// clipboard updates for as long as the session stays connected.
    pub fn handle(&self) -> SessionHandle {
        SessionHandle {
            connection: self.connection.clone(),
            commands: self.commands.clone(),
            events: self.events_tx.clone(),
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
    coordinator: Arc<TransferCoordinator>,
) {
    if !run_actions(
        machine.handle(Event::Started, rand::random),
        &mut send,
        &tx,
        &coordinator,
    )
    .await
    {
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
                        if !run_actions(machine.handle(Event::Received(msg), rand::random), &mut send, &tx, &coordinator).await {
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
                if !run_actions(machine.handle(Event::Tick(Instant::now()), rand::random), &mut send, &tx, &coordinator).await {
                    return;
                }
            }
            cmd = commands.recv(), if commands_open => {
                match cmd {
                    Some(SessionCommand::Clipboard(clip)) => {
                        if !run_actions(machine.handle(Event::SendClipboard(clip), rand::random), &mut send, &tx, &coordinator).await {
                            return;
                        }
                    }
                    Some(SessionCommand::TransferOffer(meta)) => {
                        if !run_actions(machine.handle(Event::SendTransferOffer(meta), rand::random), &mut send, &tx, &coordinator).await {
                            return;
                        }
                    }
                    Some(SessionCommand::TransferComplete { transfer_id, ok, error }) => {
                        if !run_actions(machine.handle(Event::SendTransferComplete { transfer_id, ok, error }, rand::random), &mut send, &tx, &coordinator).await {
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
    coordinator: &Arc<TransferCoordinator>,
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
            Action::PeerTransferOffer(meta) => {
                // No event here — `accept_transfers` emits `TransferOffered`
                // once it has correlated this metadata with the matching
                // payload stream, so a caller only ever hears about a
                // transfer it can actually already read.
                coordinator.announce(meta);
            }
            Action::PeerTransferComplete {
                transfer_id,
                ok,
                error,
            } => {
                let _ = tx.send(SessionEvent::TransferAcked {
                    transfer_id,
                    ok,
                    error,
                });
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

/// Accepts every unidirectional stream the peer opens — one per file
/// transfer (docs/protocol.md §6.4) — for as long as the connection lasts,
/// spawning a task per stream so one slow transfer never blocks the next
/// from being accepted. Runs alongside [`drive`], not inside it: the control
/// stream and payload streams are independent QUIC streams, and mixing them
/// into one `select!` would mean a big file's reads starving keepalive
/// ticks.
async fn accept_transfers(
    connection: quinn::Connection,
    sink: Arc<dyn TransferSink>,
    coordinator: Arc<TransferCoordinator>,
    tx: mpsc::UnboundedSender<SessionEvent>,
    commands: mpsc::UnboundedSender<SessionCommand>,
) {
    loop {
        let mut recv = match connection.accept_uni().await {
            Ok(recv) => recv,
            // Connection closed or errored — `drive`'s own `connection.closed()`
            // branch reports this; nothing more to do here.
            Err(_) => return,
        };
        let sink = sink.clone();
        let coordinator = coordinator.clone();
        let tx = tx.clone();
        let commands = commands.clone();
        tokio::spawn(async move {
            let (transfer_id, _offset) = match crate::transfer::read_header(&mut recv).await {
                Ok(header) => header,
                Err(e) => {
                    tracing::debug!(error = %e, "malformed transfer stream header; dropping");
                    return;
                }
            };
            let meta = coordinator.await_meta(transfer_id).await;
            let _ = tx.send(SessionEvent::TransferOffered {
                transfer_id,
                name: meta.name.clone(),
                size: meta.size,
            });

            let progress_tx = tx.clone();
            let total = meta.size;
            let on_progress: Box<dyn Fn(u64) + Send + Sync> = Box::new(move |bytes| {
                let _ = progress_tx.send(SessionEvent::TransferProgress {
                    transfer_id,
                    bytes,
                    total,
                });
            });

            let result = sink
                .receive(&meta.name, meta.size, meta.hash, recv, on_progress)
                .await;
            let (ok, error, path) = match result {
                Ok(path) => (true, None, Some(path)),
                Err(e) => (false, Some(e.to_string()), None),
            };
            let _ = tx.send(SessionEvent::TransferReceived {
                transfer_id,
                name: meta.name,
                path,
                ok,
                error: error.clone(),
            });
            let _ = commands.send(SessionCommand::TransferComplete {
                transfer_id,
                ok,
                error,
            });
        });
    }
}
