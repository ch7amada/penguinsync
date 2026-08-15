# PenguinSync

Peer-to-peer sync between an Android phone and a GNOME/Wayland Linux desktop, over the local network. Clipboard, files, and notification mirroring. No cloud, no server anyone else runs.

**Status: M0–M2 implemented and verified on real hardware** — pairing, QUIC reconnect, and clipboard sync both directions (manual tier). File transfer and notification mirroring are still design-only. See [`docs/design.md`](docs/design.md).

---

## What it will do

- **Clipboard sync** — text, both directions, across all paired devices
- **File transfer** — right-click in Nautilus, or Android's share sheet
- **Notification mirroring** — Android notifications on your desktop, with two-way dismissal

## What it will never do

- SMS / call mirroring
- Contacts / calendar sync
- Remote input (mouse, keyboard)
- Media player control
- Screen mirroring
- Cloud accounts, or any server you don't run

Each is a whole subsystem. This list exists so scope discussions are short.

## Not yet, but planned

LAN-only is the current boundary — the transport is abstracted, and Tailscale-style overlays work today without any code from us. Non-GNOME compositors (Plasma, Sway, Hyprland, Niri, COSMIC) arrive in v0.2 via `ext-data-control-v1`, which is ironically an easier target than GNOME. Clipboard images, file-transfer resume, and notification inline reply are deferred.

## Prior art

[GSConnect](https://github.com/GSConnect/gnome-shell-extension-gsconnect) and [KDE Connect](https://kdeconnect.kde.org/) already do this, and do it well. PenguinSync is a from-scratch project — building it is the point. Where they have solved a hard platform problem, this design copies their solution deliberately rather than rediscovering it.

## Architecture in one paragraph

Protocol, QUIC transport, TLS identity and all state machines are written once in **Rust** and shared by the Linux daemon and the Android app (via UniFFI). **Kotlin** owns the Compose UI and the platform glue Rust cannot reach — discovery, foreground service, permissions, Wi-Fi locks, clipboard and notification access. A minimal **GNOME Shell extension** exists solely because Mutter implements no clipboard-manager protocol; a small **Nautilus extension** provides the right-click send menu. Devices pair by QR with mutual key pinning.

See [`docs/design.md`](docs/design.md) for the full design and the reasoning behind every decision, and [`docs/protocol.md`](docs/protocol.md) for the normative wire specification.

## Repository layout

| Path | Contents |
|---|---|
| `crates/protocol` | Sans-I/O protocol core — no sockets, no async runtime |
| `crates/net` | QUIC, TLS, discovery, file I/O |
| `crates/ffi` | UniFFI surface for Android |
| `crates/daemon` | `penguinsyncd` — Linux background daemon |
| `crates/cli` | `penguinsync` — TUI and CLI client |
| `android/` | Android app (`:app` + `:core`) |
| `gnome-extension/` | GNOME Shell extension (clipboard access) |
| `nautilus/` | Nautilus context-menu extension |
| `packaging/` | systemd units, distro and F-Droid packaging |
| `docs/` | Design document and protocol specification |

## Building

Requires Rust 1.96.0 (pinned in `rust-toolchain.toml`).

```sh
cargo build --workspace
cargo test --workspace
```

The Android build additionally needs the Android SDK (API 37), NDK r28+, and `cargo-ndk`.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
