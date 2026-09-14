# Control-plane migration off direct RPC — implementation plan

By Loki Sonnet Hive/Halo, 2026-09-13. Scoping only, per ADR-000 D72 / ADR-013 §B (decisions 9-10)
— those already made the *decision* (nodes must move off direct Supabase RPC to a coordinator over
`/hive/ctl/1` before 200 active nodes, alert at 150). This document turns that decision into a
concrete, phased build plan. No code changes in this document.

## Where things actually stand today

Checked live against the database: **8 of 9 registered nodes were active in the last hour.** The
150-node alert is nowhere close to firing. This is not an emergency — it's exactly the kind of
work ADR-013's own risk note anticipated ("the migration work item is already open and blocks the
invite wave"): scope it now, before the invite wave, so it isn't discovered under load.

Checked against the actual code, not just the ADR text — the gap is bigger than "swap the
transport." **None of ADR-004's control-plane machinery exists yet:**

- `crates/hive-coordinator/src/lib.rs` is real but is *only* the placement algorithm — "given a
  snapshot of nodes and jobs, which node should get this job." It is deliberately I/O-free (its
  own doc comment says so) and is not wired to anything live. It has no HTTP/gRPC server, no
  heartbeat ingestion, no lease management.
- There is no `/hive/ctl/1` protocol implementation anywhere — no libp2p, no gRPC control channel,
  no hub-token minting/verification. `hub.rs`'s `coordinator_try`/`coordinator_release` are pure
  Postgres row-lock RPCs (who *holds* the coordinator lease), not a control channel nodes talk to.
- Every node control operation today — check-in, heartbeat, claim, checkpoint, complete, fail,
  release — is exactly what ADR-013 calls it: a direct PostgREST RPC from the node straight to
  Supabase, authenticated by node key, no coordinator in the loop at all.

So this isn't "point nodes at a different URL." It's building the coordinator's control-channel
surface for the first time, then migrating traffic onto it.

## Recommended approach: HTTP through regional servers first, not full libp2p on day one

ADR-004 specifies libp2p/QUIC/DHT/NAT-traversal for the full `/hive/ctl/1` channel. That's the
right end state for a fleet with heavy relay/NAT needs, but it's a large, separate engineering
program (new transport stack, peer discovery, hole-punching) that doesn't need to be built before
the 150-node trigger — regional servers (`hive-server`) already have public URLs and already run a
web server (the `/a` blob endpoint, per ADR-004 decision 9). Reusing that is a much smaller lift
that fully satisfies ADR-013 D72's actual requirement — "nodes check in to the coordinator, not
Postgres directly" — without waiting on the libp2p work. The full overlay migration remains valid
future work if/when relay and NAT-traversal needs (not control-plane load) demand it; nothing here
forecloses it, since ADR-004's protocol-id/message shapes are kept as the wire contract regardless
of transport.

## Phased plan

**Phase 1 — Give `hive-coordinator` a body.** Add a thin HTTP (not gRPC-over-libp2p yet) server to
`hive-server` exposing the ADR-004 §12 control operations (check-in/out, heartbeat, capability
update, lease claim/renew/release, checkpoint pointer) as REST/JSON endpoints, backed by
`hive_coordinator::place()` for scheduling decisions and a Postgres connection *the regional
server itself holds* (not the node). This is additive — it can run alongside today's direct-RPC
path with zero risk to existing traffic.

**Phase 2 — Hub tokens.** Implement ADR-004 decision 6: a node authenticates to a regional server
once (existing node-key flow), receives a short-lived (≤15 min) signed hub token scoped to its
`node_id` and current leases, and uses that token for every subsequent control call to that
regional server instead of presenting its long-lived node key on every request. Store issued
tokens in a new `hive.hub_tokens` table (already named in ADR-004 decision 6, not yet created).

**Phase 3 — Heartbeat batching.** The regional server aggregates heartbeats from its connected
nodes and writes them to Postgres in batched `UPDATE ... FROM unnest(...)` statements at ≤1 Hz
(ADR-004 decision 7), instead of each node hitting `hive_node_heartbeat` individually. This is the
change that actually removes the O(nodes) PostgREST load ADR-013 is worried about.

**Phase 4 — Cut over and lock the door.** Once a full heartbeat/check-in/claim/complete cycle is
proven end-to-end through a regional server for a real project (not a synthetic test), flip the
node binary's default control endpoint from Supabase directly to "nearest online regional server,"
using the existing bootstrap-seed-list mechanism (ADR-004 decision 5) nodes already have for
artifact fetching. Only after that's live and stable: revoke the `public.hive_node_*` PostgREST
grants from the authenticated/anon roles (ADR-013 D72's explicit end state), so direct Postgres
access from a node is no longer possible even as a fallback.

## What this touches

`crates/hive-coordinator/` (gains the HTTP server), `crates/hive-server/` (hosts it), `crates/
ohhive-core/src/hub.rs` (gains a coordinator-transport variant alongside `SupabaseHub` — the
`Hub` trait from ADR-025 makes this a third implementation of the same interface, not a parallel
one-off), Supabase migrations (new `hive.hub_tokens` table; eventually revoking `hive_node_*`
grants). Does **not** touch `worker.rs`'s or `coder.rs`'s step-loop logic — same as ADR-025, this
is a transport swap behind the `Hub` trait, not agent-loop behavior.

## Sequencing note

This deliberately depends on ADR-025's `Hub` trait landing first (same file, `hub.rs`, gains a
third implementation here) — queuing this as Sif's next assignment after the local-hub package is
the natural sequence, not a coincidence: she'll already have full context on the trait boundary
from building `LocalHub` against it. Logged as a queued handoff in `CONTINUITY.md`, not yet
assigned as active work.

## Open questions carried over from ADR-004 (unresolved by this plan, inherited as-is)

- Web-app gateway for streaming artifacts from the overlay (ADR-004's own open question — orthogonal to control-plane traffic, not addressed here).
- Hub-token TTL/refresh mechanics: piggyback on heartbeat (ADR-004 default) vs. a separate refresh call — recommend starting with piggyback, per the ADR's stated default, and revisiting only if it proves insufficient.
- Whether Phase 1's HTTP transport is ever replaced by real libp2p/QUIC, or whether HTTP-through-regional-servers simply becomes the permanent v1 control channel and ADR-004's fuller overlay is reserved for bulk data only (which is arguably already true in practice, since artifacts/models already go through `/hive/blocks/1`-equivalent HTTP endpoints today, not libp2p). Worth a follow-up ADR once Phase 4 ships and the team can evaluate whether libp2p's NAT-traversal/relay benefits are actually needed for control traffic specifically.
