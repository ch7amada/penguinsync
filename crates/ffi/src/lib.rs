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

#![forbid(unsafe_code)]

pub use penguinsync_net as net;
