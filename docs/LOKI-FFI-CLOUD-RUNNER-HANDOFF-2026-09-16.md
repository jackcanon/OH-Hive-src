# Handoff: wire the cloud runner into the FFI so the Swift app can answer as Claude

Loki, 2026-09-16. **Sif's next item, ahead of the settings reorg.** Jack's priority is the Den
working in the Swift app, and this is the single thing standing between the app and a working
mixed room. The file is yours (`crates/ohhive-ffi/src/bots.rs`), so it is queued rather than taken.

## What is already done

- `bots::CloudTurnRunner` — yours, committed `5abfbfe`.
- `supabase/functions/bots-turn` — yours, committed, **not deployed**.
- Executor routing by runtime kind, `with_cloud_runner`, the `NoRunner` outcome, and
  `bots_report_unroutable` moved to run *after* draining so a cloud agent that just replied is
  never also told it is unsupported — mine, committed `5abfbfe`.
- CLI construction in `hive bots work` — mine, committed `5abfbfe`, and it is the working reference
  implementation for what follows.

## What is missing

`DeliveryExecutor::with_cloud_runner` is never called from the FFI. So in the Swift app, "Claude"
and "Nous" list in the roster, join rooms, resolve from `@Claude`, get a delivery row — and nothing
ever claims it. The room looks like Claude ignored you.

## The change

In `crates/ohhive-ffi/src/bots.rs`, at the `DeliveryExecutor::new(...)` call in `drain_once`
(around line 336), attach a cloud runner when one can be built. The CLI version in
`crates/hive/src/main.rs` is the shape to copy:

```rust
let mut executor = DeliveryExecutor::new(store, runner, host, owner);
if let Some(raw_key) = /* this session's verified node key */ {
    if let Ok(cloud) = hive_core::bots::CloudTurnRunner::new(
        &cfg.hub_url, cfg.anon_key.clone(), raw_key, owner,
    ) {
        executor = executor.with_cloud_runner(std::sync::Arc::new(cloud));
    }
}
```

Four things that matter, in order of how badly they bite:

1. **Owner and node key come from this session's already-authenticated configuration** — the FFI's
   own verified owner and `cfg.node_key` — never from anything a conversation supplied. The
   constructor takes a trusted HTTPS hub origin, not a caller-supplied endpoint. Your own handoff
   said this; repeating it because it is the part worth not getting wrong.
2. **A hub it cannot reach is not fatal.** Construction failure leaves local agents working and the
   unroutable notice explains the cloud ones. Do not propagate the error out of `drain_once`.
3. **Stop treating a missing local model as a blocker for cloud agents.** Today's shell preflight
   refuses to drain without a local model. A member with an Anthropic key and no Ollama install
   should still get replies from Claude. `bots_report_unroutable`'s `local_ready` argument already
   carries this distinction — pass it honestly rather than short-circuiting the drain.
4. **Two hosts draining the same BYOK agent is safe**, so no coordination is needed:
   `ensure_provider_agents` creates them with no `preferred_host`, and `bots_delivery_claim`'s
   fenced `WHERE status='pending'` UPDATE means exactly one wins when they share an authority,
   while disjoint databases have nothing to race over. Reasoning is in the executor's comments.

Bindings must be regenerated after the Rust change, and **the app rebuilt** — otherwise it is the
launch crash from queue item S-3, which cost Jack an evening already.

## Deploy `bots-turn` or it still does nothing

Verified against production: the five RPCs it calls — `hive_admin_verify_node_key`,
`hive_admin_node_member`, `hive_admin_member_active`, `hive_admin_member_key`,
`hive_admin_member_models` — **all already exist**. So it is a pure function deploy: no migration,
no ACL change, nothing to roll back but the function itself. It is not in the deployed list today
(`interview`, `export-project`, `bridge-telegram`, `generate-image`, `code-brain-turn`).

## Acceptance

In the Swift app on Midgaard, with an Anthropic key on file: create a room with one local agent and
"Claude", address both, and get two replies. Then confirm the negative case still reads honestly —
with `bots-turn` undeployed or the key removed, Claude's delivery stays pending and the room says
why rather than going quiet.

## Then the settings reorg

`LOKI-SETTINGS-REORG-2026-09-16.md`, T-1 first.
