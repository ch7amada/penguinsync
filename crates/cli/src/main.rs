//! `penguinsync` — terminal UI and command-line client.
//!
//! A thin client over D-Bus; all protocol logic lives in the daemon.
//!
//! TUI: status, device list, pairing QR display, confirm/revoke.
//! CLI: non-interactive verbs (`penguinsync send file.pdf`, `penguinsync debug`).
//!
//! Both are frontends over one shared D-Bus client module. Room to grow into a
//! full dashboard with transfer progress at M4 — but not before the protocol
//! works (docs/design.md §4.3).

fn main() {
    eprintln!(
        "penguinsync {}: not implemented yet — see docs/design.md, milestone M0",
        env!("CARGO_PKG_VERSION")
    );
}
