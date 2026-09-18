# ADR-038: Concurrent agents on one project — uncommitted state is private state

**Status:** Proposed — drafted 2026-09-18 by Loki on Jack's question; not yet reviewed by him.
**Date:** 2026-09-18
**Author:** Claude (Loki).
**Deciders:** Jack (pending review).

## The question this answers

Jack, 2026-09-18, after a night in which two agents working the same checkout collided four times:
*"I do think it's important that we understand the issue that we're running into because agents in
Loki's Den and The Hive will also run into the same issues so it's worth understanding the problem
and creating an elegant solution that prevents issues."*

He is right that this is not a workflow annoyance. The Den's premise is several agents working a
project together. Every collision two agents hit on one machine is a collision the product will
inflict on every member who uses it as intended.

## The failure class, stated once

**An agent's inputs, verification, or outputs silently included state belonging to another agent
that was not committed.**

That single sentence covers all of it. Uncommitted state is *private* — it exists in one agent's
head and one working tree — but the things built from it are *shared*: a test result someone
believes, a binary that opens a real database, a schema version written into a member's vault.
Privacy leaking into shared artifacts is the whole bug.

## What actually happened (2026-09-17/18, Loki and Sif, one checkout)

**1. Verification contaminated.** `cargo test --workspace` in a shared tree tests both agents'
uncommitted work. Green means "our combined mess passes", not "my change passes". I read green
several times before building in an isolated worktree, where six migration tests failed
immediately — failures my own change caused and the shared tree had been hiding under someone
else's in-flight edits.

**2. Artifacts contaminated, and it reached production.** I rebuilt the desktop app from the shared
tree. It compiled Sif's uncommitted migrations, opened Jack's live vault, and moved it to schema 21
— past what any committed build could read. The room survived only because an already-running process
held the database open; a restart would have failed. Nothing was lost, and nothing about the
mistake was visible from the side that made it.

**3. Shared namespace contention.** Sif and I each allocated a schema `user_version`. She took 16 on
top of my 15. That worked because she happened to look, not because anything made it work. The
same class already has a scar in this repo: ADR-037's own header records that *two agents took the
number 036 on the same day*, and the log entry was left wrong rather than rewritten.

**4. Attribution lost.** A flaky test from her commit cost me three re-runs, each spent asking "did
I just break this?" — the expensive question, because the honest answer required investigation
every time.

Note what these have in common. Not one was caught by a gate. Each was caught by a person noticing
an inconsistency, and two were caught only because the work happened to be done twice.

## Decision

**D69 — Uncommitted state is private state. Anything shared must derive only from committed state.**

Four mechanisms follow from that one rule. They are ordered by how much they prevent rather than
detect.

### 1. Isolation by construction, not by discipline

Every agent gets its own worktree on the shared repository. Not a convention an agent remembers —
the thing it is handed. ADR-036 already decided persistent-clone-plus-worktree-per-card, but framed
it as workspace preparation and efficiency. The reframing here is the point: **the worktree is not
a performance optimisation, it is the isolation boundary that makes a verification result
attributable.** A card's worktree and an agent's worktree are the same mechanism serving two
purposes, and the second is the load-bearing one.

Consequence for the Den: an agent is never handed the user's working checkout. It is handed a
worktree, and the user's own tree is never the substrate an agent builds in.

### 2. Artifacts carry their provenance, and refuse when it matters

An artifact built from an uncommitted tree must say so, and must not touch shared state.

Half of this shipped tonight (`0b22b1a`): the desktop bundle records `OHHiveSourceCommit` in its
Info.plist, shows it in About, appends `-dirty`, and the build prints a warning. That converts an
invisible failure into a visible one. The other half is owed: **an artifact that can write to a
member's vault, a fleet database, or any shared store should refuse to run when its provenance is
dirty**, unless explicitly overridden for development against a scratch store.

The asymmetry is deliberate. Refusing to *build* dirty would break normal development, which is
mostly dirty by definition. Refusing to *touch production* while dirty costs nothing anyone wants.

### 3. Identity that does not require coordination

A single monotonic counter allocated by hand is not safe for concurrent authors. `user_version` is
one; ADR numbers are another; ports and fixture names are the same shape.

For schema migrations specifically, move from "the next integer" to a set of named migrations and
an `applied_migrations` table: each migration is a file with a time-ordered name, and opening a
database applies whatever it has not already applied. Two agents adding a migration the same
evening produce two differently-named files and no collision — the property we currently get from
one of them noticing the other.

This also makes the *guard* strictly better. Today's "database schema is newer than this worker"
refusal compares two integers, which is a coarse proxy. It did its job tonight, which is why the
vault was safe rather than corrupted — worth keeping. With an applied-set, the same check becomes
exact: *this database has applied migrations this binary does not know about*, and it can name
them.

Cost, honestly: a schema change and a one-time backfill mapping `user_version = N` onto the set of
migrations considered already applied. Bounded, and much cheaper now than after there are many
member vaults in the wild.

### 4. Production state is reached only by released builds

Even with perfect isolation, an agent pointing a development build at a member's real data is the
incident from §2. Development artifacts get a scratch store by default; reaching a real vault is an
explicit, deliberate act, not the path of least resistance.

## Consequences

Verification becomes attributable: a green gate means *this change, on a known base*, which is the
only claim worth making. Two agents stop being able to corrupt each other's evidence.

The cost is real and worth naming. Worktrees mean more disk and a colder build cache per agent —
the isolated builds tonight took two minutes each where the shared tree took ninety seconds. That
is the price of a result you can believe, and it is cheaper than the two hours spent tonight
re-deriving conclusions that a contaminated tree had quietly invalidated.

Migration renumbering is a one-time cost with a migration path of its own, which is the kind of
thing that only gets more expensive.

## Open questions (defaults noted, override if wrong)

- **Worktree per agent, or per task?** Default: per task, matching ADR-036's per-card model, since a
  long-lived agent worktree accumulates exactly the uncommitted state this ADR is about.
- **Where does the boundary live — the Den, or each agent?** Default: the Den hands out worktrees,
  because a rule an agent must remember is the failure mode we are removing.
- **Does the dirty-provenance refusal apply to the CLI too, or only the app?** Default: both, since
  the CLI is what opened the vault in the incident above.
- **Do we renumber migrations before or after the next release?** Default: after, since it touches
  every member vault and this week already shipped a schema change.

## Related records

- **ADR-036** — decided persistent clone + worktree per card. This ADR reframes that mechanism as
  the isolation boundary and extends it to concurrent agents.
- **ADR-037** — the Integrator, verification before an artifact becomes truth. Same family: that one
  is about an artifact lying about its completeness, this one about an artifact lying about its
  provenance.
- **ADR-032** — durable project coordinator, where multi-agent work is scheduled.
- `0b22b1a` — build provenance stamping, the first mechanism here to ship.
- Session record "Loki's Den — rename, signing, and the permission that ate an evening"
  (2026-09-18, Loki's Lab and Hive project) — the incident narrative this ADR generalises from.
