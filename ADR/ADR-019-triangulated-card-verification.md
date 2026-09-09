# ADR-019: Triangulated Card Verification (Rules → Judge → Human), With Logging as a First-Class Requirement

**Status:** Proposed · **Date:** 2026-09-09 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-014 §6 (proposed but explicitly unscheduled there); Jack asking in chat whether a finished card's verification would produce logs he could troubleshoot from — the answer today is no, because this system doesn't exist yet. Refined same-day per Jack's clarification: the judge stage is an actual **coordinator agent** that inspects the returned work, not a one-shot scoring function — and the system needs to surface **red flags** when a given agent/model/category is failing repeatedly, not just log individual verdicts.

## Context

ADR-014 named the failure mode directly: the Community Chat project produced four truncated cards, a backend/frontend env-var mismatch, and an over-heavy architecture, and none of it was caught before a human had to notice by reading raw text in a browser. ADR-014 §6 sketched the fix — adapted from `aden-hive`'s only "Implemented" phase — as three stages: deterministic rules, a confidence-gated LLM judge, then a human, with judge feedback reinjected on retry (reflexion). It deliberately left the build unscheduled.

Separately, `ohhive-core::worker.rs` already runs its own **Draft → Critique → (Revise → Critique)\* → Done** loop (module doc, lines 1-30), capped at `MAX_REVISIONS` (2). This matters to scope correctly: it is not the same thing as ADR-014's triangulated verification, and this ADR must not be confused with formalizing it. The existing loop is the *same model that wrote the draft* critiquing its own work, on the *same node*, with the *only visible trace* being a `critique` string buried inside a JSON blob passed to `hub.checkpoint()` (`hub.rs::checkpoint`, RPC `hive_node_checkpoint`) — not a queryable column, not shown to a human, and not category-aware or checked against structured acceptance criteria. It's useful (it does catch some bad drafts before they ever reach `review`), but it is an intra-node self-review, not an independent check, and it produces nothing a human could open later to see why a card passed or failed. When Jack asked "will we get logs for that so we can troubleshoot," the honest answer is: not from this existing loop either — its only trace is that one buried checkpoint blob, and nobody has ever queried it as an audit trail.

This ADR schedules ADR-014 §6 as its own build, with one addition Jack raised directly: **every stage must write a durable, structured, queryable record — not a transient event and not a JSON blob nested inside something else's state** — because the entire point is being able to troubleshoot *after the fact* why a card passed, failed, or landed on a human's desk.

## Decision

### 1. Where this runs in the card lifecycle

Triangulated verification is a new phase, **`Verify`**, that every card passes through between the worker loop's existing `Done` (the self-critique loop above still runs unchanged — it's a cheap intra-node quality gate) and `hive.cards.status = 'review'`. It does not replace `Draft → Critique → Revise`; it runs after it, independently, and it is what actually gates `review → done`, not a human eyeballing raw text as it does today.

1. **Deterministic rules** (fast, cheap, zero model calls). Category-scoped (ADR-014 §3/§4's rule sketches: required files present, banned patterns absent, cross-card consistency checks like the `JWT_SECRET`/`JWT_SECRET_KEY` mismatch). Priority-ordered; the first matching rule returns a definitive verdict and short-circuits — a rule failure never falls through to the coordinator agent.
2. **Coordinator (verifier) agent** (confidence-gated). Only reached if no rule resolved the card. This is not a one-shot scoring call — it's an agent in its own right, with tool access to actually read the card's full output, its artifacts (`artifact_get`), and its structured acceptance criteria (ADR-014 S3/S4 — file/path lists, pass/fail commands, hard constraints), the same way a human reviewer would open the work and check it against spec. It returns a verdict (`PASS` / `RETRY` / `ESCALATE`), a confidence number (0–1), its reasoning, and — this is the part a plain scoring function can't do — **a revised instruction set** when the verdict is `RETRY`: specific, actionable direction for what the working agent got wrong and what to fix, not just a pass/fail label. Below a configurable confidence threshold, it does not guess — it escalates directly to human, same as an `ESCALATE` verdict does.
3. **Human** (last resort, not the default gate it is today). Sees only what rules didn't resolve and the coordinator agent wasn't confident about, or what came back `RETRY` a second time and still didn't pass. This is the actual behavior change from today's "review" column, which currently shows every single card regardless of confidence.
4. **Revision loop on RETRY.** The coordinator agent's revised instruction set becomes the working agent's next attempt's actual instructions (a new `Phase::Revise` entry in the worker loop, seeded from the coordinator's structured feedback rather than the self-critique loop's own text) — the card goes back to the same working agent with a sharper brief, runs again, and returns to the coordinator for another look. Capped at a configurable max (default: 1 revision round-trip before falling to human, separate budget from the existing `MAX_REVISIONS` self-critique cap — these are two different loops with two different costs, and conflating their caps would hide how much of a card's total revision budget went to each).

### 2. The coordinator agent runs independently of the executing node/agent, on a different model where possible

