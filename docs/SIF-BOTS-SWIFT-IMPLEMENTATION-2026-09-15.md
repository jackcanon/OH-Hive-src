# Native Bots chat implementation

Implemented by Sif your friendly Codex Agent, 2026-09-15, from `LOKI-BOTS-SWIFT-HANDOFF-2026-09-15.md`.

## Delivered

Hive.app has a **Bots** destination under **Private Fleet**. It lists owned agents, registers this Mac using its display name, finds or creates a local DM on selection, displays message history, and sends text. Other-host/nonlocal agents are labeled and cannot be messaged from this screen because remote delivery is not implemented. Messages remain in the existing local SQLite store.

`BotsModel.swift` owns the connection, selection state, drafts and retry IDs. `HiveStore` owns this model for the app lifetime and updates its pairing state from the existing snapshot refresh. A single worker task starts when paired and polls every five seconds; it survives sidebar navigation. It opens one shared in-flight/cached Bots session. Disconnect clears private state and invalidates late connection results. Reconnect explicitly drops the session and reloads. Authentication still requires the configured hub on initial open; effective nodeconfig validation follows the existing environment-over-file precedence.

`BotsView.swift` owns the visible selection task. Changing the selected agent cancels polling and fences late results. History is read in ascending pages of 200, then refreshed every two seconds. All initial pages are consumed because core's before-cursor query also sorts ascending: pretending that a single page was the latest history would hide messages. A future core newest-page API can reduce initial history cost. Send receipts never advance the read cursor, so concurrent replies cannot be skipped. Repeated attempts of unchanged text reuse the client request ID; draft edits during a send are retained. Sending uses Command-Return or the Send button, with a multiline editor, visible errors, and a local-host eligibility check.

`BotsSession.drain_once()` is a new UniFFI operation. It uses the selected `HIVE_MODEL` and configured loopback endpoint, then calls the existing `DeliveryExecutor` / `LocalModelTurnRunner`. A process-wide async gate rejects overlapping app drain passes even if a session reconnects during a turn; core delivery claims arbitrate with other processes. Swift shows missing-model, capacity and failure status. No shell worker process or separate terminal is required.

## Deliberate limits

- This is local text inference, not coding tools, remote fleet routing, rooms, cloud subscription execution or pooled Halo inference.
- The existing executor does not wire cancellation into an active model turn. Canceling a Swift waiter does not abort a claimed Rust drain pass; it finishes normally rather than leaving a half-written result. App/process termination still has the existing delivery recovery limitations. The reply-versus-completion atomicity gap also remains.
- Core drain summaries lack per-message delivery status and may hide store-read errors. The screen cannot yet show reliable per-message queued/failed/canceled badges. Its worker status is a summary, not a delivery receipt.
- Before adopting a larger fleet/UI scope, move DM find-or-create uniqueness and policy-conflict refresh into a service operation. Current create can race across independent app instances. Reconnect reloads current policy, but the first screen does not automatically resolve stale policy conflicts.
- Long initial histories currently read all pages; UI message rows are lazy, but the model keeps the loaded messages in memory.

## Verification and integration

Four Rust bridge tests pass, including a new overlapping-drain rejection test. Combined core suite: 147 passed, one ignored. Swift tests cover unchanged-send retry identity, preserving the read cursor, and disconnect versus an in-flight open. Visual verification remains outstanding: native app inspection reached the old running build after an unusually long tool wait; an isolated ImageRenderer fixture then crashed inside SwiftUICore ("no current update to enqueue action to"). That unsupported fixture was removed. No light/dark visual pass or new native-screen model round trip is claimed. See continuity for final build results.

The Apple Silicon release FFI and generated Swift/header bindings were rebuilt. Generated bindings and the app bundle are ignored artifacts; other checkouts must regenerate them. `swift test -c release` exercises the new `HiveTests` target. The existing `scripts/build-app.sh` packages the developer app; this is not a notarized/distributable release.

Claude: review and commit the named changes under the one-committer convention. This work does not alter your Tauri implementation. Next acceptance is a live native-screen send/reply using the configured local model, followed by cancellation/recovery hardening. No GitHub pushes or release publication were performed.
