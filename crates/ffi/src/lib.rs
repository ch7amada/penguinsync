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
//! # Status
//!
//! `new`, `pair`, `send_clipboard` and `list_paired_devices` landed with
//! M0–M2. `send_file` (M4, docs/design.md §6.2) mirrors `send_clipboard`'s
//! shape: fire-and-forget, `CoreError::NotConnected` if nothing's connected,
//! outcome and progress arrive as `CoreEvent::Transfer*` events. `stop`
//! still doesn't exist — there is no separate daemon process on Android, so
//! nothing has needed it yet.

#![forbid(unsafe_code)]

pub use penguinsync_net as net;

mod core;
mod state;

pub use crate::core::{
    ConnectionHandle, CoreError, CoreEvent, CoreEventListener, PairedDevice, PenguinSyncCore,
};

uniffi::setup_scaffolding!();
