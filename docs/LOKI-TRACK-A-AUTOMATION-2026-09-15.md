# Track A automation: agents talking to each other, and stopping

Loki, 2026-09-15, branch `track-a-automation`. This is slices 2, 3 and 4 of
`LOKI-TRACK-A-ROOMS-AND-MENTIONS-2026-09-15.md` — the automation half — built and verified on a
branch while Sif builds the demo half (S-1..S-5) on `main`. Built on a separate git worktree on
purpose: my in-progress core edits would otherwise have broken Sif's `cargo test` runs and she'd
have reasonably concluded she broke something.

**One line summarizes the change:** `bots/executor.rs` no longer passes `Vec::new()` as its reply
recipient list. Everything else here exists to make that line safe.

## Slice 3 is done too, so it comes off Sif's list

`bots/mentions.rs` was assigned to Sif as S-3. It is implemented and green here (14 tests), so
**she should skip it** and spend the time on S-1/S-2/S-4 instead. That is one fewer thing on the
demo critical path.

`local_hub::bots_conversation_agents` also landed here, and S-4 needs it for `@` autocomplete
against the room roster — it lists the non-archived agent members of a conversation.

## What changed

### Schema v10 — `local_hub/bots_causation_schema.sql` (new)

`agent_deliveries` gains `cause_message_id`, `root_message_id` and `turn_depth`, and `'held'`
joins the `status` CHECK. Done as a **full table rebuild** rather than three `ADD COLUMN`s,
because SQLite cannot ALTER a CHECK constraint and one copy does all four things. The table is a
leaf — nothing references it — so DROP + RENAME is safe with foreign keys on. Two indexes:
`(root_message_id)` for the per-root budget, `(recipient, status)` for the active-turns budget.

Existing rows migrate as depth-0 roots with their own message as the root, which is what they
are: every delivery created before this migration was caused by a human send.

### `bots/types.rs`

- `DeliveryStatus::Held` — waiting on a person. **Not** terminal; a release returns it to
  `Pending`. `is_terminal()` already excluded it by construction.
- `AgentDelivery` gains the three causation fields.
- `DeliveryCause { cause_message_id, root_message_id, depth }` — new.
- `HandoffBudgets` gains `max_turns_per_root` (default **30**, `#[serde(default)]` so pre-v10
  `handoffs` rows still deserialize), and `max_depth` moves **2 → 6**.

### `local_hub/bots.rs`

- `bots_message_send_with_cause(..., cause: Option<DeliveryCause>, hold: bool)`. The existing
  `bots_message_send` is now a wrapper delegating with `(None, false)`, so **every existing
  caller compiles untouched** — including the FFI call sites Sif is editing concurrently. That
  was the point: widening the original signature would have churned exactly the files she has
  open.
- `MessageKind::System` messages create **no deliveries**, whatever recipient list is passed
  (slice 3 of the design doc). This is what makes budget notices free and what stops a six-agent
  room turning every "still working" post into six billable turns.
- New reads: `bots_active_turns_for_agent`, `bots_turns_for_root`, `bots_deliveries_held`.
- New write: `bots_deliveries_release_root` — every `Held` delivery for a root returns to
  `Pending`, returning the count.
- `bots_delivery_cancel` now re-reads through the shared `bots_delivery_row` helper instead of a
  second hand-written SELECT, so the new columns come back there too. `held` is deliberately
  absent from its terminal check: a person may **cancel** a chain waiting on them rather than
  release it.

### `bots/executor.rs`

Four budgets, and one refinement to the design doc worth flagging.

| Budget | Where enforced |
|---|---|
| `max_depth` (6) | send time: `turn_depth + 1 > max_depth` → reply is posted, zero deliveries |
| `max_active_specialist_handoffs_per_run` (2) | send time: truncate an agent reply's recipients |
| `max_turns_per_root` (30) | send time: `spent + new > max` → deliveries created `Held` |
| `max_active_turns_per_agent` (1) | **claim time**, in `drain_agent` — see below |

**Design-doc correction found while implementing.** The doc said to enforce
`max_active_turns_per_agent` at send time by dropping recipients who were already busy. That
silently loses a message. Enforcing at claim time is strictly better: the delivery is still
created and stays `pending`, so a busy agent is *delayed*, not skipped, and it runs on a later
pass. Within one drain process the loop is already sequential per agent; the check is what holds
when a second `hive bots work` process exists for the same agent, which nothing prevents.

