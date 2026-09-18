# Private remote coding: next implementation boundary

User goal: control private coding from Midgaard while model-heavy work runs on Overgaard by default. Midgaard model execution requires specific approval. Confirmed by Jack and Claude on 2026-09-17: Midgaard remains primary/coordinator and retains project history; Overgaard executes coding tasks, models and acceptance checks. No history migration or primary switch. Claude owns the app rename to Loki’s Den; routing changes must avoid his packaging work.

Current blockers in code:
- FFI private_job_context rejects a selected remote primary; it derives target from local verified node.
- PrivateCodeTaskRequest includes an immutable target. Owner-authenticated staging is now exposed; preparation and execution still require a durable target-worker protocol.
- Preparation uses host-local Git credentials and managed workspace paths. A path saved by one host is not evidence of a valid checkout on another.
- Existing transport exposes worker claim/complete and host-bound Bots operations; do not simply publish privileged Store methods as new RPCs.

Implementation sequence for the confirmed Midgaard-primary arrangement:
1. List signed/enrolled same-owner execution hosts with stable IDs and fresh capability/model metadata. Model discovery is metadata-only. UI explicitly shows authority and execution host separately; name is not identity.
2. Owner-authenticated submission creates a staged task bound to request ID, authority, owner, target node, model, repo/ref and acceptance checks. Replays with changed fields are rejected. No arbitrary capability JSON or local filesystem path accepted from client.
3. Target worker pulls a preparation operation addressed only to its authenticated node. Checkout/token resolution happens there, then a verified preparation receipt activates the task. No source-host paths or private tokens in RPCs/audit records. Start with host-owned Git authorization; lack of it is actionable, not a fallback to Midgaard credentials.
4. Explicit run intent becomes a durable operation. Worker claims only its target and runs after validating local receipt and model tool support. A lost response is reconciled by operation ID; never blindly rerun. Do not expose arbitrary shell commands as a controller endpoint.
5. Owner/target-authorized status, stop and retry preserve lease/generation checks. Stop is task-specific; retry requires stopped/expired lease and the existing host checkout. Disconnect reports offline/unknown rather than switching hosts or DBs. Final checks execute on the same host as task files.
6. Native host/model picker, prepare/run/status controls; no local fallback. Existing tasks keep immutable original targets. Migration/re-targeting is a separate explicit operation.

If Overgaard is primary instead: project records and execution can stay colocated, but controller RPCs still need equivalent ownership, idempotency, cancellation and credential boundaries; do not move Midgaard history implicitly.

Required tests: unverified/foreign/revoked owner denial; target spoofing; path/token injection rejection; duplicate and altered run/prepare operations; lost acknowledgement; offline host; stale model metadata; two hosts cannot claim each other's work; required checks/receipts/retry on target. Real test on Overgaard with Midgaard as controller and no Midgaard inference. Existing SSH isolated test is not this product-level test.

Independent prerequisite implemented in this increment: native installed-model picker and metadata-only tool-support probe; new tasks explicitly select a model, known non-tool models excluded, run preflight rejects known incompatible/missing model before claim. Servers not reporting tool metadata show unconfirmed support rather than a false compatibility claim. Existing saved tasks retain model/fallback semantics; default resolution is pinned for a run. This does not enable remote task routing.


## Implemented: authenticated discovery and staging foundation

`private_execution_hosts` lists only non-revoked computers with persisted signed enrollment belonging to this authority’s owner. A paired key or legacy account binding alone is insufficient. Listing is enrollment discovery, **not** a statement that a computer is online or coding-ready; fresh model metadata and host readiness are still required.

`private_code_task_stage` authenticates the calling node and verifies the target against the same owner within the same SQLite transaction as staging. Both have typed RemoteLocalHub client methods. Task input cannot supply capabilities, credentials or workspace paths; repository/ref are frozen from the authority’s project binding. Matching request replay returns the existing task; changed task/target is rejected. Staging leaves the task blocked pending preparation, so no worker can run it yet. The trusted local staging path remains compatible with existing local workflows.

