# PenguinSync Wire Protocol

**Protocol version: 1 (M0 + M1 implemented: Handshake, Ping/Pong, Clipboard)**

This document is **normative**. Every change to the wire format must edit this file in the same commit that changes the code, and must bump `PROTOCOL_VERSION` in `crates/protocol/src/lib.rs`.

Pre-1.0 there are **no compatibility guarantees**. Version mismatches are rejected outright, with a clear error surfaced in both UIs. With a small number of devices in play, a lockstep bump costs nothing, and the strict-reject error is what saves debugging time.

---

## 1. Identity

Each device generates an **Ed25519 keypair on first run**, wrapped in a self-signed X.509 certificate used for QUIC/TLS.

```
DeviceId = SHA-256(SubjectPublicKeyInfo)
```

Rendered for humans as a short comparable string (exact rendering: TBD, must be readable aloud and comparable at a glance on a phone screen).

Certificate verification is a **custom `ServerCertVerifier` performing SPKI pinning**. No system root store is consulted; `rustls-native-certs` and `webpki-roots` are deliberately not dependencies.

## 2. Discovery

Service type `_penguinsync._udp`, advertised by the Linux daemon through the system Avahi daemon over D-Bus.

TXT records:

| Key | Value |
|---|---|
| `v` | protocol version (`u16`, decimal) |
| `id` | `DeviceId` |
| `name` | human-readable device name |

Android discovers via `NsdManager` and passes resolved addresses to the Rust core.

**Reconnection does not use mDNS.** Background multicast reception is unreliable on many vendors' Wi-Fi firmware with the screen off. mDNS is for discovery while the user is present; every subsequent reconnect is unicast to a cached address.

## 3. Pairing

Linux displays a QR code; Android scans it. Trust is established **in both directions from a single scan**.

### 3.1 QR payload

```
penguinsync://pair?v=<version>&id=<DeviceId>&fp=<pubkey-fingerprint>
                  &name=<device-name>&addr=<ip:port>[&addr=<ip:port>…]
                  &token=<pairing-token>
```

- `addr` may repeat. Candidate addresses exist so the **first connection never depends on mDNS working**.
- `token` is **single-use and valid for 60 seconds**.

### 3.2 Flow

1. Linux generates a pairing token, displays the QR, and listens.
2. Android scans, connects to a candidate address, and **pins Linux's public key**.
3. Android presents the token and its own public key over the encrypted channel.
4. Linux's TUI displays Android's fingerprint and **waits for explicit confirmation**.
5. Both sides persist the pin.

### 3.3 Trust lifecycle

- **Unpair is unilateral and immediate** on the initiating side. Revoking a lost phone from the laptop must not require the phone's cooperation.
- **A changed key is a hard reject** with a prominent warning; re-pairing requires a new QR scan. The benign cause (app reinstall) is rare and costs one scan; the malicious cause is precisely what pinning exists to catch.
- **Pins never expire.**

## 4. Transport

**QUIC** (`quinn`). A single endpoint both accepts and initiates.

**Roles:** Linux listens, Android dials. Android is the side that changes networks, sleeps, and gets new addresses, so every reconnect is driven by the side that knows its network state changed.

**Streams:**

| Stream | Direction | Carries |
|---|---|---|
| Control | bidirectional, long-lived | handshake, clipboard, notifications, transfer metadata |
| Transfer | unidirectional, one per transfer | file payload, prefixed with transfer ID and offset |

A large file transfer must never stall a clipboard update. That is why QUIC was chosen over TCP.

**Keepalive:** ~20 s, so NAT bindings and AP power-save do not silently drop the path.
**Reconnect:** exponential backoff capped at ~60 s.

## 5. Encoding

**`postcard`** over `serde`. Both ends compile from the same struct definitions, so no cross-language schema exists to maintain, and no self-describing format is needed.

Debuggability is recovered by logging **decoded** messages through `tracing`, not by reading bytes off a capture.

## 6. Messages

> Handshake, Ping/Pong and Clipboard are implemented (`crates/protocol/src/message.rs`); everything past that is still sketched and changes as milestones land — keep this section in step with `crates/protocol`.

### 6.1 Handshake (M0)

Sent first on the control stream by both sides after the QUIC handshake completes.

| Field | Type | Notes |
|---|---|---|
| `version` | `u16` | Must equal `PROTOCOL_VERSION`, else reject and close |
| `device_id` | `[u8; 32]` | |
| `name` | `String` | |
| `capabilities` | `Vec<Capability>` | Feature negotiation — clipboard, files, notifications |

### 6.2 Ping / Pong (M0)

Trivial payload. Exists so the walking skeleton can prove pairing, connection and reconnect without any platform feature attached.

### 6.3 Clipboard (M1 implemented, M2–M3 pending)

Broadcast to every connected paired device — no destination field, unlike file transfer.

| Field | Type | Notes |
|---|---|---|
| `mime` | `String` | Only `text/plain` accepted in v1; the field exists so images are an addition, not a break |
| `content` | `Vec<u8>` | Size-capped (~100 KB) |
| `hash` | `[u8; 32]` | BLAKE3 of `content`. Echo suppression — mandatory, since clipboard broadcasts to all connected devices |

M1 is Linux → Android only (GNOME Shell extension read side, Android write path — no permission needed). M2 adds Android → Linux (manual read tier); M3 adds the Shizuku background-read tier. The message shape doesn't change across those — only who's allowed to send it and when.

Content marked sensitive by the source platform (Android's `EXTRA_IS_SENSITIVE`) is **never sent**.

### 6.4 File transfer (M4)

Metadata on the control stream; payload on its own unidirectional stream.

| Field | Type | Notes |
|---|---|---|
| `transfer_id` | `u64` | |
| `name` | `String` | Receiver sanitises; never overwrites, appends `(1)` on collision |
| `size` | `u64` | |
| `offset` | `u64` | Always 0 in v1. Reserved so resume is an addition, not a break |
| `hash` | `[u8; 32]` | BLAKE3, verified on arrival |

### 6.5 Notification (M5)

Shape follows KDE Connect's proven design.

| Field | Type | Notes |
|---|---|---|
| `key` | `String` | Stable handle for dedup and remote cancel |
| `package` | `String` | |
| `title`, `text` | `String` | |
| `is_clearable` | `bool` | |
| `silent` | `bool` | |
| `is_cancel` | `bool` | Dismissal event rather than a new notification |
| `request_reply_id` | `Option<String>` | Reserved; inline reply is deferred (docs/design.md §6.3) |
| `icon_hash` | `Option<[u8; 32]>` | Icon sent as a PNG payload once per app, cached by hash |
| `actions` | `Vec<Action>` | Forwarded as freedesktop notification actions; `default` = open on phone |

## 7. Out of scope for this document

Clipboard access mechanisms, foreground-service types, permission flows and D-Bus interfaces are platform integration, not wire protocol. They live in `docs/design.md`.
