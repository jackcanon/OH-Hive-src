# ADR-012: Scope, Naming and Roadmap

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q1, Q2, Q5, Q10, Q16–Q19; D1, D5, D15, D36, D56, D60, D64–D68

## Context

The interview closed with three scoping questions (Q16, Q17, Q19) that together define what "v1" means for OH Hive. Two of Jack's answers push scope up: design for 2,000 nodes on day 1 because Office Hours Global is unpredictable (D56), and support every modality on day 1 because the community is film, audio, television and radio people who will bring "anything and everything" (D60). One answer pulls it back: an explicit cut list (D67), with the mobile app promoted from "someday" to v1.1 (D68).

This ADR is the single place that records the boundary. Every other ADR describes how a subsystem works; this one says which subsystems ship in which release, in what order they should be built so the critical path is not blocked, and what the product is called. It is the document to consult before adding a feature that "would be easy" — if it is not in v1 here, it is not in v1.

Naming was settled in Q18: the product is OH Hive, the network is the Hive, the currency is $honey, and the primary domain `ohghive.com` was purchased on 2026-09-04 (D64, D65). Identifiers for code and platforms were fixed in D66 so that the scaffold can be created without another round-trip.

## Decision

### v1 scope (ships to invited members)

1. **Membership:** invite-only (D1) with three on-ramps — buy provider API credit, register compute, provide regional server hardware (D20). Any one qualifies, subject to invite. Zero-manual-step onboarding for invite + Stripe + node registration (Q16).
2. **Scale target:** 2,000 nodes and 2,000 concurrent members on day 1 (D56). Persistent Rust hub worker on an elected regional server (D57); Edge Functions for request/response only.
3. **Regional servers:** at least 5 on day 1, geo-aware (`region`) from v1 (D58), doubling as artifact store (D49) and model cache/CDN (Q17).
4. **Modalities:** text, code, image, video, audio (speech + music) all enabled (D60), each behind the one `Backend` trait so a modality can be switched off per node or per release (D61).
5. **Execution model:** distributed capacity (D14) — whole models per node, one card = one node's agent loop with checkpointing (D41, D42), sub-delegation via the hub (D44).
6. **Economics:** $honey pegged to Anthropic per-token price (D22), pure output reward (D23), local-first scheduling (D24), storage earns at a reduced rate (D50), funded pinning with grace-period return (D51).
7. **Surfaces:** web app (ADR-009), node desktop app (ADR-010), headless regional server (ADR-004). Web app mobile-responsive from v1 (D68).
8. **Ownership:** owner owns outputs; license set at creation; fork only for `open_source` (ADR-011).

### Explicit v1 exclusions (D67, D36)

9. **Distributed inference** (model sharding across peers) — v2, D15.
10. **Cmd Work mirror** (one-way Hive → `public.projects`/`work_items` sync) — later, opt-in, D36.
11. **$honey → fiat cash-out.** $honey is a closed-loop credit in v1; this simplifies tax/regulatory exposure. Revisit only on community demand.
12. **Public / non-member project pages.** Nothing leaves the Hive (D8).
13. **Training / fine-tuning jobs.** Inference and agent tool use only.
14. **Mobile app** — moved to v1.1, not dropped (D68). **Amended 2026-09-09 (ADR-021): iOS specifically is pulled forward and off this list — see below.**

### v1.1 — mobile app (D68) — **Amended 2026-09-09, ADR-021**

15. ~~iOS + Android via Expo / React Native, sharing the React component package's tokens and logic layer (D33).~~ **Superseded for iOS by ADR-021**: now that a native Swift macOS app exists (ADR-018), the standing platform call is native Swift wherever Apple offers the option — cross-platform tooling is reserved for platforms Apple doesn't cover. iOS is pulled forward from v1.1 to **shipping this week**, as a native Swift app (not a wrapper), scoped to: wallet ($honey balance + funding a project), Kanban view, project creation, agent chat, project forum read/post, and check-in/check-out of the member's registered nodes (checkout fully remote for v1; check-in surfaces as a request — see ADR-021 §4 for why). **Android is unaffected** — it has no native-Swift option, so it stays on the originally-planned Expo/React Native path, still v1.1, still sharing the React component package's tokens per D33.

