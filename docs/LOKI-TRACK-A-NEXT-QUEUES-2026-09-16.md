# Next queues after the Track A merge — Sif and Loki

Loki, 2026-09-16. Everything through Track A is merged and pushed (`main` at `11f3c40`). Rooms
work on all three surfaces; agent-to-agent turns are implemented, tested, and **off by default**.
This divides what comes next.

## Read this first: a mentioned Claude or Nous agent never answers, silently

`ensure_provider_agents` (Track B) creates **"Claude"** (`AnthropicByok`) and **"Nous"**
(`NousByok`) profiles as soon as a member has those keys on file. They are real agents: they show
in the roster, they can be added to a room, they resolve from `@Claude`, and `message_send`
happily creates an `agent_deliveries` row for them.

`bots/executor.rs` then drains only agents where
`runtime_kind == AgentRuntimeKind::Local && preferred_host == Some(self.host)`.

So a BYOK agent's delivery is created and **never claimed by anything, ever**. No error, no
timeout, no notice — the row sits `pending` forever and the room just looks like Claude ignored
you. That is worse than the agent not existing.

**Two consequences.** For tonight's demo: mention only local agents, or the demo shows an agent
silently failing to answer. For the roadmap: this is the gap that matters most to "the Hive helps
build the Hive," because the teammate Jack most wants in the room is the one that cannot speak.

Fixing it properly is the cloud turn runner scoped in `LOKI-BOTS-C2-GROUP-AND-ADAPTER-AGENTS-PLAN`
Track B: a `LocalBotsTurnRunner` implementation that calls a hub Edge Function instead of a local
endpoint, because the BYOK key is hub-side only (ADR-008) and must never reach the device. The
`interview` function (542 lines, Deno) already has `callAnthropic`/`callOpenAI`/`callNous` and a
shared `Brain` type that resolves a member's own key server-side; what is missing is a function
that accepts a Bots conversation's history and returns a reply the executor can persist.

That is **S-1 below** and it is the highest-value item on either list.

---

## Sif's queue

Surface, integration and the Deno/Supabase layer — her real toolchain, and all three clients.

**S-1 — Cloud turn runner for BYOK agents (the big one).** A new Edge Function (or a new `mode`
on `interview`) taking a Bots conversation's bounded history and returning a reply; a
`CloudBotsTurnRunner` implementing the same `LocalBotsTurnRunner` trait; and executor routing that
sends `AnthropicByok`/`NousByok` deliveries to it. Same contract `LocalModelTurnRunner` already
satisfies, different transport underneath. Key never leaves the hub.

**S-2 — Make an unroutable delivery say so.** Cheap, independent of S-1, and worth doing *first*
because it removes a silent failure today: when a delivery's recipient has no runner that can
claim it on any of this member's hosts, post a `System` notice naming the agent and why, rather
than leaving the row pending forever. Even after S-1 this still matters for a `Local` agent whose
host is offline.

**S-3 — Atomic idempotent room create.** Sif's own flagged follow-up: remote create-then-join can
leave a partially assembled room if the network drops between joins. One idempotent operation.

**S-4 — Holds and releases through FFI, transport and UI.** Core has
`bots_deliveries_held(limit)` and `bots_deliveries_release_root(root)` with **no caller outside
tests**. Needs: `dispatch()` arms plus `RemoteLocalHub` methods (see the trust note in L-2 before
wiring the release arm), FFI exposure, and a "waiting for you" affordance showing held chains with
a release action. Until this exists, a held chain is invisible and unreleasable in the app — which
means the 30-turn gate currently *stops* a chain permanently rather than pausing it. **S-4 must
land before agent-to-agent is enabled for real use.**

**S-5 — An explicit "let agents talk to each other" setting.** Fan-out is off unless a caller
passes `with_budgets(HandoffBudgets::default())`, and nothing in any app can pass it. This is the
user-facing switch: off by default, and the copy should say plainly that it lets agents start
their own turns and what the 30-turn pause means.

**S-6 — Deferred demo cuts.** `@` autocomplete on Swift and web; the web coordinator picker.
Lowest priority of the six — typed names already resolve.

## Loki's queue

Semantics, trust boundaries and loop safety — the calls that want one design in one head.

**L-1 — Should `max_turns_per_root` apply to human sends?** Today a human send is never checked
against any budget: `cause: None` skips the count, and only agent-authored replies are gated. A
person `@everyone`-ing a 16-agent room repeatedly can spend freely. My instinct is that this is
correct — a human spending their own budget deliberately is not the failure mode the gate exists
for, and gating it would be paternalistic — but it is currently an accident of the implementation
rather than a decision, and it should be one. Mine to settle and write down.

**L-2 — May a secondary release a held chain on its primary?** Release is a real spend decision.
`bots_deliveries_release_root` is unexposed over transport, so today the answer is no by
omission. Options: owner-only on the primary; any device of the bound owner; or a `Manage`
permission check on the conversation. This is a trust-boundary call, so it gets decided and
documented before S-4 wires the dispatch arm — same posture as the owner-identity gap on Track E.

**L-3 — Per-conversation budgets.** Budgets are process-level on the executor.
`Conversation::policy_revision` already exists and rooms already pin it. A quiet room and a
working project room plausibly want different numbers. Decide whether budgets become conversation
policy, and if so, how a policy change mid-chain interacts with a chain already counting.

**L-4 — Coordinator enforcement** (design doc 3.5). "One coordinator per room" is typed as
`Option<AgentId>` and unenforced; nothing rejects a second, and nothing gives the coordinator any
actual behavior. Decide whether the coordinator means anything yet, and either enforce it or
delete the concept from the surface until it does — a field that looks meaningful and isn't is a
liability.

**L-5 — Thread shape in rooms.** Every agent replying to one message threads under it
(`thread_root: incoming.thread_root.or(Some(incoming.id))`). With one agent that reads correctly;
with four replying to the same prompt and then to each other it may read as a pile rather than a
conversation. Worth checking against a real transcript from tonight's demo before changing
anything.

## Order

Sif: **S-2, then S-1**, with S-4 next and S-5 immediately after it. S-2 before S-1 because it
turns a silent failure into a visible one in an afternoon, and that failure is live right now.

Loki: **L-2 before Sif reaches S-4** (it blocks the release arm), then L-1, then L-4.

Neither list depends on the naming decision.

## Naming, for the record

Jack, 2026-09-16: **Hearth** is the desktop application; **Hive** describes the community working
together. Research ongoing, nothing selected or cleared. No renaming in this queue — when it
happens it is a deliberate terminology pass across UI strings, docs and the survey, not something
to drift into file by file. Halo keeps its established pooled-inference meaning.
