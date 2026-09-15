# C1 local Bots turn runner — implementation handoff

Sif implemented `bots::LocalModelTurnRunner` behind the existing injectable `LocalBotsTurnRunner` trait. Jack authorized this work directly. Loki retains delivery claiming, bounded history assembly, reply persistence, retry state and UI/FFI work. No live fleet jobs were submitted and no running apps were restarted.

## Construction and invocation

Enable `bots,llama-cpp` (plus `local-hub` for storage). Construct on the machine that will actually run inference:

```rust
let runner: std::sync::Arc<dyn hive_core::bots::LocalBotsTurnRunner> =
    std::sync::Arc::new(hive_core::bots::LocalModelTurnRunner::loopback(
        authenticated_local_node_id,
        selected_model_id,
        "http://127.0.0.1:11434",
    )?);
let outcome = runner.run_turn_cancellable(&agent, request, cancellation_rx).await;
```

The request/outcome/error fields are unchanged. `run_turn` remains available; `run_turn_cancellable` is an additive default trait method using a Tokio watch receiver. Send `true`, or close its sender, to cancel. Dropping the future also cancels the runner and releases its capacity guard. Keep the sender alive during normal execution. Treat `NoCapacity` as queued/retry, `Cancelled` as cancellation, and `RuntimeFailed` as failure; do not write a successful reply for the latter two. Atomically check delivery cancellation again when committing a result, since a completed future and a later cancel can race in the caller.

Production construction accepts only a literal loopback IP origin, no credentials, path, query or fragment. Examples: `http://127.0.0.1:11434`, `http://[::1]:8080`. It disables proxies and HTTP redirects. Run it on the preferred host; a Midgaard process cannot reserve Heimdall's capacity by pointing at a LAN URL. Route the delivery to a process on Heimdall, which then connects to its own loopback model server. This remote delivery transport is **not** implemented here.

## Capacity design correction

`Hub::claim_card` claims a real queued card, not a generic slot. Calling it merely to reserve a DM would steal unrelated work and couple private chat to a hub unnecessarily. Instead, `execution_capacity.rs` provides an OS file-lock permit, and **Worker::tick now takes that same permit before attempting a card claim and retains it through card execution and terminal handling**. Busy workers return no work without touching the hub. Bots returns `NoCapacity` without invoking the model.

Lock location: `dirs::data_local_dir()/OHHive/execution/local-model.lock`. This is intentionally one conservative slot across all updated worker and Bots processes under the same OS account, including workers connected to different hubs. Guards explicitly unlock on drop; process death closes the handle. The lock file must never be removed/replaced while workers are running. Lock errors fail closed. This introduces no fake cards, no chat checkpoints in a community database and no changes to existing card lease/heartbeat/release rules.

**Rollout boundary:** rebuild/restart all participating worker processes with this code before relying on exclusion. Older binaries, different OS accounts, independent LM Studio/Ollama clients and other tools do not participate in this cooperative lock. It is not a cross-machine or all-users scheduler, and it deliberately limits parallel work even when a machine could host multiple models. Physical inference servers must honor the Backend contract that dropping a stream aborts generation; real Ollama/llama.cpp cancellation timing has not been measured by this fixture suite.

## Turn behavior

- Checks enabled Local runtime and exact preferred host against constructor's authenticated node ID.
- Requires same-conversation messages; at most 64 history entries and 64 KiB message-body bytes, plus an encoded prompt ceiling of 128 KiB.
- Uses agent name and bounded message text. Does not resolve capability/role references or attach library documents, credentials or attachment payloads. Caller must authorize membership, filter history/thread boundaries, and supply any approved context. This slice provides plain text replies, not tool actions or autonomous task execution.
- Checks selected text model availability. Explicit inference-only requirements avoid demanding sandbox tools for a text reply.
- Sends one Backend inference job, `max_tokens=2048`, reasoning disabled, no tool schema; absolute turn timeout 120 seconds including model discovery/streaming.
- Limits reply text to 64 KiB; rejects empty output, backend errors, missing completion and oversized SSE buffering (256 KiB). The local-only llama.cpp adapter requires `[DONE]`; legacy card-adapter completion behavior is unchanged.
- Returns backend usage via existing rough `TurnUsage` contract. The adapter can approximate from delta counts when the server omits usage; do not label those counts exact billing data.

## Verification

- `cargo test -p hive-core --features subscription-coordinator,bots,local-hub,llama-cpp --lib --quiet`: **147 passed, 1 ignored** with loopback-test access. The ignored test is the separate real Codex account probe; it is outside this task.
- Tests cover independent handles/processes contending for one slot, busy runner never invoking the model, real Worker withholding a card claim while the slot is occupied, success/usage, timeout, cancellation, dropped futures, backend start/stream errors, incomplete/empty/oversized output, wrong host/context, and remote endpoint rejection.
- A local HTTP fixture exercises the real production constructor, model discovery, SSE request/response parsing, successful reply and truncated/oversized SSE failure. It is not a live LLM or physical fleet test.
- Claude's `a5e2908` migration assertion fixes landed during this work. The combined suite now passes their preservation checks; I did not change those assertions.

- `cargo check -p hive -p hive-ffi --features hive/bots --quiet`: passed, including Claude's new agent-registration CLI. Remaining warnings are three pre-existing unused Bots persistence helpers.
- `git diff --check`: passed.

## Claude's next integration

Use the constructor and cancellation method above in your drain loop. Keep the same trait injected in your tests. Do not execute a turn on a foreign preferred host, silently fall back to cloud, or treat agent registration alone as a working DM. Preserve cancelled delivery state during reply commit and deduplicate receipts/replies. UI/FFI still needs the agent list, DM view and lifecycle status. Full-role instruction resolution, tool policy enforcement, remote delivery routing and physical model tests remain separate integration tasks.

Sif your friendly Codex Agent
