# ADR-035 C2 Track A: rooms, mentions, and agent-to-agent turns

Loki, 2026-09-15. The design for multi-agent chat -- several agents and a human in one room,
addressing each other by name, with turns that provably terminate. Written after reading the
code rather than the plan, which matters here: my own C2 kickoff note
(`LOKI-BOTS-C2-GROUP-AND-ADAPTER-AGENTS-PLAN-2026-09-15.md`) got the starting position wrong,
and the correction changes what gets built.

## Correction to the C2 kickoff note: rooms are mostly already built

That note said, as item 1 of "Group chat + mentions":

> **Storage**: wire `ConversationKind::Team` in `local_hub/bots.rs` (today only `AgentDm` is
> handled) -- multi-member rooms, not one-fixed-coordinator DMs.

That is **wrong**, and anyone building from it would have written a migration nobody needs.
What `local_hub/bots.rs` actually does today:

- `bots_conversations_create` (538) inserts whatever `ConversationKind` the draft carries --
  `Team` and `Project` round-trip through `conversation_kind_to_str`/`_from_str` (71/78) with
  no special-casing and no rejection. It seeds `conversation_members` with the owner at
  Read+Post+Manage and, if present, the coordinator at Read+Post.
- `bots_conversations_join` (726) adds any principal the owner controls at Read+Post,
  idempotently, with a `history_boundary` set at join time.
- `bots_conversations_list` (605) joins *through* `conversation_members`, so a Team room
  already appears for every member, human or agent.
- `bots_message_send` (827) already takes **`recipient_ids: Vec<AgentId>`**, checks each one
  is a member (`bots_is_member`, 710), and inserts **one `agent_deliveries` row per recipient
  inside the same transaction** as the message. `DeliveryKey` is `(message_id, recipient)`.

So N-agent fan-out is implemented, atomic, and authorized at the storage layer. `AgentDm` is
not a storage restriction -- it is what C1's *callers* happen to pass. A three-agent room with
a human in it is reachable today through existing calls with no schema change at all.

That is a much better starting position than the kickoff note claimed, and it moves the real
work up the stack.

## What is actually missing

Four things, in dependency order.

### 1. Nothing computes `recipient_ids`

`message_send` is handed a recipient list; no code anywhere produces one from a message body.
There is exactly one occurrence of "mention" in the entire Bots subsystem
(`bots/mod.rs:16`) and it is a comment saying mentions are not built. This is the mention
layer, and it belongs **above** `message_send`, not inside it -- storage should keep taking an
explicit, already-authorized recipient list.

### 2. An agent's reply creates zero deliveries

`bots/executor.rs::attempt_reply` calls `message_send(...)` with `Vec::new()` as the recipient
list. Hardcoded. This single argument is why agent-to-agent conversation does not exist: an
agent's reply is persisted as a message and reaches no other agent, ever.

Changing `Vec::new()` to a resolved recipient list is a one-line diff **and is the most
dangerous change in this track.** It is the moment agent A's reply can wake agent B, whose
reply can wake agent A. Everything in section 3 exists so that line is safe to write.

### 3. The delivery path cannot enforce a budget, because it does not know its own depth

`HandoffBudgets` is fully specified in `bots/types.rs:316` with sensible defaults (one active
turn per agent, two active specialist handoffs per run, two correction rounds, depth two, two
followups). `local_hub/bots.rs:24` states plainly that nothing enforces it.

The reason it cannot be enforced is structural, not an oversight: **`HandoffBudgets` lives on
`Handoff`, and `Handoff` is a different entity from the message/delivery path the executor
actually runs.** `Handoff` carries a `depth` field. `AgentDelivery` carries none. `Message`
has `turn_ref` and `source_event_ref` (both `Option<String>`), and the executor passes `None`
for both. There is no causation link from a reply back to the message that caused it, so there
is nothing to count.

You cannot bound a chain you cannot measure. This is the architectural piece of Track A, and
it has to land before section 2's one-line change, not after.

### 4. Enforcement that fan-out makes newly load-bearing

Two properties are typed as intent and unenforced. Both are harmless in a two-party DM and
actively bad in a room:

- **One coordinator per room.** `Conversation::coordinator` is `Option<AgentId>` and section 6
  of the design doc says one per room. Nothing checks it.
- **`MessageKind::System` must not wake every participant.** `types.rs:147` says so explicitly
  ("these must NOT wake every participant"). Today `message_send` creates a delivery for every
  recipient it is given regardless of `kind`. In a DM that is one spurious turn. In a room of
  six agents, every "still working" status post becomes six billable turns.

## The design

### 3.1 Mention resolution: once, at send time, persisted

A pure resolver above storage, in `bots/mentions.rs` (new):

```
pub struct MentionSet {
    pub recipients: Vec<AgentId>,
    pub unresolved: Vec<String>,   // @names that matched no roster entry
    pub everyone: bool,
}

pub fn resolve_mentions(body: &str, roster: &[AgentProfile], author: Principal) -> MentionSet
```

Rules, chosen deliberately:

