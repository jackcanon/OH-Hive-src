# Sif's queue and the file-level split with Loki

Loki, 2026-09-16. Jack wants us both working without stepping on each other. Today we duplicated
`mentions.rs`, duplicated the room-roster method and collided on schema 10 — all recoverable, all
avoidable. So this starts with ownership, not tasks.

Naming, decided by Jack 2026-09-16 and settled: the harness is **Loki's Den**, shorthand **Den**.
Advertised at **lokisden.app** and on **lokislab.org**. The community site **ohghive.com** is
unchanged, and **Hive** stays the word for the community working together. Rebrand is a later,
deliberate terminology pass — **not tonight, and not file by file.** Nothing in this queue renames
anything.

## Ownership

Exclusive means: change it without asking, and expect the other not to.

**Sif owns**

| Path | |
|---|---|
| `supabase/functions/**` | Edge Functions, Deno |
| `supabase/migrations/**` | server schema |
| `crates/ohhive-ffi/**` | the native bridge |
| `apps/desktop-swift/**` | Swift app |
| `apps/desktop/src/**`, `apps/desktop/src-tauri/**` | web + Tauri |
| `crates/ohhive-core/src/bots/runner.rs` | `LocalModelTurnRunner` — hers since C1 |
| `crates/ohhive-core/src/bots/cloud_runner.rs` | new, hers (see S-A) |

**Loki owns**

| Path | |
|---|---|
| `crates/ohhive-core/src/bots/{types,executor,local_executor,mentions,service}.rs` | domain types, delivery loop, the runner trait boundary, loop safety |
| `crates/ohhive-core/src/local_hub/bots.rs`, `bots_*.sql` | Bots storage + its migrations |
| `crates/hive/src/main.rs` | the CLI |

**Contended — say so in the continuity log before touching**

- `crates/ohhive-core/src/local_hub/transport.rs` — we both need `dispatch()` arms.
- `crates/ohhive-core/src/local_hub/mod.rs` — the schema ladder. **Claim a schema number in the
  continuity log before you use it.** Sif already did this today and it turned a day-long mess
  into twenty minutes; it is now the rule.

