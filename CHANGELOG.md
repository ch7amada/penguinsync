# Changelog

All notable changes to this project are recorded here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The section for each released version is what GitHub shows as that release's
notes — the release workflow reads it straight out of this file.

## [Unreleased]

### Added

- **File transfer.** Send a file (or several) from Nautilus's context menu on
  Linux, or from Android's share sheet or in-app picker; it lands
  auto-accepted in `$XDG_DOWNLOAD_DIR/PenguinSync/` (or the Android app's
  downloads folder), BLAKE3-verified on arrival, never overwriting an
  existing file of the same name. A large transfer runs on its own QUIC
  stream, so it can never stall a clipboard update. Progress and completion
  surface over D-Bus for the TUI to show. Protocol version bumped to 2.
  Android's share target and FFI wiring are written and Kotlin-compiles, but
  are **not yet verified on a real device** — Rust core, daemon, CLI/TUI and
  the Nautilus extension are (`cargo test`, `clippy`, `py_compile`).

## [0.1.0] - 2026-08-15

First release. **Clipboard sync works, in both directions, on GNOME/Wayland.**
File transfer and notification mirroring are designed but not built.

### Added

- **Pairing** — the desktop shows a QR code, the phone scans it, and both
  sides confirm the same fingerprint before any key is trusted. Keys are
  pinned mutually; unpairing from either side is unilateral and immediate.
  The code refreshes itself every 60 seconds, so a stale code on screen is
  never the reason a scan does nothing.
- **Encrypted transport** — QUIC with TLS 1.3 and self-signed Ed25519
  identities, over the local network only. No cloud, no relay, no account.
  Reconnects on its own after a drop, a network change, or a suspend.
- **Clipboard sync, phone → desktop** — from the app, from a Quick Settings
  tile, or from the ongoing notification's action. Android does not let a
  background app read the clipboard, so this tier is manual by design; the
  tile and the notification exist so it costs one tap rather than opening
  the app.
- **Clipboard sync, desktop → phone** — automatic, whenever you copy. Goes
  through a small GNOME Shell extension, because GNOME implements no
  clipboard-manager protocol that anything outside the shell could use.
- **Android app** — four screens (Devices, Pair, Settings, Debug), themed
  with Material 3 Expressive in light and dark, with an optional Material You
  mode that follows your wallpaper. A foreground service keeps the connection
  alive while the app is in the background.
- **Terminal UI** — `penguinsync` for pairing, device status and unpairing,
  plus non-interactive `pair` / `unpair` / `debug` subcommands for when a TUI
  is not handy.
- **Installers** — a Linux tarball that installs to your home directory with
  no root and sets up the systemd user service, and a signed APK.

### Known limitations

- **GNOME on Wayland only.** Other compositors arrive in 0.2 via
  `ext-data-control-v1`, which is an easier target than GNOME.
- **Text clipboard only.** Images are deferred.
- **Phone → desktop clipboard is manual.** This is an Android restriction,
  not an oversight.
- **No file transfer, no notification mirroring.** Both are specified in
  `docs/design.md` and neither is implemented.
- Tested against one phone paired with one desktop. Multi-device works by
  design but has not been exercised.
- The APK is signed with a self-signed key and distributed outside any store,
  so Android will warn about installing it.

[Unreleased]: https://github.com/ch7amada/penguinsync/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ch7amada/penguinsync/releases/tag/v0.1.0
