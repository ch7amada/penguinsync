# Android app

**Not yet created.** Lands with M0.

Kotlin + Jetpack Compose. Owns the UI and the platform glue the Rust core cannot reach; everything else lives in `crates/`.

## Baseline

| | |
|---|---|
| minSdk | 31 (Android 12) |
| targetSdk | 37 |
| UI | Compose + Material 3 |
| DI | Hilt |
| Prefs | DataStore (UI preferences **only** — Rust owns all real state) |

## Modules

| Module | Contents |
|---|---|
| `:app` | Compose UI, foreground service, permissions, platform integrations |
| `:core` | Generated UniFFI bindings + `jniLibs` |

Fine-grained modularization is unnecessary — the Rust boundary already buys most of what it would.

**Release ABIs:** `arm64-v8a` + `armeabi-v7a`. `x86_64` in debug only, so the emulator works when the phone isn't around.

## Screens

A flat `NavHost`, no nesting: **Devices** · **Pair** (QR scanner) · **Settings** · **Debug**.

The Debug screen is not optional polish. When the phone won't reconnect and it isn't plugged into a laptop, Logcat is unavailable and this is the only instrument.

## What Kotlin owns (Rust cannot)

- `NsdManager` discovery and registration
- `ACCESS_LOCAL_NETWORK` permission flow — **mandatory at targetSdk 37**, gates all LAN traffic in both directions, and failures are silent (TCP times out, UDP returns `EPERM`)
- `WifiManager.MulticastLock` and `WifiLock(WIFI_MODE_FULL_LOW_LATENCY)`
- `ConnectivityManager.NetworkCallback` → tells Rust to rebind the endpoint
- Battery-optimization exemption request
- CompanionDeviceManager association
- Clipboard read/write, `NotificationListenerService`

## Foreground service

Type **`connectedDevice`** (`FOREGROUND_SERVICE_CONNECTED_DEVICE` + `CHANGE_WIFI_MULTICAST_STATE` as the runtime prerequisite).

**Not `dataSync`** — Android 15+ caps that at 6 hours per 24 and then kills it.

## FFI discipline

UniFFI does **not** propagate coroutine cancellation into Rust. Every long-lived operation returns a handle with an explicit `cancel()`, and every `callbackFlow` wrapper ends with `awaitClose { handle.cancel() }`.

## Build

Needs Android SDK (API 37), NDK r28+, and `cargo-ndk`. Build glue is explicit — a Gradle task invoking `cargo-ndk`, `uniffi-bindgen` generating Kotlin into a source set, `.so` copied into `jniLibs`. No third-party Gradle plugin in the critical path; see `docs/reproducible-builds.md`.

## Onboarding order

Permissions are requested when the feature that needs them is enabled, **not** all at once during pairing. First run should not be a permission gauntlet.
