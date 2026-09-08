# ADR-017: Cloud Compute Pool -- Purchased-Honey-Funded Provider Execution

**Status:** Proposed · **Date:** 2026-09-08 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** this conversation

## Context

This started as a small UI question: a test card showed a "CLOUD" badge next to a node (Odin) that had just run a card entirely locally, using its own quantized model, with `allow_internet = false`. Checking the actual code (`nodeKindBadge()` in `apps/web/app/projects/[id]/page.tsx`) showed the badge has nothing to do with which model answered a card -- it fires whenever a node's `role` is `regional_server` or `compute_and_server`, i.e. it labels a node's place in the Hive's own server infrastructure, not the source of its compute. That's a coincidental collision of the word "cloud," not a bug, but it now needs resolving because Jack's real ask uses "cloud" to mean something specific and different.

The real ask: members who can't offer a worker or server of their own -- no paired machine -- should still be able to participate by buying Honey, with that purchase feeding a shared pool of third-party API compute (Anthropic, OpenAI, etc.) that projects can draw on. Framed as opt-*out* rather than opt-in, since the community isn't privacy-sensitive about most project data -- the default should be "use whatever's available," with an explicit toggle for members who don't want their project touching a third party. Jack's own framing for why that toggle matters: some members are "vegetarians" about this -- not because anything is private, but because running local-only is the reason they joined the Hive in the first place, the same way the Hive itself runs on donated local machines rather than a cloud vendor's servers.

