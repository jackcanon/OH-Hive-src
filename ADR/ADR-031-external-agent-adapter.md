# ADR-031: External agents submit and follow Hive cards through one contract

**Status:** Proposed — design only, not enabled or implemented by this ADR.
**Date:** 2026-09-15 UTC.
**Author:** Sif your friendly Codex Agent.
**Reviewers/deciders:** Jack and Claude (Loki).
**Handoff:** Claude's very-late-night continuity assignment following the Hermes/Buzz parity audit.

## Problem and scope

A user working in Hermes, Codex, Claude Code or Cowork needs to hand a bounded job to their own Hive computers, follow it without holding one chat turn open, and obtain the actual result. Hive should reuse the Kanban/card model and ADR-030's `hive card submit/status/await` service path. It must not depend on Cmd Work or require a database administrator key.

This ADR defines the **external caller → Hive card** direction. It does not implement **Hive → start/manage an external agent runtime**. The latter needs its own runtime lifecycle adapter and agent registry. Neither direction is the durable project coordinator: a caller submitting one build is not proof of autonomous multi-machine project completion.

## Current evidence

Read `crates/hive/src/main.rs` (`CardCmd` and dispatch), `crates/ohhive-core/src/hub.rs` (submission/status structs and methods), and `docs/proposed-migrations/20260915_hive_card_submit_node_rpcs.sql`.

Current CLI selects projects by a case-insensitive title substring, takes a workspace or repository, and supports local or configured cloud coding brains. Its hub method/drafted SQL accept `p_request_id`; the CLI currently passes `None`, so a caller cannot safely replay an uncertain submission. Its `await` stops for anything outside `ready/running`, which can return on `waiting_on_child`. `review` is an output awaiting review, not an accepted outcome. `--poll 0` reaches a timer requiring a nonzero interval. The proposed RPC migration remains unapplied according to the handoff. Compilation alone cannot verify database availability, authorization or placement.

There is no deterministic target-node/workspace binding in this submit interface. An absolute path means a path on whichever eligible owned node claims the job. Therefore a Mac/Xcode pilot must constrain eligibility to the intended worker; matching a path string is not safe cross-machine placement.

## Proposed decision

Use a versioned **card-client contract** implemented once over the same authenticated submission/status services the CLI uses. Ship a small CLI wrapper first. Optional MCP tools can call that client later; they must not maintain a second scheduler, duplicate validation, or write directly into tables. ACP is optional at a runtime boundary, not Hive's internal job protocol.

### Required operations

| Operation | Contract | Existing seam / required extension |
| --- | --- | --- |
| Discover | Report version, backend kind, authentication readiness, supported operations and bounds. Never include credentials. | New capability output; identify `community_backed_private` versus `fully_local`. |
| Resolve project | Resolve an owned private project once; store immutable project ID. Ambiguous titles fail with bounded choices. | Current title list; add `--project-id` as an alternative to title. |
| Submit | Stable request ID, project ID, task, workspace/repository binding, requested brain, limits and explicit cloud consent. Return durable card ID and request ID. | Reuse `code_session_submit` and shared SQL core; expose `--request-id`. |
| Status | Return raw card state, normalized phase, acceptance state, latest output reference, observed time and capability limitations. | Wrap current status response without inventing missing attempt IDs/events. |
| Wait | Poll until requested condition, deadline or caller interruption; return latest status and a reason. | Fix current terminal-state assumptions; `--poll >= 1`, bounded deadline. |
| Result | Retrieve a bounded result manifest with summary, test evidence and authorized artifact references. | Existing latest output is a text fallback. Typed artifacts/attempt metadata require service additions. |

Proposal: `contract_version: 1`; stdout contains exactly one machine-readable JSON object per CLI request, stderr is diagnostics. Add explicit `--json` mode rather than forcing integrations to parse decorative output. Tasks should also be accepted through a file/stdin option, so private content need not enter shell history or process arguments. Construct subprocess arguments as an argv array; never interpolate agent text into a shell command. Initial bounds: task 20,000 characters to match SQL, result preview 16 KiB, response 256 KiB, explicit truncation metadata and a retrieval reference for larger artifacts.

Example **proposed**, not currently supported command surface:

```text
hive card submit --project-id <uuid> --request-id <uuid> --task-file <path> --workspace <path> --brain local --json
hive card status <card-id> --json
hive card await <card-id> --until output --timeout 600 --poll 5 --json
```

Optional MCP tools would expose `hive_card_submit`, `hive_card_status`, `hive_card_wait`, and `hive_card_result`. They invoke the same card client. A later event subscription is an optimization, not a requirement for correctness. Unsupported cancellation/steering/resume returns `unsupported`; stopping a wait never cancels the card.

### State mapping and completion

