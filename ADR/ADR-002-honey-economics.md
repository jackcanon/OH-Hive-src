# ADR-002: $honey Economics

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q1, Q6, Q7, Q11, Q14, Q19 — D2, D17, D19, D22–D25, D38, D50, D51, D67

## Context

OH Hive needs a unit of account that lets members contribute idle compute, storage, or money, and later withdraw burst capacity for their own projects. The interview settled the model as a **time-shifting compute bank**: contribute cycles off-peak, spend them when a project needs to move fast. The currency is provisionally named `$honey`. It must span two compute pools with very different cost structures — member-owned local nodes and third-party provider APIs (Claude, OpenAI/Codex, Nous) — while remaining a single number in a member's wallet.

Jack's priority is volume of local compute, and the reward rule he chose is deliberately simple: **you earn what you generate**. There is no uptime bonus, no hardware-class multiplier, and no reputation weighting. To give that rule a stable meaning across a fleet of heterogeneous machines, `$honey` is pegged to an external reference: Anthropic's per-token price. One token generated on a Hive node earns the `$honey` value of one Anthropic output token at the reference rate in force at that moment.

Because the peg is external and Anthropic's prices change, the rate cannot be a constant. Because the ledger is the only proof of who earned what, it must be append-only and auditable without a blockchain — the hub is the source of record (ADR-001). Because `$honey` cannot be cashed out to fiat in v1, it is a closed-loop credit, which keeps regulatory and tax exposure minimal and lets the first version ship without a money-transmission review.

Several quantities the interview left unspecified — the reference Anthropic model, the input-token fraction, pricing for non-token modalities, and the storage rate — are recorded here as open rather than decided.

## Decision