Next: durable prepare/run operations pulled by the target, host-local credentials and receipt validation, status/reconciliation/stop/retry, then native execution-host selection and a live Midgaard-to-Overgaard test. No app rename files or runtime machine configuration changed in this foundation increment.


## Implemented: durable target-side preparation

Schema v16 adds `private_preparations`, after Claude’s v15 Bots lease migration. Typed authenticated request/status/take/complete operations preserve an operation ID and immutable task/target. Only the verified target may take or acknowledge work, and an acknowledgement must match the claiming session. Matching request/completion replays reconcile lost replies; changed IDs/paths and other sessions fail. Status survives authority reopen. Neither enrollment nor preparation implies that a model is running.

`RemoteLocalHub::prepare_next_private_checkout` uses execution-host-local data and Git credentials with the existing managed Git checkout/receipt/lock implementation. Credentials are never RPC fields. The helper validates/reuses an existing completed checkout following an interrupted acknowledgement; unrelated files remain intact. Success sets `awaiting_private_run` and keeps the card **blocked**. No agent runs until the separate run-intent protocol is implemented.

A claim interrupted by a worker restart currently requires explicit reconciliation; another session is intentionally not permitted to silently take over. Failure reports remain local/generic and a failed helper leaves claimed status for inspection. Recovery UI, durable run/stop/retry, worker-loop integration and native host controls remain unfinished. No live fleet configuration or app database has been changed by these fixture tests.

Verification: 108 combined LocalHub tests passed, including real HTTP and isolated real Git preparation/replay, retained files, wrong target/session/revocation rejection, no runnable card after preparation, persisted status after reopen, and upgrades through v15 to v16. Strict core Clippy passed. Logs `/private/tmp/hive-target-prepare-{suite,clippy}.log`. Claude’s concurrent Bots work contributes tests to that combined total.


## Implemented: explicit one-attempt remote Run authorization

Schema v17 follows v15 Bots leases and v16 checkout preparation. `private_run_request` records owner-approved Run intent for a prepared task; exact operation replay reconciles state, and a second operation for that task is rejected. `private_run_status` reports task state and lease activity (finished, blocked or interrupted rather than permanently claiming execution is active).

`RemoteLocalHub::for_private_run` scopes a normal Worker Hub’s claim to one approved operation. Claim validation and consumption of authorization occur in the same transaction as the card lease. The target must match the enrolled owner, authenticated computer, task and queued operation. Generic workers skip all cards in the remote preparation flow. Once consumed, the operation cannot authorize another attempt, even after lease expiry or a repair that sets the card back to ready. Existing heartbeat/output/completion fencing stays in force. Run does not carry shell commands, paths, tokens or an alternative model payload.

This is protocol/claim wiring, not the native Run button or a deployed worker loop. Target-side capability/model/workspace preflight must happen before using the scoped Worker, and no model was invoked in validation. Task-specific stop, explicit recovery/retry, host readiness discovery, native controls and the live Overgaard test remain outstanding. Until recovery is implemented, an interrupted operation stays blocked/interrupted instead of automatically replaying.


## Implemented: target Worker helper and per-run Stop

Schema v18 adds durable `private_run_stops`, following v15/v16/v17. Owner-authenticated Stop is idempotent and bound to an operation; queued work becomes blocked, active work reports stopping until the lease is gone. Completed work remains finished. Atomic claim checks reject stopped operations. Other tasks are unaffected.

`private_run_work` exposes preflight data only to the assigned enrolled target. `RemoteLocalHub::execute_private_run` requires local coding permission, an explicit installed tool-capable (or metadata-unconfirmed) model, and the target's managed prepared checkout. It uses the existing Worker with the operation-scoped remote Hub, rechecks the checkout through existing locks/receipts, and polls for Stop while handling local stop signals. Loss of authority contact signals local cancellation and returns the error, never changing authority or execution host. This helper executes one explicit operation; it is not an automatically installed background loop or native button wiring. Model capabilities are discovered from the supplied host-configured endpoint; hardware probing remains a host-app concern.

