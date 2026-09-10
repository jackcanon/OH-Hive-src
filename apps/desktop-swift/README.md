# Hive — native macOS app (ADR-018, phase 1)

Native SwiftUI shell over the shared `ohhive-core` Rust crate, bridged through
`crates/ohhive-ffi` (UniFFI). Covers pairing, the compute-node worker, snapshot/about, and
config — the Tauri `desktop-macos` job keeps building the old app in parallel until phase 2
(Setup wizard, regional-server role, Cloudflare Tunnel) lands here too.

**Nothing in `Sources/` has been compiled yet.** I don't have Xcode/a macOS toolchain, so the
Rust crate hasn't been built for `aarch64-apple-darwin` and the UniFFI Swift bindings haven't
been generated — these Swift files are written against my best understanding of what UniFFI
0.28 generates for the Rust in `crates/ohhive-ffi/src/lib.rs`, but the first real build is where
we find out if I got a method or field name wrong. Small naming mismatches (Xcode will point
right at them) are the most likely issue, not a structural one.

## 1. Build the Rust side

```
cd "crates/ohhive-ffi"
cargo check                                    # catch plain Rust errors first, fast
cargo build --release --target aarch64-apple-darwin
```

If `aarch64-apple-darwin` isn't installed: `rustup target add aarch64-apple-darwin`.

This produces `target/aarch64-apple-darwin/release/libohhive_ffi.a` (and a `.dylib`).

## 2. Generate the Swift bindings

From `crates/ohhive-ffi`:

```
mkdir -p bindings
cargo run --bin uniffi-bindgen -- generate \
  --library ../../target/aarch64-apple-darwin/release/libohhive_ffi.dylib \
  --language swift --out-dir bindings
```

That writes three files into `crates/ohhive-ffi/bindings/`:

- `ohhive_ffi.swift` — the real Swift API (this is what `HiveStore.swift` etc. actually call)
- `ohhive_ffiFFI.h` — the C header for the bridging
- `ohhive_ffiFFI.modulemap` — not needed for the bridging-header approach below; ignore it

## 3. Create the Xcode project and wire it up

1. Xcode → File → New → Project → macOS → App. Product name `Hive`, interface **SwiftUI**,
   language **Swift**. Save it anywhere outside this repo (or inside, under
   `apps/desktop-swift/OHHive.xcodeproj` — either works, it's gitignored either way for now).
2. Delete the auto-generated `ContentView.swift` and `OHHiveApp.swift` Xcode created.
3. Drag every file in `apps/desktop-swift/Sources/` into the project (check "Copy items if
   needed").
4. Drag `crates/ohhive-ffi/bindings/ohhive_ffi.swift` in too.
5. Project settings → target → **Build Settings** → search "bridging header" → set
   **Objective-C Bridging Header** to the path of `ohhive_ffiFFI.h` (copy it into the project
   first, e.g. into a `Bridging/` group, "Copy items if needed").
6. Target → **Build Phases** → **Link Binary With Libraries** → **+** → **Add Other, Add
   Files...** → pick `libohhive_ffi.a` from step 1.
7. Target → **Build Settings** → **Library Search Paths** → add the folder containing
   `libohhive_ffi.a` (`.../target/aarch64-apple-darwin/release`).
8. Target → **Signing & Capabilities** → set your team (same Apple Developer Program account
   already used for the Tauri app's Developer ID cert — a plain "Sign to Run Locally" is fine
   for now).
9. Build and run (⌘R).

If Xcode complains about a missing/extra method on `HiveNode` or a field on `HiveSnapshot` /
`AboutInfo` / etc., open `ohhive_ffi.swift` and check the actual generated name — UniFFI
converts Rust's `snake_case` to Swift's `camelCase` automatically (`app_version` →
`appVersion`, `worker_start` → `workerStart`), which is what every file here assumes, but it's
worth confirming against the generated file rather than guessing twice.

## What's here vs. not

Wrapped (phase 1): `about`, `snapshot`, `pair_begin`/`pair_cancel`, `worker_start`/`worker_stop`,
`set_config`, and an activity/changed event callback.

Not wrapped (phase 2, deferred — ADR-018 decision 4): first-run Setup (hardware assessment,
Ollama install, model ladder), the regional-server role, Cloudflare Tunnel. The Tauri app is
still the only way to run those roles until phase 2 adds a second `#[uniffi::export] impl`
block in `ohhive-ffi` for them.

## Once this builds

- Re-run step 2 any time `crates/ohhive-ffi/src/lib.rs` changes, and re-copy
  `ohhive_ffi.swift` into the Xcode project (or add a Run Script build phase that does steps 1–2
  automatically — worth doing once this is proven out by hand).
- The chat/agent engine (ADR-015, macOS-first per ADR-018 decision 5) is a separate follow-on:
  a new Swift file calling Apple's Foundation Models framework directly, not part of this pass.
