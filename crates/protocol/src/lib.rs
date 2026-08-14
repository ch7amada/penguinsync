//! Sans-I/O protocol core.
//!
//! This crate defines the PenguinSync wire protocol and the state machines that
//! drive it: pairing, trust decisions, reconnect, echo suppression, and transfer
//! bookkeeping. It consumes events and produces actions. It never touches a
//! socket, a file, or an async runtime — see the invariant in `Cargo.toml`.
//!
//! Both the Linux daemon and the Android app run this same code.
//!
//! The normative wire specification lives in `docs/protocol.md`. Every change to
//! the wire format must edit that file in the same commit.

#![forbid(unsafe_code)]

pub mod backoff;
pub mod clipboard;
pub mod connection;
pub mod message;
pub mod pairing;

pub use clipboard::{Clip, ClipError};
pub use connection::{Action, ConnectionMachine, Event, LocalIdentity, RejectReason};
pub use message::{Capability, DeviceId, Handshake, Message, Ping, Pong};
pub use pairing::{PairingToken, QrPayload};

/// Wire protocol version, advertised in the mDNS TXT record and re-checked
/// during the handshake.
///
/// Mismatches are rejected outright — there are no compatibility guarantees
/// before 1.0 (docs/design.md §5.4). Bump this in the same commit as any wire
/// change, alongside `docs/protocol.md`.
pub const PROTOCOL_VERSION: u16 = 1;

/// Service type advertised over mDNS/DNS-SD.
pub const SERVICE_TYPE: &str = "_penguinsync._udp";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_is_pre_release() {
        assert_eq!(
            PROTOCOL_VERSION, 1,
            "docs/protocol.md must be updated in the same commit"
        );
    }
}
