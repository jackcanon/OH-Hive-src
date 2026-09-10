# ADR-005: Scheduler, Leases and Card Dispatch

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q5–Q7, Q11–Q13, Q16–Q17; D14, D17, D18, D24, D40–D44, D46–D48, D56–D58, D63

## Context

Hive's unit of work is a **card** on a project kanban. Cards are not single prompts: each one is an agentic, multi-modal task (text, code, image, video, audio) produced by the interviewer/planner agent (ADR-006) and typed with its modality, required capabilities and dependencies (D40). A card is executed by exactly one node running a full local agent loop (D41), with checkpoints written back at step boundaries so another node can resume it (D42). The scheduler's job is to match cards to nodes that can actually run them, hand out and police leases, and account for the tokens generated along the way so contributors are paid (ADR-002).

Two compute pools serve the same currency (D17): Hive-registered local nodes and third-party provider APIs (Claude, OpenAI, Nous, etc.) behind one provider-agnostic adapter layer whose keys live on the hub (D18). Local compute is the point of the network (Q7), so the scheduler is **local-first**: provider APIs are overflow and fallback, never the default (D24).

Scale forces the shape of the scheduler. The community could produce 2,000 nodes on day one (Q16), and video jobs can run 10–60 minutes (Q17). A request-scoped Edge Function cannot hold leases, reap heartbeats, or keep a DAG in memory across that horizon. The scheduler is therefore a **persistent Rust worker** built from the same `hive` core crate as the node, run on a regional server, with one elected **coordinator** holding a Postgres advisory lock so any regional server can take over (D56, D57). Supabase (schema `hive`) remains the source of record (ADR-001); the coordinator is the only writer of scheduling state and the only thing nodes talk to for control traffic (D59, ADR-004).

Node capability is uneven and policy-laden. Nodes advertise per-modality capability (D63), hardware, available models, a whole-node `allow_internet` flag (D46) and a `tools_level` (D48); cards declare `requires_internet` (D47). A card must never be placed on a node that cannot run it or that has not opted in to the access it needs.

## Decision

