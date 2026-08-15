# PenguinSync — Design Document

**Status:** design agreed. M0–M2 implemented and verified on real hardware (walking skeleton, clipboard sync both directions, manual tier); see §9 for what's next.
**Date:** 2026-08-14
**Author:** ch7amada
**Supersedes:** `mynotes.md` (kept for provenance; where the two disagree, this document wins)

---

## 1. What PenguinSync is

A peer-to-peer sync tool between an Android phone and a GNOME/Wayland Linux desktop, over the local network, with no cloud and no server anyone else runs. Clipboard sync, file transfer, and notification mirroring.

It is a **from-scratch, learning-driven project**. GSConnect and KDE Connect already occupy this space; PenguinSync is not trying to displace them, and where they have solved a hard platform problem well, this design copies their solution deliberately rather than rediscovering it.

**Distribution:** open source, GPL-3.0-or-later, published on GitHub, targeting F-Droid. **Not** targeting the Play Store — that decision buys real technical freedom on Android and is load-bearing in several places below.

**Topology:** one Linux machine ↔ N Android devices. Not a mesh. No Linux↔Linux.

### 1.1 Non-goals (published in the README)

These will never be part of PenguinSync:

- SMS / call mirroring
- Contacts / calendar sync
- Remote input (mouse, keyboard)
- Media player control
- Screen mirroring
- Cloud accounts, or any server the user does not run

Each is a whole subsystem, and a published non-goals list turns every "why not add X?" into a one-line link instead of a debate.

### 1.2 Deliberately deferred (not non-goals — just later)

- Off-LAN operation. LAN-only is the shipping boundary; the transport is abstracted so a relay or overlay *could* slot in, and "just use Tailscale" works today without a line of code from us.
- Non-GNOME compositors (Plasma, Sway, Hyprland, Niri, COSMIC) — see §4.4.1, this is cheaper than GNOME and is the headline feature of v0.2.
- Clipboard images / rich MIME types.
- File transfer resume, directory transfer.
- Notification inline reply.

---

## 2. Verified environment

Facts about the development machine, checked rather than assumed:

| Thing | Value |
|---|---|
| OS | Fedora Linux 44 Workstation |
| Desktop | GNOME Shell **50.4**, Wayland session |
| Nautilus | 50.2.2 |
| `nautilus-python` | **not installed** (available as `nautilus-python-4.1.0-2.fc44`) |
| Avahi | active **and** enabled |
| firewalld | active; `FedoraWorkstation` zone already opens 1025–65535 TCP+UDP |
| Rust | rustc / cargo **1.96.0** |
| Rust Android targets | all four installed (`aarch64`, `armv7`, `i686`, `x86_64`) |
| `cargo-ndk` | **4.1.2** |
| Android SDK | `~/Android/Sdk` — platforms to **android-37.1**, build-tools **37.0.0** |
| Android NDK | **28.2.13676358** (gives 16 KB page alignment for free) |
| Android Studio | not installed |
| JDK | OpenJDK 25 |
| `adb` | present; **no device currently connected** |
| git | not initialised in this directory yet |

Note the firewall observation applies to *this* machine's default zone. Other users' distros and zones will need documented firewall guidance.

---

## 3. Platform constraints that shaped this design

These were researched against primary sources (AOSP source, Mutter source, GNOME GitLab, KDE Connect / GSConnect source, vendor docs) rather than taken from blog posts. They are the "why" behind several decisions that would otherwise look arbitrary.

### 3.1 Android will not let a background app read the clipboard

The authority is `ClipboardService.clipboardAccessAllowed()` in AOSP. Read access requires **one of**: holding `READ_CLIPBOARD_IN_BACKGROUND`, **being the default IME**, or **having window focus**. Consequences:

- **`READ_CLIPBOARD_IN_BACKGROUND` is `signature|role`.** `adb shell pm grant` cannot give it to a third-party app.
- **AccessibilityService does not work.** No exemption exists in the source. This is the most commonly suggested workaround and it is simply wrong — and Play policy would punish it besides.
- **A foreground service does not count as focus.** Ever.
- **Listener callbacks are gated by the same check** — a background app receives *silence*, not an error or an empty clip.
- **Reads fail on a locked device**, unconditionally.
- **`com.android.shell` holds the permission.** This is the crack in the wall: a **Shizuku** user-service running as shell UID passes the check outright, with working change listeners.
- **Writing is unrestricted.** `setPrimaryClip()` works from anywhere. Linux→Android needs no permission at all.

For reference, KDE Connect's workaround is to run `logcat` and watch for the system's own `E/ClipboardService: Denying clipboard access to org.kde.kdeconnect…` line — learning that the clipboard changed by observing its own denial — then flash an invisible activity to steal focus for a frame. It needs `pm grant READ_LOGS` plus a `SYSTEM_ALERT_WINDOW` appop, it breaks when the log format changes (it already did at Android 16), and it steals focus on every copy. **We are not doing this.** See §7.1.

### 3.2 Mutter implements no clipboard-manager protocol

