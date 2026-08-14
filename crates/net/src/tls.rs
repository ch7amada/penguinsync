//! Certificate verification: SPKI pinning, no system root store.
//!
//! Trust is pinned on pairing, in both directions, from one QR scan plus one
//! confirmation (docs/design.md §7). Concretely: connections are mutual TLS,
//! and the same [`PinningVerifier`] backs both directions — it accepts a
//! peer certificate if its SPKI hash is already a pinned [`DeviceId`], or if
//! a pairing window is currently open (docs/protocol.md §3.2, §3.3).
//!
//! `rustls-native-certs` and `webpki-roots` are deliberately not
//! dependencies — nothing here consults the system root store.

use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, Error as TlsError, SignatureScheme};

use penguinsync_protocol::DeviceId;

use crate::identity::device_id_from_spki;

/// The set of pinned device keys, plus an optional open pairing window
/// during which an unpinned peer is provisionally accepted.
///
/// The pairing window is deliberately coarse: it does not track *which*
/// device is expected, only that pairing is in progress and for how much
/// longer. The actual trust decision — does the presented pairing token
/// match, does a human confirm — happens above TLS, in the app-layer
/// handshake (`penguinsync-protocol`'s `ConnectionMachine`); TLS only needs
/// to let an unknown cert far enough in for that exchange to happen at all.
#[derive(Debug, Default)]
pub struct TrustStore {
    paired: RwLock<HashSet<DeviceId>>,
    pairing_deadline: RwLock<Option<Instant>>,
}

impl TrustStore {
    pub fn new(paired: impl IntoIterator<Item = DeviceId>) -> Self {
        Self {
            paired: RwLock::new(paired.into_iter().collect()),
            pairing_deadline: RwLock::new(None),
        }
    }

    pub fn is_paired(&self, id: &DeviceId) -> bool {
        self.paired.read().unwrap().contains(id)
    }

    pub fn pin(&self, id: DeviceId) {
        self.paired.write().unwrap().insert(id);
    }

    /// Unpair is unilateral and immediate on the initiating side
    /// (docs/design.md §7): the next connection attempt from `id` is
    /// rejected at the TLS layer, with no cooperation from the peer needed.
    pub fn unpair(&self, id: &DeviceId) {
        self.paired.write().unwrap().remove(id);
    }

    pub fn paired_devices(&self) -> Vec<DeviceId> {
        self.paired.read().unwrap().iter().copied().collect()
    }

    /// Open the window until `deadline` — an unpinned peer cert is accepted
    /// by TLS until then. Matches the 60 s single-use QR token
    /// (docs/protocol.md §3.1); the caller is responsible for actually
    /// enforcing that TTL on the token itself.
    pub fn open_pairing_window(&self, deadline: Instant) {
        *self.pairing_deadline.write().unwrap() = Some(deadline);
    }

    pub fn close_pairing_window(&self) {
        *self.pairing_deadline.write().unwrap() = None;
    }

    fn pairing_open(&self) -> bool {
        match *self.pairing_deadline.read().unwrap() {
            Some(deadline) => Instant::now() < deadline,
            None => false,
        }
    }
}

/// A `ServerCertVerifier` and `ClientCertVerifier` in one: the same
/// SPKI-pinning check applies to whichever side is being authenticated.
#[derive(Debug)]
pub struct PinningVerifier {
    trust: Arc<TrustStore>,
    provider: CryptoProvider,
}

impl PinningVerifier {
    pub fn new(trust: Arc<TrustStore>) -> Arc<Self> {
        Arc::new(Self {
            trust,
            provider: rustls::crypto::ring::default_provider(),
        })
    }

    fn device_id_of(cert: &CertificateDer<'_>) -> Result<DeviceId, TlsError> {
        let end_entity = webpki::EndEntityCert::try_from(cert)
            .map_err(|_| TlsError::General("malformed peer certificate".into()))?;
        Ok(device_id_from_spki(
            end_entity.subject_public_key_info().as_ref(),
        ))
    }

    fn check(&self, cert: &CertificateDer<'_>) -> Result<(), TlsError> {
        let id = Self::device_id_of(cert)?;
        if self.trust.is_paired(&id) || self.trust.pairing_open() {
            Ok(())
        } else {
            Err(TlsError::General(format!(
                "unpinned peer device {}",
                penguinsync_protocol::message::short_fingerprint(&id)
            )))
        }
    }
}

impl ServerCertVerifier for PinningVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        self.check(end_entity)?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

impl ClientCertVerifier for PinningVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, TlsError> {
        self.check(end_entity)?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpinned_and_no_pairing_window_is_rejected() {
        let trust = Arc::new(TrustStore::new([]));
        let verifier = PinningVerifier::new(trust);
        let identity = crate::identity::Identity::generate().unwrap();
        assert!(verifier.check(&identity.cert_der()).is_err());
    }

    #[test]
    fn pinned_device_is_accepted() {
        let identity = crate::identity::Identity::generate().unwrap();
        let trust = Arc::new(TrustStore::new([identity.device_id]));
        let verifier = PinningVerifier::new(trust);
        assert!(verifier.check(&identity.cert_der()).is_ok());
    }

    #[test]
    fn open_pairing_window_accepts_unknown_peer_until_deadline() {
        let trust = Arc::new(TrustStore::new([]));
        trust.open_pairing_window(Instant::now() + std::time::Duration::from_secs(60));
        let verifier = PinningVerifier::new(trust);
        let identity = crate::identity::Identity::generate().unwrap();
        assert!(verifier.check(&identity.cert_der()).is_ok());
    }

    #[test]
    fn expired_pairing_window_rejects_unknown_peer() {
        let trust = Arc::new(TrustStore::new([]));
        trust.open_pairing_window(Instant::now() - std::time::Duration::from_secs(1));
        let verifier = PinningVerifier::new(trust);
        let identity = crate::identity::Identity::generate().unwrap();
        assert!(verifier.check(&identity.cert_der()).is_err());
    }

    #[test]
    fn unpair_revokes_immediately() {
        let identity = crate::identity::Identity::generate().unwrap();
        let trust = Arc::new(TrustStore::new([identity.device_id]));
        trust.unpair(&identity.device_id);
        let verifier = PinningVerifier::new(trust);
        assert!(verifier.check(&identity.cert_der()).is_err());
    }
}