1. **Persistent coordinator on an elected regional server (D56, D57).** The scheduler is a long-running process in the `hive-server` binary. Exactly one instance is active: it acquires `pg_try_advisory_lock(hashtext('hive.coordinator'))` on a dedicated session and records itself in `hive.coordinator_lease(server_id, acquired_at, heartbeat_at)`. Standby regional servers retry every 5 s; if the lock holder's session dies, the lock releases and a standby wins. Edge Functions do not schedule; they only enqueue (interview turns, invite acceptance, Stripe webhooks).
2. **Two pools, one queue, local-first (D17, D24).** Every card is enqueued into `hive.cards` regardless of pool. On each scheduling tick the coordinator tries Hive nodes first; a card overflows to the provider pool only when (a) no eligible node exists or is expected within the card's `max_wait`, or (b) the project/owner has explicitly allowed provider execution for the card. Provider execution goes through the hub-side adapter layer (D18); nodes never hold provider keys.
3. **Capability-first, region-second matching (D58, D63).** Eligibility is a hard filter over `required_capabilities` (modality, model, min RAM/VRAM, backend), `requires_internet` ⇒ `node.allow_internet`, and `tools_level` compatibility (a card needing tools cannot land on an `inference_only` node). Ranking among eligible nodes prefers: same `region` as the card's artifacts and project owner, then lowest current load, then longest idle time. Region never overrides capability.
4. **Card schema carries the matching inputs (D40, D47).** `hive.cards` includes `required_capabilities jsonb`, `requires_internet bool not null default false`, `tools_level text check (tools_level in ('inference_only','sandboxed_tools'))`, `modality text`, `depends_on uuid[]`, and a nullable `shard_plan jsonb` reserved for distributed inference (D15) so v2 lands without a migration. v1 always writes `shard_plan = null` and the scheduler ignores it.
5. **Leases with heartbeats and per-modality timeouts (D41, D42).** A node claims a card by the coordinator inserting `hive.leases(card_id, node_id, issued_at, expires_at, heartbeat_at, segment_no)`. Nodes heartbeat to the coordinator every 10 s over the overlay; the coordinator batches heartbeat writes to Postgres (one upsert per tick, not per node). Lease TTL is per modality: text/code 120 s, image 300 s, audio 300 s, video 900 s (initial defaults, config-driven in `hive.modality_policy`). A lease whose `heartbeat_at` is older than its TTL is reaped; the card returns to `queued` with `resume_from = last checkpoint`.
6. **Card DAG drives readiness (D40, D44).** A card is `queued` only when every id in `depends_on` is `done`. The kanban is a view over the DAG: `Backlog` = blocked or unfunded, `Ready` = queued, `In Progress` = an active lease exists, `Review`/`Done` per acceptance. Parallelism comes from independent DAG branches, never from the hub splitting a prompt.
7. **Child jobs for sub-delegation (D44).** A running agent loop may request a capability its node lacks (e.g. text node needs an image). The node core submits a **child card** via the coordinator with `parent_card_id`, `project_id` inherited, and its own `required_capabilities`. The child is scheduled like any card; its result artifact hash is returned to the parent's loop, which is checkpointed as waiting on the child. Children inherit the parent's `requires_internet` unless narrower; they can never widen it.
8. **Admin dedicated compute and priority (D6, Q2).** `hive.project_roles` rows with `role='admin'` may bind a node they own to a project via `hive.compute_reservations(project_id, node_id, priority)`. A reserved node only accepts cards from that project while the reservation is active; the coordinator treats reservation as a pre-filter before general matching. Project priority is otherwise proportional to allocated $honey (D2): the tick orders `queued` cards by `funded_balance / estimated_cost` descending, then age.
9. **Per-lease token metering (D23, D42 implication).** Usage is recorded per `(lease_id, segment_no)`, not per card. `hive.usage(lease_id, node_id, input_tokens, output_tokens, compute_seconds, hardware_class, modality, rate_id)` is written by the coordinator from the output it receives, never from node self-report alone. A resumed card pays each node for the segment it ran. Non-token modalities record `compute_seconds × hardware_class` and are converted by the economics rate table (ADR-002).
10. **Control plane stays off Supabase Realtime (D59).** Node ↔ coordinator traffic (heartbeats, lease grants, checkpoint pointers, child-job submission) uses the libp2p overlay / gRPC-over-QUIC channel from ADR-004. Nodes authenticate to the coordinator with short-lived hub tokens the coordinator mints; they hold no Supabase write JWT.

Schema sketch (schema `hive`):

```sql
create table hive.cards (
  id uuid primary key, project_id uuid not null references hive.projects(id),
  parent_card_id uuid references hive.cards(id),
  modality text not null check (modality in ('text','code','image','video','audio')),
  required_capabilities jsonb not null default '{}',
  requires_internet boolean not null default false,
  tools_level text not null default 'sandboxed_tools'
    check (tools_level in ('inference_only','sandboxed_tools')),
  depends_on uuid[] not null default '{}',
  shard_plan jsonb,                       -- null in v1 (D15)
  status text not null default 'backlog',
  resume_from uuid references hive.checkpoints(id)
);
create table hive.leases (
  id uuid primary key, card_id uuid not null references hive.cards(id),
  node_id uuid not null references hive.nodes(id), segment_no int not null default 1,
  issued_at timestamptz not null default now(), heartbeat_at timestamptz not null default now(),
  expires_at timestamptz not null, released_at timestamptz,
  unique (card_id) where released_at is null      -- one live lease per card
);
create table hive.coordinator_lease (
  singleton boolean primary key default true, server_id uuid not null,
  acquired_at timestamptz not null, heartbeat_at timestamptz not null
);
```

Scheduling tick (pseudocode, runs every 1 s on the coordinator):