Verified by reading mutter's `src/meson.build` on `main` (post-50.4): **neither `wlr-data-control-unstable-v1` nor `ext-data-control-v1` is in the protocol list.** Four separate requests (2019 → 2025) were closed, most within a day. This is a settled position, not a backlog item — do not design around it landing.

- `wl-paste --watch` **fails on GNOME** — watch mode is implemented only over data-control.
- The **portal route works but is unshippable**: a clipboard-only `RemoteDesktop` session still registers a remote-access handle, lighting up the permanent **"Stop Screen Sharing"** indicator in the top panel. Verified in mutter's `meta-remote-desktop-session.c` and gnome-shell's `remoteAccess.js`.
- The working route is what GSConnect, GPaste and Clipboard Indicator all use: **a GNOME Shell extension** watching `global.display.get_selection()`'s `owner-changed` signal and exposing D-Bus.

### 3.3 Android 17 gates all LAN traffic behind a runtime permission

`ACCESS_LOCAL_NETWORK` is mandatory for `targetSdk ≥ 37`. It gates outgoing TCP, **incoming TCP**, UDP unicast/multicast/broadcast in both directions, and `.local` mDNS resolution. **Failures are silent and confusing** — blocked TCP presents as a *timeout*, blocked UDP as `EPERM`.

There is a permission-free escape hatch: addresses obtained through `NsdManager` with `DiscoveryRequest.FLAG_SHOW_PICKER` (a system-mediated device picker) are exempt. It fits a one-shot share flow, not a daemon that must silently reconnect.

### 3.4 There is no viable QUIC for Kotlin

- **Netty's QUIC incubator was archived** (2026-05) and merged into Netty 4.2 — but **no Android binaries are published**. Building it yourself means cross-compiling BoringSSL + Cloudflare **quiche** (which is Rust) on every F-Droid build, at ~5–7 MB per ABI.
- **Cronet is disqualified twice**: it is an HTTP client with no listening API at all, and it ships under the *Android SDK License* — not free, so F-Droid cannot take it.
- **kwik** is alive and LGPL, but its author's own README says its TLS stack *"is not security tested nor reviewed by security experts."*

Against that, a measured build of `quinn` + `rustls(ring)` + `tokio` + `uniffi` for Android came out at **1.66 MB (arm64) / 1.07 MB (armv7)** stripped — roughly 4× smaller than Cronet. This is why §4.2 exists.

### 3.5 Assorted platform facts that changed specific decisions

- **Android 15 redacts OTP content** from notification listeners unless the app holds a **CompanionDeviceManager association**.
- **Play Protect auto-blocks internet-sideloaded APKs** that declare `NOTIFICATION_LISTENER`. Installing via the F-Droid *client* avoids the classification.
- **GNOME Shell does not support notification inline reply.** `GetCapabilities()` returns exactly `actions, body, body-markup, icon-static, persistence, sound`. `inline-reply` is a KDE vendor extension. GSConnect gets reply only by subclassing private Shell JS — which broke at GNOME 45 and again at 48.
- **GNOME does render notification action buttons**, supports `replaces_id` for atomic updates, and `CloseNotification` + the `NotificationClosed` signal make **two-way dismissal sync** work.
- **`rustls` now defaults to `aws-lc-rs`**, which fails to cross-compile for `aarch64-linux-android` (open upstream issue).
- **UniFFI has no cancellation support.** A cancelled Kotlin coroutine will not stop the Rust work.
- **`MulticastLock` can only be acquired from Java/Kotlin**, and background multicast reception is unreliable on many vendors' Wi-Fi firmware when the screen is off.
- **Nautilus 49 bumped its GIR namespace 4.0 → 4.1**, breaking every extension that hardcoded `gi.require_version("Nautilus", "4.0")`. GSConnect survived by never calling it.
- **`quinn` has no Android CI** and a known GSO throughput regression there.

---

## 4. Architecture

### 4.1 Component map

```
┌─────────────────────────── Linux ───────────────────────────┐
│                                                             │
│  gnome-shell process                                        │
│   └── penguinsync@… (GJS extension)  ── D-Bus ──┐           │
│         watches Meta selection, reads/writes     │           │
│                                                  ▼           │
│  penguinsyncd (systemd --user)  ◄── D-Bus ── penguinsync    │
│   └── crates: net → protocol                    (TUI + CLI) │
│                       ▲                                      │
│  nautilus-python ext ─┘ (D-Bus ObjectManager)               │
└──────────────────────────────┬──────────────────────────────┘
                               │  QUIC over LAN (mDNS + QR pairing)
┌──────────────────────────────┴──────────────────────────────┐
│                          Android                             │
│  :app  (Compose UI, FGS, NsdManager, permissions, locks)    │
│   └── UniFFI ──► :core (libpenguinsync.so)                  │
│                    crates: ffi → net → protocol             │
└─────────────────────────────────────────────────────────────┘
```

Five codebases: **Rust** (the bulk), **Kotlin** (Android UI + platform glue), **GJS** (Shell extension), **Python** (Nautilus extension), and packaging.

