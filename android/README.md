# Android app

Kotlin + Jetpack Compose. Owns the UI and the platform glue the Rust core cannot reach; everything else lives in `crates/`.

Shipping as of 0.1.0: QR pairing with a camera scanner, clipboard sync both directions (manual tier phone → desktop, per Android's clipboard restrictions), a `connectedDevice` foreground service that keeps the connection alive in the background, and the four screens below. Verified live throughout on a Samsung SM-S937B (Android 16) against `penguinsyncd`.

## Building

```
cd android
./gradlew :app:assembleDebug
```

For a release build — R8-shrunk, release-profile Rust, shipping ABIs only:

```
./gradlew :app:assembleRelease -Ppenguinsync.release=true
```

`-Ppenguinsync.release=true` is what selects the `release-android` cargo profile in `:core` and drops `x86_64`. Without a keystore the output is `app-release-unsigned.apk`; see [`../docs/RELEASING.md`](../docs/RELEASING.md). Normally you want `packaging/build-android-release.sh`, which passes the flag and checks the version numbers agree.

`:core`'s build does the Gradle glue itself — `cargo-ndk` cross-compiles `crates/ffi`, then `uniffi-bindgen` generates the Kotlin from the resulting `.so`'s embedded metadata (see `android/core/build.gradle.kts`). Needs `cargo-ndk` and the Android NDK (both already required by `rust-toolchain.toml`/the SDK install), and a JDK with `javac` — Fedora's `java-25-openjdk` package here is headless-only; point Gradle at a full JDK via `org.gradle.java.home` in `~/.gradle/gradle.properties` (machine-specific, not committed) if `assembleDebug` fails with a missing `JAVA_COMPILER` capability.

Note: AGP 9.0+ has Kotlin support built in — do not add the `org.jetbrains.kotlin.android` plugin back, it's now a hard error.

## Baseline

| | |
|---|---|
| minSdk | 31 (Android 12) |
| targetSdk | 37 |
| UI | Compose + Material 3 **Expressive** (material3 1.5.0-alpha — the API is `internal` in 1.4.0 stable) |
| Theme | Generated from seed `#01579B`; light/dark, plus an optional Material You mode |
| DI | Hilt — planned, not yet used |
| Prefs | `SharedPreferences` for the one UI preference there is; DataStore when M3 brings real per-device state. Rust owns all real state either way. |

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
