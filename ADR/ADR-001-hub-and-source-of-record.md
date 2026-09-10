# ADR-001: Hub and Source of Record

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q3, Q9, Q10, Q16 — D9, D10, D30–D36, D56, D57, D59

## Context

Hive is a distributed network of member-owned compute nodes and regional servers that collaboratively execute agentic, multi-modal project work. Some state in that system must be authoritative and tamper-resistant: member identity, the $honey ledger, project/kanban structure, the job queue, node registry, and artifact metadata. Three topologies were considered in Q3 — a central coordinator, a fully peer-to-peer ledger/CRDT, and a hybrid. A pure P2P ledger would force a consensus or blockchain-style design for a currency that is closed-loop and invite-only (see ADR-002), which is disproportionate to the trust problem.

Happy Jack Media already operates a Supabase project for Cmd Work (Postgres + Auth + Realtime + Storage, Pro tier: 8 GB DB, 250 GB/mo egress). Cmd Work is a native Swift app with no web front end yet; its schema lives entirely in `public` and uses `is_project_member(pid)` / `is_project_admin(pid)` SECURITY DEFINER helpers for RLS, with Realtime enabled on its main tables. Sign in with Apple and Google are federated to a single `auth.users` row mirrored by `public.profiles`.

The scale target (Q16) is up to 2,000 nodes on day 1 with at least five regional servers, spread across every continent. That volume rules out designs where every node holds a live Supabase Realtime subscription for control traffic or where the scheduler is a request-scoped serverless function. The hub must therefore be split into two parts: Postgres as the passive source of record, and a long-running coordinator process that mediates between the database and the node fleet.

This ADR fixes where the truth lives, how the Hive data is isolated inside the shared Supabase project, how access is enforced, and which hub-side compute runs where.

## Decision

1. **Hybrid, hub-centric topology (D9).** Supabase Postgres in the existing Cmd Work project is the single source of record for identity, `$honey` ledger, projects/cards, job queue, leases, node registry, regional-server registry, and artifact metadata. Peers execute compute and move data; they never hold authoritative state.
2. **Reuse the Cmd Work Supabase project; do not create a new one (D10).** Rationale: one auth realm, one billing line, one operational surface. The cost is shared quotas (DB size, egress), which ADR-007 (artifact-storage) mitigates by keeping bulk data off Supabase.
3. **Dedicated Postgres schema `hive` (D32).** All Hive tables, functions, triggers, and policies live in schema `hive`, never `public`. Cmd Work's grants, RLS, and Realtime publication remain untouched, and the boundary is auditable with `\dn`/`pg_policies` filtered by schema. The schema must be exposed via PostgREST (`db-schemas` setting) and added to the `supabase_realtime` publication only for the tables named in decision 8.
4. **Web app is a separate codebase and deployment (D30).** Hive's web app (ADR-009) shares only the Supabase project (DB + Auth) with Cmd Work; it does not live inside Cmd Work's UI or repo.
5. **Identity is inherited from Supabase Auth (D31).** `public.profiles.id == auth.users.id` remains the member identity. Hive adds `hive.hive_members(profile_id PK → public.profiles.id, invite_status, onramp enum('purchase','compute','infra'), wallet_id, region, created_at)`. No parallel user table.
6. **Hive projects and cards are independent tables (D34).** `hive.projects`, `hive.cards`, `hive.project_roles(project_id, profile_id, role enum('owner','admin','follower'))`. They are not Cmd Work `public.projects`/`work_items` rows.
7. **RLS model (D35).** Two helpers in schema `hive`, both SECURITY DEFINER with `search_path` pinned: `hive.is_active_member()` and `hive.project_role(pid) → role`. Policies: any row in `hive.hive_members` with `invite_status='active'` may `SELECT` every project, card, artifact-metadata row, and ledger row (Hive-internal transparency, D8). `INSERT/UPDATE/DELETE` on projects and cards require role `owner` or `admin`; followers may only `INSERT` into `hive.card_suggestions`. Ledger and lease tables are write-only via `SECURITY DEFINER` RPCs or the service role; no client policy grants direct writes to them.
8. **Realtime usage split (D59).** Supabase Realtime is reserved for web-app UI: `hive.cards`, `hive.projects`, `hive.ledger_entries` (wallet view), `hive.leases` (card status badges). Node control traffic — heartbeats, job dispatch, lease renewal, checkpoint pointers — never uses Supabase Realtime; it flows over the libp2p overlay / gRPC-over-QUIC channel to the coordinator (ADR-004).
9. **Scheduler is a persistent hub worker, not an Edge Function (D56).** The scheduler is a long-running Rust process built from the same `hive` core crate as the node, so it can speak the overlay protocol natively.
10. **Coordinator election on regional servers (D57).** The hub worker runs on a regional server, not on Vercel or Supabase. Exactly one regional server holds the coordinator role at a time, guarded by a `hive.coordinator_lease(singleton bool PK, server_id, expires_at, epoch)` row renewed on a short interval; a Postgres advisory lock is held for the duration of the renewal transaction to prevent split-brain. Any online regional server can win the lease when it lapses.
11. **Edge Functions scope (D56).** Supabase Edge Functions host only request/response work that needs authority but not long-lived state: interview turns (ADR-006), Stripe webhooks and $honey issuance (ADR-002), invite acceptance and membership on-ramp (ADR-008), minting of short-lived hub tokens on behalf of the coordinator when it is unreachable. Edge Functions never run the scheduler loop or agent loops.
12. **Cmd Work mirror is deferred and one-way (D36).** A later, opt-in sync from `hive.projects`/`hive.cards` into `public.projects`/`work_items` so Cmd Work and its agent MCP can observe Hive work. Not v1; it must never write back into `hive`.