### 4.2 The shared Rust core

Protocol, transport, crypto, identity, and all state machines are written **once** in Rust and consumed by both the Linux daemon and the Android app.

This was the highest-blast-radius decision in the design, and the counterargument is on the record: the FFI boundary is where the bugs will live, UniFFI's missing cancellation is a permanent tax on an app built around a long-lived interruptible connection, Rust panics surface on Android as unattributable native crashes, and `quinn` has no Android CI. Two independent implementations would genuinely be faster to first packet.

It was chosen anyway because that advantage is *Phase-1-only* and inverts the moment encryption stops being a checkbox — and because the alternative doubles the protocol work of every subsequent phase. The FFI tax is fixed, bounded, and has three F-Droid precedents to copy (**Element X**, which is this exact shape; **Delta Chat**; **RustDesk**).

#### Crate layout (Cargo workspace)

| Crate | Contains | Depends on |
|---|---|---|
| `protocol` | **Sans-I/O.** Message types, pairing state machine, trust decisions, reconnect logic, echo suppression, transfer state. Events in → actions out. Never learns what a socket is. | serde, postcard |
| `net` | quinn endpoint, rustls config + custom verifier, mDNS, file I/O; drives `protocol` | `protocol`, tokio, quinn, rustls |
| `ffi` | UniFFI surface, Android-specific concerns | `net` |
| `daemon` | `penguinsyncd` — D-Bus service, systemd integration, GNOME extension client | `net`, zbus |
| `cli` | `penguinsync` — TUI + CLI, D-Bus client | zbus, ratatui |

`protocol` being pure is what makes Q24's testing strategy real rather than aspirational: pairing, reconnect, echo suppression and transfer state are all unit-testable with no sockets, no emulator, and no phone. It is also what keeps the FFI surface small — `ffi` wraps `net`, and `protocol` never crosses the boundary.

#### FFI surface

Deliberately narrow. A single `PenguinSyncCore` handle object (`start`, `stop`, `pair`, `sendFile`, …). Events flow Rust → Kotlin through a UniFFI callback interface, wrapped once per stream in a Kotlin `callbackFlow`.

**Every long-lived operation returns a handle with an explicit `cancel()`**, and every `callbackFlow` wrapper ends with `awaitClose { handle.cancel() }`. UniFFI will not propagate coroutine cancellation; getting this wrong leaks tokio tasks inside a process Android is trying to kill.

#### Build glue

Explicit, not plugin-driven: `cargo-ndk` invoked from a Gradle task, `uniffi-bindgen` generating Kotlin into a source set, `.so` files copied into `jniLibs`. Roughly forty lines of Gradle that you own and can explain to an F-Droid maintainer. Gobley's plugin would automate this but is at v0.3.7 and would sit in the critical path of every build including F-Droid's — a bad pairing with reproducibility.

**Crypto backend: `ring`**, via `default-features = false` on `rustls`, on **both** platforms. `aws-lc-rs` does not cross-compile for arm64 Android, and `ring` additionally avoids a CMake/NASM-heavy build that would complicate reproducibility. *This must be commented in `Cargo.toml`* or someone will eventually "clean up" the `default-features = false`.

### 4.3 Linux: daemon, TUI, CLI

Two binaries from one workspace.

**`penguinsyncd`** — systemd **user** service (clipboard is per-session), `WantedBy=default.target`, `Restart=on-failure`. Always running; not D-Bus-activated and not idle-exiting, because a clipboard daemon that idle-exits is not a clipboard daemon.

**D-Bus interface** — bus name `org.penguinsync.Daemon1`, root object `/org/penguinsync/Daemon`, implementing `org.freedesktop.DBus.ObjectManager` with **one object per paired device** exposing a `Device` interface (`Name`, `Connected`, `SendFiles(as uris)`, …). Clients call `GetManagedObjects()` then follow `InterfacesAdded` / `InterfacesRemoved`. This is the shape GNOME clients already know how to consume, and it is what makes the Nautilus extension ~120 lines of borrowed boilerplate.

**`penguinsync`** — `ratatui` TUI for status, device list, pairing QR display, and confirm/revoke; plus non-interactive CLI verbs (`penguinsync send file.pdf`, `penguinsync debug`). Both are thin clients over a shared D-Bus client module. Room to grow into a full dashboard with transfer progress at M4, but not before the protocol works.

The pairing view shows a live countdown and re-mints the code the moment its `TOKEN_TTL` runs out, so the QR on screen is always the one the daemon will accept. Refreshing exactly *at* expiry rather than early is deliberate: the daemon holds a single token, so an early refresh would discard one that a phone might be redeeming right then. It never refreshes while a confirmation prompt is open, for the same reason.

**Config vs state, strictly separated by owner:**

| | Path | Format | Owner |
|---|---|---|---|
| Config | `$XDG_CONFIG_HOME/penguinsync/config.toml` | TOML | user, hand-editable |
| State | `$XDG_DATA_HOME/penguinsync/devices.json` | JSON | daemon, never hand-edited |
| Private key | `$XDG_DATA_HOME/penguinsync/` | 0600 file | daemon |

