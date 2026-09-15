# Track E handoff: wire Bots roster + messaging through LocalHub's RPC transport

Loki, 2026-09-15. Scoped piece of Track E (see
`docs/LOKI-BOTS-C2-GROUP-AND-ADAPTER-AGENTS-PLAN-2026-09-15.md`, reprioritized above Track A
in `8b05dc7`, off-LAN addendum in `1343fe6`). This is the concrete "core work" that section
names: extend `local_hub/transport.rs`'s `dispatch()` and `RemoteLocalHub` with `bots_*`
methods, mirroring the existing `vault_*` pattern exactly. Grounded in what I actually read in
the transport/bots storage code, not assumption -- including one real trust-boundary gap I
found and am deliberately not resolving myself. Please implement the plumbing below, but read
the "What I'm asking you to flag, not fix" section before touching authorization semantics.

## Why this exists

Jack, tonight: "if I create an agent on Midgaard... if I logged into Hive on Overgaard, I
should still see the same agents no matter where I am... it doesn't matter where a local agent
is because it connects to the chat." I checked every C1/C2 Bots entry point -- every one of
them (CLI, Tauri, Swift, the new `ensure_provider_agents`) opens `LocalHubStore` against a
local SQLite file path with no remote dispatch anywhere. An agent registered on one Mac is
invisible from any other device today, BYOK-backed agents included (their *identity row* is
just as node-local as a `Local` agent's, even though the BYOK *key* already lives hub-wide per
ADR-008). This is the bridge that makes a remote `RemoteLocalHub` client able to see and use
another node's Bots roster and conversations at all -- prerequisite for "which LocalHub is
authoritative" (still an open design question, still mine to answer, see the plan doc) ever
mattering in practice.

## What's already proven -- build on this, don't redo it

`local_hub/transport.rs` is real, tested, and already used in production for Vault and card
claiming:

- `dispatch(h: &LocalHub, m: &str, p: &Value) -> Result<Value>` -- a big `match` on method
  name. Each arm: `"method_name" => wire(h.method(argument(p, "field")?, ...)?)`.
- `argument::<T>(p, "key")` deserializes one named field out of the request's JSON params;
  `wire(v)` serializes a response. Both already generic over any `Serialize`/`Deserialize` type
  -- no new helper code needed for Bots types (`AgentProfile`, `Conversation`, `Message`, etc.
  already derive both).
