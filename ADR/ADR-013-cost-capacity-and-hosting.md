# ADR-013: Cost Posture, Capacity Planning and Hosting Tiers

**Status:** Proposed · **Date:** 2026-09-05 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** Cost review of 2026-09-05 (Supabase price sheet, live Cmd Work project inspection, ADR-001/002/004/005/007/008/009); new decisions D69–D79 appended to ADR-000.

## Context

The Hive shares the Cmd Work Supabase project (Pro, Micro compute, 60 direct connections, 18 MB used, ADR-001 D10). A review against Supabase's current price sheet found that the plan as written is cheap at rest and unbounded under load, and that the failure mode is not a bill but an outage: the Pro spend cap is on by default, so a Hive spike would throttle Realtime or the database for Cmd Work too.

Three findings drove this ADR.

1. **Realtime fan-out.** Supabase Realtime bills per message, where messages = row changes × subscribers, and RLS is evaluated per row per subscriber on the database instance. `hive.nodes` is currently in the `supabase_realtime` publication. ADR-004 has the coordinator batch-writing node presence at ~1 Hz. At 2,000 nodes and 2,000 browser sessions that is millions of messages per second — five-figure overage or a shut-off. `hive.leases` (planned for Realtime in ADR-001 D59) has the same shape.
2. **v0 control plane hits Postgres directly.** The shipped v0 (`hive work --poll 5`, heartbeats every 30 s via PostgREST RPCs, pg_cron housekeeping, web pages polling every 5–10 s) is correct and convenient but puts every node on the database. At 2,000 nodes that is ~500 RPC calls/s through RLS on a 1 GB instance. ADR-004/005 already say control traffic goes through the coordinator; v0 predates the coordinator.
3. **Provider API spend is the only real money.** The interview Edge Function calls Anthropic directly for every project, and members who joined via the compute on-ramp pay for that with *earned* $honey, which is backed by nothing Happy Jack Media was paid. Provider overflow (ADR-005) has the same exposure.

Jack's constraint: count on volunteers who can provide **resources** (machines, disk, bandwidth, uptime), not funding. Paid cloud is a backup that may become necessary, never the plan. Day-1 regions are North America, Europe and Asia-Pacific.

## Decision

### A. Supabase is the trust store, and only the trust store (D69, D70)

1. Supabase holds what must be authoritative and tamper-resistant: `auth.users`, `hive.members`, the append-only ledger and rate table, project/card/role structure, node and server registries, artifact metadata, the coordinator lease, provider keys (Vault) and Stripe secrets. Nothing else.
2. **No Supabase Realtime for the Hive.** Remove `hive.nodes`, `hive.projects`, `hive.cards` from the `supabase_realtime` publication. Do not add `leases` or `ledger_entries`. ADR-001 decision 8 and ADR-009 decision 6 are superseded.
3. **Rule:** any table written by the coordinator on a timer (presence, heartbeats, lease renewals, usage) is never in a Realtime publication. The existing CI check (every `hive` table has RLS) gains a second assertion: `pg_publication_tables` contains no `hive.*` rows.
4. **Live UI state comes from the coordinator.** Regional servers expose `wss://<server>/live/<project_id>` (and `/live/wallet/<member>`), fed by the coordinator over the overlay, authenticated with the hub-signed token already used for artifact fetches (ADR-004 §6). The web app subscribes there for kanban, lease badges, eligible-node counts and wallet deltas; it reads Supabase for the member's own rows and writes through RLS. Polling at 15 s remains the fallback.
5. **Hive-wide reads come from a snapshot.** The coordinator publishes a materialized read-all snapshot (projects, cards, artifact metadata, rate table, per-project eligible-node counts) as a content-addressed artifact every few seconds; regional servers serve it via the gateway. The Hive browser (D8) reads the snapshot, not Postgres.
6. **Ledger stays small.** Hot window 90 days in `hive.ledger_entries`; the nightly job exports older entries as a signed Parquet artifact (`kind='ledger_archive'`, replication 3) and inserts a per-account checkpoint balance. Balances remain re-derivable from checkpoint + hot entries + archives.
7. **Backups live on the overlay.** Nightly `pg_dump --schema=hive`, encrypted with a hub key, pinned as `kind='backup'` with replication 3 across at least two regions. This is the PITR substitute (Supabase PITR is $100/mo) and survives Supabase itself.
8. **Edge Functions shrink to Stripe.** Interview turns, invite redemption and node/server registration move to coordinator HTTPS/gRPC endpoints once the coordinator exists (ADR-005 §1). The Stripe webhook stays on Edge Functions because it needs a stable, Supabase-trusted HTTPS URL and touches money.

