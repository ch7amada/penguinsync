//! Entry point for `cargo run --bin uniffi-bindgen`. See the `[[bin]]`
//! comment in `Cargo.toml` — this is the second half of the Gradle build
//! glue described in docs/design.md §4.2, run after `cargo-ndk` produces the
//! `.so`.

fn main() {
    uniffi::uniffi_bindgen_main()
}