- `RemoteLocalHub` (same file): a `reqwest`-backed client struct. Each public async method is
  `self.rpc("method_name", json!({"field": value, ...})).await`. `rpc()` itself (below the part
  I've pasted -- read the rest of the file) handles the HTTP POST/auth header/error mapping;
  you don't touch it.
- `client()` already enforces the off-LAN rule from the Track E addendum correctly: HTTPS
  required unless the host is loopback or a numeric private-LAN address. Nothing to change
  there for this pass.
- Real end-to-end coverage exists for this exact mechanism today:
  `local_hub/tests.rs::two_http_clients_pair_claim_and_complete_without_cloud` spins up a real
  `axum::serve` instance and drives it through `RemoteLocalHub`, no mocks. That's the pattern
  to extend or mirror for `bots_*` -- a real HTTP round trip, not a dispatch()-only unit test.

## What's missing -- three layers, all currently at zero

1. **`LocalHub` has no Bots wrapper methods at all.** Every `bots_*` method today lives only on
   `LocalHubStore` (`local_hub/bots.rs`), called directly by `BotsSession` in
   `ohhive-ffi/src/bots.rs` (`s.store.bots_agents_list(...)`, etc.) -- never through a `LocalHub`
   session. Compare: Vault has both (`LocalHubStore` in `vault.rs`'s first `impl` block for raw
   storage, plus a second `impl LocalHub` block layering session-derived access control on top,
   e.g. `vault_access()` checking the session's own node against `vault_readers`). Bots has no
   second layer yet -- that's most of this task.
2. **`dispatch()` has zero `bots_*` match arms.**
3. **`RemoteLocalHub` has zero `bots_*` client methods.**

## Scope for this pass: roster + messaging, not delivery/handoff

Full `bots_*` surface on `LocalHubStore` is large (agents, conversations, messages, handoffs,
deliveries). Keep this pass to what "see and use another node's agents/conversations
remotely" actually needs -- the rest either doesn't need to be remote or is a separate design
question:

**In scope** (wrap on `LocalHub`, add to `dispatch()` and `RemoteLocalHub`):
- `bots_agents_list`, `bots_agents_create`, `bots_agents_update`, `bots_agents_archive`
- `bots_conversations_list`, `bots_conversations_create`, `bots_conversations_join`
- `bots_messages_list`, `bots_message_send`, `bots_conversation_mark_read`

**Out of scope, leave alone**: `bots_delivery_claim/complete/fail`,
`bots_deliveries_pending_for_agent`, `bots_handoff_*`. These are how a specific node's local
worker executes a turn it already owns -- claiming/completing a delivery only makes sense on
the node actually running that agent's runtime, so there's no "remote" version of them to
build; wiring them into `dispatch()` would be scope creep, not the fleet-visibility problem
Jack described. If that reasoning turns out wrong once rooms/mentions (Track A) land, that's a
separate task, not this one.

## The trust-boundary gap I found -- flag it back to me, don't resolve it unilaterally

Every in-scope `LocalHubStore::bots_*` method takes its owner/actor as an **explicit parameter**
(`owner: UserId`, `actor: Principal`) -- nothing derives it from session state. Compare that to
how `LocalHub`'s existing wrappers work: `vault_list()` calls `self.with_node(|tx, node| ...)`,
which resolves `node` from `local_node_keys` keyed on the session's own key hash -- the schema
(`local_hub/schema.sql`) has no owner/member column anywhere (`nodes(id, name, caps,
checked_in)`, `local_node_keys(hash, node_id, revoked)`). A `LocalHub` RPC session
cryptographically proves *which node* is calling, never *which member/owner* is acting.

Mechanically, that just means the new `LocalHub` wrapper methods for this pass take
owner/actor as an explicit argument too (deserialized via `argument(p, "owner")` /
`argument(p, "actor")`, same as `vault_read` already takes an explicit `vault_id` rather than
deriving it from session) -- straightforward, same shape as everything else in the file.

But it also means: **today, any device holding a valid, non-revoked local node key for *any*
node can call `bots_agents_list` naming an arbitrary owner UUID and get that owner's agent
roster back** -- node-key auth proves "a paired device is asking," not "this device's owner is
who they claim." That's a real widening of the trust surface the moment this dispatch code
ships, not a cosmetic gap. I don't want you to decide the fix (e.g., adding an owner column to
`nodes` and checking it, requiring a separately-signed owner assertion, rate-limiting, or
deciding this is acceptable because pairing itself is already gated) -- that's a security
boundary decision I said earlier this session I want to own single-threaded, the same way
you own real-toolchain verification. Implement the plumbing as scoped above, write the finding
into your handoff doc exactly as you did for `ensure_provider_agents`, and leave the actual
authorization check as a `// TODO(owner-auth)`-style marker plus a comment pointing at your
handoff doc -- don't add a check you're guessing at, and don't skip flagging it because the
mechanical part works fine without one.

## Exact pattern to follow

`LocalHub` wrapper (new block in `local_hub/bots.rs`, mirroring `vault.rs`'s `impl LocalHub`):

```rust
impl LocalHub {
    pub fn bots_agents_list(&self, owner: UserId) -> Result<Vec<AgentProfile>> {
        self.store.bots_agents_list(owner)
    }
    pub fn bots_agents_create(&self, draft: NewAgentProfile) -> Result<AgentProfile> {
        self.store.bots_agents_create(draft)
    }
    // ...agents_update, agents_archive, conversations_*, messages_*, conversation_mark_read,
    // each a thin pass-through to the matching LocalHubStore method for now (see trust-boundary
    // note above for why there's no session-derived check to add here yet).
}
```

`dispatch()` arms (same file as the other cases, `local_hub/transport.rs`):

```rust
"bots_agents_list" => wire(h.bots_agents_list(argument(p, "owner")?)?),
"bots_agents_create" => wire(h.bots_agents_create(argument(p, "draft")?)?),
"bots_agents_update" => wire(h.bots_agents_update(
    argument(p, "actor")?,
    argument(p, "agent_id")?,
    argument(p, "patch")?,
)?),
"bots_agents_archive" => wire(h.bots_agents_archive(argument(p, "actor")?, argument(p, "agent_id")?)?),
"bots_conversations_list" => wire(h.bots_conversations_list(argument(p, "actor")?)?),
"bots_conversations_create" => wire(h.bots_conversations_create(argument(p, "draft")?)?),
"bots_conversations_join" => wire(h.bots_conversations_join(
    argument(p, "actor")?,
    argument(p, "conversation_id")?,
)?),
"bots_messages_list" => wire(h.bots_messages_list(
    argument(p, "actor")?,
    argument(p, "conversation_id")?,
    argument(p, "page")?,
)?),
"bots_message_send" => wire(h.bots_message_send(
    argument(p, "actor")?,
    argument(p, "conversation_id")?,
    argument(p, "client_request_id")?,
    argument(p, "expected_policy_revision")?,
    argument(p, "recipient_ids")?,
    argument(p, "draft")?,
)?),
"bots_conversation_mark_read" => wire(h.bots_conversation_mark_read(
    argument(p, "actor")?,
    argument(p, "conversation_id")?,
    argument(p, "up_to_sequence")?,
)?),
```

`RemoteLocalHub` client methods, same 1:1 mirror as `vault_list`/`vault_read`:

```rust
impl RemoteLocalHub {
    pub async fn bots_agents_list(&self, owner: Uuid) -> Result<Vec<AgentProfile>> {
        self.rpc("bots_agents_list", json!({"owner": owner})).await
    }
    pub async fn bots_message_send(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
    ) -> Result<Message> {
        self.rpc("bots_message_send", json!({
            "actor": actor, "conversation_id": conversation_id,
            "client_request_id": client_request_id,
            "expected_policy_revision": expected_policy_revision,
            "recipient_ids": recipient_ids, "draft": draft,
        })).await
    }
    // ...remaining methods, same shape.
}
```

`Principal` already serializes cleanly (`#[serde(rename_all = "snake_case", tag = "kind",
content = "id")]`, e.g. `{"kind":"user","id":"..."}`) -- no custom (de)serialization needed for
any of these argument types; everything Bots already derives `Serialize`/`Deserialize`.

## What's explicitly NOT part of this task

- No FFI/UniFFI exposure and no UI wiring. This is core-crate transport only. Once the
  trust-boundary question above is resolved, exposing `RemoteLocalHub`'s Bots methods through
  `ohhive-ffi` (mirroring how Vault's already exposed via `ohhive-ffi/src/local_hub.rs`) is a
  follow-up handoff.
- No "which LocalHub is authoritative for a given owner" client-side selection logic -- still
  open, still mine, noted in the plan doc.
- No off-LAN tunnel provisioning (`local_hub/tunnel.rs` FFI/UI) -- separate, already-scoped
  piece of the Track E addendum, not this task.
- No delivery/handoff dispatch -- see "Scope for this pass" above.

## Verification I'm expecting

Your real toolchain, same bar as C1/Track D: extend or add to `local_hub/tests.rs` with a real
HTTP-round-trip test in the shape of
`two_http_clients_pair_claim_and_complete_without_cloud` -- pair two `RemoteLocalHub` clients
against one served `LocalHubStore`, create an agent and a conversation through one client,
confirm the other client (or the same one after reconnecting) sees it via `bots_agents_list`/
`bots_conversations_list`, send a message through `bots_message_send` and read it back via
`bots_messages_list`. Cover at minimum: a valid node key succeeding, an invalid/revoked key
being rejected (`BadKey`, same as every other dispatch case), and -- explicitly, so it's
documented rather than silently working -- one test demonstrating the trust-boundary gap above
(a valid key for node A successfully listing an arbitrary owner's agents), with a comment
pointing at this doc so nobody mistakes "the test proves this works" for "the test proves this
is safe."

Dry-run only from my side (brace/paren balance, manual review) as usual until your build
confirms it -- no Rust toolchain in my sandbox.
