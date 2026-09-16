# Agent profile buildout — parity with Buzz's agent profile

Loki, 2026-09-16. **Backlog, not the current queue.** Jack asked that this be documented and
addressed "pretty soon," not prioritized now. Filed so it does not evaporate.

Jack's ask, from screenshots of Buzz beside the Den: the Den's **Agent details** panel should be
comparable to a Buzz agent profile — a persona, an activity log you can peek into, custom agent
instructions, memories, per-agent customization.

## The finding that matters: an agent has no instructions field at all

`AgentProfile` is: `id, owner, name, role_revision, runtime_kind, preferred_host,
capability_policy_ref, provider_account_ref, memory_namespace, archived, created_at, updated_at`.

There is **no description, no instructions, no persona, no avatar.** `AgentProfilePatch` can edit
name, preferred host, capability policy ref and memory namespace — nothing else, because there is
nothing else to edit.

So the entire prompt an agent gets is `"You are {name}."` plus the participants note and the
transcript (`bots/runner.rs`). Every Den agent is a name and nothing more. Two agents in a room
differ only by the word in front of the colon.

**I walked into this myself and did not notice.** Writing the live three-agent test I gave the
agents roles — "You investigate and report facts plainly", "You propose concrete implementation
steps", "You look for what will break" — and then had to bind them to `_role` and throw them away,
because there is nowhere to put a role. The test named its agents Scout, Builder and Skeptic and
they answered identically, which reads as model weakness and is actually a missing column.

**The hard half is already built.** `role_revision` is documented as "bumped every time the agent's
role/instructions change; conversations pin the revision they were bound under so an instruction
edit mid-conversation doesn't silently reinterpret history" — the careful part, versioned
instructions that can't retroactively rewrite a transcript. It is **never incremented anywhere in
the tree**; every construction site hardcodes `1`. We built revision pinning for a field we never
added.

## Where the Den stands against Buzz

| Buzz | Den today | Size |
|---|---|---|
| Agent instructions / persona | **nothing** | the one that matters |
| Description | nothing | XS |
| Avatar | nothing | S |
| Memories tab | `memory_namespace` exists, nothing reads or writes it | M |
| Activity log | node-level `activity` table; nothing agent-scoped | **S — see below** |
| Channels tab | rooms exist and `conversations_list(Principal::Agent)` already works | **XS** |
| Runtime: harness / provider / model | `runtime_kind` + `preferred_host`; the local model is node-level, not per agent | M |
| Message | DM path exists | done |
| Start agent | no equivalent concept | needs a definition before a button |
| Managed by | `owner` | XS to surface |
| Agent type | `runtime_kind` | done |
| Archive | `bots_agents_archive` exists | XS to surface |
| Duplicate / Export / Delete | none | S each |
| Public key | agents have no keypair | real work, and needs a reason |
| Trading card | — | Buzz whimsy, skip |

## Two things that are nearly free and worth taking first

**Channels tab.** `bots_conversations_list(Principal::Agent(id))` already returns exactly the rooms
an agent belongs to. This is a list view over an existing call.

**Activity log, from `agent_deliveries`.** Every delivery already carries status, lease generation,
retry deadline, timestamps, and now `cause_message_id` / `root_message_id` / `turn_depth`. "What has
this agent actually done" is a query we can already answer: turns run, turns failed, turns requeued
for capacity, and — once the gate is live — **turns held waiting on a person.**

That last point makes this more than a nice panel: it is the natural home for Sif's S-C (holds and
releases have no caller outside tests). A "waiting for you" row in an agent's activity log, with a
Release action, is the release surface *and* the debugging surface in one screen. Worth building
those together rather than twice.

## What is real work, and what should not be faked

- **Instructions + description + revision bumping.** New columns, a migration, `AgentProfilePatch`
  gains the fields, `agents_update` bumps `role_revision` when instructions change, and the runner
  interpolates them. Straightforward, and it is the whole difference between a named agent and a
  teammate.
- **Memories.** `memory_namespace` is a scope with nothing behind it. Real memory is storage plus
  retrieval plus a write path, and it should land beside the room library work rather than as a
  private store per agent.
- **Per-agent model / harness.** Today the local model is a node-level setting (`--model`, `HIVE_MODEL`).
  Buzz's per-agent provider and model picker implies routing per agent. That is a real feature and
  interacts with the cloud runner routing just landed.