### v2 — distributed inference (D15, D16) — **codenamed Project Halo (named 2026-09-09)**

16. **Evaluate exo** as the sharding layer over the existing node core; it already handles heterogeneous-device sharding. The v1 schema keeps the door open: capability records carry RAM/VRAM/bandwidth, and `hive.cards` carries a nullable `shard_plan` column from day 1 so no migration is needed. The node core's pluggable "execution mode" (D15) is where the sharded runner slots in.
17. **Project Halo, Jack's name for v2**: pooled compute lets multiple members' machines load a single larger agent "blob" together than any one of them could alone — the plain-language framing for what D15/D16 above describe more technically. Research phase starts by identifying the real technological limitations and concepts involved (network bandwidth/latency between member machines vs. exo's assumed-LAN sharding model, heterogeneous hardware/quantization mismatches across arbitrary volunteer machines, checkpoint/failure semantics when a shard-holding node drops mid-inference, and how this interacts with ADR-006's sandbox trust boundary once inference itself is distributed across machines that don't trust each other the way LAN-clustered hardware does) — not yet scoped as an ADR; first review is the 2026-09-10 10am session.

### Scope-risk mitigation: staged invites by modality (Q17)

17. If the video/audio path lags the text/code path, **invites are staged by modality**, not the modality set cut. Wave 1 invites members whose first projects are text/code/image; wave 2 opens video and music once enough video-capable nodes (Windows + NVIDIA, Apple Silicon with sufficient RAM) are registered. The eligible-nodes indicator (D63) makes the gap visible rather than silent. The modality set itself stays as D60 says: all five on day 1 of general availability.

### Suggested build order / milestones

Order is chosen so each milestone is testable end-to-end and unblocks the next. Names are working names; durations are deliberately not estimated here.

| # | Milestone | Contents | Unblocks |
|---|---|---|---|
| M0 | Scaffold | Monorepo `ohhive` (Rust workspace + pnpm workspace), schema `hive` migration skeleton, CI for macOS/Linux/Windows, shared React package stub, Cmd Work project registered | everything |
| M1 | Hub of record | `hive.*` tables, RLS with `hive_members`, append-only ledger + balance view, rate table, invite acceptance Edge Function | M2, M4 |
| M2 | Core + text backend | `ohhive-core` with `Backend` trait, llama.cpp adapter, libp2p overlay bootstrap, `hive` and `hive-server` binaries from one crate | M3 |
| M3 | Scheduler + leases | Coordinator worker with advisory-lock election, lease/heartbeat/checkpoint, capability matching incl. `allow_internet` / `tools_level`, hub-minted node tokens | M5, M6 |
| M4 | Web app alpha | Next.js on Vercel at ohghive.com, auth, kanban DAG view, wallet, Hive browser; Realtime for UI | M5 |
| M5 | Interview → project | Interviewer Edge Function with structured output (title, goal, license, requires_internet, cards[]), metered against wallet, materialises `hive.projects`/`cards` | first real card runs end-to-end |
| M6 | Agent runtime + sandbox | wasmtime tools, network shim, scratch-dir purge, checkpoint resume on a second node | trust story complete |
| M7 | Node desktop app | Tauri shell, registration flow, tray, Preferences (incl. About), earnings, check-in/out + schedule | contributor onboarding |
| M8 | Media backends + sidecar | `uv` venv, ComfyUI adapter (image, video, music), whisper.cpp, TTS; Windows CUDA CI | D60 modality set |
| M9 | Artifacts on regional servers | content-hash store, 2 replicas, funded pinning, grace/return path, pending-returns inbox, model cache/CDN | project browser media, D51 |
| M10 | Purchases | Stripe checkout, hub-as-reseller provider adapters (Claude, OpenAI, Nous), $honey issuance, provider overflow pool | third on-ramp, D17 |
| M11 | Launch hardening | ToS text live, Apple web Services ID, 5 regional servers online, 2,000-node soak test, staged invite tooling | invites go out |
| v1.1 | Mobile | Expo app per decision 15 | |
| v2 | Sharding | exo evaluation, `shard_plan` runner | |

### Naming and identifiers (D64–D66)

18. **Product:** OH Hive. **Network/community:** the Hive. **Currency:** $honey.
19. **Domain:** `ohghive.com`, purchased 2026-09-04 via Vercel (team Happy Jack Media, order `01M1R0GATRE48GQB0VM5EYB41K`, $11.25/yr, auto-renew on). Optional later pickups: `ohg-hive.com`, `ohg-hive.app`. `ohhive.com` is taken.
20. **Identifiers:** bundle/app ID `media.happyjack.ohhive`; Rust crate/workspace `ohhive`; Postgres schema `hive`; CLI binaries `hive` (node core) and `hive-server` (regional); Tauri app name "OH Hive". Source folder stays `OH Cloud-src` until the scaffold lands, then renames to `OH Hive-src`.

## Consequences

### Positive
- One page answers "is this in v1?" for every engineer and for Jack.
- Build order puts the hub, core and scheduler first, so the riskiest integration (a card actually running on a stranger's machine and paying them) is proven before any polish.
- Staging invites instead of cutting modalities honours D60 while giving the video path room to land.
- Naming and identifiers fixed now means no renames mid-scaffold.

### Negative
- "All modalities on day 1" plus "2,000 nodes on day 1" is a large v1 by any measure; the milestone list is long before a single invite is sent.
- Excluding fiat cash-out may reduce the incentive for members with lots of idle hardware but no projects of their own.
- No Cmd Work mirror in v1 means Hive work is invisible to Cmd Work's agent MCP until D36 lands.

### Risks & mitigations
- **Scope creep from the community** once they see the browser. Mitigation: this ADR is the gate; additions go to v1.1/v2 unless they replace something.
- **Video path slips.** Mitigation: staged invites (decision 17); per-modality enable switches mean the release is not blocked.
- **Day-1 surge never comes, or comes at once.** Mitigation: the 2,000-node design costs little extra at the schema level; the coordinator is a single elected process either way, and regional servers are the scaling knob.
- **Regulatory reading of $honey changes if cash-out is added later.** Mitigation: keep the ledger double-entry and rate-stamped from v1 so any later decision has clean data.

## Open questions
- Which Anthropic model tier sets the $honey reference rate? (Default: Sonnet-tier; ADR-002.)
- What is the first proof project? (Open — Q17 answered "all modalities" but did not name one. Default: none required for build; a named project would still help M5–M9 acceptance testing.)
- Will v1.1 mobile require the node app to expose remote check-in via the coordinator, or is read-only status enough for launch? (Default: remote check-in is in v1.1 scope per D68.)
- Should staged invites be by modality only, or also by region so each of the 5 regional servers has local nodes before load arrives? (Default: modality first; region as a secondary filter if a region has no server capacity.)
- When does the source folder rename to `OH Hive-src` and does Cmd Work's project record move with it? (Default: at M0 completion; Cmd Work project is renamed in place.)

## Related
- ADR-001-hub-and-source-of-record
- ADR-002-honey-economics
- ADR-003-node-core-and-backends
- ADR-004-p2p-overlay-and-regional-servers
- ADR-005-scheduler-and-leases
- ADR-006-agent-runtime-and-sandbox
- ADR-007-artifact-storage
- ADR-008-auth-and-membership
- ADR-009-web-app
- ADR-010-node-desktop-app
- ADR-011-ownership-and-licensing