**One trespass to declare:** I edited `bots/runner.rs` (Sif's file) in `be6d78e` to render speaker
names instead of `Principal` UUIDs — it was a live group-chat defect and I had the context. It is
pushed. **Sif: pull before you touch `runner.rs`.** I won't go in there again without asking.

## The seam for cloud agents: a trait, not a merge

Same pattern that made C1 work. `LocalBotsTurnRunner` (in *my* `local_executor.rs`) is the
interface. Sif implements it for cloud providers; I route deliveries to it. Neither of us edits the
other's side.

- **Sif:** the Edge Function, and `cloud_runner.rs` implementing `run_turn` for
  `AnthropicByok`/`NousByok`.
- **Loki:** `executor.rs` learns a second runner and which runtimes go to it.

If the trait needs to change to make the cloud case work, that is a conversation, not a unilateral
edit — it is the one interface we both depend on.

## Sif's queue

**S-A — Cloud turn runner for BYOK agents. Highest value on either list.**
Today `ensure_provider_agents` creates **"Claude"** (`AnthropicByok`) and **"Nous"** (`NousByok`)
profiles the moment a member has keys on file. They list in the roster, join rooms, resolve from
`@Claude`, and get an `agent_deliveries` row — and `executor.rs` drains only
`runtime_kind == Local && preferred_host == this host`. So their deliveries are **never claimed by
anything, ever.** No error, no timeout: the row sits `pending` and the room looks like Claude
ignored you. The teammate Jack most wants in a room is the one that cannot speak.

Two pieces, yours:
1. An Edge Function taking a Bots conversation's bounded history and returning a reply.
   `supabase/functions/interview/index.ts` (542 lines) already has
   `callAnthropic`/`callOpenAI`/`callNous` and a shared `Brain` that resolves a member's own BYOK
   key **server-side** — the key must never reach the device (ADR-008). What's missing is a
   function accepting a Bots history shape instead of `interview`'s chat/plan shape.
2. `crates/ohhive-core/src/bots/cloud_runner.rs` implementing `LocalBotsTurnRunner` against it,
   returning `LocalTurnError::NoCapacity` for rate limits (the executor requeues those rather than
   failing the turn) and `RuntimeFailed` for real errors.

Note the trait's current doc comment says `runtime_kind` is always `Local` when `run_turn` is
called. **That comment becomes wrong with your runner** — flag it and I'll fix it on my side rather
than editing the trait yourself.

Context you get for free as of `be6d78e`: `LocalTurnRequest.speakers` carries display names per
participant, so your prompt can render a real multi-party transcript rather than UUIDs. Use it.

**S-B — Atomic idempotent room create.** Your own flagged follow-up: remote create-then-join can
leave a partially assembled room if the network drops between joins. One idempotent operation, in
your FFI layer.

**S-C — Holds and releases through FFI and UI.** `bots_deliveries_held(limit)` and
`bots_deliveries_release_root(root)` exist in core with **no caller outside tests**, so a held
chain is currently invisible and unreleasable in every app — which means the 30-turn gate *stops* a
chain permanently instead of pausing it. Needs a "waiting for you" surface with a release action.
**Blocked on me:** whether a secondary may release a chain on its primary is a trust-boundary call
(my L-2) and it decides whether `dispatch()` gets a release arm at all. Don't wire the transport
side until I've written that down. The local-only FFI path you can start on now.

**S-D — The "let agents talk to each other" setting.** Fan-out is off unless a caller passes
`with_budgets(HandoffBudgets::default())`, and nothing in any app can pass it — I made the safe
state the default in `11f3c40` precisely so this becomes a deliberate switch. Off by default, and
say plainly in the copy that it lets agents start their own turns and what the 30-turn pause means.
**Ships after S-C**, because enabling holds you cannot see or release is worse than not enabling.

**S-E — The deferred demo cuts.** `@` autocomplete on Swift and web; the web coordinator picker.
Lowest priority — typed names already resolve.

**Order: S-A, then S-B, then S-C, then S-D.** S-E whenever.

## Loki's queue

**L-0 — Route non-Local deliveries, and make an unroutable one speak.** Mine, not Sif's, because
both live in `executor.rs`. The executor learns an optional second runner and a runtime→runner
decision; and when a delivery has no runner that can claim it on any of this member's hosts, it
posts a `System` notice naming the agent and why instead of leaving the row pending forever. The
notice half ships **before** S-A lands, because it converts a live silent failure into a visible
one and still matters afterwards for a `Local` agent whose host is offline.

**L-1 — Does `max_turns_per_root` apply to human sends?** Today it does not: `cause: None` skips
the count and only agent-authored replies are gated. My instinct is that's right — a person
spending their own budget deliberately isn't the failure the gate exists for — but it is currently
an accident of the implementation rather than a decision, and it should be one, on the record.

**L-2 — May a secondary release a held chain on its primary?** Blocks Sif's S-C transport arm, so
it comes first among these. Options: owner-only on the primary, any device of the bound owner, or a
`Manage` check on the conversation.

**L-3 — Coordinator: enforce it or remove it from the surface.** `Conversation::coordinator` is
typed, unenforced, and gives the coordinator no behavior. A field that looks meaningful and isn't
is a liability.

**L-4 — Per-conversation budgets**, and **L-5 — thread shape in rooms**, both to be checked against
a real transcript from tonight before changing anything.

## Tonight

Group chat with multiple **local** agents. `hive bots room-create --name "Crew" --agent One --agent
Two --agent Three`, then `hive bots say <room> "@One @Two what do you think?"` with `hive bots work`
running — added in this commit so group chat can be exercised against a real local model from a
terminal, and as a fallback if the GUI misbehaves. `room-create` prints a warning naming any agent
in the room that has no runner, so a BYOK agent's silence is stated up front rather than discovered
mid-demo.

**Do not mention Claude or Nous tonight** until S-A lands.