The coordinator must not be the same agent instance, ideally not the same *model*, that produced the draft — an agent judging its own homework is what the existing self-critique loop already does, weakly, and is precisely the gap this ADR exists to close. As the Hive takes on cards from multiple different working agents (different models, different nodes, potentially different member-run agent configurations per ADR-006), the coordinator is the one consistent, independent check every card passes through regardless of who drafted it. Two acceptable coordinator execution paths, mirroring the interviewer Edge Function's own provider-first design (memory 2026-09-06):

- **Provider-backed coordinator** (default): the hub calls a coordinator-agent model via the existing provider-budget path (ADR-002's purchased/earned/grant split), same pattern as the interviewer. This is the safer default for actually getting an independent read, and its cost is metered exactly like interviewer spend already is.
- **Different-node coordinator** (opportunistic, not required at launch): a second compute node with a different model in its ladder acts as coordinator for another node's card output. Deferred — it needs a trust/selection mechanism (which node gets picked, how its own reliability is tracked) that doesn't exist yet, and is explicitly not scoped by this ADR. Left as an open question below.

The macOS Foundation Models on-device chat engine (ADR-018 amendment decision 7, shipped 2026-09-09) is explicitly **not** used as the coordinator, per that same ADR's guardrail: Apple on-device/PCC models serve `execution_mode='local'` runs only, never Hive-distributed card verification for other members' work.

### 2a. Red flags: detecting a failing agent, not just a failing card

A single `RETRY` or `ESCALATE` is normal and expected — that's the pipeline working as designed. What this ADR must also catch is the pattern underneath: the same working agent, model, or category racking up an abnormal rate of coordinator rejections, which is a signal that something is systemically wrong (a bad prompt template, a model that's drifted below what its ladder position promises, a category whose acceptance criteria are ambiguous) rather than one unlucky card. Concretely:

- A background rollup job (not the coordinator agent itself, which only ever sees one card at a time) periodically aggregates `hive.card_verifications` by `judge_model` (i.e. which working-agent/model combination drafted the card, joined from `hive.cards`) and by category, computing rolling `ESCALATE`-rate and average confidence.
- When a working agent/model/category combination crosses a configurable threshold (e.g. escalate-rate over N% across the last M cards), it's flagged as **red** on the fleet health / model leaderboard surfaces, and a **deep-dive** is triggered: the rollup job pulls every `card_verifications` row plus the associated card's full attempt history for that agent/model/category over the flagged window and produces a single summary (common failure themes across the `reasoning` text, which rule IDs fire most, whether confidence is trending down over time) rather than leaving Jack to read dozens of individual verdict rows by hand.
- This directly extends the existing Model Leaderboard project (evidence-based tracking of what each model is good/weak at) — red-flag data becomes a real evidence source there instead of anecdotal incident reports.
- Red flags do not themselves stop a working agent from taking more cards — that remains a human/ops decision — but they turn "why does this keep failing" from a manual investigation into a report that already exists when Jack goes looking for it.

### 3. Schema — logging is the point, not an afterthought

New table, `hive.card_verifications` (one row per verification *attempt*, not per card — a card that goes rules-fail → reflexion → judge-pass has two rows, both queryable):

```
id                 uuid primary key
card_id            uuid references hive.cards
attempt            int              -- 1, 2, ... (revision-loop count)
stage              text             -- 'rules' | 'coordinator' | 'human'
verdict            text             -- 'pass' | 'retry' | 'escalate' | 'fail' | 'override'
confidence         numeric          -- null for rules (deterministic) and human stages
rule_id            text             -- which rule matched, null unless stage = 'rules'
reasoning          text             -- coordinator's actual finding text, or human's override note
revised_instructions text          -- coordinator's specific fix-it brief for the working agent, set only on 'retry'
worker_agent       text             -- which working agent/model drafted the card being checked (join target for red-flag rollups)
acceptance_snapshot jsonb           -- the structured acceptance criteria checked against, frozen at verify time
judge_model        text             -- which model/provider acted as coordinator, null for rules/human stages
decided_by         uuid             -- member id, null unless stage = 'human'
created_at         timestamptz
```