- Grammar: `@` followed by `[A-Za-z0-9_-]+`, case-insensitive match against `AgentProfile.name`.
  Not inside a code fence or inline backticks -- an agent pasting shell or Rust containing `@`
  must not summon anyone. This is a real failure mode for a room whose whole purpose is
  discussing code.
- `@everyone` resolves to every non-archived agent member of the conversation.
- **Self-mentions are dropped.** An agent that writes its own name does not wake itself. This
  is the cheapest infinite loop available and it costs one line to close.
- An `@name` matching no roster member resolves to nothing and is reported in `unresolved` --
  never silently dropped, never an error that rejects the message. The UI shows it unlinked;
  the agent sees it as plain text. A typo must not fail a send.
- Ambiguity (two agents with the same name) resolves to **neither**, and both land in
  `unresolved`. Guessing which teammate the user meant is worse than saying so.
- Resolution happens **once, at send time**, and the resulting recipient list is what creates
  `agent_deliveries` rows. Readers never re-parse a body. That means an agent renamed tomorrow
  does not retroactively change who was addressed yesterday, and a message edit does not
  silently re-fan-out.

`message_send`'s existing membership check stays exactly as it is: the resolver proposes,
storage still refuses a recipient who is not a member. Mention resolution is convenience, not
authorization.

### 3.2 Causation and depth on the delivery path

Schema change to `local_hub/bots_schema.sql`, `agent_deliveries` (currently 8 columns, DDL at
line 81):

```sql
cause_message_id TEXT REFERENCES messages(id),   -- the message whose reply produced this one
root_message_id  TEXT REFERENCES messages(id),   -- the human-authored message that began the chain
turn_depth       INTEGER NOT NULL DEFAULT 0      -- 0 for a delivery caused by a human message
```

`root_message_id` is what makes the budgets in 3.3 queryable in one statement each. All three
are nullable-or-defaulted, so existing rows migrate forward as depth-0 roots, which is what
they are.

`bots_message_send` gains one parameter, a `Option<DeliveryCause>`:

```
pub struct DeliveryCause { pub cause_message_id: MessageId, pub root_message_id: MessageId, pub depth: u32 }
```

`None` means a human-originated send: depth 0, root = this message. The executor passes
`Some(..)` derived from the delivery it is currently draining, with `depth + 1`.

This is the whole mechanism. Depth is carried forward through the chain the same way a hop
counter is, and `root_message_id` makes "everything caused by that one thing Jack said" a
single indexed query.

### 3.3 Enforcement: four checks, and a human gate

Enforced in `bots/executor.rs` before a reply's recipients are turned into deliveries -- one
place, not scattered.

| Budget | Check | Why this one matters |
|---|---|---|
| `max_depth` (**2 -> 6**, Jack 2026-09-15) | `cause.depth >= max_depth` -> send the reply as a normal message, create **zero** deliveries | The backstop, not the control. See below for why 2 was wrong once a turn gate exists |
| `max_active_turns_per_agent` (1) | `COUNT(*) FROM agent_deliveries WHERE recipient=? AND status='running'` | The strongest single brake, and nearly free. An agent already thinking is not given a second thought |
| `max_turns_per_root` (**new, 30**) | `COUNT(*) FROM agent_deliveries WHERE root_message_id=?` | The real governor. On reaching it the chain **pauses for a human**, it does not die -- see 3.3.1 |
| fan-out width (`max_active_specialist_handoffs_per_run`, 2) | cap recipients per *agent-authored* reply at 2 | A human may `@everyone`; an agent may not. Asymmetric on purpose |

**Why a per-root turn count is a genuine addition, not belt-and-braces.** Depth alone bounds
the chain but not its cost. A human `@everyone` in a room of six agents produces 6 deliveries
at depth 1; if each reply mentions two others, 12 more at depth 2. That terminates correctly
and has burned 18 model turns from one sentence, and it is quadratic in room size. Depth proves
termination; it does not prove affordability.

### 3.3.1 The gate: 30 turns, then a human, not a wall

Jack's correction, 2026-09-15, and it changes the mechanism rather than just the number. An
earlier draft of this doc treated budget exhaustion as a circuit breaker: chain halts, System
message explains why. Jack's framing is "30 turns before human intervention is necessary" -- a
**pause-and-ask gate**. The difference is not cosmetic:

- A hard stop discards the chain. A human reconstructs the context and re-asks.
- A gate suspends it with state intact. Continuing is one action.

That is also what justifies the higher number. With a hard stop you tune low, because hitting
it is expensive. With a gate you tune to "where a human should probably be looking anyway,"
which is how 30 gets chosen and 24 does not. The number and the mechanism are the same
decision.

**Mechanics.** `DeliveryStatus` today is Pending/Running/Done/Failed/Cancelled/Unknown -- none
of which means "waiting on a person." Add `Held`, with the matching value in
`bots_schema.sql`'s `status` CHECK constraint (slice 2 is already editing that table, so this
costs nothing extra to land there). On reaching `max_turns_per_root` the executor writes the
would-be deliveries as `Held` instead of `Pending`, and
`bots_deliveries_pending_for_agent` must exclude them -- a held delivery is not work.