### B. Node control plane leaves Postgres before 200 nodes (D72)

9. v0 direct-RPC polling is a **documented exception**, valid while `count(hive.nodes where last_seen > now()-'1h') < 200`. Above that, nodes must check in to the coordinator over `/ohhive/ctl/1` (ADR-004 §12) and the PostgREST wrappers `public.hive_node_*` are revoked from the member role. Housekeeping in pg_cron stays as a safety net; the coordinator owns placement.
10. Migration trigger is a Grafana alert on active-node count at 150, so the switch is scheduled, not discovered.

### C. Provider spend is bounded by purchases (D71)

11. **Interview is local-first.** `spend_interview` runs on the Hive text pool like any card (ADR-005 §2 applies). Admin-reserved always-on text nodes (`hive.compute_reservations`, project = the Hive's own interview project) keep latency acceptable.
12. **Provider APIs spend only purchased $honey.** The ledger distinguishes `purchase`-sourced balance from `earn_*`-sourced balance (a per-wallet view over `entry_type`); `spend_job` on the provider pool and provider-backed `spend_interview` draw only from the purchased portion. Earned $honey buys local compute and storage, never Anthropic tokens.
13. A monthly provider budget (`hive.provider_budget(month, usd_cap, usd_spent)`) is enforced in the adapter layer; at the cap, provider execution returns `overflow_unavailable` and cards stay queued for local nodes.

### D. Web hosting and ingress (D74)

14. The web app ships as a **static Next.js export** with client-side `supabase-js` and the live-broadcast client. No server functions, no streaming through Vercel. Production is served from Cloudflare Pages (free) at `ohghive.com`; Vercel keeps preview deployments only. `docs/JOIN.md` and `install.sh` move with it.
15. **Cloudflare Tunnel is the standard ingress for volunteer regional servers** — stable HTTPS hostname, TLS, no port forwarding, no certificate management. `hive-server register` prints the `cloudflared` one-liner. Servers with a real public IP may skip it. This settles ADR-004's gateway open question as option (a).

### E. Infrastructure roles earn $honey (D75)

16. `earn_infra` (ADR-002 §9) extends beyond bytes-stored to: relay bandwidth served, live-broadcast connections served, snapshot bytes served, backup replicas held, and hosting the monitoring stack. All at the reduced infra rate. Rates are per-region rows in `hive.rate_table` so a thin region can be incentivized without touching others.

### F. Cloud servers are a standby tier, scripted and ready (D76)

17. Cloud machines run the **same `hive-server` binary** and register like any regional server with two extra columns: `operator in ('volunteer','hjm')` and `tier in ('primary','standby')`.
18. Placement rules: a `standby` server never holds a sole replica while a `primary` in the same region is healthy; the coordinator prefers `primary` for relay, broadcast and storage; `standby` is used for the anchor, backups, model-weight seeding, and whenever a region has fewer than two healthy `primary` servers.
19. **Anchor.** Exactly one HJM-controlled `hive-server` always exists: default coordinator, backup seeder, Hugging Face puller, trust root. A second anchor in another region at ≥10,000 members. This is the one paid box that is not optional.
20. **Spin-up is scripted.** `packaging/cloud-init/hive-server.yaml` (and a `hive-server cloud up --region <r>` wrapper) brings a Hetzner instance to registered-and-replicating in under ten minutes. Storage bytes go to a mounted Hetzner Storage Box, never to cloud block volumes (~€57/TB/mo vs ~€2–4/TB/mo). Backblaze B2 is the optional cold third replica.
21. **Triggers for paying.** Spin up a standby when: a day-1 region has < 2 healthy primary servers; fleet `storage_offered − storage_pinned` < 20 % for 48 h; relay saturation alerts in a region for 24 h; or the anchor's coordinator-lease renewals exceed 500 ms p95 from the slowest region. Tear down when the condition clears for 14 days and a volunteer has taken over.

### G. Capacity model (D77, D79)

Planning assumptions (revise from telemetry): nodes ≈ members (Q16); ~10 % of members in the browser at peak; ~10 % of members with an active funded project; ~20 GB pinned per active project; default model set ~40 GB per region; interview ≈ 10k output tokens ≈ 10–12 GPU-minutes on a 70B-class model.

| | 10 | 100 | 1,000 | 2,000 (day-1 target) | 10,000 |
|---|---|---|---|---|---|
| Anchor (HJM) | 1 | 1 | 1 | 1 | 2 |
| Regional servers | 1 | 3 | 6–8 | 10–12 | 30–40 |
| Regions | 1 | NA, EU, APAC | ≥ 2 per region | 3–4 per region | every continent, 2+ each |
| Fleet storage (×2 replicas) | < 100 GB | ~0.5 TB | ~4–5 TB | ~8–10 TB | ~40–50 TB |
| Always-on text nodes | 0 | 1 | 2–3 (one per region) | 3–4 | 8–10 |
| Monitoring | on anchor | on anchor | 1 dedicated | 1 dedicated | 2 |
| Total volunteer machines | 1 | 4 | 9–12 | 14–17 | 40–52 |
| Supabase compute | Micro | Micro | Micro → Small | Small | Medium |
| Coordinator load | trivial | trivial | ~100 heartbeats/s | ~200/s | ~1,000/s; per-region sub-coordinators |
| HJM cash/mo (ex-provider APIs) | $5–20 | $5–20 | $20–35 | $25–40 | $80–120 |

Rules of thumb: one regional server per 150–200 members, never fewer than two per region, never fewer than three total; one always-on text node per 300–400 members; one anchor (two at 10k). Server count is driven by storage and geography, not CPU — a Pi-class server coordinates the full 2,000-node fleet.

**Regional server spec (volunteer ask):** Pi 5 8 GB / N100 mini PC / retired laptop; 2–4 TB SSD or HDD; ≥ 50 Mbps upload; ≥ 95 % uptime; Cloudflare Tunnel or public IP; no GPU.
**Always-on text node spec:** Mac M-series 32–64 GB or a 24 GB GPU, left on, `compute_reservations` bound to the interview project.

### H. Paid standby menu (D76 detail, prices September 2026, revisit quarterly)

| Phase | Machines | Where | ≈ HJM $/mo |
|---|---|---|---|
| Bootstrap (≤ 100) | Anchor+NA regional (Hetzner Ashburn/Hillsboro CPX22 + 1 TB Storage Box); EU regional (Falkenstein CX22/CAX21 + 1–5 TB Storage Box); APAC regional (Singapore CPX22); B2 1 TB cold | Hetzner, Backblaze | $45–70 |
| Launch, volunteer-short (1–2k) | + one more per region; EU dedicated auction server with 8–16 TB HDD as storage backbone; Storage Boxes to 5–10 TB; second anchor in EU; B2 ~3 TB | Hetzner; Vultr/DO Sydney or Tokyo if Singapore is thin | $120–180 |
| Free tier | Oracle Cloud always-free ARM (4 OCPU / 24 GB / 200 GB) in NA, EU, APAC — treat as ~90 % uptime volunteer, never sole replica | Oracle | $0 |

Do not pay for: GPUs (use RunPod/Vast spot by the hour if interviews stall, never a monthly box); Vercel Pro; managed Kubernetes, load balancers or a second managed database. APAC egress at Hetzner Singapore is ~7× EU (€7.40/TB) — keep APAC fetches in-region and retire paid APAC first.

### I. Observability and guardrails (D78)

22. Self-hosted Prometheus + Grafana + Uptime Kuma + Loki on a regional server (anchor until 1,000 members), scraping every `hive-server` and Supabase's included metrics endpoint. Alerts to Discord/Slack webhooks at 60 % of every Supabase quota (connections, DB CPU, disk, egress, Edge invocations) and on the capacity triggers in §F.21 and §B.10.
23. Dashboard first-class numbers: active nodes, fleet storage offered vs pinned, per-region healthy primary servers, provider budget spent vs cap, Supabase disk, and coordinator lease p95.
24. **Spend cap decision before invites.** Jack chooses: cap on (Hive spike degrades Cmd Work; zero bill risk) or cap off with alerts (bill risk bounded by the alerts above). Recorded as a Decision either way.
25. Load test before invites measures Postgres CPU and RPC/s at 2,000 simulated nodes and 200 browser sessions — not just sign-ups/hour.

## Consequences

### Positive
- Supabase cost is flat at the $25 Pro plan (already paid for Cmd Work) plus at most a Small/Medium compute step; incremental Hive cost at 2,000 members is ~$15–60/mo.
- Every scalable resource — bytes, bandwidth, fan-out, GPU minutes — is provided by volunteers and paid in $honey, which costs HJM nothing in dollars.
- Provider API exposure is exactly what members paid for, so the treasury float can be near zero.
- Cloud is a lever, not a dependency: scripted, same binary, clear triggers, clear teardown.
- The volunteer ask is one sentence: run `hive-server` on a box with disk and uptime.

### Negative
- The coordinator becomes the live-state hub as well as the scheduler; a coordinator failover pauses UI updates for the lease TTL (polling fallback covers it).
- Two data paths for the web app (Supabase for own rows, overlay for shared state) is more client code than "subscribe to Postgres".
- Ledger archival adds a reconciliation step the nightly integrity job must include.
- Static export forecloses Next.js server features; anything that needs a secret lives in the coordinator or an Edge Function.
- Cloud-init and Cloudflare Tunnel docs are more surface to maintain.

### Risks & mitigations
- **Publication drift.** Someone re-adds a `hive` table to Realtime. Mitigation: CI assertion (§A.3) and a nightly check from the monitoring host.
- **v0 exception outlives its trigger.** Mitigation: alert at 150 active nodes; the migration work item is already open and blocks the invite wave.
- **Purchased-vs-earned split gamed via `fund_project`.** Mitigation: funding a project carries the source mix with it (two fund sub-balances); provider spend draws only from the purchased sub-balance.
- **Volunteer churn in one region.** Mitigation: §F triggers spin up a standby automatically-ish (scripted, human-approved); replication never depends on a single region.
- **Price drift.** Hetzner rose ~30 % in April 2026. Mitigation: §H is dated and reviewed quarterly; the binary is hosting-agnostic.

## Open questions
- Broadcast transport: plain WebSocket over Cloudflare Tunnel (default) or WebTransport/libp2p-in-browser (ADR-004 option c)? Default: WebSocket now, revisit at v2.
- Snapshot cadence and size: every 5 s full snapshot (default) or deltas? Default: full, gzip, until it exceeds 2 MB.
- Ledger hot window: 90 days (default) or 12 months for member-facing history? Default 90; the web wallet fetches archives on demand.
- Should `standby` servers earn $honey to the HJM treasury account (keeps the books symmetrical) or earn nothing? Default: earn to treasury.
- Anchor location: NA (closest to Jack) or EU (cheapest, €1/TB egress)? Default: NA anchor, EU second anchor when needed.
- Which cloud-init secrets bootstrap a standby without exposing the Postgres credential to cloud-init logs? Default: one-time registration token minted by the anchor, exchanged for `hive_coordinator`-scoped credentials only if the box wins the lease.

## Amendments to earlier ADRs (apply on acceptance)

| ADR | Section | Change |
|---|---|---|
| 001 | Decision 8 (D59) | Replace: "Supabase Realtime is not used by the Hive. Live UI state is served by regional servers from coordinator broadcast (ADR-013 §A)." Appendix A Realtime column → all "no". |
| 001 | Decision 11 | Edge Functions scope narrows to the Stripe webhook once the coordinator ships; interview/invite/registration move to coordinator endpoints. |
| 001 | Open questions | Resolve "Realtime for leases?" → no; "expose `hive` via PostgREST?" → yes, RLS-protected, for member-own rows only; shared reads via snapshot. |
| 002 | Decision 8 / new 14 | Provider spend draws only from purchased balance; add `provider_budget`; `spend_interview` is local-first. |
| 002 | Decision 9 | `earn_infra` covers relay, broadcast, snapshot, backup, monitoring roles; per-region rates. |
| 004 | Decision 6 note; Open questions | Gateway resolved to option (a) via Cloudflare Tunnel; add `operator`/`tier` columns to `regional_servers`; standby placement rules. |
| 005 | Decision 2 | Interview cards are scheduled like any card, local-first; provider overflow requires purchased balance and budget headroom. |
| 005 | Context | Document the v0 direct-RPC exception and its 200-node trigger. |
| 007 | Decision 2 / new 13 | `kind` gains `'backup'` and `'ledger_archive'`; hub-pinned with replication 3. |
| 009 | Decisions 2, 6 | Static export on Cloudflare Pages; Vercel previews only; Realtime replaced by coordinator broadcast + snapshot; polling fallback 15 s. |
| 012 | Roadmap | Insert "coordinator live broadcast + snapshot" and "control-plane migration off direct RPC" before the invite wave. |

## Related
- ADR-001, 002, 004, 005, 007, 008, 009, 012 (amended above)
- ADR-000 — D69–D79 appended
