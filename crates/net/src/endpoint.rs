//! QUIC endpoint setup.
//!
//! A single `quinn::Endpoint` both accepts and initiates (docs/design.md
//! §5.3) — that shape maps directly onto the one Linux ↔ N Android topology.
//! In practice only Linux ever needs [`Endpoint::listening`] (it listens);
//! Android only ever needs [`Endpoint::dialing`] (it always dials, since it's
//! the side that changes networks and needs to reconnect).
//!
//! TLS is mutual: both sides present a certificate, both are checked by
//! [`crate::tls::PinningVerifier`]. QUIC requires TLS 1.3, so protocol
//! versions are pinned to that explicitly (see quinn's own reference
//! `QuicClientConfig::inner`, mirrored here).

use std::net::SocketAddr;
use std::sync::Arc;

use crate::identity::Identity;
use crate::tls::{PinningVerifier, TrustStore};

/// ALPN protocol identifier. Versioned so a future breaking wire change can
/// ship a new one rather than silently talking past an old peer.
const ALPN: &[u8] = b"penguinsync/0";

#[derive(Debug, thiserror::Error)]
pub enum EndpointError {
    #[error("building TLS config: {0}")]
    Tls(#[from] rustls::Error),
    #[error("TLS config has no QUIC-compatible cipher suite: {0}")]
    NoInitialCipherSuite(#[from] quinn::crypto::rustls::NoInitialCipherSuite),
    #[error("binding UDP socket: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Endpoint {
    inner: quinn::Endpoint,
}

impl Endpoint {
    /// Linux: binds `listen_addr`, accepts incoming connections. Can also
    /// dial, though nothing in v1 exercises that.
    pub fn listening(
        identity: &Identity,
        trust: Arc<TrustStore>,
        listen_addr: SocketAddr,
    ) -> Result<Self, EndpointError> {
        let server_cfg = server_config(identity, trust.clone())?;
        let mut inner = quinn::Endpoint::server(server_cfg, listen_addr)?;
        inner.set_default_client_config(client_config(identity, trust)?);
        Ok(Self { inner })
    }

    /// Android: dials only, from an ephemeral local port.
    pub fn dialing(identity: &Identity, trust: Arc<TrustStore>) -> Result<Self, EndpointError> {
        let mut inner = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())?;
        inner.set_default_client_config(client_config(identity, trust)?);
        Ok(Self { inner })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    /// Wait for the next incoming connection attempt. `None` means the
    /// endpoint has been closed.
    pub async fn accept(&self) -> Option<quinn::Incoming> {
        self.inner.accept().await
    }

    /// Dial `addr`. No hostname verification is performed — trust is by
    /// pinned SPKI, not by name — so the peer's IP address doubles as the
    /// TLS "server name".
    pub fn connect(&self, addr: SocketAddr) -> Result<quinn::Connecting, quinn::ConnectError> {
        self.inner.connect(addr, &addr.ip().to_string())
    }

    pub fn close(&self) {
        self.inner.close(0u32.into(), b"");
    }
}

fn client_config(
    identity: &Identity,
    trust: Arc<TrustStore>,
) -> Result<quinn::ClientConfig, EndpointError> {
    let verifier = PinningVerifier::new(trust);
    let mut crypto = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .expect("ring provider supports TLS 1.3")
    .dangerous()
    .with_custom_certificate_verifier(verifier)
    .with_client_auth_cert(vec![identity.cert_der()], identity.key_der())?;
    crypto.alpn_protocols = vec![ALPN.to_vec()];

    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?;
    Ok(quinn::ClientConfig::new(Arc::new(quic_crypto)))
}

fn server_config(
    identity: &Identity,
    trust: Arc<TrustStore>,
) -> Result<quinn::ServerConfig, EndpointError> {
    let verifier = PinningVerifier::new(trust);
    let mut crypto = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .expect("ring provider supports TLS 1.3")
    .with_client_cert_verifier(verifier)
    .with_single_cert(vec![identity.cert_der()], identity.key_der())?;
    crypto.alpn_protocols = vec![ALPN.to_vec()];

    let quic_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(crypto)?;
    Ok(quinn::ServerConfig::with_crypto(Arc::new(quic_crypto)))
}
