# Track A, demo cut: group chat and group project tonight

Loki, 2026-09-15, ~16:30 MST. Jack wants to demo group chat and a group project **tonight**.
This re-cuts the Track A slices in `LOKI-TRACK-A-ROOMS-AND-MENTIONS-2026-09-15.md` around that
deadline. The design doc is still the design; this says what to build first and why the
ordering changed.

## The key insight: tonight's demo needs no loop safety at all

Track A's expensive half — causation columns, `HandoffBudgets` enforcement, the 30-turn human
gate — exists for one reason: **agent replies waking other agents.** That is depth 1 -> depth 2
and beyond.

A human addressing a room of agents is depth 0 -> depth 1. It fans out once and stops, because
`bots/executor.rs::attempt_reply` still passes `Vec::new()` as its recipient list, so an
agent's reply creates zero deliveries. **No chain can form. No loop is possible by
construction.**

So the demo is safe with *none* of slices 2–4 built. Do not build them for tonight. Do not
"temporarily" flip that `Vec::new()` to get agents answering each other — that is precisely the
change the budget work exists to make safe, and it is mine, after the demo.

What Jack demos tonight: he posts into a room with three agents in it, mentions two of them by
name, both reply. That is real group chat. It is human-driven turn-taking rather than agents
conversing unprompted, and that distinction should be stated plainly in the demo rather than
glossed — it is a deliberate safety property, not a missing feature.

## The whole blocker is two hardcoded lines in `crates/ohhive-ffi/src/bots.rs`

Storage has supported N-agent rooms all along (see the design doc's correction section). The
FFI is what refuses them, in exactly two places:

1. **`conversations_create` (512)** takes a single `agent_id` and hardcodes
   `kind: ConversationKind::AgentDm`, `coordinator: Some(agent)`, `project_id: None`.
2. **`send` (215)** requires `c.kind == ConversationKind::AgentDm` (233) and then
   `recipients.len() != 1 || conversation.coordinator != recipients.first()` -> "DM recipient
   must be its coordinator" (242).

Everything else is already in place: `BotsSend.recipient_ids` is already `Vec<String>` with a
16-recipient cap (220), and `BotsConversation` already surfaces `kind`, `project_id` and
`coordinator` to Swift. This is a smaller job than it looks.

## Slices, in build order

### S-1 — FFI: `conversations_create` accepts a kind, a member list, and a project

Generalize to something like:

```
conversations_create(kind: String, agent_ids: Vec<String>, project_id: Option<String>,
                     coordinator_id: Option<String>) -> BotsConversation
```

- `kind` accepts `"agent_dm"` | `"team"` | `"project"`; reject anything else.
- Every id in `agent_ids` goes through the existing `owned_agent` check — no agent the caller
  doesn't own may be added. Keep that check, per-agent, no exceptions.
- Create the conversation, then `bots_conversations_join` each agent. Core already authorizes
  each join against the room owner (`bots_authorize_join`), so this is not a new trust path.
- `coordinator_id`, when given, must be one of `agent_ids`.
- `project_id` is only accepted when `kind == "project"`; reject it otherwise rather than
  silently ignoring it.
- **Keep the existing single-agent DM call working**, either as a thin wrapper or by defaulting
  — `BotsModel.swift:174` reuses an existing `agent_dm` room by coordinator and that behavior
  must not regress.

Cap `agent_ids` at 16 to match `send`'s existing recipient cap, so a room can never be built
that `send` cannot address.

### S-2 — FFI: `send` accepts rooms

- Widen the conversation lookup (230–236) to accept `Team` and `Project`, not only `AgentDm`.
- Apply the coordinator-equality rule (242) **only** when `kind == AgentDm`. For a room, the
  rule is just: every recipient is a member. Core's `bots_message_send` already enforces that
  (`bots_is_member`, `local_hub/bots.rs:710`) and returns "recipient is not a member of this
  conversation" — do not duplicate the check in FFI, just stop blocking ahead of it.
- Keep the 16-recipient cap and every existing size/request-id guard exactly as they are.
- A zero-recipient send into a room is **valid** and must be allowed: that is a human talking
  in the room without summoning anyone. It creates a message and no deliveries.

### S-3 — mention resolver (`bots/mentions.rs`, was design-doc slice 1)

Unchanged from the design doc, and now on the critical path because it is what turns typing
`@Nous` into `recipient_ids`. Full spec is in section 3.1 there. The cases that must be tested:
mention inside a fenced code block or inline backticks is **not** a mention; an agent's
self-mention is dropped; `@everyone` resolves to every non-archived agent member; an unknown
`@name` is reported in `unresolved` and never errors the send; two agents sharing a name
resolve to **neither**; case-insensitive; trailing punctuation (`@Sif,` `@Sif.`) terminates the
name; an email address is not a mention.

Resolution happens once at send time. Readers never re-parse a body.

### S-4 — Swift UI: room creation, roster multi-select, multi-author rendering

`BotsView.swift` (133 lines) and `BotsModel.swift` (250) currently assume one other party.

- A "New room" affordance: name it, multi-select agents from the roster, optionally pick a
  coordinator. Same sheet creates a **project room** when a project is chosen — that is the
  group-project demo, and it is the same code path as S-1 with `kind: "project"`.
- Message rows must show **who is speaking**. Today's row assumes a single counterpart; a room
  needs an author label/avatar per message or the transcript is unreadable with three agents in
  it. This is the single most demo-visible item in the list.
- `@` autocomplete against the room's agent roster, feeding `recipient_ids` from S-3's
  resolver. If autocomplete is running short on time, **ship plain typed `@names` resolved on
  send** — the resolver does the work either way and the demo does not depend on the popup.
- Show unresolved mentions plainly (unlinked text plus a quiet note), so a typo is visible
  rather than mysterious.

### S-5 — web `apps/desktop/src/TeamChat.tsx`

Same treatment, **after** the demo path works in Swift. One surface working tonight beats two
half-working. Do not start this until S-1 through S-4 are green.

## If time runs short, cut in this order

S-5 first, then `@` autocomplete (typed names still work), then the coordinator picker (a room
with no coordinator is fine — nothing enforces coordinator behavior yet anyway). **Do not cut**
multi-author rendering; a room where you cannot tell who said what does not read as group chat
on a screen.

## Demo acceptance

Two runs, both on one Mac, no fleet involved:

1. **Group chat.** A Team room with Jack and three agents. Jack posts mentioning two by name.
   Exactly two replies appear, each labeled with its author. The third agent stays quiet.
   `agent_deliveries` contains exactly two rows for that message.
2. **Group project.** A Project room created against a real project id, same interaction.
   `BotsConversation.project_id` is populated and survives a reopen.

Then confirm the safety property holds: after the replies land, **nothing further happens** —
no agent answers another agent, `agent_deliveries` gains no rows, and the room goes quiet. That
is the `Vec::new()` behavior working as intended, and it is worth checking explicitly so
nobody mistakes it for a bug during the demo.

## Not in this cut

Slices 2, 3 and 4 from the design doc (causation columns, `Held` status, System-message
suppression, budget enforcement, the 30-turn human gate, agent-to-agent fan-out). Those are the
next session's work and slice 4 stays mine. Fleet item 4 (host-authorized remote delivery) is
also out — tonight is single-Mac, so a secondary reading the room is not needed.

Sif: S-1 and S-2 are the unlock and they are small — start there, and Jack has a demoable path
even if everything after runs late. Flag anything that looks like it needs the coordinator or
budget semantics decided; those calls are mine on this track.