1. **Single currency, two pools (D2, D17).** `$honey` is the only unit of account. Cards may execute on Hive local nodes or on provider APIs; both are priced in `$honey`. The scheduler picks the pool per card, preferring local nodes (D24, ADR-005).
2. **Peg to Anthropic's output-token price (D22).** `1 $honey unit == USD price of 1 Anthropic output token` for the reference model. A node that generates `N` output tokens under a hub-issued lease earns `N × rate(t)` where `rate(t)` is the effective row in `hive.rate_table` at the lease segment's end.
3. **Pure output-based reward (D23).** Earnings are tokens generated × reference rate. No multipliers for uptime, hardware, model size, or seniority. Server operators are the one exception, at a reduced rate (decision 9).
4. **Rate table with history.** `hive.rate_table(id, kind enum('compute_output','compute_input','storage_gb_hour','egress_gb','api_provider_markup'), model_ref text, honey_per_unit numeric(20,10), effective_from timestamptz, effective_to timestamptz null, set_by profile_id, note)`. Rows are never updated; a new rate closes the previous row's `effective_to`. Every ledger entry records the `rate_table.id` it was priced with.
5. **Append-only double-entry ledger (D19).** `hive.ledger_entries(id uuid, txn_id uuid, account_id, entry_type, amount_honey numeric(24,6), direction enum('debit','credit'), rate_id → hive.rate_table, tokens_in, tokens_out, compute_seconds, job_id, lease_id, memo, created_at)`. Each `txn_id` groups entries that sum to zero across accounts. Table has `INSERT` only; a trigger raises on `UPDATE`/`DELETE`, and clients reach it solely via `SECURITY DEFINER` RPCs (`hive.post_txn(...)`). Balances are a materialized view over entries, never a stored column.
6. **Accounts.** `hive.accounts(id, kind enum('member_wallet','project_fund','treasury','provider_cost','storage_pool'), owner_profile_id null, project_id null)`. Every member has one wallet; every project has one fund; the treasury issues `$honey` on purchases; `provider_cost` absorbs API spend; `storage_pool` pays server operators.
7. **Ledger entry types (D19, D38, D50).** `purchase` (treasury → wallet, backed by Stripe), `earn_compute` (project_fund → wallet, per lease segment), `earn_infra` (storage_pool → wallet, per storage period), `fund_project` (wallet → project_fund, transferable to any project incl. others', D19), `spend_job` (project_fund → provider_cost or project_fund → node wallet, per card), `spend_interview` (wallet → treasury/provider_cost for the interviewer agent, D38), `storage_charge` (project_fund → storage_pool), `refund` and `adjustment` (admin-only, with mandatory memo).
8. **Purchases issue `$honey` at the peg (D17, D18).** The hub is the reseller: member pays Happy Jack Media via Stripe, the hub holds provider keys, and `$honey` is credited at `usd / rate(t)` less any `api_provider_markup` row. Provider API spend is debited from `project_fund` at the provider's actual token price converted through the peg, so a Claude job and a local job on the same card are commensurable.
9. **Storage earns at a reduced rate (D50).** Regional server operators earn `earn_infra` for `bytes_stored × hours` (and, if enabled, bytes served) at `storage_gb_hour` / `egress_gb` rates that are strictly below the compute rate on a per-USD basis. A server that also donates compute earns both streams independently.
10. **Pinning is funded (D51).** An artifact's storage is charged to its project fund each accounting period. When the fund cannot cover the next period, the artifact enters a grace period (`hive.artifacts.grace_until`), is then returned to the project owner via their node app, and is evicted from regional servers (ADR-007). Same-hash resubmission after refunding is deduplicated (D52).
11. **Closed loop (D67).** No `$honey → fiat` cash-out in v1. `$honey` can be earned, purchased, transferred to projects, and spent; it cannot leave the Hive. The ledger has no `withdraw` entry type.
12. **Metering is hub-trusted, not self-reported.** Only tokens produced under a hub-issued lease, counted from the output stream the coordinator receives, are credited. Nodes never post ledger entries; the coordinator posts `earn_compute` at lease segment close. Spot-check replay of a sample of jobs is a scheduler feature (ADR-005).
13. **Time-shifting compute bank framing (D25).** Product copy, wallet UI, and docs describe `$honey` as banked compute, not as a token or investment. This constrains naming in the UI, not the schema.

## Consequences

### Positive
- One number for everything: local inference, API overflow, storage, and interviews all reconcile in the same wallet.
- Append-only entries with rate ids make every balance re-derivable and every historical payout explainable at the rate then in force.
- External peg removes the temptation to hand-tune a bespoke exchange rate; changes are a rate-table row with an author and a note.
- Closed loop keeps v1 free of money-transmission and securities questions.
- "Earn what you generate" is trivially explainable to a non-technical film/audio community.

### Negative
- Pegging to Anthropic's price imports their pricing decisions; a price cut halves every contributor's future earnings overnight (past entries are unaffected).
- Without hardware multipliers, an M4 Max earning at the same per-token rate as a laptop only wins by generating more tokens per hour — accurate, but expensive nodes have no premium.
- Double-entry adds friction to every feature that touches money: each new flow needs an RPC and a balancing account.
- Storage pricing below compute may under-incentivize regional server volunteers, whose hardware is the network's backbone (ADR-004).

### Risks & mitigations
- **Token-count fraud.** Mitigation: hub-assigned jobs only; counts derived from output received by the coordinator; per-node sampling replay on a second node with divergence thresholds; invite-only membership as defense in depth, never as the only control.
- **Rate-table tampering.** Mitigation: `rate_table` inserts require an admin RPC that logs `set_by`; rows immutable; web app renders the full rate history publicly to members (D8).
- **Ledger drift.** Mitigation: nightly job asserts `SUM(credits) == SUM(debits)` per `txn_id` and per account kind; any violation pages the operator and freezes `post_txn`.
- **Provider price divergence.** If OpenAI/Nous cost far less than Anthropic per token, the peg over-rewards local generation relative to overflow. Mitigation: `spend_job` on provider pool is charged at actual provider cost through the peg, so the fund pays true cost; only earnings are normalized to Anthropic.
- **Fund starvation orphaning artifacts.** Mitigation: grace period, owner-return path, and a web-app banner with the projected run-out date derived from the storage rate.

## Open questions
- Which Anthropic model sets the reference rate? Default assumption: Sonnet-tier output price; revisit if the community skews toward small local models.
- Do input (prompt-processing) tokens earn, and at what fraction? Default assumption: output tokens earn 100%, input tokens earn a fraction mirroring Anthropic's input/output ratio for the reference model; the fraction is a `compute_input` rate row.
- How are non-token modalities (image, video, speech, music) priced? Default assumption: `compute_seconds × hardware class`, normalized to the token rate via a published table; the "no hardware multiplier" rule (D23) may need an explicit exception here, since seconds on a 4090 and a Pi are not equivalent work.
- What is the storage rate relative to compute? Fixed: storage < compute per USD. Ratio open; default assumption for modelling: 1 GB·month ≈ 10k output tokens.
- Does a server that donates both storage and compute get any bonus beyond the sum of the two streams? Interview says "will earn more" — default: additive, no bonus.
- Should `api_provider_markup` exist at all in v1, or does the Hive resell at cost? Default: at cost (markup row present but zero).
- Accounting period for storage charges and `earn_infra`: default hourly accrual, daily posting.
- Is the reference rate re-fetched automatically from Anthropic's published pricing or entered manually? Default: manual entry by an admin, with an alert if the published price differs.

## Related
- ADR-001-hub-and-source-of-record.md — ledger lives in schema `hive`, RPC-only writes.
- ADR-003-node-core-and-backends.md — `Backend::usage()` is the metering source per modality.
- ADR-004-p2p-overlay-and-regional-servers.md — regional servers as the storage-earning role.
- ADR-005-scheduler-and-leases.md — lease segments, local-first pool selection, replay sampling.
- ADR-006-agent-runtime-and-sandbox.md — `spend_interview` and the interviewer agent.
- ADR-007-artifact-storage.md — funded pinning, grace, owner return, dedup.
- ADR-008-auth-and-membership.md — purchase as a membership on-ramp; Stripe webhook.
- ADR-009-web-app.md — wallet, rate-history view, fund-project UI.
- ADR-010-node-desktop-app.md — earnings display.
- ADR-011-ownership-and-licensing.md — contributors paid in `$honey` acquire no rights.
- ADR-012-scope-and-roadmap.md — no cash-out in v1.

## Appendix A — Worked ledger example (non-normative)

Member A buys $100 of credit; reference rate is `r` USD per output token, so treasury issues `100 / r` honey.

| txn | entry_type | from → to | amount (honey) |
|---|---|---|---|
| T1 | `purchase` | treasury → A.wallet | 100/r |
| T2 | `fund_project` | A.wallet → P1.fund | 25/r |
| T3 | `fund_project` | A.wallet → P2.fund (A's own) | 75/r |
| T4 | `spend_job` | P2.fund → B.wallet (node B generated 40k tokens) | 40 000 |
| T5 | `storage_charge` | P2.fund → storage_pool (12 GB, 1 day) | per `storage_gb_hour` row |
| T6 | `earn_infra` | storage_pool → S1.wallet (server S1 held replica 1) | half of T5 |
| T6 | `earn_infra` | storage_pool → S2.wallet (server S2 held replica 2) | half of T5 |

Invariants: every `txn_id` sums to zero; T4's `earn_compute` for node B is the credit side of the same `txn_id` as P2's debit; each row carries `rate_id`.

## Appendix B — Entry-type to account-kind matrix

| entry_type | debit account kind | credit account kind | poster |
|---|---|---|---|
| `purchase` | treasury | member_wallet | Stripe webhook Edge Function |
| `earn_compute` / `spend_job` (local) | project_fund | member_wallet | coordinator |
| `spend_job` (provider) | project_fund | provider_cost | coordinator |
| `spend_interview` | member_wallet | provider_cost or member_wallet | interview Edge Function |
| `fund_project` | member_wallet | project_fund | web app RPC |
| `storage_charge` | project_fund | storage_pool | coordinator |
| `earn_infra` | storage_pool | member_wallet | coordinator |
| `refund` / `adjustment` | any | any | admin RPC, memo required |
