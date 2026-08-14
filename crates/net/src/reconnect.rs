//! The dialer's reconnect loop.
//!
//! Android always dials (docs/design.md §5.3): it's the side that changes
//! networks, sleeps, and gets new addresses, so every reconnect is driven
//! from here. Reconnection is unicast to a cached address, never mDNS
//! (docs/protocol.md §2) — that address is simply whatever the caller passes
//! in, which is `net`'s entire contract with the discovery layer above it.
//!
//! This is the piece M0 exists to prove: pull the Wi-Fi, walk away, come
//! back, and this loop reconnects untouched (docs/design.md §9).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use penguinsync_protocol::pairing::TokenBytes;
use penguinsync_protocol::{LocalIdentity, backoff};

use crate::endpoint::Endpoint;
use crate::session::{Session, SessionEvent, SessionHandle};

pub enum DialerEvent {
    /// A connection came up. `handle` is a cheap, cloneable capability
    /// (e.g. to push a clipboard update) that stays valid for as long as
    /// this connection lasts — one `Connected` per successful (re)connect.
    Connected(SessionHandle),
    /// An event from the current session — including its terminal `Closed`,
    /// right before this loop starts backing off to retry.
    Session(SessionEvent),
    /// The previous attempt ended (or this is the very first one); retrying
    /// after `delay`.
    Reconnecting { attempt: u32, delay: Duration },
}

/// Runs until `events`'s receiver is dropped. Owns the session for its
/// entire lifetime — connects, drains its events, and only *then* backs off
/// and retries, so there is never more than one live connection in flight.
///
/// `pairing_token` is consumed on the first successful connection only —
/// every reconnect after that presents no token, matching a device that's
/// already paired (docs/protocol.md §6.1).
pub async fn run(
    endpoint: Arc<Endpoint>,
    addr: SocketAddr,
    local: LocalIdentity,
    keepalive_interval: Duration,
    mut pairing_token: Option<TokenBytes>,
    events: mpsc::UnboundedSender<DialerEvent>,
) {
    let mut attempt: u32 = 0;
    loop {
        match connect_once(
            &endpoint,
            addr,
            local.clone(),
            keepalive_interval,
            pairing_token.take(),
        )
        .await
        {
            Ok(session) => {
                attempt = 0;
                if events
                    .send(DialerEvent::Connected(session.handle()))
                    .is_err()
                {
                    session.close();
                    return;
                }
                let tx = events.clone();
                session
                    .drain(move |event| {
                        let _ = tx.send(DialerEvent::Session(event));
                    })
                    .await;
            }
            Err(e) => {
                tracing::debug!(error = %e, "connect attempt failed");
            }
        }

        if events.is_closed() {
            return;
        }
        let delay = backoff::delay(attempt);
        if events
            .send(DialerEvent::Reconnecting { attempt, delay })
            .is_err()
        {
            return;
        }
        attempt = attempt.saturating_add(1);
        tokio::time::sleep(delay).await;
    }
}

#[derive(Debug, thiserror::Error)]
enum ConnectOnceError {
    #[error("QUIC connect failed: {0}")]
    Connect(#[from] quinn::ConnectError),
    #[error("QUIC handshake failed: {0}")]
    Connection(#[from] quinn::ConnectionError),
    #[error("opening control stream: {0}")]
    Session(#[from] crate::session::SessionError),
}

async fn connect_once(
    endpoint: &Endpoint,
    addr: SocketAddr,
    local: LocalIdentity,
    keepalive_interval: Duration,
    pairing_token: Option<TokenBytes>,
) -> Result<Session, ConnectOnceError> {
    let connecting = endpoint.connect(addr)?;
    let connection = connecting.await?;
    let session = Session::open(connection, local, keepalive_interval, pairing_token).await?;
    Ok(session)
}
