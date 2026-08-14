//! Pairing: the QR payload, the single-use token, and the trust decision.
//!
//! Sans-I/O throughout — randomness and "now" are supplied by the caller
//! (`net`/`daemon`), never read from the OS here, so every path is a plain
//! unit test (docs/design.md §4.2, §8).
//!
//! Flow (docs/protocol.md §3): Linux generates a token and displays a QR;
//! Android scans it, dials the candidate addresses, pins Linux's key, and
//! presents this token plus its own key back over the connection; Linux's TUI
//! then asks a human to confirm before the pin is persisted.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::message::DeviceId;

/// Length in bytes of a pairing token.
pub const TOKEN_LEN: usize = 16;

/// How long a token remains redeemable after issuance (docs/protocol.md §3.1).
pub const TOKEN_TTL: Duration = Duration::from_secs(60);

pub type TokenBytes = [u8; TOKEN_LEN];

/// A single-use pairing token with an expiry.
///
/// `redeem` consumes it: calling it twice, or after expiry, fails. That
/// "single-use" property is enforced here rather than left to whoever holds
/// the token, so there is exactly one place that can get it wrong.
#[derive(Debug, Clone)]
pub struct PairingToken {
    bytes: TokenBytes,
    issued_at: Instant,
    redeemed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RedeemError {
    #[error("pairing token does not match the one that was issued")]
    Mismatch,
    #[error("pairing token already used")]
    AlreadyRedeemed,
    #[error("pairing token expired")]
    Expired,
}

impl PairingToken {
    /// Mint a token from caller-supplied randomness.
    pub fn issue(random: TokenBytes, now: Instant) -> Self {
        Self {
            bytes: random,
            issued_at: now,
            redeemed: false,
        }
    }

    pub fn bytes(&self) -> TokenBytes {
        self.bytes
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        now.duration_since(self.issued_at) >= TOKEN_TTL
    }

    /// Attempt to redeem the token. On success, the token is marked used and
    /// every subsequent call fails — including a retry with the same bytes.
    pub fn redeem(&mut self, presented: TokenBytes, now: Instant) -> Result<(), RedeemError> {
        if self.redeemed {
            return Err(RedeemError::AlreadyRedeemed);
        }
        if self.is_expired(now) {
            return Err(RedeemError::Expired);
        }
        if presented != self.bytes {
            return Err(RedeemError::Mismatch);
        }
        self.redeemed = true;
        Ok(())
    }
}

/// The contents of the `penguinsync://pair` QR code (docs/protocol.md §3.1).
#[derive(Debug, Clone, PartialEq)]
pub struct QrPayload {
    pub version: u16,
    pub device_id: DeviceId,
    pub name: String,
    /// Candidate addresses to dial first, so the first connection never
    /// depends on mDNS working.
    pub addrs: Vec<SocketAddr>,
    pub token: TokenBytes,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QrParseError {
    #[error("not a penguinsync:// pairing URI")]
    WrongScheme,
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
    #[error("malformed field `{0}`")]
    MalformedField(&'static str),
}

const SCHEME: &str = "penguinsync";

/// Encode a [`QrPayload`] as the URI string that goes into the QR code.
pub fn encode_qr_uri(payload: &QrPayload) -> String {
    let mut url = url::Url::parse(&format!("{SCHEME}://pair")).expect("static scheme is valid");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("v", &payload.version.to_string());
        q.append_pair("id", &crate::message::to_hex(&payload.device_id));
        q.append_pair("name", &payload.name);
        for addr in &payload.addrs {
            q.append_pair("addr", &addr.to_string());
        }
        q.append_pair("token", &hex_encode(&payload.token));
    }
    url.into()
}

/// Parse a scanned QR string back into a [`QrPayload`].
pub fn decode_qr_uri(uri: &str) -> Result<QrPayload, QrParseError> {
    let url = url::Url::parse(uri).map_err(|_| QrParseError::WrongScheme)?;
    if url.scheme() != SCHEME {
        return Err(QrParseError::WrongScheme);
    }

    let mut version = None;
    let mut device_id = None;
    let mut name = None;
    let mut addrs = Vec::new();
    let mut token = None;

    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "v" => {
                version = Some(
                    value
                        .parse::<u16>()
                        .map_err(|_| QrParseError::MalformedField("v"))?,
                )
            }
            "id" => {
                device_id = Some(
                    crate::message::from_hex(&value).ok_or(QrParseError::MalformedField("id"))?,
                )
            }
            "name" => name = Some(value.into_owned()),
            "addr" => addrs.push(
                value
                    .parse::<SocketAddr>()
                    .map_err(|_| QrParseError::MalformedField("addr"))?,
            ),
            "token" => {
                token = Some(hex_decode_token(&value).ok_or(QrParseError::MalformedField("token"))?)
            }
            _ => {}
        }
    }

    Ok(QrPayload {
        version: version.ok_or(QrParseError::MissingField("v"))?,
        device_id: device_id.ok_or(QrParseError::MissingField("id"))?,
        name: name.ok_or(QrParseError::MissingField("name"))?,
        addrs,
        token: token.ok_or(QrParseError::MissingField("token"))?,
    })
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode_token(s: &str) -> Option<TokenBytes> {
    if s.len() != TOKEN_LEN * 2 {
        return None;
    }
    let mut out = [0u8; TOKEN_LEN];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> QrPayload {
        QrPayload {
            version: 0,
            device_id: [9u8; 32],
            name: "desk-fedora".into(),
            addrs: vec![
                "192.168.1.42:5555".parse().unwrap(),
                "[fe80::1]:5555".parse().unwrap(),
            ],
            token: [0xab; TOKEN_LEN],
        }
    }

    #[test]
    fn qr_round_trips() {
        let p = sample();
        let uri = encode_qr_uri(&p);
        assert!(uri.starts_with("penguinsync://"));
        assert_eq!(decode_qr_uri(&uri).unwrap(), p);
    }

    #[test]
    fn qr_rejects_wrong_scheme() {
        assert_eq!(
            decode_qr_uri("https://example.com").unwrap_err(),
            QrParseError::WrongScheme
        );
    }

    #[test]
    fn qr_rejects_missing_field() {
        assert_eq!(
            decode_qr_uri("penguinsync://pair?v=0&id=00").unwrap_err(),
            QrParseError::MalformedField("id")
        );
    }

    #[test]
    fn token_redeems_once() {
        let now = Instant::now();
        let mut token = PairingToken::issue([1u8; TOKEN_LEN], now);
        assert!(token.redeem([1u8; TOKEN_LEN], now).is_ok());
        assert_eq!(
            token.redeem([1u8; TOKEN_LEN], now).unwrap_err(),
            RedeemError::AlreadyRedeemed
        );
    }

    #[test]
    fn token_rejects_wrong_bytes() {
        let now = Instant::now();
        let mut token = PairingToken::issue([1u8; TOKEN_LEN], now);
        assert_eq!(
            token.redeem([2u8; TOKEN_LEN], now).unwrap_err(),
            RedeemError::Mismatch
        );
    }

    #[test]
    fn token_expires() {
        let now = Instant::now();
        let token = PairingToken::issue([1u8; TOKEN_LEN], now);
        assert!(!token.is_expired(now));
        assert!(token.is_expired(now + TOKEN_TTL));
    }
}
