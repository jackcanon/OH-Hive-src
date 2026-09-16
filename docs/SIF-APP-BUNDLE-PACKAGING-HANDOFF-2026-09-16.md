# S-3: self-contained native app bundle

Built and verified locally. The previous app's Mach-O dependency pointed directly to `target/aarch64-apple-darwin/release/deps/libohhive_ffi.dylib` in the checkout. Rebuilding that file could change the ABI underneath the already-built Swift app.

`apps/desktop-swift/scripts/build-app.sh` now builds Rust, snapshots the resulting library, regenerates the Swift/header bindings from that exact snapshot, and links Swift to the snapshot. It ships the engine at `Contents/Frameworks/libohhive_ffi.dylib` with `@rpath` and an executable-relative Frameworks search path. Subsequent repository Rust builds cannot replace this bundled copy. The script serializes bundle builds with a lock and stages/signs/verifies the new bundle before replacing the previous app. Failed publication attempts restore the prior bundle; failed restoration preserves its staging location rather than deleting it.

`Package.swift` accepts the script's `OHHIVE_FFI_LIBRARY_DIR` override. Direct development Swift builds keep their existing development-library default. Build the `.app` with the script; manually combining bindings/libraries is still unsupported. Generated files remain ignored.

The executable named `Hive` is now a small AppKit launcher with no dependency on the Rust bridge. It checks the main executable and loads the bundled library before exec'ing `Hive-bin`. Missing/unloadable-engine failures get a readable alert and `~/Library/Logs/Hive/launch-error.log`. `--hive-bundle-check` performs the same preflight without showing the UI or starting Hive workers. This diagnoses bundle failures; it does not catch arbitrary later application crashes or protect against intentional replacement with an ABI-incompatible but loadable engine.

`verify-app.sh` rejects nonportable library dependencies and exercises the engine loader. `test-app-bundle.sh` copies the signed app into a separate folder with spaces, verifies signatures/relocated loading, and checks missing-engine, corrupt-engine and missing-executable diagnostics. Release CI uses the same build entry point and runs these checks before packaging the DMG. Nested code is signed inside-out rather than relying on `--deep` for signing.

Verification:

- Final Rust/bindings/Swift release build and ad-hoc signing passed.
- Strict deep signature verification and relocated engine load passed.
- All three incomplete/damaged bundle checks failed with the expected readable messages.
- A concurrent bundle-build attempt was rejected by the build lock; cleanup removed the lock afterward.
- All 7 existing Swift tests passed. Claude's concurrent `2d25983` added `ensureProviderAgents()` to roster refresh; the existing fake initially fell through to a nil Rust pointer. Added the missing fake override, leaving his production code intact.
- Shell syntax and whitespace checks passed.

Built artifact: `apps/desktop-swift/Hive.app`. No UI interaction or live-worker/provider run was performed. Developer ID signing/notarization and a new remote CI run are not claimed. No production deployment, commit or push. Other shared-checkout changes remain intact.

Next queue item: S-4 broader storage/interview/general payout safeguards; speech reservations alone do not close it.

Sif your friendly Codex Agent
