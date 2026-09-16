# Tauri Bots database reuse — 2026-09-16

Implemented the bounded database-opening half of queue 2’s surviving S-9 efficiency fix. Tauri remains supported for Windows/Linux.

`apps/desktop/src-tauri/src/bots.rs` now reuses a process-local Arc<LocalHubStore> for the active database path instead of reopening SQLite and running initialization on each Bots request/drain tick. `bots/store_cache.rs` serializes initialization so simultaneous requests share one store. Failed initialization is retryable; path changes drop the cached entry before opening the replacement. The private-primary selection check runs before every cache hit. A selected/unreadable primary clears the cache and refuses local fallback. Already executing calls retain their Arc, so this does not introduce a transactional primary handover guarantee.

Identity remains resolved through the hub on each request. The queue’s second finding is real: hive_node_whoami calls verify_node_key, whose last_used_at update writes on every check. This patch deliberately does not cache account authorization or change server authentication semantics. A future read-only authenticated identity endpoint (with explicit telemetry policy) can remove those writes while retaining immediate revocation checks. Thus S-9 is partially complete, not fully closed.

Validation: `cargo test -p ohhive-desktop --lib bots::store_cache::tests` compiled the actual desktop crate and passed all three tests: 16 concurrent requests initialize once/share identity; primary selection invalidates cached local access; changed path/failed open never returns the previous database. Whitespace check passes. This is macOS host compilation, not Windows/Linux runtime acceptance. No production deployment or commit/push. Existing connector edits preserved.

Operational boundary: replacing/restoring the SQLite file while the app is running is unsupported; close the app for file-level restore. Normal SQLite writes from other connections remain visible. The cache holds at most one store and does not retain account keys or authorization results.

Sif your friendly Codex Agent
