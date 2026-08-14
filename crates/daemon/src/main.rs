//! `penguinsyncd` — the PenguinSync background daemon.
//!
//! Runs as a systemd **user** service (clipboard is per-session), started at
//! login and kept running. Not D-Bus-activated and not idle-exiting: a clipboard
//! daemon that idle-exits is not a clipboard daemon.
//!
//! Serves `org.penguinsync.Daemon1` at `/org/penguinsync/Daemon`, implementing
//! `org.freedesktop.DBus.ObjectManager` with one object per paired device. The
//! TUI, the Nautilus extension and any future client all consume that.
//!
//! Must start and run successfully **without** the GNOME Shell extension
//! present — file transfer and notification mirroring do not need it. Clipboard
//! is then reported as unavailable (docs/design.md §4.4).

fn main() {
    eprintln!(
        "penguinsyncd {}: not implemented yet — see docs/design.md, milestone M0",
        env!("CARGO_PKG_VERSION")
    );
}
