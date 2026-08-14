//! Networking and platform I/O.
//!
//! Drives [`penguinsync_protocol`] over a real network: QUIC via `quinn`, TLS
//! identity with SPKI pinning, discovery, and file I/O. This is the layer both
//! the Linux daemon and the Android core (via `penguinsync-ffi`) consume.
//!
//! Clipboard access is abstracted behind a `ClipboardBackend` trait
//! (`watch()` / `read()` / `write()`), probed at startup. v1 ships one backend,
//! talking to the GNOME Shell extension over D-Bus; an `ext-data-control-v1`
//! backend for Plasma/Sway/Hyprland/Niri/COSMIC lands in v0.2
//! (docs/design.md §4.4.1).
//!
//! Note the platform split on Android: this crate cannot observe network
//! changes, acquire a `MulticastLock`, or run mDNS. Kotlin owns those and calls
//! in (docs/design.md §4.6).
//!
//! # M0 shape
//!
//! [`endpoint::Endpoint`] wraps the QUIC transport; [`identity::Identity`]
//! and [`tls::TrustStore`]/[`tls::PinningVerifier`] give it mutual,
//! SPKI-pinned TLS; [`session::Session`] drives one connection's control
//! stream with the protocol crate's state machine; [`listener`] and
//! [`reconnect`] are the two loops built on top — accept-forever for Linux,
//! dial-with-backoff for Android.

#![forbid(unsafe_code)]

pub use penguinsync_protocol as protocol;

pub mod clipboard;
pub mod endpoint;
pub mod framing;
pub mod identity;
pub mod listener;
pub mod reconnect;
pub mod session;
pub mod tls;

pub use clipboard::{ClipChanged, ClipboardBackend, ClipboardError};
pub use endpoint::{Endpoint, EndpointError};
pub use identity::{Identity, IdentityError};
pub use listener::ListenerEvent;
pub use reconnect::DialerEvent;
pub use session::{CloseReason, Session, SessionEvent, SessionHandle};
pub use tls::{PinningVerifier, TrustStore};
