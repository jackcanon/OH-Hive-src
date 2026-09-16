# ADR-037: The Integrator — verification before an artifact becomes truth

**Status:** Proposed — drafted 2026-09-16 by Loki on Jack's question; not yet reviewed by him.
**Date:** 2026-09-16
**Author:** Claude (Loki).
**Deciders:** Jack (pending review).

> Numbering note: `CONTINUITY.md:1332` records this as owed at ADR-036. That number was taken the
> same day by *Git workspaces and GitHub workflows*, so the Integrator is 037. The continuity log's
> entry is not rewritten — see the log's own don't-rewrite rule.

## The question this answers

Jack, 2026-09-16, on finding that a code card's default 1024-token cap had silently truncated a
file: *"we should have a mechanism in place that checks an artifact is what it's supposed to be
before it gets written to the repo and reported as done. We talked earlier today about a Maintainer
or Integrator. The Integrator is our safety mechanism that should prevent this right? Or is this
something else?"*

Both, and the distinction is the whole point of this ADR.

## Two different failures, two different defences

A card that reports done can be wrong in two unrelated ways.

**The producer lied about its own completeness.** The model ran out of output budget mid-file, the
tool call arrived with half its JSON, the stream ended early. Nothing downstream can detect this by
looking at the artifact: a Rust file that stops mid-function is a perfectly valid string, and a
truncated tool call that defaults its missing arguments is a perfectly valid call. The only place
this is knowable is where the completion was produced, at the moment the backend said *why* it
stopped. **This is not the Integrator's job — it is the node's, and it has to be, because the
evidence does not survive the trip.**

That half is now built (commit `f046fd5`, 2026-09-16): `finish_reason == "length"` reaches callers as
`backend::Completion::truncated`; a truncated Draft or Revise fails the card naming the cap and the
knob that raises it; unparseable tool arguments are refused above the tool dispatch, so no tool —
not just `write_file` — can read a truncated call as "every argument absent"; `write_file` requires a
real `path` and a present `content` rather than defaulting either.

**The producer was complete, and the work is still wrong.** It compiles but breaks something else.
It ignores the acceptance criteria. It rewrites a file it was not asked to touch. It is three
paragraphs of apology where a patch was wanted. Here the artifact *is* the evidence, the producing
node cannot judge it (it already believes it succeeded), and a second opinion is the only mechanism
that works. **This is the Integrator.**

Jack's instinct was right that a safety mechanism was missing. It was the first one, and it belongs
at the node.

## Decision

**1. The Integrator is a gate between "an agent produced an artifact" and "the artifact is the
project's truth."** Every path that writes into a repo, a branch or the vault as a result of agent
work passes through it. It is not advice attached to a finished card; a card whose artifact has not
cleared the Integrator is not done.

**2. It is a role, not a machine, and never the producer.** The Integrator for a card is a different
agent from the one that produced it (ADR-035's identity model makes this checkable: `agent_id` is
distinct from `node_id`, so "not the producer" is a real constraint and not a naming convention).
Self-integration is refused rather than warned about. Where no other agent is available, the artifact
waits for a human — the same shape as `DeliveryStatus::Held` in the chat subsystem, and for the same
reason: a queue a person can drain beats an automatic yes.

**3. What it checks, in order, cheapest first.** Each stage can refuse on its own; nothing later runs
after a refusal.

- **Shape.** Is this the kind of artifact the card asked for? A card whose acceptance names a patch
  and whose artifact is prose fails here, for free, with no model call.
- **Integrity.** Does it show the marks of a cut-off producer — unbalanced delimiters, a file ending
  mid-token, a diff that does not apply? This is a backstop for defence one, not a replacement:
  cheap, catches the cases that slip through a mixed-version fleet, and never the primary guard.
- **Mechanical truth.** Does it build, and do the tests pass? Run in the existing ADR-006 sandbox, on
  the node holding the card's lease. Not a new execution surface.
- **Acceptance.** Does it do what the card's `acceptance` field says? This is the one stage that
  needs a model, and it is the last one, so it is only ever spent on artifacts that already build.
- **Blast radius.** Does it touch only what the card's scope allows? A card asked to fix a footer
  that rewrites twelve files is refused even if every test passes.

**4. Refusal returns the card, with a reason the producer can act on.** Not "rejected" — the stage
that failed, the evidence, and the counter-example. A refused card goes back to the producer with a
bounded number of attempts (default 2, ADR-035's correction-round budget), and after that to a human.
An infinite produce/refuse loop is the failure mode to design against, not a tuning problem.

**5. It reports what it did not check.** An Integrator that returns a bare "passed" is the thing we
are trying to get away from. "Builds, tests pass, acceptance met; did not check runtime behaviour or
the migration path" is a useful verdict. An honest partial verdict is worth more than a confident
complete one.

**6. It is not the Librarian.** The Librarian curates what the library *keeps* — what is worth
retrieving later, how it is organized, what is stale (ADR-028's vault, and the Den Rooms library
Jack described on 2026-09-16). The Integrator decides what is *true enough to enter*. Sequential,
not overlapping: the Integrator is the door, the Librarian is the shelving. An artifact rejected by
the Integrator never reaches the Librarian; an artifact the Librarian later retires was still true
when it entered.

## Consequences

- Every card gets more expensive. A build and test run per artifact, plus one model call for
  acceptance, on top of production. That is the price of "reported as done" meaning something, and it
  is the cost Jack was asking to pay.
- Throughput drops and quality rises, which is the right trade while a fleet is small and each
  artifact matters. At scale, the shape stage and the sandbox run are the parts that stay cheap; the
  acceptance call is the part to sample rather than run universally, and that sampling policy is
  deliberately left out of this ADR.
- It needs the card's `acceptance` field to actually say something checkable. Today many cards do
  not. This makes vague acceptance criteria expensive in a visible way, which is a feature.
- **Handing work to an agent is only as safe as this gate.** This is the ADR that makes "let a Hive
  agent write into my repo" a defensible thing to offer, and until it is built, agent-written
  artifacts want a human between them and `main`.

## Open questions, with defaults

- **Who integrates the Integrator's own work?** Default: nothing. It does not produce artifacts; it
  produces verdicts, and a verdict is auditable after the fact rather than gated before it.
- **Can a member disable it for their own local-mode projects?** Default: yes, explicitly and per
  project, with the setting visible in the project rather than buried in settings. It is their
  machine and their repo (ADR-015's framing). Never off by default, and never off for anything that
  reaches the Hive.
- **Does it gate the chat subsystem's agent replies too?** Default: no. A reply in a room is not an
  artifact and does not enter the project's truth; the loop budgets and the 30-turn human gate
  already govern that surface.
- **Where does it run?** Default: the node holding the card's lease, in the existing sandbox, so this
  adds no new execution surface and no new trust boundary. A hub-side Integrator would need the
  artifact shipped to it, which ADR-007 deliberately avoids.

## Related records

ADR-006 (agent runtime and sandbox — where the mechanical stage runs), ADR-019 (triangulated card
verification — the closest existing idea; the Integrator is its single-gate successor for the
non-community case), ADR-024 (coding agent on the private fleet — the producer this gates), ADR-028
(vault/Librarian), ADR-032 (durable coordinator — where cards spawn and complete), ADR-035
(identity model that makes "not the producer" checkable, and the correction-round budget), ADR-036
(git workspaces — the branches and PRs this decides whether to write into). Commit `f046fd5` is
defence one, at the producing node.

Claude (Loki), answering Jack's question of 2026-09-16.
