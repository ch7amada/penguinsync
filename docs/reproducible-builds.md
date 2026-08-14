# Reproducible builds (F-Droid)

Shipping Rust `.so` files in an APK means build paths, timestamps and toolchain versions leak into the binary. F-Droid builds from source and compares against the published APK, so those leaks break reproducibility.

**This checklist is applied from day one, not retrofitted.** Retrofitting means bisecting which of six environment leaks broke it with no known-good baseline to compare against. Each item below is a one-line change while the build is still trivial.

[Element X Android](https://github.com/element-hq/element-x-android) is the reference: Compose UI, Rust core, UniFFI, on F-Droid, reproducible. Copy its setup rather than deriving one.

## Checklist

- [x] **Pin the exact Rust toolchain** — `rust-toolchain.toml`, currently `1.96.0`. F-Droid's build recipe must install the same version.
- [ ] **`CARGO_HOME`** exported to a fixed path *before* rustup runs — it leaks into the built `.so`.
- [ ] **`CARGO_TARGET_DIR`** set to a stable absolute path (e.g. `/tmp/build`); embedded paths otherwise differ per machine.
- [ ] **`SOURCE_DATE_EPOCH`** exported, to kill embedded timestamps.
- [ ] **`--remap-path-prefix`** (or nightly `trim-paths`) stripping build paths from the binary.
- [ ] **NDK at the same path across builds** — symlink if necessary.
- [ ] **CI builds twice and diffs** the resulting `.so` files.

## Related decisions

- **Crypto backend is `ring`, not `aws-lc-rs`.** Besides the arm64 Android cross-compilation failure, `ring` avoids a CMake/NASM-heavy build that would make F-Droid's job harder. Recorded in the workspace `Cargo.toml`.
- **Build glue is explicit** — `cargo-ndk` invoked from a Gradle task, `uniffi-bindgen` generating into a source set. No third-party Gradle plugin sits in the critical path of an F-Droid build.
- **Release ABIs are `arm64-v8a` and `armeabi-v7a`.** `x86_64` is debug-only, for the emulator.

## Known distribution caveat

Play Protect blocks internet-sideloaded APKs that declare `NOTIFICATION_LISTENER`. Installing through the **F-Droid client** avoids that classification. This is documented for users, not architected around.