- **Tools.** Do **not** ship toggles. `capability_policy_ref` is hardcoded `"default"` at every
  creation site and read back nowhere, and `bots/runner.rs` hardcodes `ToolsLevel::InferenceOnly` —
  Bots turns have never called a tool. The current panel is honest about this ("Raw reference only —
  not yet enforced"), and it should stay honest until the Card tool surface is wired into a Bots
  tool-calling loop (Track C piece 2). A switch that does nothing is worse than an absent one.

## One security note, because it is the same thread as context import

A free-text instructions field that lands in the prompt is the untrusted-input surface from
`LOKI-CONTEXT-IMPORT-SPEC-2026-09-16.md`, arriving from the other direction. Instructions a person
typed for their own agent are trusted by that person. Instructions **imported** from a `SOUL.md` or
a `.clinerules-architect` are third-party text, and they land in exactly this field.

So the field needs provenance from day one — authored here, or imported from where — and imported
instructions belong in the untrusted-data envelope rather than spliced into the system prompt.
Related and already flagged by the audit: `runner.rs` interpolates `agent.name` raw into
`"You are {}"`, which was a curiosity when names were local and becomes a real surface once a name
or a persona can arrive from a file.

## Addendum: show the model in the agent list (Jack, 2026-09-16)

Jack wants the agent list to show the model, the way a Buzz card reads **Freyja / gpt-5.6-sol**.
The Den's row currently reads **Midgaard / This Mac** — that is the *host*, from
`BotsView.swift:30` (`agent.preferredHost == model.hostID ? "This Mac" : "Another computer"`).

**This is not a label change: nothing in the app knows what model any agent uses.**

- **Local agents.** The model is a flag on a *different process* — `hive bots work --model`, or
  `HIVE_MODEL` — handed to `LocalModelTurnRunner::loopback(host, model, endpoint)` at construction.
  `nodeconfig` carries `whisper_model` and `comfyui_checkpoint` but **no LLM model field**, so the
  app has nothing to read. Two workers on the same Mac could be draining with different models and
  the agent would be identical in both.
- **BYOK agents.** `cloud_runner.rs` has no model either; the choice is made server-side inside the
  `bots-turn` function. The app knows the provider, not the model.
- **Result:** the list shows the host because the host is the only thing it actually knows.

### What it takes to show it truthfully

Two fields, and they answer different questions:

1. **Configured model — intent.** A nullable `model` on `AgentProfile`, where `None` means "whatever
   the host is running." This is the per-agent model/provider routing already listed above, and it
   is what makes the picker in a Buzz-style edit sheet mean anything. It also has to be honoured by
   the runner, which today takes its model from the worker's flag and ignores the agent entirely.
2. **Last model used — truth.** The runner reports the model it actually ran, recorded per turn
   (`LocalTurnOutcome` carries `usage` today and no model; `agent_deliveries` is the natural place).

Both, not either. Buzz can show one string because a Buzz agent's model is fixed configuration, but
the Den already has an `Automatic` shape coming — Halo pooling, a model falling back, a BYOK
provider choosing server-side — and in all of those the configured value is a preference, not a
fact. A list that shows intent while something else ran is the kind of small lie that costs an hour
the first time a reply looks wrong. **Show last-used when known, fall back to configured, fall back
to the host.**

Cheap interim while the real fields are absent: for a BYOK agent, show the provider ("Claude",
"Nous") instead of the host, since that much *is* known from `runtime_kind`. For a local agent, the
host remains the only honest thing to print.

## Suggested slicing when it comes up the queue

1. **Instructions + description + `role_revision` bumping** (Loki: core, schema, runner) and the
   edit UI (Sif). Nothing else on this list matters as much.
2. **Channels tab and the activity log** (Sif, on existing calls), with **S-C's release action built
   into the activity log** rather than as a separate screen.
3. Avatar, Managed by, Archive, Duplicate — the cheap profile furniture.
4. Per-agent model/provider routing — and with it the model in the agent list (addendum above):
   a `model` field for intent, a recorded last-used model for truth, list shows last-used then
   configured then host.
5. Memories, alongside the room library.
6. Tools — only after enforcement exists.
