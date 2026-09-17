# Negative case: a child that cannot succeed

**Run** `82943d27` · **Coordinator card** `92fe2372-c686-4ea2-b4a9-6cf8117fd530` · 2026-09-17

Every coordinator run so far has been a run that worked. That proves the happy path and
nothing else. The question this run asks is the one that actually matters for unattended
work: **when a child cannot succeed, does the failure stop, or does it spread?**

## How the failure was made unavoidable

The `count` child's acceptance check was patched to be self-contradictory:

```
assert f('hello hive') == 2
assert f('hello hive') == 3
```

No implementation satisfies both. This is deliberate — a check that merely *might* fail
would leave the result ambiguous. Before submitting, the patched check was run against a
known-correct implementation and exited 1 with an `AssertionError`, so the run began with
the unsatisfiability proven rather than assumed.

## Result

| | outcome |
|---|---|
| `count` child | **blocked**, receipt `{"status":"failed", "name":"count-unsatisfiable"}` |
| parent | **blocked**, output `FAILED: spawned child team-82943d27-…-count failed` |
| `normalize` child | **review** (succeeded on its own merits) |
| children spawned | exactly 2 — **no respawn** |

All four criteria hold. The failure stayed where it happened: the sibling that had nothing
to do with the contradiction finished normally and was not dragged down with it, and the
parent stopped rather than retrying a child that could never pass.

## What this run did NOT prove

Worth being plain about, because the gap is easy to miss when the headline says PASS.

The parent blocked through **child-failure propagation** — the child reported its own
failure honestly, having diagnosed the contradiction itself. So the path that ran was
"child says it failed → parent believes it → parent stops."

The **receipt-matching** path of the completion gate is still unexercised: a child that
lands in `review` carrying a missing or mismatched receipt — the dishonest child, not the
honest one. That is the harder and more important case, because it is the one where the
gate is the only thing standing between a wrong answer and a merged result. The model's
honesty here is welcome, but it is not a control; the next negative case has to remove the
model's cooperation from the equation, not rely on it.

## Related

The same day's capability gate (`20260917180000`) addresses the adjacent failure: a card
that declares checks being claimed by a worker that cannot run them at all, which completes
with **no** receipt — indistinguishable from a card that never declared checks. Honest
failure, dishonest failure, and no receipt at all are three different shapes; this run
covers the first.

---

## Deploy record for the capability gate (same day)

`20260917180000_acceptance_capability_gate` deployed 2026-09-17 ~18:23 UTC.

Sequencing was the whole risk and it was checked before deploying rather than after. At
18:01 **every** code-capable node reported `capabilities.acceptance = null`. Deploying at
that moment would have made every card declaring checks unclaimable fleet-wide — the
starvation failure, which is worse than the one the gate replaces. So the workers were
rolled first:

| node | how | acceptance |
|---|---|---|
| Odin | `hive` binary replaced, launchd worker kickstarted | true |
| Jotunheim | same | true |
| Overgaard | already on the current binary | true |
| Midgaard | Swift app rebuilt and relaunched | true |
| Heimdall | Linux, still needs a CI-built artifact | **null** |

Heimdall staying `null` is the gate working, not an outage: it genuinely cannot run
acceptance checks, so it is no longer offered cards that declare them. Before today it was
offered them and completed them with no receipt.

Verified after deploy: `md5(pg_get_functiondef(oid))` of the live `hive.node_claim_card`
equals the fresh PGlite replay byte for byte — `11fc4e74fdb91a01a944c7d870b62664`
(was `99f95e90…`). Workers polling cleanly at 5s with no claim errors.

### One more thing the gate found on the way in

`scripts/migration-replay/node-targeting.mjs` broke when the migration was replayed. Its
fixture nodes advertise no capabilities beyond `modalities`, and its cards declare checks —
so the gate correctly refused to hand them over. That break was the gate working. The
accidental coverage was replaced with deliberate coverage: a card pinned to one node and
declaring checks now waits when that node cannot run them, rather than falling through to
the one node it is allowed to go to.