Behind a trait, so swapping to SQLite when history arrives is an implementation change.

### 4.4 Linux: the GNOME Shell extension

Unavoidable (§3.2), and therefore kept **as small as humanly possible**. It runs inside the `gnome-shell` process, where a bug hangs the user's desktop.

Mirrors GSConnect's `src/shell/clipboard.js`:

- Watch: `global.display.get_selection()` → `owner-changed`, filtered to `Meta.SelectionType.SELECTION_CLIPBOARD`
- Read: `selection.transfer_async(…)`
- Write: `Meta.SelectionSourceMemory.new(mimetype, bytes)` → `selection.set_owner(…)`
- Expose D-Bus: `GetMimetypes()`, `GetText()`, `SetText(s)`, `GetValue(s)`, `SetValue(ay, s)`, signal `OwnerChange()`

The Rust daemon uses `zbus` to `watch_name` it, subscribe to `OwnerChange`, and call the methods. **All logic stays in Rust; the GJS does selection access and byte transfer and nothing else.**

**The daemon must run fine without it** — file transfer and notification mirroring don't need clipboard access. When the extension is absent, clipboard is reported as unavailable in the TUI and everything else works. A daemon that refuses to start because an extension is missing is a support nightmare.

Cost to accept, openly: the GJS/Shell API breaks roughly every six months, `shell-version` must be revalidated each cycle, and the extension needs review on extensions.gnome.org.

#### 4.4.1 Other compositors

`net` exposes a `ClipboardBackend` trait (`watch() -> Stream<ClipEvent>`, `read(mime)`, `write(mime, bytes)`) with backend probing at startup. **v1 ships one backend** (the GNOME extension).

An `ext-data-control-v1` backend is the v0.2 headline: ~300 lines, or free via `wl-clipboard-rs`, and it unlocks Plasma, Sway, Hyprland, Niri and COSMIC with **no extension, no indicator, and no maintenance treadmill** — ironically an easier target than GNOME. It is deferred only because every compositor supported is a compositor you're expected to test on.

### 4.5 Linux: the Nautilus extension

`nautilus-python`, modeled directly on GSConnect's `nautilus-gsconnect.py`. Shipped as a separate `penguinsync-nautilus` subpackage so headless installs don't pull Python.

- Caches devices from the daemon's `GetManagedObjects()` + `InterfacesAdded`/`InterfacesRemoved`
- 0 connected devices → item shown **greyed with "No devices connected"** (a menu item that vanishes is a menu item users think is broken)
- 1 device → flat `Send to <name>`
- ≥2 devices → `Send with PenguinSync ▸` submenu

**Two mandatory version-proofing rules**, both stolen from GSConnect and both non-negotiable:

1. **Never call `gi.require_version("Nautilus", …)`** — just `from gi.repository import Nautilus`
2. **Accept `*args` and read the file list as `args[-1]`**

These two lines are why GSConnect sailed through the Nautilus 49 GIR bump that broke half the ecosystem.

**Fallback:** a `NoDisplay=true` + `MimeType=all/all` `.desktop` file, so *Open With ▸ PenguinSync* works even where `nautilus-python` is absent; the app then shows its own device picker.

A native C/Rust extension was considered and rejected — the only Rust bindings are GTK3-era and last released in 2022.

### 4.6 Android app

**Baseline:** minSdk **31** (Android 12), targetSdk **37**, Compose + **Material 3 Expressive**, coroutines/Flow. Hilt and DataStore are still unused as of M2 — there is no dependency graph worth injecting and no preference bigger than a boolean; both arrive when M3's per-device state does. Room only when there is history to store.

**Theming:** the palette is generated from a single seed, `#01579B` — the same deep ocean blue as the launcher icon background — through Material's *expressive* scheme variant, which rotates secondary and tertiary away from the primary hue. That rotation is load-bearing rather than decorative: tertiary lands on green, so "connected" is a real theme role instead of a hardcoded literal that breaks in dark mode. Amber is supplied by hand for "reconnecting", which Material has no role for. Dynamic colour (Material You) is offered as a Settings toggle and is **off** by default, so the app shows its own identity unless asked not to. The expressive API surface exists but is `internal` in material3 1.4.0, so `:app` pins **material3 1.5.0-alpha**; it is the only pre-release artifact in the graph, and the pin goes away when 1.5.0 stabilises.

**Gradle modules:** `:app` (UI, services, permissions) and `:core` (generated UniFFI bindings + `jniLibs`). Fine-grained modularization is unnecessary — the Rust boundary already buys most of what it would.

**Shipped ABIs:** `arm64-v8a` + `armeabi-v7a` in release; `x86_64` in debug only, so the emulator works when the phone isn't around.

**Screens** (a simple `NavHost`, no nesting): **Devices** (list, status, pair) · **Pair** (QR scanner) · **Settings** (per-device toggles, notification allow-list) · **Debug** (recent protocol events).

