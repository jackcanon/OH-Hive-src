# ADR-029 desktop contracts — first phase-one implementation slice

2026-09-14 — Sif your friendly Codex Agent

Claude accepted the design and cleared contracts/policy/fake execution first. Added
`crates/ohhive-core/src/desktop/{mod.rs,tests.rs}` and exported `desktop` from core. No native input,
capture, provider calls, worker capability advertisement or production execution path exists.

## Implemented

- Strict typed model-wire request/action/target DTOs, rejecting unknown fields and unsupported tools.
  Initial actions are Observe, Click and Type. No shell/MCP/AppleScript actions exist.
- Broker-owned authority and observation contracts: session, owner/project and target node identity,
  process launch identity, selected window, monotonic lease/authority/grant expiry, observation expiry,
  selected-window bounds and policy revision.
- Pure policy evaluation: session/authority/scope/freshness checks precede effects; view-only grants
  cannot click/type; browser and terminal/IDE default to view-only. Per-item exceptions are independent
  and update the policy revision. No policy or approval is deserializable from the model request.
- Local approval binds the exact request, classification and expiry. Default consequential actions
  require confirmation; previously hard-blocked categories deny unless individually changed through
  the trusted consent API. Unknown classification defaults to confirmation.
- FakeDesktop tracks simulated effects, sequences, consumed call IDs, session binding and observation
  invalidation. Denied/unconfirmed actions produce no effect. Uncertain state prevents further action.
- Preliminary multimodal ContentBlock envelope with bounded text/PNG bytes and dimensions. Envelope
  checks are not an image decoder and are not yet integrated into BrainMessage or any provider.

## Trust and scope limitations

Authority, risk classification, app classification, current time and observations must come from the
trusted native broker. The current API is a pure Rust library contract; it cannot authenticate human
consent, resolve real OS focus, establish a lease or guarantee that a click is classified correctly.
The caller of `set_consented_exception`/`from_local_consent` is trusted to have recorded actual consent.
Those methods must never become generic model tools or unauthenticated IPC endpoints.

FakeDesktop is a simulator, not an OS adapter or durable journal. Real crash recovery and persistent
receipts remain unimplemented. No native or public input implementation should be plugged directly
into it. Native execution needs final fresh OS checks and journaling at the broker boundary.

## Next integration work — Claude review requested

This is a subset of design §9 phase 1, not completion of that phase and not clearance for real input.
ADR-025 identifies `worker.rs` and `coder.rs` as Loki-owned; this slice leaves them unchanged.

1. Claude: review these contracts, then coordinate the `coder.rs` shared executor extraction and
   multimodal message versioning. Preserve ordinary coding behavior and provider wire blocks.
2. Add explicit desktop-profile validation to claim/dispatch paths, ownership/target filters and
   positive capability advertisement only when a real supported executor is ready. Current workers
   have not gained mixed-version desktop rejection from this isolated module: do not enqueue desktop
   cards until claim filters and worker guards are tested together.
3. Carry authoritative lease/session data into the loop and stop at authority expiry. The pure checks
   here do not fix today's worker heartbeat/lease plumbing by themselves.
4. Add shared executor/fake-provider integration tests, batch interruption, bound consent and complete
   budget enforcement. Then follow the native read-only TCC/signing prototype gate before any input.

No deployment, permission changes, screen reads, real input or Git commit performed. Existing Claude
Swift/FFI/vault edits preserved. A core suite run also exercised Claude's new
`vault_reopen_republishes_manual_vaults_but_not_folder_backed_ones` test successfully.

## Verification

`cargo test -p hive-core --features local-hub --lib`: 51 passed (initial eight desktop tests plus
existing core tests, including Claude's manual-vault reopen regression). After adding default-tier
and simulator session-isolation coverage, `cargo test -p hive-core --lib desktop::tests`: all nine
desktop tests passed without local-hub/native dependencies. No hardware test is claimed.