Stop responsiveness includes a one-second poll and the existing remote request timeout; it is cooperative, not an immediate process-kill guarantee. If offline, the primary cannot prove the remote process has stopped until communication/lease reconciliation. Local cancellation or transport failure can leave blocked/interrupted state requiring explicit recovery; no automatic new attempt is authorized.

Verification: 109 LocalHub tests passed. The added test runs the real Worker against an isolated HTTP model simulator and Git checkout, checks local tool opt-in before claim, waits for the simulated model request, requests Stop through the controller RPC and observes stopped/no active lease. No actual model inference. Tests also prove queued Stop replay and isolation from another queued task. Logs `/private/tmp/hive-run-worker-{tests,suite,clippy}.log`. Outstanding: recovery/retry, host selection/readiness, native integration and live Overgaard validation.


## Implemented: explicit run retry with retained history

Schema v19 removes the per-card Run uniqueness constraint and adds `private_run_retries` linking the retired attempt to its successor. Existing run/Stop records survive the migration; foreign keys are disabled only for table reconstruction, validated before commit, and reenabled. Each task may have historical attempts; only its current authorization can execute.

`private_run_retry(previous, next)` is owner-authenticated and idempotent, requires a fresh operation ID, rejects active leases, finished output, child work and checkpoints needing review. It retains the same card, target, model, checkout and acceptance checks, and blocks the card at `awaiting_retry_validation`. Target-only `private_run_ready` permits claim only after the helper validates the local checkout again. Replaying an old Stop cannot stop a newer run. Older attempts report superseded and do not borrow the current attempt’s lease activity.

Each scoped remote Worker gets a fresh session. Claim additionally rejects a session previously used for another attempt of the same task, so a delayed old completion cannot own a new lease. Model preflight and host-local workspace locks still apply. Initial Run and Retry are distinct: ordinary Run does not create another attempt of an existing task.

Verification: 110 LocalHub tests and strict core Clippy passed. Real Worker + mock model stop/retry cycle preserves existing files, revalidates the checkout and leaves old Stop isolated from the new run; direct tests reject active retry, altered replay, stale-session claim/completion and retry of finished work. Migration preserves run/Stop records and enforces foreign keys afterward. Logs `/private/tmp/hive-retry-{suite,clippy}.log`. No actual model inference.

Remaining: recovery of interrupted **preparation** (before any Run), host readiness/pickers, native buttons and worker invocation, then live Overgaard validation. This completes the run-retry backend increment, not the whole desktop workflow.


## Implemented: owner-requested interrupted preparation recovery

Schema v20 adds durable `private_preparation_recoveries`. `private_preparation_recover(request, operation)` records explicit owner intent to retire a claimed session and requeue setup on the same target. Replaying that recovery request returns current status without resetting a replacement worker’s claim. The retired session cannot claim or complete this preparation again. Recovery refuses completed/queued preparations, conflicting request identities, or tasks that already have execution operations/leases. A matching historical recovery replay remains read-only after completion.

No checkout is deleted/reset. The replacement worker acquires the existing managed lock and validates the receipt/branch/root before acknowledging completion. If the old process still holds the lock, replacement preparation fails safely until that lock is released. This is owner-directed recovery, not automatic dead-process detection. The UI should reconnect the target’s preparation client before retrying after its session is retired.

Verification extends the real HTTP/Git fixtures with retired-worker rejection, duplicate recovery before/after replacement claim and after completion, active checkout-lock contention, file preservation and resumed preparation followed by normal Run/Stop/Retry. Full LocalHub suite: 110 passing. Logs `/private/tmp/hive-preparation-recovery-{suite,clippy}.log`. Remaining product work: host readiness/selection, native controls and target-worker invocation, then live Midgaard-to-Overgaard verification.
