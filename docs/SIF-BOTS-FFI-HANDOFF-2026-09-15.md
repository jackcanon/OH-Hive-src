# Bots desktop bridge — integration handoff

Implemented 2026-09-15 by Sif your friendly Codex Agent. This completes the Rust/UniFFI bridge assigned in continuity; the visible chat UI and fleet acceptance remain separate work.

## Files and interface

`crates/ohhive-ffi/src/bots.rs` exports `BotsAgent`, `BotsConversation`, `BotsMessage`, `BotsPage`, `BotsSend`, and a scoped `BotsSession`. `Cargo.toml` enables the core `bots` feature; `lib.rs` registers the module.

Call `HiveNode.botsOpen()` once when opening Bots. It authenticates through the configured hub's `whoami`, derives owner and host IDs from that response, and opens the same `vault-host.sqlite3` used by the CLI. Keep the returned session for the screen lifetime. Initial opening requires a paired node and reachable hub; offline initial authentication is not implemented. Database operations run on blocking workers and do not transmit chat content to the hub. Effective configuration changes invalidate subsequent operations; this follows nodeconfig's environment-over-file precedence, so external file edits can be masked by existing environment overrides. This is not continuous server-side revocation checking.

Session operations generated for Swift:

- `ownerId()`, `hostId()` identify the authenticated session.
- `agentsList()` and `agentsCreate(name:)` list owned agents and register a local agent on this host. Creation uses the CLI's existing `default` policy placeholder; subscription agent creation is not exposed yet.
- `conversationsList()`, `conversationsCreate(agentId:)`, `conversationsJoin(conversationId:)` expose conversations. Creation makes a local DM with one owned, active agent. Joining is restricted to an already-visible owned conversation; it is not a public invitation mechanism.
- `messagesList(conversationId:page:)` accepts one before/after sequence cursor and a limit of 1–200. Follow core ordering; use `after` for incremental polling and deduplicate by message ID.
- `messageSend(draft:)` accepts text, a stable client request ID, expected conversation policy revision, exactly the DM coordinator as recipient, and optional thread root from that same conversation. Preserve the request ID when retrying a send. Refresh conversation state after policy conflicts. The author is always the session user; callers cannot spoof an agent reply or task receipt.

Bodies are limited to 64 KiB, request IDs to 200 bytes, and agent names to 256 bytes. Provider secrets are never part of these records. Return values include message identity, sequence, timestamps and artifact/reference fields already present in core.

The bridge writes the delivery outbox; it does **not** start a model runner. Claude's `hive bots work` must run against the same local store to produce replies. This does not yet route DMs to a separate machine's database. Swift bindings are regenerated; Tauri still needs its own command exposure/client UI. No room or cloud-message storage policy is introduced here.

## Findings for Claude to review

The core `bots_conversations_join` checks only conversation existence before granting membership. The bridge blocks joining another account's conversation. Please add authorization/invitation checks in core before exposing broader transports.

Core send does not validate recipient membership or thread conversation. The bridge restricts recipients to the local DM coordinator and checks thread scope. Core idempotency is conversation + request ID, returning the original message even if retry payload differs; UI must preserve a request ID only for the same logical send. Move these invariants into core as the API broadens.

Claude's delivery executor now compiles. Existing cancellation and reply/completion atomicity gaps remain; this handoff does not claim to fix them. Next work: wire a visible agents list and DM screen, decide how the app owns the delivery worker lifecycle, then verify a real local-model conversation and cancellation. Remote fleet delivery needs an explicit transport/storage design before implying that another machine can receive these local DMs.

## Verification

- Three FFI tests pass: DM create/list/join/send/read and idempotent retry; account isolation, recipient isolation and cross-conversation thread rejection; malformed inputs and stale policy rejection.
- Combined core suite: 147 passed, 1 ignored, 0 failed.
- CLI with Bots feature compiles; three existing unused core helper warnings remain.
- Apple Silicon release bridge built; UniFFI generated Swift/header successfully (optional swiftformat unavailable); native Swift release build passed. Generated bindings are ignored build artifacts and must be regenerated on other checkouts.

Changes are left uncommitted for Claude's review and integration under the shared-checkout convention.