This is deliberately a real table, not a jsonb column bolted onto `hive.cards` or folded into the existing checkpoint blob — the entire motivation (Jack's question) is being able to run a real query like "show me every card that got RETRY from the coordinator with confidence between 0.4 and 0.6 this week" months from now, which a blob doesn't support. `revised_instructions` and `worker_agent` are what make the red-flag rollup in §2a possible — without a queryable "who drafted this" field, there's no way to aggregate failure rate by agent/model.

`hive.cards` gains one column, `verification_status` (`pending` / `verified` / `needs_human` / `overridden`), so the board can filter/badge without joining `card_verifications` for the common case.

A second, small table, `hive.verification_red_flags` (agent/model or category identifier, window start/end, escalate_rate, avg_confidence, summary text, `raised_at`, `cleared_at`), stores each red-flag rollup's output — the "deep dive" report from §2a — so the fleet health and Model Leaderboard surfaces have something durable to read rather than recomputing on every page load.

### 4. Desktop/web surfacing

The web app's project board gets a verification badge per card (✓ auto-verified, a confidence percentage if coordinator-checked, or a flag if escalated), and a card detail view lists its full `card_verifications` history including the coordinator's `revised_instructions` on any retry round — this is the literal answer to "will we get logs to troubleshoot," made concrete as a UI a human can actually open. A separate red-flags panel (fleet health / Model Leaderboard) surfaces `hive.verification_red_flags` rows so a systemically-failing agent/model/category is visible without digging through individual cards. The native Swift app doesn't get its own verification UI in this pass; members review cards on the web app today (ADR-009), and duplicating that surface into Swift isn't scoped here.

## Consequences

### Positive
- Directly closes the Community Chat failure mode: a cross-card consistency rule would have caught the `JWT_SECRET` mismatch before a human ever saw it.
- Every verdict is queryable after the fact — this was Jack's explicit ask, not an assumed nice-to-have.
- The coordinator agent's `revised_instructions` turn a bare pass/fail into an actual second chance for the working agent — closer to how a human reviewer hands back notes than to a lint pass.
- Red flags turn "is this agent/model quietly failing" from something Jack would only notice by accident into a standing report, and feed real evidence into the existing Model Leaderboard project instead of anecdote.
- Reuses existing primitives (ADR-002's provider-budget path, the interviewer's provider-first pattern) rather than inventing a new spend mechanism.
- Clearly separates from, and doesn't disturb, the existing intra-node self-critique loop — that loop keeps doing its (different, cheaper) job unchanged.

### Negative
- A new spend category (coordinator-agent calls) against the provider budget, on top of interviewer spend — needs its own budget line or a shared cap, not addressed by ADR-002 today (open question below).
- `card_verifications` grows one-to-several rows per card forever; no retention/archival policy is defined here (ADR-013's ledger-archival pattern is the obvious template, not yet applied to this table).
- Category-scoped rule sets only exist for the two specified categories (Software, Research) per ADR-014 — cards in the seven placeholder categories get coordinator-only verification (no rules stage) until their rule sets are written.
- The red-flag rollup job is a new piece of always-on infrastructure (something has to run it periodically and own its thresholds) — not free, and not detailed to an implementation level by this ADR.

### Risks & mitigations
- **Coordinator false confidence** (a wrong verdict reported with high confidence, escalating nothing when it should have). Mitigation: track coordinator accuracy over time by comparing its verdicts against human overrides on escalated cards — the `decided_by`/`override` verdict rows are exactly the data needed for this, but the comparison job itself is not built by this ADR.
- **Revision loop cost surprise.** Mitigation: separate, explicit budget cap from the existing self-critique `MAX_REVISIONS`, surfaced in the funding UI the same way ADR-014's open fan-out-cost question flags for `spawn_child_card`.
- **Rule-set drift/maintenance burden as categories multiply.** Same mitigation ADR-014 already named: rules stay category-scoped and reviewed alongside each category's own spec, not as one global rule set.
- **Red-flag noise/false positives** (a category with genuinely hard or ambiguous cards looks "red" through no fault of the working agent). Mitigation: red flags key on agent/model **and** category together, not just the agent/model alone, so a hard category doesn't tank every agent's apparent score — and flags are advisory (surfaced to a human), never automatically punitive.

## Open questions
- Coordinator spend: its own `hive.provider_budget` line, or shared with the interviewer's existing budget? (Default: its own line, so a busy verification week can't starve the interviewer or vice versa — needs an ADR-002 amendment to actually add it.)
- Different-node coordinator (§2's opportunistic path): what selection/trust mechanism picks a coordinator node, and how is that node's own judging reliability tracked? Explicitly deferred, not scoped here.
- Retention/archival for `card_verifications` — same 90-day-hot-window-then-Parquet pattern as ADR-013's ledger archival, or something simpler given this table is far smaller? Default: revisit once real volume exists, same discipline as the ledger-archival ADR used.
- Does `hive.card_accept` (today's human-only gate) get replaced outright, or does it stay as the explicit human path used only for `needs_human` cards? Default: the latter — `card_accept` becomes the human stage's actual mechanism, not a parallel path.
- Red-flag thresholds (what escalate-rate over what window counts as "red") — start with a conservative fixed default (e.g. >40% escalate rate over the last 10 cards) and let Jack tune it once real data exists, rather than guessing a permanent number now.
- Should a red flag ever gate anything automatically (e.g. pausing a chronically-failing agent from taking new cards) versus staying purely advisory? Default: purely advisory for v1 — automatic gating is a meaningful trust/fairness decision that deserves its own explicit call once there's real red-flag data to look at.

## Related
- ADR-014-project-categories-and-verification (§6, the origin of this design — this ADR is that section's scheduled build)
- ADR-006-agent-runtime-and-sandbox (the existing Draft → Critique → Revise self-critique loop this ADR runs independently alongside, not instead of)
- ADR-002-honey-economics (provider budget the judge draws from; needs its own line per the open questions above)
- ADR-013-cost-capacity-and-hosting (ledger archival pattern, the likely template for `card_verifications` retention)
- ADR-018-native-macos-swift-shell (the on-device Foundation Models guardrail that explicitly excludes it from serving as this ADR's judge)