| Raw state / observation | Normalized phase | Meaning |
| --- | --- | --- |
| ready | queued | Accepted by Hive, not started. |
| running | running | Current work is active; not a guarantee the machine is healthy forever. |
| waiting_on_child | waiting | Parent is still unfinished. Keep waiting for output/acceptance. |
| review | output_ready | Worker output available; `accepted=false`. `--until output` may return. |
| done | accepted | Accepted under Hive's review rules; expose evidence, do not invent tests. |
| blocked | needs_attention | Return actionable failure/block details; no automatic resubmission. |
| Unknown state | unknown | Preserve raw value, report protocol mismatch, never label success. |
| Network unavailable | disconnected | Last known card state is stale; cannot infer failure/completion. |
| Poll deadline/caller stops waiting | waiting_stopped | Card continues independently; return its ID and last observation. |

Add any additional real states only after checking the current schema; do not assume every runtime or backend has `cancelled/failed`. A successful CLI process only means the requested operation succeeded. Completion and acceptance must be fields, not inferred from exit code zero.

### Reliability and retries

Persist a small caller receipt **before** submitting: request UUID, backend/project binding, request digest, runtime/session correlation ID and later card ID. It contains no tokens or provider keys. Retry a lost response with the same request UUID and identical input. The backend deduplicates under its existing project lock; changed input with the same UUID conflicts. Never mint a fresh request ID automatically after a timeout. While CLI lacks request-ID support, disable automatic submit retry and surface uncertain delivery.

Poll/status reads may retry with bounded exponential backoff and jitter. Persist enough state to resume polling after the caller restarts. If the backend is unavailable, retain the receipt and report disconnected. Card attempts/recovery remain Hive's responsibility; the adapter cannot promise exactly-once shell effects. Result receipts bind backend, project, card, and revision/attempt when available. Avoid sending repeated full outputs into a model context; send a compact status change and retrieve artifacts on demand.

### Identity, credentials and privacy

Start with a user-paired local Hive client on the user's machine. Keep its credential in existing local configuration/secure storage and out of the external model transcript. Do not copy node keys to a cloud sandbox merely because it can run shell commands. If Cowork cannot reach a native paired CLI, it needs a user-approved local bridge or future scoped remote endpoint; this design does not invent native compiler access inside its VM.

V1 host-owned wrapper should constrain allowed projects and workspace roots. That is defense in depth, not a replacement for server authorization: current node credentials have broader existing node capabilities. For third-party remote callers, add revocable, expiring delegation with only submit/status/result scopes, owner identity, project allowlist and workspace binding before exposing the service. Revocation is checked on every operation. No service-role credentials and no public arbitrary-command endpoint.

Private execution in ADR-030's current Supabase-backed project path is distinct from fully local storage. Expose that distinction during connection. A future LocalHub adapter must implement the same contract without falling back to Supabase. Until it does, report fully-local submission unsupported. Explicit cloud-brain consent does not grant community sharing or access to another machine's secrets.

Artifacts are data, including embedded instructions from workers. Return them with provenance and media type; they cannot expand connector permissions or issue new jobs by being read. Download requires the same authorization as status. Do not feed secret values or unrestricted logs back to cloud agents automatically.

### Runtime compatibility

