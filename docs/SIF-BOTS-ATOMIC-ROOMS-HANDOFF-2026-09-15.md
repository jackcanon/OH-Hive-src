# S-B: atomic room creation and retry receipts

The native bridge and Tauri room form now create the conversation, owner membership, complete agent roster, and retry receipt in one primary-side SQLite transaction. Failure rolls back all four. A secondary issues one `bots_rooms_create` RPC; there is no fallback to sequential create/join against older primaries.

## Contract

`LocalHubStore::bots_rooms_create(request_id: Uuid, draft: NewConversation, agents: Vec<AgentId>) -> Result<Conversation>`. The paired `LocalHub` wrapper replaces draft.owner with its authenticated owner; callers cannot select the account. RemoteLocalHub exposes the same request shape. FFI `rooms_create` now takes a UUID string request_id as its first argument. Bindings must be regenerated with the rebuilt native library.

A title and 1–16 agent entries are required. Only local-only team/project rooms are accepted; project rooms require a project reference, team rooms forbid one. Agents must belong to the owner and be unarchived at first creation; coordinator, if selected, must be in the roster. Agent validation and insertion share the write transaction. Agent IDs are sorted/deduplicated, and the title is trimmed, before binding the request payload.

Schema **13** adds `bots_room_create_receipts`, keyed by owner+request UUID, holding the normalized payload and original result. Exact retries return the original receipt, even if an agent has since been archived; retries do not re-add members or reset metadata. Reusing a key with different details is rejected. Receipts persist across primary restart and serialize concurrent requests from different database connections. New schema reservation starts at **14**.

Swift keeps the retry key across a failed attempt and ordinary reconnect in the same model session, scoped to owner/primary and normalized room details. Changed details create a new intent/key; success clears it and deduplicates the displayed room. Tauri keeps its key while the form/component stays mounted. Neither UI persists an unresolved draft/key across a full application restart: reload the room list before creating anew after restarting. The backend receipt itself is durable for clients retaining their request key.

## Integration boundary

Sif added only `mod rooms;` to Loki's bots.rs, putting implementation and tests in the new child module `local_hub/bots/rooms.rs`. Shared transport/ladder edits were announced in continuity before editing. Other changes are Sif-owned bridge/UI files and schema-version test expectations. No delivery executor, automation policy, or held-chain release changes.

Loki's CLI room-create path still uses its existing create/join sequence. Migrate that caller to this method and accept/persist a caller request UUID where retry across invocations is needed. Update the primary and secondary together; older primaries should fail the unknown atomic operation, never fall back to partially creating a room. A schema-12 binary cannot open schema-13 data; preserve ordinary database backups before rollout. No live user database was opened for this task.

## Verification completed

163 core library tests, 11 loop-safety tests and 6 route-notice tests passed against the shared working tree; 8 FFI tests passed with loopback transport enabled. Five new core room tests cover immutable receipts, transactional rollback via an injected mid-write trigger, concurrent independent connections and database reopen, schema-12 migration, and remote paired-owner enforcement/revocation. Seven Swift BotsModel tests passed, including same-key retry after reconnect and room-list deduplication. Rust release FFI built, Swift bindings regenerated/copied, and Swift app compiled in the test build. Desktop web production build and Tauri check passed. No signed app packaging or live fleet database migration performed.
