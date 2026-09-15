# Agent inspector panel — handoff for Sif

Loki, 2026-09-15. Jack, after seeing the Bots screen live: pointed at the empty space to the
right of "Reconnect" and wants an agent detail panel there — status, plus editing the agent's
attributes and "what tools it has access to." Full context in
`docs/LOKI-BOTS-C2-GROUP-AND-ADAPTER-AGENTS-PLAN-2026-09-15.md`'s Track C section — read that
first, it explains what's real and what isn't.

Scope of *this* handoff, deliberately narrower than what Jack described: the panel, plus
editing name and a raw `capability_policy_ref` string. **Not** in scope here: any actual tool
enforcement. `bots/runner.rs` is explicitly "Bounded, tool-free local reply execution" today —
no Bots agent has ever called a tool, `capability_policy_ref` is a free-text field nothing
reads back, and every agent anywhere in the codebase is hardcoded to `"default"` at creation.
Wiring real tool access needs a Bots tool-calling loop built against the existing Card tool
surface (`tools.rs`) first — that's a trust-boundary design I want to own single-threaded, not
something to converge on from two directions. So the panel's `capability_policy_ref` field is
honestly a raw string editor for now (labeled as such — don't imply it does something it
doesn't), not a tool-toggle UI. That follow-up is mine once the loop exists.

## What's already there to build on

`BotsSession` (Rust, `crates/ohhive-ffi/src/bots.rs`) already has `agents_list`/
`agents_create`, and `BotsAgent` (the UniFFI record) already carries every field the panel
needs to display and edit: `name`, `runtimeKind`, `preferredHost`, `roleRevision`,
`capabilityPolicyRef`, `memoryNamespace`, `archived`. Core already has the update path end to
end — `LocalHubStore::bots_agents_update(actor, agent_id, patch: AgentProfilePatch)` and
`bots_agents_archive(actor, agent_id)` both exist in `crates/ohhive-core/src/local_hub/
bots.rs` and are already wired into the `BotsService` trait impl. Nothing there needs touching.
What's missing is purely the FFI export and the Swift UI.

## FFI (`crates/ohhive-ffi/src/bots.rs`)

Add next to `agents_create` (same `self.call(...)` pattern everything else here uses):

```rust
pub async fn agents_update(
    self: Arc<Self>,
    agent_id: String,
    name: Option<String>,
    capability_policy_ref: Option<String>,
) -> Result<BotsAgent, HiveError> {
    self.call(move |s| {
        let id = parse_id(&agent_id)?; // same helper apps/desktop/src-tauri/src/bots.rs uses
        if let Some(n) = &name {
            if n.trim().is_empty() || n.len() > 256 {
                return Err(fail("Agent name must be 1-256 bytes"));
            }
        }
        s.store
            .bots_agents_update(s.owner, id, AgentProfilePatch {
                name,
                preferred_host: None,
                capability_policy_ref,
                memory_namespace: None,
            })
            .map(Into::into)
            .map_err(storage)
    })
    .await
}
```

`parse_id` doesn't exist in this file yet (the Tauri module has its own copy) — add a small
`Uuid::parse_str(&s).map_err(|_| fail("invalid id"))` helper here, or inline it. `owner` is
already the session's own authenticated identity (`s.owner`), so this can't touch another
account's agent — same pattern `message_send`/`conversations_join` already rely on.

A status concept worth adding while you're in here, since Jack explicitly asked for "status":
`BotsAgent` has no live status today (paired/online is a `BotsModel`-level concept in Swift,
not per-agent). Simplest honest status for now: whether this agent's `preferred_host` equals
`hostId()` (i.e. "This Mac" vs "Another computer" — `BotsView.swift` already computes this
inline for the roster) plus whatever `workerStatus` the model already surfaces
("Local replies enabled" / "Waiting for capacity" / an error). No new backend needed — the
panel just needs to read what `BotsModel` and the roster row already compute, not invent a new
status field.

## Swift (`BotsModel.swift` / `BotsView.swift`)

- `BotsModel`: add `func update(agentID: String, name: String?, capabilityPolicyRef: String?)
  async` calling the new `agentsUpdate` binding, then refresh the roster (`agents = ...` from
  the returned `BotsAgent`, or a full `refreshAgents()` — your call which is cleaner given the
  existing generation-token fencing).
- `BotsView`: an inspector panel in the trailing space next to "Reconnect" (Jack's screenshot
  shows that whole right strip empty — a fixed-width trailing panel, `HSplitView` or a plain
  `HStack` addition, your call for what fits the existing layout best), shown when `selected !=
  nil`. Contents: agent name (editable text field, save on submit/blur), runtime kind + host
  ("This Mac" / "Another computer", read-only), the worker status line already shown at the
  top today, and a `capability_policy_ref` text field labeled plainly, e.g. "Capability policy
  (raw, not yet enforced)" — don't let the UI imply this restricts anything yet, it doesn't.
  Archive is in scope too if it's a natural fit (`agents_archive` already exists core-side and
  is a one-line FFI add if you want it) but isn't the ask — skip it if it doesn't fit cleanly
  today.

## Verification

Same bar as the last handoff: real Rust tests for the new FFI method (a round trip — create,
update, assert the returned/re-listed `BotsAgent` reflects the change, plus an
unauthorized-account rejection case mirroring `bots_message_send`'s existing pattern), and
Swift tests if the model logic is non-trivial enough to warrant one (the update-and-refresh
flow probably is, given `BotsModelTests.swift`'s existing bar). Leave changes uncommitted for
the one-committer workflow, same as last time, with a short doc note (or just a clear commit-
message-shaped paragraph in continuity) on what's covered and what's still open.