## Consequences

### Positive
- A single authoritative Postgres removes the need for distributed consensus on money, identity, or ownership; ledger integrity is enforced by constraints and append-only RPCs (ADR-002).
- Schema isolation makes the Hive footprint in a shared database explicit and reversible: dropping schema `hive` removes every Hive object.
- Web app and node app share one auth realm with Cmd Work, so a member has one identity across Happy Jack Media apps.
- Moving the scheduler to a regional server keeps the hub cheap: Supabase is used as a database, not as a compute platform.
- The Realtime split keeps Supabase connection counts proportional to active web users, not fleet size.

### Negative
- Hive shares Cmd Work's quotas and blast radius. A runaway migration, a leaked service-role key, or a DB-size overrun affects both products.
- Two helper families (`public.is_project_member` and `hive.project_role`) coexist; engineers must not cross-reference them.
- Coordinator election adds an operational component (the lease renewal loop) that must be monitored; a stalled coordinator stalls dispatch until the lease lapses.
- PostgREST exposure of a second schema means the generated TypeScript types and the Swift client for Cmd Work must be regenerated with `hive` excluded or included deliberately.

### Risks & mitigations
- **Cross-schema leakage via RLS bugs.** Mitigation: every `hive` table has RLS enabled at creation; a CI check fails if any table in schema `hive` lacks `rowsecurity = true` or lacks at least one policy; the service role is used only by the coordinator and Edge Functions.
- **Service-role key on regional servers.** The coordinator needs write authority. Mitigation: the coordinator uses a dedicated Postgres role `hive_coordinator` with grants limited to schema `hive` (no `public` access), and credentials are rotated per coordinator epoch; volunteer-run regional servers never receive the Supabase service-role key.
- **Split-brain coordinators.** Mitigation: advisory lock + `epoch` column; every coordinator write includes its epoch and a trigger rejects writes from a stale epoch.
- **Supabase outage.** Mitigation: nodes continue running leased cards and buffer checkpoints locally; the coordinator queues ledger writes and replays them idempotently (entry ids are client-generated UUIDs).

## Open questions
- Which Postgres role executes coordinator writes — a dedicated `hive_coordinator` role (default assumption) or the service role behind a narrow Edge Function proxy?
- Should `hive` be exposed through PostgREST at all, or should the web app reach it only via RPCs in `public` that delegate to `hive`? Default: expose `hive` directly, RLS-protected.
- Does Cmd Work's existing `notifications` trigger pattern get reused for Hive notifications, or does `hive` get its own? Default: own table `hive.notifications`, same trigger style.
- Coordinator lease TTL and renewal interval (default: 15 s TTL, 5 s renewal) — to be validated against Postgres round-trip latency from the slowest day-1 regional server.
- Is Supabase Realtime needed for `hive.leases` at all, or is polling from the web app sufficient for card status badges? Default: Realtime, revisit if connection cost is material.

## Related
- ADR-002-honey-economics.md — ledger tables and issuance RPCs live in `hive`.
- ADR-003-node-core-and-backends.md — the `hive` crate shared by node and coordinator.
- ADR-004-p2p-overlay-and-regional-servers.md — coordinator election, hub tokens, node control channel.
- ADR-005-scheduler-and-leases.md — the hub worker's dispatch loop and lease semantics.
- ADR-006-agent-runtime-and-sandbox.md — interviewer Edge Function contract.
- ADR-007-artifact-storage.md — why artifacts bypass Supabase Storage.
- ADR-008-auth-and-membership.md — `hive_members`, invites, on-ramps.
- ADR-009-web-app.md — Next.js on Vercel, `supabase-js`, Realtime consumption.
- ADR-010-node-desktop-app.md — Tauri shell over the same core.
- ADR-011-ownership-and-licensing.md — license columns on `hive.projects`.
- ADR-012-scope-and-roadmap.md — Cmd Work mirror deferral.

## Appendix A — Initial `hive` schema inventory (non-normative)

| Table | Owner of writes | Realtime | Notes |
|---|---|---|---|
| `hive.hive_members` | Edge Function (invite/on-ramp) | no | keyed to `public.profiles.id` |
| `hive.projects` | web app (owner/admin via RLS) | yes | includes `license`, `requires_internet` |
| `hive.cards` | web app + coordinator | yes | typed cards, DAG via `hive.card_deps` |
| `hive.card_suggestions` | web app (followers) | no | accept/reject by admins |
| `hive.project_roles` | web app (owner) | no | owner/admin/follower |
| `hive.nodes` | coordinator | no | capabilities, `allow_internet`, `tools_level`, `region` |
| `hive.regional_servers` | coordinator | no | `status`, `region`, `storage_gb_offered`, `bandwidth_mbps` |
| `hive.coordinator_lease` | coordinator | no | singleton row, `epoch` |
| `hive.leases` | coordinator | yes | `card_id, node_id, expires_at` |
| `hive.checkpoints` | coordinator | no | pointers to artifact hashes only |
| `hive.artifacts` | coordinator | no | hash, size, replica locations |
| `hive.ledger_entries` | RPC only (append-only) | yes | see ADR-002 |
| `hive.rate_table` | admin RPC | no | effective-dated `$honey` rates |
| `hive.hub_tokens` | coordinator | no | short-lived node credentials (ADR-004) |

Realtime "yes" means the table is added to the `supabase_realtime` publication for web-app consumption only; nodes never subscribe.