```
reap_expired_leases()                       -- heartbeat_at + ttl(modality) < now
promote_ready_cards()                       -- all depends_on done, project funded
for card in queued ordered by (reservation, funded_ratio desc, created_at):
    nodes = eligible(card)                  -- capability, internet, tools, reservation
    if nodes: grant_lease(card, rank_by_region_then_load(nodes)[0])
    elif overflow_allowed(card): dispatch_provider(card)
flush_batched_heartbeats(); flush_usage()
```

## Consequences

### Positive
- One long-running process holds the DAG, leases and heartbeats in memory; Postgres sees batched writes (≈1 upsert/s for heartbeats instead of 200/s at 2,000 nodes).
- Failover is free: the advisory lock plus a standby loop on every regional server reuses the self-healing story from ADR-004 (D28) with no new infrastructure.
- Capability-first matching with explicit `requires_internet`/`tools_level` guarantees a contributor's opt-outs are honoured by construction, not by convention.
- Per-lease metering makes resumed cards pay fairly and gives the economics ADR a clean primary key for fraud spot-checks.
- `shard_plan` reserved now keeps the v2 distributed-inference path migration-free.

### Negative
- A single active coordinator is a throughput ceiling and a single point of latency; 2,000 nodes × 10 s heartbeats is fine, but a second tier (per-region sub-coordinators) will be needed before ~20k nodes.
- Lease reaping on long video jobs (15-minute TTL) means a crashed node can strand a card for up to 15 minutes before resumption.
- Provider overflow silently changes cost per card; owners must see which pool ran a card or the $honey burn will surprise them.
- Child jobs add hub round-trips inside an agent loop; a chatty loop could fan out many small cards.

### Risks & mitigations
- **Split brain during failover.** Mitigation: the advisory lock is held on a dedicated Postgres session with `statement_timeout`; the coordinator row is updated only by the lock holder, and every lease write includes `server_id` checked against `hive.coordinator_lease`.
- **Heartbeat storms / thundering herd on coordinator restart.** Mitigation: nodes jitter heartbeats ±2 s and back off exponentially when the coordinator is unreachable; the new coordinator rebuilds state from `hive.leases` before accepting claims.
- **Capability lies (node advertises VRAM it lacks).** Mitigation: nodes are probed at registration (ADR-003) and after N failed leases a capability is downgraded automatically.
- **Starvation of rare modalities (video).** Mitigation: web app shows eligible-node counts per card (D63); the coordinator raises overflow eligibility for cards older than `max_wait`.
- **Metering fraud.** Mitigation: usage derives from hub-received output and rate table, with replay spot-checks (ADR-002); invite-only membership (D1) lowers but does not remove the risk.

## Open questions
- What is the per-modality lease TTL and heartbeat interval in production? Default assumption: text/code 120 s, image/audio 300 s, video 900 s, heartbeat 10 s ± 2 s jitter.
- When may a card overflow to a provider API without owner opt-in? Default assumption: never; overflow requires a per-project `allow_provider_overflow` flag set by the interviewer.
- How is "expected eligible node within `max_wait`" computed when nodes have schedules (D4)? Default assumption: read `hive.node_schedules` and treat a node scheduled to check in within `max_wait` as pending-eligible.
- Do admins' reserved nodes still earn $honey at the full rate for their own project's cards? Open — economics ADR-002.
- Should regional coordinators (per-region sharding) be designed in v1 or deferred? Default assumption: deferred; single coordinator with a documented ceiling.
- Which Anthropic model's price is the reference rate applied in `usage.rate_id`? Open — ADR-002 (default Sonnet-tier per D22).

## Related
- ADR-001-hub-and-source-of-record
- ADR-002-honey-economics
- ADR-003-node-core-and-backends
- ADR-004-p2p-overlay-and-regional-servers
- ADR-006-agent-runtime-and-sandbox
- ADR-007-artifact-storage
- ADR-009-web-app
- ADR-010-node-desktop-app
- ADR-012-scope-and-roadmap
