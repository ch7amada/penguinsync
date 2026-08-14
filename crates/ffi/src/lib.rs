//! UniFFI surface for the Android app.
//!
//! Keep this surface **as narrow as you can stand** — every type crossing the
//! boundary is a type maintained in two representations.
//!
//! The shape is a single `PenguinSyncCore` handle object (`start`, `stop`,
//! `pair`, `send_file`, …), with events pushed to Kotlin through a callback
//! interface that Kotlin wraps once per stream in a `callbackFlow`.
//!
//! # Cancellation
//!
//! UniFFI does **not** propagate Kotlin coroutine cancellation into Rust. Every
//! long-lived operation must return a handle with an explicit `cancel()`, and
//! every Kotlin `callbackFlow` wrapper must end with
//! `awaitClose { handle.cancel() }`. Getting this wrong leaks tokio tasks inside
//! a process Android is trying to kill (docs/design.md §4.2).
//!
//! # M0
//!
//! `PenguinSyncCore::new` and `PenguinSyncCore::pair` are the whole surface —
//! enough to pair, connect over QUIC, and exchange ping/pong from Kotlin.
//! `stop`/`send_file`/clipboard arrive with their milestones.

#![forbid(unsafe_code)]

pub use penguinsync_net as net;

mod core;
mod state;

pub use crate::core::{ConnectionHandle, CoreError, CoreEvent, CoreEventListener, PenguinSyncCore};

uniffi::setup_scaffolding!();
