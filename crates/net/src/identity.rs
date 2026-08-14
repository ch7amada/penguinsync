//! Device identity: the Ed25519 keypair and self-signed cert every device
//! generates on first run (docs/design.md §5.1, docs/protocol.md §1).
//!
//! `DeviceId` is the SHA-256 fingerprint of the certificate's
//! SubjectPublicKeyInfo — computed directly from the freshly generated
//! keypair here, and re-derived from a peer's presented certificate in
//! [`crate::tls`] for pin comparison.
//!
//! Private key storage relies on the OS sandbox for now (0600 file on Linux,
//! app-private directory on Android); platform keystore integration is Phase
//! 3 hardening (docs/design.md §5.1). This module writes PEM to a directory
//! the caller chooses and sets 0600 on the key file where the platform
//! supports Unix permissions — it does not decide *where* that directory is.

use std::path::Path;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

use penguinsync_protocol::DeviceId;

const CERT_FILE: &str = "identity-cert.pem";
const KEY_FILE: &str = "identity-key.pem";

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("generating keypair/certificate: {0}")]
    Generate(#[from] rcgen::Error),
    #[error("reading identity file: {0}")]
    Io(#[from] std::io::Error),
    #[error("parsing stored identity: {0}")]
    Parse(String),
}

/// This device's long-lived identity: a keypair, a self-signed cert wrapping
/// it, and the resulting [`DeviceId`].
pub struct Identity {
    pub device_id: DeviceId,
    key_pair: rcgen::KeyPair,
    cert: rcgen::Certificate,
}

impl Identity {
    /// Generate a fresh Ed25519 identity. Does not touch disk — call
    /// [`Identity::save`] to persist it.
    pub fn generate() -> Result<Self, IdentityError> {
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
        let device_id = device_id_from_spki(&key_pair.public_key_der());
        let params = rcgen::CertificateParams::new(vec!["penguinsync".to_string()])?;
        let cert = params.self_signed(&key_pair)?;
        Ok(Self {
            device_id,
            key_pair,
            cert,
        })
    }

    /// Load a persisted identity from `dir`, or generate and save a new one
    /// if none exists yet. This is the entry point most callers want.
    pub fn load_or_generate(dir: &Path) -> Result<Self, IdentityError> {
        let cert_path = dir.join(CERT_FILE);
        let key_path = dir.join(KEY_FILE);
        if cert_path.exists() && key_path.exists() {
            Self::load(dir)
        } else {
            let identity = Self::generate()?;
            identity.save(dir)?;
            Ok(identity)
        }
    }

    fn load(dir: &Path) -> Result<Self, IdentityError> {
        let key_pem = std::fs::read_to_string(dir.join(KEY_FILE))?;
        let key_pair =
            rcgen::KeyPair::from_pem(&key_pem).map_err(|e| IdentityError::Parse(e.to_string()))?;
        let device_id = device_id_from_spki(&key_pair.public_key_der());
        let params = rcgen::CertificateParams::new(vec!["penguinsync".to_string()])?;
        let cert = params.self_signed(&key_pair)?;
        Ok(Self {
            device_id,
            key_pair,
            cert,
        })
    }

    /// Persist to `dir`, creating it if necessary. The private key is
    /// written 0600; the certificate is regenerated deterministically from it
    /// on every load, so only the key strictly needs saving — the cert is
    /// saved too, for inspection.
    pub fn save(&self, dir: &Path) -> Result<(), IdentityError> {
        std::fs::create_dir_all(dir)?;
        let key_path = dir.join(KEY_FILE);
        std::fs::write(&key_path, self.key_pair.serialize_pem())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::write(dir.join(CERT_FILE), self.cert.pem())?;
        Ok(())
    }

    pub fn cert_der(&self) -> CertificateDer<'static> {
        self.cert.der().clone()
    }

    pub fn key_der(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_pair.serialize_der()))
    }
}

/// `DeviceId = SHA-256(SubjectPublicKeyInfo)` (docs/protocol.md §1).
pub fn device_id_from_spki(spki_der: &[u8]) -> DeviceId {
    Sha256::digest(spki_der).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_a_stable_device_id() {
        let identity = Identity::generate().unwrap();
        // Re-deriving from the same public key must be deterministic.
        let again = device_id_from_spki(&identity.key_pair.public_key_der());
        assert_eq!(identity.device_id, again);
    }

    #[test]
    fn two_identities_differ() {
        let a = Identity::generate().unwrap();
        let b = Identity::generate().unwrap();
        assert_ne!(a.device_id, b.device_id);
    }

    #[test]
    fn save_then_load_round_trips_device_id() {
        let dir = tempdir();
        let original = Identity::generate().unwrap();
        original.save(dir.path()).unwrap();

        let loaded = Identity::load_or_generate(dir.path()).unwrap();
        assert_eq!(original.device_id, loaded.device_id);
    }

    #[test]
    fn load_or_generate_persists_across_calls() {
        let dir = tempdir();
        let first = Identity::load_or_generate(dir.path()).unwrap();
        let second = Identity::load_or_generate(dir.path()).unwrap();
        assert_eq!(first.device_id, second.device_id);
    }

    #[cfg(unix)]
    #[test]
    fn key_file_is_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir();
        Identity::generate().unwrap().save(dir.path()).unwrap();
        let mode = std::fs::metadata(dir.path().join(KEY_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// Minimal scratch-dir helper so this crate doesn't need a `tempfile` dev
    /// dependency for five tests.
    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "penguinsync-identity-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