The Debug screen is not optional polish. When the phone won't reconnect and it isn't plugged into your laptop, Logcat is unavailable and it is your only instrument.

**Foreground service:** type **`connectedDevice`** (`FOREGROUND_SERVICE_CONNECTED_DEVICE` + `CHANGE_WIFI_MULTICAST_STATE` as the runtime prerequisite). Explicitly **not `dataSync`**, which Android 15+ caps at 6 h per 24 h and then kills.

**Kotlin owns, Rust cannot:**

- `NsdManager` discovery and registration
- `ACCESS_LOCAL_NETWORK` permission flow
- `WifiManager.MulticastLock` and `WifiLock(WIFI_MODE_FULL_LOW_LATENCY)`
- `ConnectivityManager.NetworkCallback` → tells Rust to rebind the endpoint
- Battery-optimization exemption request
- CompanionDeviceManager association
- Clipboard read/write, notification listener

**Rust owns:** everything else, including **all persistent state** — device identity (`rcgen`), pinned peer keys, paired-device list — written to the app-private directory that Kotlin passes in at startup. Kotlin's DataStore holds *only* UI preferences. One state machine, one persistence format, one implementation of "am I paired with this device."

**Share target:** PenguinSync registers as a system share target, so it appears in Android's share sheet from any app. Plus an in-app file picker. The share sheet is the only file-sending affordance Android users actually reach for; absent from it, the feature effectively does not exist.

**Onboarding** asks, in order, with honest explanations: notification listener access (only when the feature is enabled), local network permission, battery-optimization exemption, CDM association (only when notification mirroring is enabled). Deliberately *not* all at once during pairing — first-run should not be a permission gauntlet.

---

## 5. Protocol

### 5.1 Identity

Every device generates an **Ed25519 keypair on first run**, wrapped in a self-signed X.509 certificate for QUIC/TLS. `DeviceId` = **SHA-256 fingerprint of the public key**, rendered as a short human-comparable string.

Private key storage relies on the OS sandbox for now — 0600 file on Linux, app-private directory on Android. Platform keystore integration (Secret Service / Android Keystore) is Phase 3 hardening, done on both platforms at once.

### 5.2 Pairing

Linux shows a QR; Android scans it. Trust is established **in both directions from one scan**:

1. QR encodes a `penguinsync://pair?…` URI carrying: protocol version, device name, `DeviceId`, public-key fingerprint, candidate `ip:port` pairs, and a **single-use pairing token valid for 60 s**
2. Android scans, connects (candidate addresses first, so pairing does not depend on mDNS working), and **pins Linux's key**
3. Android presents its own key over the encrypted channel
4. Linux's TUI displays Android's fingerprint and **asks for explicit confirmation**

The candidate addresses in the QR matter: the first connection should never depend on discovery.

### 5.3 Transport and discovery

**QUIC** via `quinn`. A single `quinn::Endpoint` both accepts and initiates — which is exactly this topology, and something neither Netty nor kwik does as cleanly.

**Roles:** Linux listens, **Android always dials.** Android is the side that changes networks, sleeps and gets new IPs, so it is the natural initiator, and every reconnect is driven by the side that actually knows its network state changed.

**Discovery:**

- **Linux** registers `_penguinsync._udp` with the **system Avahi daemon over D-Bus** (already running here, and it avoids two processes fighting over UDP 5353). TXT records: protocol version, device name, `DeviceId`.
- **Android** discovers via **`NsdManager`** in Kotlin, passing resolved addresses down to Rust. `NsdManager` was rewritten into the Connectivity mainline module, needs no multicast lock, and is the only route to the permission-free `FLAG_SHOW_PICKER` hatch should we ever want it.
- **Reconnect is unicast to a cached address, not mDNS.** Background multicast reception is unreliable on many vendors' Wi-Fi firmware with the screen off. mDNS is for discovery *while the user is present*; steady-state reconnection must not depend on it.

**Streams:**

- **One long-lived bidirectional control stream** — clipboard, notifications, transfer metadata. Ordered, small, latency-sensitive.
- **One unidirectional stream per file transfer**, carrying transfer ID and offset in its header.

A 4 GB video cannot stall a clipboard update. That is the entire reason QUIC was chosen. Datagrams may be added later for presence/heartbeat; not now.

**Keepalive and reconnect:**

- QUIC keepalive at **~20 s** (NAT and AP power-save otherwise drop the path silently)
- `WifiLock(WIFI_MODE_FULL_LOW_LATENCY)` held while connected
- Exponential backoff, capped at ~60 s; timers use `setExactAndAllowWhileIdle()`
- Kotlin's `NetworkCallback` triggers an endpoint rebind on network change — QUIC connection migration handles path changes, but an SSID switch generally needs a rebind

### 5.4 Wire format and versioning

**`postcard` + `serde`.** With both ends compiled from the same struct definitions, there is no cross-language schema to maintain and no reason left to pay for a self-describing format. Debuggability is recovered by making `protocol` types `Debug` and logging **decoded** messages via `tracing`, rather than reading bytes off a capture.

