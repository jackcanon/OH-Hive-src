# App/core build identity

Item e6edc4ed. The launcher previously established that a dynamic library could load, but could not identify whether it was the core the app was packaged against.

The bundle builder now passes its source stamp to Cargo via `OHHIVE_BUILD_SOURCE_COMMIT`. The FFI build script embeds this value and tells Cargo to rebuild when it changes. A tiny C ABI function exposes the static stamp without generated bindings, allocations or worker startup. Ordinary non-bundle Cargo builds receive `unstamped`.

Before executing Hive-bin, the launcher loads the bundled library, reads that function, and compares it with Info.plist's OHHiveSourceCommit. Missing, unstamped or mismatched identities stop launch before user data opens. Check-only mode prints both stamps without starting workers or touching logs/data. Normal launch records them in `~/Library/Logs/Hive/build-identity.log`, replacing the previous startup receipt. Existing damaged-bundle error handling covers failures.

The publisher completeness/model-response diagnostics are integrated on branch sif-build-identity-20260919, based on main d95ca6a. Do not transplant this launcher into an older bundle: older cores intentionally fail the missing-stamp check. Build the full app, matching bindings and core together.

This is build provenance for accidental stale artifacts, not proof against malicious bundle modification. The source stamp is supplied by the trusted build script; normal signing verification remains required. It also does not compare the app against a separately deployed CLI server. Dirty-source builds retain the existing dirty suffix and warning.

Tests use the real compiled Swift launcher with tiny disposable C libraries to cover matching/mismatched builds, absent core symbol, absent app stamp, and unstamped pairs. No real app data or inference is used. The Rust unit test verifies that the exported NUL-terminated string equals the compiled stamp. No updated app has been installed yet.
