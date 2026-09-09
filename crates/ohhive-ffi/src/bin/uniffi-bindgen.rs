//! Run this to generate the Swift binding module from `ohhive-ffi`'s `#[uniffi::export]`
//! surface. No `.udl` file involved (ADR-018 decision 3) -- the proc-macro annotations in
//! `src/lib.rs` are the whole interface definition.
//!
//! From `crates/ohhive-ffi`, after `cargo build --release` (or `--target aarch64-apple-darwin`):
//!
//!   cargo run --bin uniffi-bindgen -- generate --library \
//!     ../../target/release/libohhive_ffi.dylib \
//!     --language swift --out-dir ./bindings
//!
//! That produces `ohhive_ffi.swift` plus a `ohhive_ffiFFI.h` / `.modulemap` pair -- drop all
//! three into the Xcode project (see the Swift files under `apps/desktop-swift/`).

fn main() {
    uniffi::uniffi_bindgen_main()
}
