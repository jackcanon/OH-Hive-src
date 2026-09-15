# C2 native agent inspector — Sif handoff

Implemented the scoped `LOKI-AGENT-INSPECTOR-SWIFT-HANDOFF-2026-09-15.md` assignment. The newer C2 plan assigns provider agents to Loki and reserves rooms/mentions/tool-enforcement architecture to him; this work completes the native inspector portion, not all of C2.

## Behavior

Selecting an agent exposes a native trailing inspector, with an Agent details toggle beside Reconnect. The inspector shows runtime, assigned host, local worker status (only for a local agent on this Mac), role revision and selectable agent ID. It does not invent remote online status. Name and capability policy reference are editable, saved on Return or the explicit Save changes button. Draft state is keyed to agent identity, and save failures remain visible.

The Tools section says that current Bots replies have no tool access and explicitly labels the policy reference as raw and not enforced. Editing it neither enables nor restricts tools. No tool toggles or new policy enforcement are included. Archive was optional and is omitted.

`BotsModel.update` applies the returned record to the roster without reopening the DM or replacing the message draft. Account-generation fencing discards late results after disconnect/reconnect. `BotsSession.agents_update` uses the authenticated owner, accepts only name/policy changes, checks an active owned agent, and validates nonblank names up to 200 UTF-8 bytes and policy references up to 500. The actual core create limit is 200 for names, despite the handoff example's 256; this implementation follows core. Host, memory namespace, runtime and credentials cannot be changed through this method.

## Changes and verification

- Rust: `crates/ohhive-ffi/src/bots.rs` update export and ownership/validation/persistence tests. Five bridge tests passed. These Rust changes were included by Loki in concurrent commit `a0096c1` along with provider identity work; do not recommit/revert that portion.
- Swift: new `BotsAgentInspector.swift`, inspector attachment in `BotsView.swift`, update method in `BotsModel.swift`, and model test in `BotsModelTests.swift` verifying roster changes preserve selected agent and message draft.
- Release FFI rebuilt; generated bindings/header refreshed. Final Swift test/package results are recorded in continuity.
- No production agent metadata was edited for tests. Visual and live-save acceptance remain to be performed; no screenshot validation is claimed.

## Integration note for Loki

While your provider work was arriving, I noticed `ensure_provider_agents` in the shared FFI module directly awaits `HubClient.member_key_status()` outside `RUNTIME.spawn`, unlike `bots_open` and other network FFI exports. Please check/wrap that network section on the owned Tokio runtime before wiring it to Swift, and validate session identity before provider lookup. This is a code-review finding, not a reproduced live failure. I left your actively edited provisioning path untouched. Concurrent calls can also race its list-then-create provider identity check; enforce durable uniqueness before advertising concurrent provisioning as idempotent.

Sif your friendly Codex Agent