**Versioning:** a single `u16` protocol version, advertised in the mDNS TXT record **and** re-checked in the handshake. **Strict reject on mismatch**, with a clear error surfaced in both UIs. No compatibility guarantees before 1.0 — with only your own devices in play, a lockstep bump costs nothing, and the strict-reject error message is what saves debugging time.

**`docs/protocol.md` is the normative spec.** Every wire change edits that file in the same commit that changes the code. Forward compatibility (ignore-unknown-message-types) is the right end state but is premature while the message set churns weekly.

---

## 6. Feature design

### 6.1 Clipboard

**Content rules:**

- **`text/plain` only** in v1. The MIME plumbing exists in the protocol from day one; only text is accepted.
- **Hard size cap** (~100 KB)
- **Clips marked `EXTRA_IS_SENSITIVE` are never synced.** A sync tool that silently broadcasts password-manager clips across the LAN is a security incident waiting to happen, and Android hands us the flag for free.
- **Echo suppression by content hash** — mandatory, not optional, because clipboard broadcasts to all connected devices.

**Fan-out:** clipboard **broadcasts** to all connected paired devices; files are **targeted** at one. Per-device direction toggles in config (`send` / `receive` / `both` / `off`) — "receive from my phone but never send to the shared tablet" is a real requirement, and retrofitting direction control into a broadcast design is painful.

**Linux side:** GNOME Shell extension (§4.4).

**Android side — two tiers, and only two:**

| Tier | Mechanism | Setup |
|---|---|---|
| **Baseline** (everyone) | Write is free. Read is **manual, one tap**: QS tile + notification action + in-app button, each launching a transparent activity that reads in `onWindowFocusChanged(true)` and finishes | none |
| **Shizuku** (power users) | Shell-UID user-service calling `IClipboard` as `com.android.shell` → **true background read with working change listeners** | Shizuku + ADB pairing |

Behind a `ClipboardReader` interface, so a third tier stays possible.