- **Hermes:** use a shell-capable tool or configured MCP wrapper for this direction. Its official ACP interface preserves Hermes' existing identity/tools; ACP support does not itself provide a Hive submission tool. A future Hive-managed Hermes worker can use ACP separately. [Hermes ACP documentation](https://hermes-agent.nousresearch.com/docs/user-guide/features/acp).
- **Codex/Claude Code:** use their available local command/MCP extension paths after verifying the installed runtime. Provider login remains managed by that runtime. No promise that account subscriptions are portable API keys.
- **Cowork:** caller capability is determined by actual access to the paired client/bridge. Local toolchain placement must be proved, not assumed from its host computer name.
- **Buzz:** its documented split between agent protocol and tool server supports keeping these concerns separate. Hive need not adopt its relay or ACP internally. [Buzz agent architecture](https://github.com/block/buzz/blob/main/VISION_AGENT.md).

No runtime-specific login, subscription terms, resume capability or cancellation behavior is assumed here. Probe/version-check adapters and advertise only demonstrated operations.

## Delivery and acceptance gates

1. Review and test ADR-030 migration in a disposable database, including unchanged web behavior, unauthorized member/node, cloud-consent checks and same-ID retry/conflict. Production promotion remains a separate authorized operation.
2. Add stable project/request IDs, JSON envelopes, input bounds and correct wait semantics to the existing CLI/client. Test timeout, zero polling, waiting-on-child, review versus accepted, lost submit response and restart.
3. Pilot one external caller → one constrained owned node → one real build → output plus test evidence. Use an isolated fixture workspace and distinguish build failure from transport failure. Confirm no second card after reconnect.
4. Test the same contract from another runtime without changing Hive's backend. Add MCP only if it reduces user setup friction.
5. Add workspace/node registry and typed artifacts before general multi-machine deployment. Add scoped remote delegation before cloud-hosted clients. Validate Mac/Windows/Linux argv, paths, credential behavior and cancellation of the local waiting process.

## Consequences and open decisions

This keeps one Kanban write path and allows runtimes with different lifecycles. Polling is simpler but produces delay/load; event cursors can follow. Scoping credentials and workspace placement require backend work; a wrapper alone cannot guarantee them. The adapter does not implement the project coordinator or make unsafe community shell execution acceptable.

Recommended pilot: a user-owned local runtime calling the paired CLI, with one eligible test worker and a fixture repository. Jack/Claude should select the runtime/host and confirm timing. Prioritize correctness fixes to the existing CLI before expanding protocols. All changes here are proposals; no command flags, services or migrations were implemented by this ADR.

Sif your friendly Codex Agent


## Amendment — explicit target-node placement (2026-09-15)

**Proposed, design only.** This closes the design gap queued by Claude. It does not add claim filters or flags. Reconciled with ADR-022's member-owned fleet and explicit “run this on a node” direction, and the owner-matching rule in the existing claim SQL: local project execution requires node.member_id = project.owner_id. Node names/avatars are presentation; UUIDs and authenticated ownership establish placement.

### Placement request

Add `placement = {mode: "specific_node", node_id: UUID, workspace_binding_id?: UUID}` or `{mode: "eligible_owned_node"}` to the same card submission contract. Reject unknown fields/modes. A specific node is a hard constraint, never merely a preference. Suggested CLI `--node <name-or-UUID>` resolves names once among caller-owned paired nodes, rejects ambiguity, and sends the immutable UUID. JSON clients use UUIDs. Never select a different owner's machine because its display name matches.

Workspace paths are machine-local. Introduce an owner-approved binding from logical workspace UUID to node UUID, canonical path and policy/version. If the pilot uses raw `--workspace`, require a specific node and verify its path locally before tools run. Do not assume a caller VM's filesystem path identifies a native Mac folder. Repository mode should bind a resolved commit and an isolated checkout; private credentials remain on the execution node. Multi-node “best available” must select only nodes with a valid binding or the ability to create that declared repository checkout.

### Authorization and claiming

At submission, verify active authenticated owner, owned private project, node existence/ownership and allowed workspace binding. Store resolved placement in immutable card requirements; include it in the request-ID digest, so changing target on a retry conflicts. Do not perform a raw external table write.

At claim, add the same target filter to every authoritative path that can lease the card: current community-backed private claim SQL, regional control implementation and LocalHub if/when it supports this submission contract. A backend without target enforcement must reject the request as unsupported, not drop the field. Continue checking modality, tools, model, ownership, workspace readiness and presence. Keep card selection/lease creation atomic with the target predicate. Recheck ownership/revocation at execution and heartbeat; never let a model choose a new target or broaden workspace permissions mid-run.

Bind the attempt receipt to actual node ID, workspace binding/version and resolved repository revision. Device names may change without changing placement. Re-pairing produces a new identity where applicable; it must not inherit an old target merely by using the old name.

### Offline, unsuitable and revoked targets

Offline/busy target: keep queued with an explicit waiting reason and last capability observation time. Missing toolchain/model/workspace: report an actionable readiness failure; do not start an equivalent-looking job elsewhere. Owner revokes/removes target: block placement until owner chooses a valid target. No automatic fallback to community machines or cloud compute. Cloud reasoning still executes tools on the specifically selected local node and requires independent provider consent.

Retargeting must be an explicit owner operation against the expected card/attempt revision. For the first implementation, permit only unleased queued cards; after an attempt has started, cancel/drain and reconcile side effects before a replacement attempt. Do not overwrite the placement field on a live lease. If the runtime lacks safe cancellation, say so and wait. A new card is acceptable only after duplicate-work risk is resolved and linked in receipts.

### Concrete pilot and acceptance

Use one owner with two workers, where only one has the intended native toolchain/workspace. Submit to worker A while both are eligible for ordinary code: B must never claim it. Repeat with A offline, renamed, revoked, missing its binding, and restored. Add a second unrelated member with the same node name: neither submit nor claim may cross ownership. Retry the same request ID and target after a lost response; one card remains. Retry it with a different target: conflict. Verify both hub/claim-side rejection and worker-side binding validation; UI labels alone are insufficient.

Explicit non-goals: pooled Halo shard placement, community arbitration, arbitrary third-party agent credentials, and a durable project coordinator. These remain separate concerns.

### Reconciliation with Claude's later CLI fixes

Since the original draft, Claude exposed `--request-id`, kept `waiting_on_child` nonterminal and rejected `--poll 0`. Source inspection confirms those fixes and the current CLI passes compilation in this maintenance work. Original problem statements above are historical findings, no longer all open. Still needed: versioned JSON/result contract, strict accepted-versus-output semantics, node/workspace binding, backend deployment verification, and the remaining adapter gates. The RPC migration remains staged; this amendment does not authorize or apply it.