Checking the database rather than assuming turned up something important: most of this is already built, under ADR-002 and ADR-013, just scoped to one narrow feature. Every `hive.ledger_entries` row carries a `source` tag -- `purchased`, `earned`, or `grant` -- so Honey balances aren't one fungible number, they're three buckets by provenance. `hive.charge_interview()` (used when a member's project-planning chat uses a shared/hosted model rather than their own BYO key) already debits *only* from the `purchased` and `grant` buckets via `hive.split_debit(wallet, amt, array['purchased','grant'], ...)`, crediting a dedicated `provider_cost` account, and hard-refuses otherwise with the message "earned $honey buys local compute only." That is exactly the rule Jack described, already enforced in code -- just never extended past the interview step to actual card execution. `hive.fund_project()` already preserves this provenance correctly when a member funds a project: it splits their debit by source and credits the project's fund account with matching source tags, so `hive.account_sources(fund_account_id)` already tells you, for any project, exactly how much of its balance is purchased/earned/grant Honey, for free, today.

The one place reality diverges from what Jack described: the "budget" gating provider spend (`hive.provider_budget`) is a flat, admin-set monthly USD cap ($25/month currently -- clearly a placeholder), unrelated to how much Honey has actually been purchased. Jack's model wants the pool's capacity to *be* the real purchase revenue, not an arbitrary number someone picks. Asked directly, Jack chose: retire the flat cap: the pool's spendable balance is exactly the lifetime USD value of real Honey purchases minus lifetime USD spent on provider costs -- self-funding, no separate ceiling.

Also discovered in the same pass: the "buy Honey" purchase flow itself does not exist anywhere yet. `entry_type = 'purchase'` is reserved in the schema's enum, but no function anywhere ever inserts one. This ADR's mechanism has no real money behind it until that's built.

## Decision

### 1. Resolve the naming collision first
Rename the existing node-role badge (today's "CLOUD," meaning "this node is part of the Hive's own server infrastructure") to something unambiguous -- "SERVER" or "INFRA" -- before any user-facing feature uses "cloud" to mean third-party API compute. Two meanings of the same word on the same project page is a real usability problem, not a cosmetic one.

### 2. Extend purchased-Honey-only provider charging from the interview to card execution
Introduce a system-owned virtual node -- a new `hive.node_role` value, `cloud_pool` -- that calls real third-party model APIs and becomes eligible to claim `execution_mode = 'hive'` cards (never `'local'` cards; local mode is peer-owned-machines-only by definition, per ADR-015, and is untouched by this ADR). On completion, charge the project's fund account exactly the way `charge_interview` charges a member wallet today: `split_debit(fund, amt, array['purchased','grant'], ...)`, crediting `provider_cost`. Earned Honey can never pay this bill, in card execution any more than in the interview -- that's the rule that keeps the closed loop solvent.

### 3. Per-project opt-out, not opt-in
New column `hive.projects.allow_cloud_pool boolean not null default true`, owner-editable, meaningful only when `execution_mode = 'hive'`. When false, the `cloud_pool` node is never eligible to claim that project's cards, regardless of how much balance the pool has -- this is a hard exclusion, not a deprioritization, so it can actually honor a member's principled "local-only" stance (Jack's "vegetarian" framing) rather than just their privacy preference.

### 4. A real, running, purchase-backed pool -- no ceiling
Retire `hive.provider_budget`'s monthly-reset flat cap. Replace the spend check with a lifetime running balance: total USD value of all `purchase`-sourced Honey ever bought, minus total USD ever debited to `provider_cost`. The pool is exactly as large as real revenue has made it, continuously, with no separate admin-set number to keep in sync by hand.

### 5. Prerequisite, explicitly out of scope here: the purchase flow itself
Nothing in this ADR is real without an actual "buy Honey" payment path (a processor like Stripe, `entry_type = 'purchase'` finally getting inserted somewhere, real compliance/refund handling). That is necessary, non-trivial, separate work this ADR depends on but does not design.

### 6. Project funding is unchanged
A non-worker member funds a project exactly as today -- buys Honey themselves, or other members add Honey via the existing anonymous/credited contributor mechanism (built earlier this session). No new funding path is needed, because cloud-eligibility is decided at *spend* time by the source tags a project's fund account already carries (via `fund_project`'s existing provenance-preserving credits), not at fund time.

## Consequences

### Positive
- Reuses infrastructure that already exists and already works correctly (source-tagged Honey, `split_debit`, `provider_cost`, `fund_project`'s provenance-preserving credits) instead of building a parallel accounting system.
- Gives members without a paired machine a real way to get projects done beyond waiting indefinitely for a peer node to pick up their cards, without the platform ever fronting real dollars it hasn't collected.
- The purchased-Honey-only rule, already proven in production for the interview feature, extends cleanly to card execution with no new trust primitive.
- The hard opt-out gives principled local-only members a structural guarantee, not a best-effort promise, matching why they joined the Hive in the first place.

### Negative
- The payment flow this depends on doesn't exist; this ADR's mechanism has no teeth until that's built.
- A pool with no ceiling, sized off "purchased" Honey the instant it's bought, can outrun money Happy Jack Media has actually settled with a payment processor if payouts lag purchases -- a real operational risk once real money is involved.
- Adding a fourth `node_role` touches every place that already switches on it (board UI, scheduler capability checks) -- small, but real, surface area, on top of the badge rename in decision 1.

### Risks & mitigations
- **Settlement-timing risk** (pool sized off gross purchases vs. cash actually received): mitigate by deriving the running cap from settled/cleared purchases if the payment processor distinguishes that, not the instant a purchase is initiated.
- **Enum/UI surface-area risk**: mitigate with a grep-and-fix pass across every `node_role` switch before shipping, the same discipline used for the badge rename.
- **A member opts out without realizing peer-only execution may simply be slower, not just "safer"**: mitigate with an explicit label on the toggle ("peer nodes only -- may take longer to run"), not a silent behavior change.

## Open questions
- Who operates the `cloud_pool` node process and whose API keys does it hold -- Happy Jack Media's own account, pooled member-contributed BYO keys (reusing the existing `hive.member_keys`/`provider` table), or both? (Open.)
- Does card execution reuse the interview's `api_provider_markup` rate, or does compute-heavy card work need its own markup rate? (Open.)
- Should `allow_cloud_pool` really default to `true` from day one with real money involved, or start `false` until the pool has a track record? (Open -- default assumed `true` per Jack's opt-out framing, worth revisiting before launch.)
- What happens to a `hive`-mode card when `allow_cloud_pool` is off, no peer node claims it, and the project's fund has no earned/grant Honey to pay a peer with either -- does it just wait indefinitely? (Open.)

## Related
- ADR-002-honey-economics (the closed-loop, purchased-vs-earned distinction this ADR extends to card execution rather than replaces)
- ADR-013-cost-capacity-and-hosting ("provider spend from purchased $honey only" -- the rule `charge_interview` already enforces, extended here)
- ADR-015-local-workstation-and-hive-promotion (`execution_mode='local'`; this ADR's pool applies to `'hive'` mode only and leaves local mode untouched)
- ADR-016-hub-portability-and-local-fleet-independence (companion decision keeping local-fleet mode independent of everything in this ADR)