**Every budget event posts a `System` message** naming what was stopped — depth stop, suppressed
fan-out, unresolved names, and above all a hold ("30-turn limit reached for this thread; 2
replies are held. Release to continue."). A held chain nobody can see is indistinguishable from a
broken one.

`DeliveryExecutor::with_budgets` lets a caller tighten the numbers; `release_root` is the one
obvious entry point for a UI or CLI to let a chain continue.

### `bots/mentions.rs` (new, pure)

`resolve_mentions(body, roster, author) -> MentionSet { recipients, unresolved, everyone }`.
Proposes only — `bots_message_send`'s `bots_is_member` check on every recipient is untouched, so
a name resolved here that is not a room member is still refused at the storage boundary.

Code fences and inline backticks are excluded from scanning, and an unterminated fence fails
**closed** (treats the remainder as code, summons nobody). This is not polish: a room whose
purpose is discussing code will contain `@State`, `@escaping` and `@Sendable` constantly, and a
resolver that woke a teammate for each would be unusable in exactly the rooms this feature is
for. Self-mentions are dropped, `@everyone` resolves to the roster minus the author, an unknown
name is reported rather than dropped and never errors a send, and a name matching two agents
resolves to **neither** — guessing which teammate was meant is worse than saying it is ambiguous.

## Verification (real toolchain, on Midgaard)

- `cargo test -p hive-core --features bots,local-hub`: **152 passed, 0 failed**
- `tests/bots_loop_safety.rs` (new): **9 passed**
- `tests/bots_transport_gate.rs`: 1 passed
- `cargo check --workspace --all-targets`: clean (pre-existing warnings only)

The nine loop-safety tests are the point of the slice, so what they actually assert:

- **`mutual_mention_cycle_terminates_at_max_depth`** — Alpha replies "@Beta", Beta replies
  "@Alpha". An unbounded loop without enforcement. Terminates with exactly 4 deliveries (depths
  0..=3 at `max_depth: 3`) and posts a visible depth notice.
- **`mutual_mention_cycle_terminates_under_shipped_defaults`** — same cycle at the real defaults:
  exactly 7 deliveries (depths 0..=6), and the 30-turn gate correctly does *not* fire.
- **`per_root_budget_holds_for_a_human_then_releases`** — at a tightened budget the next turn is
  `Held`, not dropped; a notice says so; `release_root` returns it to pending and the chain makes
  further progress.
- **`held_deliveries_are_never_drained`** — five further drain passes change nothing while a hold
  stands. Holds neither expire nor multiply.
- **`self_mention_creates_no_delivery`**, **`agent_fan_out_is_capped_and_says_so`**,
  **`system_messages_create_no_deliveries`**, **`code_fenced_mention_wakes_no_one`**,
  **`human_send_is_its_own_root_at_depth_zero`**.

Every drain loop in those tests runs under an explicit pass cap that panics with "the chain is
not terminating", so a future regression fails the suite rather than hanging it.

## Five pre-existing tests were updated, deliberately

Four pinned `PRAGMA user_version == 9` and now expect 10, each with the reason named in a
comment. The fifth, `bots/tests.rs`'s budget-defaults pin, asserted `max_depth == 2`; it now
asserts 6 and 30 with a comment recording that the section-6 number was superseded by Jack on
2026-09-15 and that the design doc must change in the same commit as any future edit. A new test
`handoff_budgets_deserialize_without_the_new_field` proves pre-v10 `handoffs` rows still load.

None of these were weakened — the assertions are the same shape against the agreed new values.

## Not done

- No FFI or UI surface for any of this. Nothing in the Swift/Tauri/web apps can create a room,
  release a held chain, or see a hold. `release_root` exists in core with no caller.
- Not merged to `main`, by design — the demo comes first.
- No coordinator enforcement (design doc 3.5) and no `Held` exposure through
  `local_hub/transport.rs` dispatch, so a secondary cannot release a chain on its primary.
- Nothing about which machine a turn runs on (fleet item 4).

## Merging

Expected to be clean: this branch touches `bots/{types,executor,mentions,mod,tests}.rs`,
`local_hub/{bots,mod}.rs`, the new schema file, and four version-pin tests. Sif's demo slices
touch `crates/ohhive-ffi/src/bots.rs` and Swift. The one shared file is `local_hub/bots.rs`,
where my edits are new methods plus the `_with_cause` split, and her S-2 is in the FFI layer
above it.

Merge order should be **Sif's demo work first, then this branch**, so the demo is never blocked
on reconciling a merge.