**Resuming.** A human with `Manage` on the room releases the hold; each release grants another
full `max_turns_per_root` allowance against that root and is recorded. Two consequences worth
having on purpose: the human is the only thing that can extend a chain, and "this exchange
took three approvals" becomes a number you can look at. A held chain that is never released
stays held -- it does not expire into `Cancelled`, because a silent expiry is the failure mode
the gate exists to avoid.

**Why `max_depth` moves 2 -> 6.** These two numbers are coupled and setting 30 alone would
have been dead code. Under depth 2 with agent replies capped at 2 recipients, a root mentioning
k agents yields ~3k turns total -- a six-agent room tops out near 18 and hits the depth wall
before the turn counter is close. `max_turns_per_root: 30` would then never fire in any room
we would realistically build. Depth 2 also describes a shape that is not a team working a
problem: A asks B, B asks C, done -- no agent can act on an answer and report back. At depth 6
the turn count becomes the binding, tunable control and depth is the backstop that normally
never fires, which is the right division of labor between the two.

**Both additions to `HandoffBudgets`:** `max_turns_per_root: u32` (default 30), and
`max_depth`'s default changes 2 -> 6. That struct is `Copy` and serialized into `handoffs`, so
the new field is additive with `#[serde(default)]` -- confirm existing rows deserialize before
relying on it. The changed default only affects rows that don't pin their own budgets.

**No budget event is silent.** Every trip -- a depth stop, a suppressed fan-out, and above all
a gate hold -- posts a `MessageKind::System` message naming which budget stopped what ("30-turn
limit reached; 2 replies are held, release to continue"). Section 6's own language is that
exhaustion "produces a concise blocker for the user." A room where agents quietly stop
answering each other is much harder to debug than one that says why, and this is a system Jack
intends to *watch working*.

### 3.4 System messages do not create deliveries

One condition in `bots_message_send`: if `draft.kind == MessageKind::System`, skip the
`agent_deliveries` insert loop entirely, regardless of the recipient list passed. The message
is persisted and readable; it wakes nobody. This makes 3.3's exhaustion notices free, and it
closes the six-agents-times-every-status-post problem before fan-out can create it.

### 3.5 Coordinator

Enforce what the type already claims: `bots_conversations_create` and any future
coordinator-change path reject a second coordinator for a room. No new concept, one check.

## Build slices

Ordered by dependency. Slices 1-3 must land before slice 4, which is the one that turns agent-
to-agent on.

**Slice 1 -- mention resolver (Sif).** New `bots/mentions.rs`, pure, no storage. Full unit
coverage: code fences and inline backticks ignored, self-mention dropped, `@everyone`,
unresolved names reported not dropped, duplicate names resolve to neither, case-insensitivity,
adjacent punctuation (`@Sif,` and `@Sif.`), `@` in an email address not treated as a mention.
Pure function, no toolchain risk, high test value -- a good first slice.

**Slice 2 -- causation columns and the `Held` status (Sif).** Schema bump for the three
`agent_deliveries` columns, plus `Held` added to `DeliveryStatus` and to that table's `status`
CHECK constraint. `DeliveryCause` threaded through `bots_message_send`; existing 8-value
positional INSERT at line ~899 updated. `bots_deliveries_pending_for_agent` must exclude
`Held`. Migration test: existing rows read back as depth-0 roots, and a held delivery is never
returned as pending work.

**Slice 3 -- system messages create no deliveries (Sif).** Small, independent, testable on its
own: send a `System` message with a non-empty recipient list, assert zero delivery rows.

**Slice 4 -- budget enforcement, the human gate, and agent-to-agent fan-out (mine).** The
`Vec::new()` line, the four checks, the hold/release path and its allowance accounting, the
System notices, `max_turns_per_root` on `HandoffBudgets` and `max_depth`'s new default. I am keeping
this one because its correctness property is a termination argument across three files, and
because the cost of getting it wrong is a bill rather than a failed test. It wants one design
held in one head, same reasoning as the owner-identity boundary on Track E.

**Slice 5 -- UI, all three surfaces (Sif).** Room creation with multi-agent selection, `@`
autocomplete against the roster, multi-author rendering (`BotsMessageRow`/`TeamChat.tsx` both
assume one other party today), and system/exhaustion notices rendered distinctly from chat.

**Not in this track:** tool access for Bots agents (Track C piece 2 -- still needs the Card
tool surface wired into a Bots tool-calling loop first), and anything about which machine a
turn runs on (Track E item 4, host-authorized delivery routing). A room where agents talk to
each other is useful before either exists.

## The test that says this works

Not a unit test -- the acceptance one. In a room containing Jack, a local agent, and the
Anthropic-BYOK agent: Jack posts a question mentioning both. Both reply. One reply mentions the
other by name, which produces exactly one further turn. Nothing at depth 2 wakes anything. The
transcript reads as a conversation, `agent_deliveries` contains exactly the rows the budgets
predict, and total turns for the exchange is a number we can point at.

That is the milestone Jack actually asked for: the Hive able to help build the Hive.
