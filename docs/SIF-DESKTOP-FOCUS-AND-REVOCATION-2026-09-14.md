# Desktop simulator: focus changes and explicit grant revocation

Date: 2026-09-14
Author: Sif your friendly Codex Agent

## Completed behavior

The Rust-only `FakeDesktop` now tracks trusted broker-reported focus. Unknown focus rejects requests. A focus event invalidates observations through the broker-supplied high-water mark, including observations held by queued requests. Returning to the original window requires a newer observation; actions are never redirected to the newly focused window. The high-water mark can only increase.

`revoke_grant` explicitly revokes a target for the simulator session lifetime. Execution and local review both check revocation, so an existing action approval cannot bypass it. Revocations survive journal serialization and simulated restart. There is deliberately no in-place regrant operation in this slice.

Recovered simulators start with unknown focus and remain paused for local review. A fresh trusted focus report and current observation are required before review can resume work.

## Integration contract and limits

The focus event and revocation methods are trusted host APIs, not model tools. The broker must provide the highest observation ID issued before a focus event, including queued observations, and issue monotonically increasing observation IDs. A model cannot be allowed to supply these broker events. Target identity includes application, process identity and window.

Journal snapshots now use version 2 with required revoked-target state. Recovery rejects version 1 rather than silently dropping revocation information. This is a prototype schema change; no backward migration was added.

This remains a simulator. Queued requests are held by callers and rechecked at execution; no production queue or native focus monitor was added. Native dispatch still needs broker-side target checks immediately before input, authenticated grant/revocation delivery, and durable trusted storage. No AX, CGEvent, capture, Swift, FFI, worker or coder changes were made for this task.

## Verification

`cargo test -p hive-core --features local-hub --lib`: **65 passed, 0 failed** on Midgaard (macOS). Three new tests cover focus loss/return with a queued action, explicit revocation defeating an existing approval and surviving restart, and unknown focus plus monotonic observation invalidation. Rejected attempts leave effect and receipt counts unchanged. Existing recovery coverage was updated to supply a trusted focus report. Formatting and `git diff --check` passed. Linux and Windows were not rerun for this slice.

Files: `crates/ohhive-core/src/desktop/mod.rs`, `journal.rs`, and `tests.rs`.

## Handoff to Claude

Please review the trusted focus-event/high-water-mark contract and the version-2 journal boundary before native broker integration. This completes the queued Rust-only focus/revocation slice. Native event delivery and grant lifecycle remain the next integration work, under the existing Claude/Jack ownership.
