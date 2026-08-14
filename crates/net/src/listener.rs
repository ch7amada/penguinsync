//! The listener's accept loop.
//!
//! Linux listens; Android always dials (docs/design.md §5.3). Every incoming
//! connection is TLS-verified by [`crate::tls::PinningVerifier`] before this
//! loop ever sees it — an unpinned peer outside an open pairing window never
//! completes the handshake. What's left here is just: accept, open the
//! control stream, and drain its events — one task per connection, so a slow
//! or misbehaving peer can never block the next one from being accepted
//! (docs/design.md §4.3, the daemon must stay responsive).
//!
//! Unlike the dialer, the listener may have many concurrent sessions (one
//! Linux machine ↔ N Android devices), so every event is tagged with the
//! remote address it came from.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use penguinsync_protocol::LocalIdentity;

use crate::endpoint::Endpoint;
use crate::session::{Session, SessionEvent, SessionHandle};

pub enum ListenerEvent {
    /// A connection came up. `handle` is a cheap, cloneable capability
    /// (e.g. to push a clipboard update) that stays valid for as long as
    /// the session does — hold on to it if you need it later, this event
    /// fires exactly once per connection.
    Connected {
        remote: SocketAddr,
        handle: SessionHandle,
    },
    Session {
        remote: SocketAddr,
        event: SessionEvent,
    },
}

pub async fn run(
    endpoint: Arc<Endpoint>,
    local: LocalIdentity,
    keepalive_interval: Duration,
    events: mpsc::UnboundedSender<ListenerEvent>,
) {
    loop {
        let Some(incoming) = endpoint.accept().await else {
            return;
        };
        let local = local.clone();
        let events = events.clone();
        tokio::spawn(async move {
            if let Err(e) = accept_and_drain(incoming, local, keepalive_interval, events).await {
                tracing::debug!(error = %e, "incoming connection did not complete");
            }
        });
    }
}

#[derive(Debug, thiserror::Error)]
enum AcceptError {
    #[error("accepting connection: {0}")]
    Connection(#[from] quinn::ConnectionError),
    #[error("opening control stream: {0}")]
    Session(#[from] crate::session::SessionError),
}

async fn accept_and_drain(
    incoming: quinn::Incoming,
    local: LocalIdentity,
    keepalive_interval: Duration,
    events: mpsc::UnboundedSender<ListenerEvent>,
) -> Result<(), AcceptError> {
    let connecting = incoming.accept()?;
    let connection = connecting.await?;
    let remote = connection.remote_address();
    let session = Session::accept(connection, local, keepalive_interval).await?;

    if events
        .send(ListenerEvent::Connected {
            remote,
            handle: session.handle(),
        })
        .is_err()
    {
        session.close();
        return Ok(());
    }

    session
        .drain(move |event| {
            let _ = events.send(ListenerEvent::Session { remote, event });
        })
        .await;
    Ok(())
}
