# Private remote coding: next implementation boundary

User goal: control private coding from Midgaard while model-heavy work runs on Overgaard by default. Midgaard model execution requires specific approval. Authority placement is pending user choice: retain Midgaard as primary and dispatch, or use Overgaard as primary. No migration or primary switch is authorized by this design note.

Current blockers in code:
- FFI private_job_context rejects a selected remote primary; it derives target from local verified node.
- PrivateCodeTaskRequest includes an immutable target, but staging/preparation are trusted LocalHubStore-only methods, not owner-authorized remote controller operations.
- Preparation uses host-local Git credentials and managed workspace paths. A path saved by one host is not evidence of a valid checkout on another.
- Existing transport exposes worker claim/complete and host-bound Bots operations; do not simply publish privileged Store methods as new RPCs.

Recommended first implementation, if primary stays on Midgaard:
1. List signed/enrolled same-owner execution hosts with stable IDs and fresh capability/model metadata. Model discovery is metadata-only. UI explicitly shows authority and execution host separately; name is not identity.
2. Owner-authenticated submission creates a staged task bound to request ID, authority, owner, target node, model, repo/ref and acceptance checks. Replays with changed fields are rejected. No arbitrary capability JSON or local filesystem path accepted from client.
3. Target worker pulls a preparation operation addressed only to its authenticated node. Checkout/token resolution happens there, then a verified preparation receipt activates the task. No source-host paths or private tokens in RPCs/audit records. Start with host-owned Git authorization; lack of it is actionable, not a fallback to Midgaard credentials.
4. Explicit run intent becomes a durable operation. Worker claims only its target and runs after validating local receipt and model tool support. A lost response is reconciled by operation ID; never blindly rerun. Do not expose arbitrary shell commands as a controller endpoint.
5. Owner/target-authorized status, stop and retry preserve lease/generation checks. Stop is task-specific; retry requires stopped/expired lease and the existing host checkout. Disconnect reports offline/unknown rather than switching hosts or DBs. Final checks execute on the same host as task files.
6. Native host/model picker, prepare/run/status controls; no local fallback. Existing tasks keep immutable original targets. Migration/re-targeting is a separate explicit operation.

If Overgaard is primary instead: project records and execution can stay colocated, but controller RPCs still need equivalent ownership, idempotency, cancellation and credential boundaries; do not move Midgaard history implicitly.

Required tests: unverified/foreign/revoked owner denial; target spoofing; path/token injection rejection; duplicate and altered run/prepare operations; lost acknowledgement; offline host; stale model metadata; two hosts cannot claim each other's work; required checks/receipts/retry on target. Real test on Overgaard with Midgaard as controller and no Midgaard inference. Existing SSH isolated test is not this product-level test.

Independent prerequisite implemented in this increment: native installed-model picker and metadata-only tool-support probe; new tasks explicitly select a model, known non-tool models excluded, run preflight rejects known incompatible/missing model before claim. Servers not reporting tool metadata show unconfirmed support rather than a false compatibility claim. Existing saved tasks retain model/fallback semantics; default resolution is pinned for a run. This does not enable remote task routing.
