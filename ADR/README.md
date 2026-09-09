# OH Hive — Architectural Decision Records

**Product:** OH Hive · **Network:** the Hive · **Currency:** $honey · **Domain:** ohghive.com
**Owner:** Jack Blair, Happy Jack Media · **Architect/scribe:** Loki · **Started:** 2026-09-04

OH Hive lets members of Office Hours Global and Loki's Lab pool their idle computers into one
distributed AI compute network. Members earn $honey by contributing compute, storage, or purchased
API credit, and spend it on agent-driven projects (text, code, image, video, audio) run on a
community kanban.

## Index

| # | ADR | Covers |
|---|-----|--------|
| 000 | [Interview log](ADR-000-interview-log.md) | Verbatim Q&A, 19 questions, decisions D1–D68. Source of truth for everything below. |
| 001 | [Hub & source of record](ADR-001-hub-and-source-of-record.md) | Supabase (Cmd Work project), `hive` schema, RLS, coordinator, Edge Function scope |
| 002 | [$honey economics](ADR-002-honey-economics.md) | Anthropic-token peg, append-only ledger, rate table, funded pinning, closed loop |
| 003 | [Node core & backends](ADR-003-node-core-and-backends.md) | Rust core, one core two shells, `Backend` trait, llama.cpp/MLX/ComfyUI/whisper, Python sidecar |
| 004 | [P2P overlay & regional servers](ADR-004-p2p-overlay-and-regional-servers.md) | libp2p, self-healing relays, regions, bootstrap, artifact + model cache |
| 005 | [Scheduler & leases](ADR-005-scheduler-and-leases.md) | Coordinator election, capability matching, leases/heartbeats, card DAG, child jobs |
| 006 | [Agent runtime & sandbox](ADR-006-agent-runtime-and-sandbox.md) | Node-owned agent loop, checkpointing, wasmtime sandbox, internet opt-in, interviewer contract |
| 007 | [Artifact storage](ADR-007-artifact-storage.md) | Content-addressed on regional servers, replication 2, grace → return to owner |
| 008 | [Auth & membership](ADR-008-auth-and-membership.md) | Supabase Auth, invite-only, three on-ramps, Owner/Admin/Follower, ToS |
| 009 | [Web app](ADR-009-web-app.md) | Next.js on Vercel, interview, kanban, wallet, Hive browser, shared React package |
| 010 | [Node desktop app](ADR-010-node-desktop-app.md) | Tauri "OH Hive", registration flow, trust toggles, earnings, pending returns, About |
| 011 | [Ownership & licensing](ADR-011-ownership-and-licensing.md) | Owner owns outputs, `owner_only` vs `open_source`, inspect ≠ reuse |
| 012 | [Scope & roadmap](ADR-012-scope-and-roadmap.md) | v1 scope and exclusions, v1.1 mobile, v2 distributed inference, build order |
| 013 | [Cost, capacity & hosting](ADR-013-cost-capacity-and-hosting.md) | **Proposed.** Supabase as trust store only, no Realtime for `hive.*`, coordinator broadcast + snapshot, provider spend from purchased $honey only, static web on Cloudflare Pages, cloud as scripted standby tier, capacity table, observability (D69–D79; amends 001/002/004/005/007/009/012 on acceptance) |
| 014 | [Project categories & verification](ADR-014-project-categories-and-verification.md) | 9-category taxonomy, full specs for Software/Research, Triangulated Card Verification, sub-delegation is 1:1 not N-way fan-out |
| 015 | [Local workstation & Hive promotion](ADR-015-local-workstation-and-hive-promotion.md) | `execution_mode='local'`, free/no-ledger own-machines-only execution, full machine access, separate local agent engine, movable-per-project promotion to Hive |
| 016 | [Hub portability & local-fleet independence](ADR-016-hub-portability-and-local-fleet-independence.md) | Three hub tiers (community/personal-cloud/fully-local), boring-Postgres schema discipline, desktop-bundled local Postgres goal |
| 017 | [Cloud compute pool](ADR-017-cloud-compute-pool.md) | Purchased-Honey-only funded third-party API execution via a `cloud_pool` node, per-project opt-out, revenue-backed pool replacing the flat provider budget cap |
| 018 | [Native macOS shell](ADR-018-native-macos-swift-shell.md) | Swift/SwiftUI app for macOS over a shared Rust core via UniFFI; Windows/Linux stay on Tauri; Apple Foundation Models (macOS 27) as a native local inference option |

## Conventions

- Status lifecycle: Proposed → Accepted → Superseded. All ADRs are **Proposed** pending Jack's review.
- Decision IDs `D1`–`D68` refer to ADR-000. New decisions get the next D-number and are appended to ADR-000 first, then reflected in the topical ADR.
- Open questions live in each ADR's "Open questions" section with a default assumption; defaults are what the scaffold implements until overridden.
- Every ADR also exists as a Decision record in Cmd Work (project: OH Hive).

## Next

1. Jack reviews and flips statuses to Accepted (or edits).
2. Scaffold the monorepo per ADR-003/009/010/012 build order.
3. Fleet distribution: each ADR is self-contained enough to hand to a separate agent.