**Explicitly rejected:** AccessibilityService (does not work, §3.1); the `READ_LOGS` + logcat-scraping route (strictly worse than Shizuku — fragile log parsing that already broke once, a focus-stealing window on *every* copy, and built on a background-activity-launch mechanism Android 17 is actively hardening); a bundled IME (that's a different product, and it makes us responsible for everything the user types).

**Known limitation, documented up front:** clipboard sync does not work while the phone is locked. That is a platform fact on both sides, and users who discover it themselves will file it as a bug.

### 6.2 File transfer

- **Auto-accept from paired devices, no prompt.** Pairing *is* the trust decision; re-asking afterwards is security theatre that trains people to click Accept. Per-device configurable.
- Destination `$XDG_DOWNLOAD_DIR/PenguinSync/`
- Name collisions get a `(1)` suffix — **never overwrite on receive**
- Desktop notification on arrival with **Open** and **Show in Files** actions
- **BLAKE3 integrity check** on arrival
- Single and multi-file selection. **No resume** in v1 — an interrupted transfer is discarded and retried. But transfers carry an **ID and an offset field from day one** even though the offset is always 0, so resume is a protocol addition rather than a protocol break.
- Directory transfer deferred (or handled by tarring on the fly)

**Send surfaces:** Nautilus context menu (§4.5) on Linux; system share sheet on Android.

### 6.3 Notification mirroring

Android → Linux, read-only plus dismissal.

- `NotificationListenerService`, onboarded via `ACTION_NOTIFICATION_LISTENER_SETTINGS`, state checked with `NotificationManagerCompat.getEnabledListenerPackages()`
- Run the component disable/enable + `requestRebind()` dance on app start — the listener is known to fail to rebind after app updates
- **CompanionDeviceManager association requested when the user enables mirroring** — without it, Android 15+ redacts OTP content, and mirrored 2FA codes are the single most useful thing this feature does
- **Per-app allow-list, default-deny.** Mirroring everything unfiltered is unusable within a day.
- **App list is built from observed packages** — never enumerate, so no `QUERY_ALL_PACKAGES`. Icons come from the notification itself.
- **Dismissal sync both ways**: `NotificationClosed`(reason 2) on Linux → `cancelNotification(key)` on Android; `onNotificationRemoved` with its `REASON_*` code distinguishes "user dismissed on phone" from "we dismissed it", so dismissals don't echo
- **Action buttons are forwarded** — GNOME renders them natively. The `default` action key is reserved for "open on phone".
- **No inline reply.** GNOME cannot do it without monkey-patching private Shell JS (§3.5), and the Shell extension already carries a maintenance burden for clipboard, which is load-bearing. The KDE `inline-reply` path (~30 lines, capability-checked, no-op on GNOME) comes with Plasma support.

**Wire shape** mirrors KDE Connect's proven design: `key`, title, text, `isClearable`, `silent`, `isCancel`, `requestReplyId`, conversation history, and the app icon as a **PNG payload plus a hash** so Linux caches one icon per app rather than receiving it with every notification. `requestReplyId` stays in the schema despite reply being deferred — the field costs nothing and its later absence would be a protocol break.

**Distribution caveat, documented not architected around:** Play Protect blocks internet-sideloaded APKs declaring `NOTIFICATION_LISTENER`. Installing through the F-Droid client avoids the classification. Splitting into a second APK to dodge a warning dialog would mean two app identities, two pairing flows, and an IPC boundary between our own apps.

---

## 7. Trust and security model

**Threat model:** an attacker on the same LAN. Not a compromised device, not a hostile OS, not a nation-state.

- **Transport is encrypted from commit one** — QUIC gives it for free. There was never a "plaintext first" phase.
- **Trust is pinned on pairing.** Both directions, from one QR scan plus one confirmation.
- **Unpair is unilateral and immediate** on the side that initiates. You must be able to revoke a lost phone from the laptop alone, without the phone's cooperation — that is the entire point.
- **Key change is a hard reject** with a prominent warning, requiring manual re-pair. The benign cause (app reinstall) is rare and costs one QR scan; the malicious cause is exactly what pinning exists to catch. Never make the security-critical prompt the one users see routinely.
- **Pins never expire.**
- **Sensitive clipboard content is never synced** (§6.1).
- Certificate verification is a **custom `ServerCertVerifier`** doing SPKI pinning — no system root store is consulted, and `rustls-native-certs` / `webpki-roots` are deliberately not dependencies.

**Phase 3 ("Trust & identity") is where this becomes a real subsystem:** pairing UX, revocation flows, key rotation, and platform keystore integration on both sides.

---

## 8. Engineering practice

**Testing** — `protocol` is sans-I/O and unit-tested with zero sockets. Integration tests run two `net` instances over loopback QUIC in one test binary. The Android side is a thin driver, so most correctness is provable on the desktop with `cargo test`.

**Observability** — `tracing` throughout the Rust core, with a **journald layer on Linux**, an **Android log layer to Logcat**, and a subscriber that pushes recent events over the callback interface into the in-app Debug screen. One instrumentation vocabulary, both platforms. `panic = "abort"`, with a breadcrumb logged before any panic-capable path — Rust panics arrive on Android as unattributable native crashes, and the last log line is often the only clue.

Add a **connectivity self-check** to the debug tooling that distinguishes "local network permission denied" from "peer not found". Android 17's blocked traffic presents as a plain timeout; without this, every diagnosis starts from the wrong hypothesis.

**CI** (GitHub Actions) — Rust job (fmt, clippy `-D warnings`, test, build) + Android job (assembleDebug, unit tests) on every push. Clippy-as-error from commit one is the cheapest Rust teacher available. Release automation waits until there is a release.

**F-Droid reproducibility, from day one, not retrofitted:**

- `rust-toolchain.toml` pinning the exact version
- `CARGO_HOME` and `CARGO_TARGET_DIR` fixed to stable absolute paths (they leak into the `.so`)
- `SOURCE_DATE_EPOCH` set
- `--remap-path-prefix` stripping build paths
- CI builds twice and diffs

Retrofitting means bisecting which of six environment leaks broke it with no known-good baseline. Each item is a one-line change while the build is still trivial. Element X's setup is the reference.

**Repo** — monorepo on GitHub, GPL-3.0-or-later throughout.

```
penguinsync/
├── crates/{protocol,net,ffi,daemon,cli}/
├── android/{app,core}/
├── gnome-extension/
├── nautilus/
├── docs/{design.md,protocol.md}
└── packaging/
```

---

## 9. Roadmap

| Milestone | Contents | Proves |
|---|---|---|
| **M0 — Walking skeleton** ✅ | Pair via QR, connect over QUIC, exchange `Ping`/`Pong`, survive a Wi-Fi drop and reconnect, visible in both TUI and Android UI. No clipboard, no files. | Pairing, QUIC across the FFI boundary, reconnect — the three riskiest unknowns, isolated from platform combat. **Done when you can pull the Wi-Fi, walk away, come back, and it reconnects untouched.** |
| **M1 — Clipboard: Linux → Android** ✅ | GNOME Shell extension (read side), Android write path | The easy direction, which needs no Android permissions at all |
| **M2 — Clipboard: Android → Linux, manual** ✅ | QS tile, notification action, in-app button; extension write side | Round-trip clipboard for everyone, no setup |
| **M3 — Clipboard: Shizuku tier** | Shell-UID helper, automatic background read | The power-user experience |
| **M4 — File transfer** | Transfer streams, BLAKE3, auto-accept, Nautilus extension, Android share target, TUI progress | The transport under real load; both send surfaces |
| **M5 — Notification mirroring** | Listener, allow-list, icon cache, dismissal sync, action buttons, CDM association | |
| **v0.2** | `ext-data-control` backend → Plasma, Sway, Hyprland, Niri, COSMIC | |

Clipboard is split by direction because that asymmetry is the real structure of the work: Linux→Android is nearly free, Android→Linux is the hardest problem in the project.

*Considered and not chosen:* moving file transfer ahead of clipboard entirely. Files are unobstructed on both platforms and would exercise the transport, pairing, both send surfaces and the transfer state machine with no platform combat at all. If M0 lands and appetite for the Android clipboard fight is low, **swapping M4 ahead of M1 is a clean, low-cost reordering** — nothing in M1–M3 is a prerequisite for it.

---

## 10. Open items to verify during implementation

Not design gaps — facts that need a device, and are cheap to check at the right moment:

1. **Does `getApplicationLabel()` resolve for a package observed only via a notification, without `QUERY_ALL_PACKAGES`?** Research flagged this as unverified; expectation is that it fails. If it does, the notification allow-list shows raw package names like `com.whatsapp` until an icon and label arrive with a notification. ~10 minutes on-device. Decides whether §6.3's filtering approach needs a fallback.
2. **`quinn`'s Android GSO throughput regression** — pin `quinn-udp` and measure real throughput on hardware during M4, when large files first matter.
3. **`aws-lc-rs` arm64 failure** — the upstream issue is verified open but was not reproduced locally. The `ring` path is verified working; this only matters if someone proposes switching back.
4. **Whether `ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS`** (deep-link to the app's own row) behaves as expected on API 31+.
5. **`nautilus-python` install path on Fedora 44** — confirm before packaging.
6. No Android device is currently connected to this machine (`adb devices` is empty); M0 needs one.

---

## 11. Decision log

Every decision, in the order it was settled. Rationale is in the sections above.

**Foundations** — 1 from-scratch learning project, not a KDE Connect replacement · 2 open source, F-Droid, no Play Store · 3 one Linux ↔ N Android · 4 Rust · 5 two binaries, Cargo workspace · 6 D-Bus IPC via zbus · 7 QUIC + mDNS + QR · 8 encrypted transport day one; Phase 3 renamed "Trust & identity" · 9 clipboard + files + notification mirroring; typed versioned messages over multiplexed streams · 10 Android Studio + physical device

**Project shape** — 11 GPL-3.0-or-later · 12 monorepo on GitHub · 13 Ed25519 self-signed cert, DeviceId = pubkey fingerprint · 14 QR + TUI confirmation, two-sided trust, 60 s single-use token · 15 Avahi over D-Bus, `_penguinsync._udp` · 16 always-on systemd user unit, `org.penguinsync.Daemon1` · 17 TOML config + JSON state, split by owner · 18 TUI for status/pairing + CLI verbs · 19 minSdk 31, Compose/M3/Hilt/DataStore

**Reach and practice** — 20 LAN-only, transport abstracted · 21 Linux listens, Android dials · 22 `u16` version, strict reject, `docs/protocol.md` normative · 23 M0 = ping-level walking skeleton · 24 sans-I/O core + loopback integration tests · 25 tracing → journald/Logcat + in-app debug screen · 26 CI now, release automation later

**Later-phase semantics** — 27 unilateral unpair, hard reject on key change, pins never expire · 28 clipboard broadcasts, files targeted, per-device direction toggles · 29 auto-accept files into `$XDG_DOWNLOAD_DIR/PenguinSync/`, `(1)` suffix · 30 integrity-checked, no resume, ID+offset reserved · 31 Nautilus submenu of connected devices, greyed when none · 32 mirror + dismissal sync; allow-list default-deny

**Android surface** — 33 four screens incl. Debug · 34 system share target + in-app picker · 35 non-goals published · 36 Android wired for i18n, Rust English-only

**Post-clipboard-research** — 37 baseline manual + Shizuku only; no AccessibilityService, no `READ_LOGS`, no IME · 38 text/plain, size-capped, sensitive excluded, hash echo suppression · 39 Shell extension required for clipboard, daemon degrades cleanly · 40 `ClipboardBackend` trait now, data-control backend in v0.2 · 41 targetSdk 37 + `ACCESS_LOCAL_NETWORK` · 42 battery exemption in onboarding; locked-phone limitation documented · 43 clipboard split by direction

**Post-integration-research** — 44 D-Bus ObjectManager, per-device objects · 45 nautilus-python subpackage + `.desktop` fallback + version-proofing · 46 one APK, document Play Protect · 47 CDM association when mirroring is enabled · 48 observed-packages list, no `QUERY_ALL_PACKAGES` · 49 no inline reply; KDE path with Plasma support · 50 KDE Connect wire shape, icon PNG + hash cache

**Core architecture** — 51 **shared Rust core** · 52 one control stream + one stream per transfer · 53 ~20 s keepalive, WifiLock, NetworkCallback rebind, capped backoff · 54 `NsdManager` in Kotlin; unicast reconnect to cached address

**Inside the core** — 55 three crates: `protocol` (sans-I/O) → `net` → `ffi` · 56 postcard + serde · 57 explicit cargo-ndk/uniffi-bindgen glue; handle object + explicit cancel · 58 `ring`, both platforms · 59 `:app` + `:core`; arm64 + armv7 release, x86_64 debug · 60 Rust owns all state; DataStore for UI prefs only · 61 reproducibility hygiene from day one · 62 tracing → journald/Logcat + in-app mirror
