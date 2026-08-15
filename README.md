# PenguinSync

Peer-to-peer sync between an Android phone and a GNOME/Wayland Linux desktop,
over the local network. No cloud, no account, no server anyone else runs.

**v0.1.0 — clipboard sync works, both directions.** File transfer and
notification mirroring are designed but not built. See
[`CHANGELOG.md`](CHANGELOG.md) for exactly what is and isn't in this release.

---

## Install

Download both files from the [latest release][releases].

### Linux

```sh
tar -xzf penguinsync-0.1.0-x86_64-linux.tar.gz
cd penguinsync-0.1.0-x86_64-linux
./install.sh
```

No root. Binaries go to `~/.local/bin`, the systemd user service to
`~/.config/systemd/user`, and the GNOME Shell extension to
`~/.local/share/gnome-shell/extensions`. The installer starts the daemon.

Then enable the extension — clipboard sync *from* the desktop needs it,
because GNOME implements no clipboard protocol anything outside the shell can
use:

```sh
gnome-extensions enable penguinsync-clipboard@penguinsync.org
```

On Wayland a freshly-installed extension is only picked up once the shell
reloads its extension list. If that command says the extension is unknown,
log out and back in.

### Android

Install `penguinsync-0.1.0.apk` on the phone. It is signed with a self-signed
key and distributed outside any store, so Android will warn you; that warning
is about provenance, not about this app in particular. Verify the download if
you like:

```sh
sha256sum -c penguinsync-0.1.0.apk.sha256
```

### Pair them

On the desktop:

```sh
penguinsync
```

Press `p`. Scan the QR code with the app's **Pair** screen. Compare the
fingerprint shown on both screens, then press `y`. The code refreshes itself
every 60 seconds, so a stale one is never the reason a scan does nothing.

### Use it

Copy on the desktop and it lands on the phone automatically.

Going the other way is one tap, by choice of Android's, not ours: a
background app is not allowed to read the clipboard. Copy on the phone, then
either use the **Quick Settings tile**, the **Send clipboard** action on the
ongoing notification, or the button in the app.

If the connection drops while the phone is in your pocket, check
**Settings → Background reliability** in the app — battery optimisation is
the usual culprit.

[releases]: https://github.com/ch7amada/penguinsync/releases/latest

## Requirements

| | |
|---|---|
| Desktop | GNOME on Wayland, Shell 50; systemd |
| Phone | Android 12 or newer |
| Network | Both devices on the same LAN |

## What it will do

- **Clipboard sync** — text, both directions, across all paired devices
- **File transfer** — right-click in Nautilus, or Android's share sheet
- **Notification mirroring** — Android notifications on your desktop, with
  two-way dismissal

## What it will never do

- SMS / call mirroring
- Contacts / calendar sync
- Remote input (mouse, keyboard)
- Media player control
- Screen mirroring
- Cloud accounts, or any server you don't run

Each is a whole subsystem. This list exists so scope discussions are short.

## Not yet, but planned

LAN-only is the current boundary — the transport is abstracted, and
Tailscale-style overlays work today without any code from us. Non-GNOME
compositors (Plasma, Sway, Hyprland, Niri, COSMIC) arrive in v0.2 via
`ext-data-control-v1`, which is ironically an easier target than GNOME.
Clipboard images, file-transfer resume, and notification inline reply are
deferred.

## Prior art

[GSConnect](https://github.com/GSConnect/gnome-shell-extension-gsconnect) and
[KDE Connect](https://kdeconnect.kde.org/) already do this, and do it well.
PenguinSync is a from-scratch project — building it is the point. Where they
have solved a hard platform problem, this design copies their solution
deliberately rather than rediscovering it.

## Architecture in one paragraph

Protocol, QUIC transport, TLS identity and all state machines are written once
in **Rust** and shared by the Linux daemon and the Android app (via UniFFI).
**Kotlin** owns the Compose UI and the platform glue Rust cannot reach —
discovery, foreground service, permissions, Wi-Fi locks, clipboard and
notification access. A minimal **GNOME Shell extension** exists solely because
Mutter implements no clipboard-manager protocol; a small **Nautilus
extension** provides the right-click send menu. Devices pair by QR with mutual
key pinning.

See [`docs/design.md`](docs/design.md) for the full design and the reasoning
behind every decision, and [`docs/protocol.md`](docs/protocol.md) for the
normative wire specification.

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
| `packaging/` | systemd unit, installers, release build scripts |
| `docs/` | Design document, protocol specification, release process |

## Building from source

Requires Rust 1.96.0 (pinned in `rust-toolchain.toml`).

```sh
cargo build --workspace
cargo test --workspace
```

The Android build additionally needs the Android SDK (API 37), NDK r28+, and
`cargo-ndk`; `:core`'s Gradle glue drives `cargo-ndk` and `uniffi-bindgen`
itself. See [`android/README.md`](android/README.md).

To build the release artifacts as the release workflow does:

```sh
./packaging/build-linux-bundle.sh     # → dist/penguinsync-<version>-<arch>-linux.tar.gz
./packaging/build-android-release.sh  # → dist/penguinsync-<version>.apk (needs a keystore)
```

Cutting a release is [`docs/RELEASING.md`](docs/RELEASING.md).

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
