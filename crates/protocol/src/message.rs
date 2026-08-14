//! Wire message types.
//!
//! Encoding is `postcard` over `serde` (docs/design.md §5.4, docs/protocol.md
//! §5). Both ends compile from these same struct definitions, so there is no
//! cross-language schema to maintain.
//!
//! `net` is responsible for framing these on a QUIC stream (length-prefixing);
//! this module only turns a [`Message`] into bytes and back.

use serde::{Deserialize, Serialize};

/// A device's identity: the SHA-256 fingerprint of its certificate's
/// SubjectPublicKeyInfo (docs/protocol.md §1).
pub type DeviceId = [u8; 32];

/// Render a [`DeviceId`] as a short, human-comparable fingerprint: groups of
/// four hex characters from the first 8 bytes, e.g. `a1b2-c3d4-e5f6-0718`.
///
/// The full 32 bytes are what gets pinned; this is only what a human reads
/// aloud to compare two screens (docs/protocol.md §1).
pub fn short_fingerprint(id: &DeviceId) -> String {
    id[..8]
        .chunks(2)
        .map(|pair| format!("{:02x}{:02x}", pair[0], pair[1]))
        .collect::<Vec<_>>()
        .join("-")
}

/// Render a [`DeviceId`] as a full lowercase hex string, for storage keys and
/// D-Bus object paths.
pub fn to_hex(id: &DeviceId) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

/// Parse a full lowercase hex string back into a [`DeviceId`].
pub fn from_hex(s: &str) -> Option<DeviceId> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in out.iter_mut().enumerate() {
        *chunk = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// A feature a device offers. Negotiated in [`Handshake::capabilities`] so a
/// v1 device (clipboard + files only) and a later one can talk without a
/// protocol break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Clipboard,
    Files,
    Notifications,
}

/// First message either side sends on the control stream, immediately after
/// the QUIC/TLS handshake completes (docs/protocol.md §6.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handshake {
    /// Must equal [`crate::PROTOCOL_VERSION`] on both sides; mismatch is a
    /// hard reject (docs/design.md §5.4).
    pub version: u16,
    pub device_id: DeviceId,
    pub name: String,
    pub capabilities: Vec<Capability>,
    /// Present only when this connection is a fresh pairing attempt (the
    /// dialing side proving possession of the QR's single-use token). Absent
    /// on every reconnect of an already-paired device.
    pub pairing_token: Option<[u8; crate::pairing::TOKEN_LEN]>,
}

/// The M0 walking skeleton's entire payload: a value round-tripped to prove
/// the connection is alive end to end, with nothing platform-specific
/// attached (docs/design.md §9, docs/protocol.md §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ping {
    pub nonce: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pong {
    pub nonce: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    Handshake(Handshake),
    Ping(Ping),
    Pong(Pong),
}

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("encode failed: {0}")]
    Encode(postcard::Error),
    #[error("decode failed: {0}")]
    Decode(postcard::Error),
}

/// Serialize a message. `net` prefixes the result with a length header before
/// writing it to a QUIC stream — this crate never touches a stream.
pub fn encode(msg: &Message) -> Result<Vec<u8>, CodecError> {
    postcard::to_allocvec(msg).map_err(CodecError::Encode)
}

/// Deserialize a message produced by [`encode`].
pub fn decode(bytes: &[u8]) -> Result<Message, CodecError> {
    postcard::from_bytes(bytes).map_err(CodecError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_ping() {
        let msg = Message::Ping(Ping { nonce: 42 });
        let bytes = encode(&msg).unwrap();
        assert_eq!(decode(&bytes).unwrap(), msg);
    }

    #[test]
    fn round_trips_handshake() {
        let msg = Message::Handshake(Handshake {
            version: 0,
            device_id: [7u8; 32],
            name: "pixel".into(),
            capabilities: vec![Capability::Clipboard, Capability::Files],
            pairing_token: Some([1u8; crate::pairing::TOKEN_LEN]),
        });
        let bytes = encode(&msg).unwrap();
        assert_eq!(decode(&bytes).unwrap(), msg);
    }

    #[test]
    fn hex_round_trips() {
        let id: DeviceId = std::array::from_fn(|i| i as u8);
        assert_eq!(from_hex(&to_hex(&id)).unwrap(), id);
    }

    #[test]
    fn short_fingerprint_is_stable_and_short() {
        let id: DeviceId = std::array::from_fn(|i| i as u8);
        assert_eq!(short_fingerprint(&id), "0001-0203-0405-0607");
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode(&[0xff, 0xff, 0xff]).is_err());
    }
}
