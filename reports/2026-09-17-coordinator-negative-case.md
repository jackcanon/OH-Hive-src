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

### Post-deploy live proof

Sif's pilot (`ebac5358`, Overgaard) proved a capable worker runs the checks, but it ran
*before* the migration deployed. The question the deploy itself raises is different and
bigger: **did the new predicate break claiming for everyone?** A subtle bug there is silent
— cards simply stop moving.

Card `798d7399-3db0-4363-8fcf-c8ce8cd04318`, one required check, no target node. Odin
claimed it within seconds, wrote the file, ran the check on the worker and recorded:

```
"status":"passed", "name":"gate-live", "exit_status":0, "required":true, "timed_out":false
```

Card reached `review`. The file was then read back off Odin directly rather than taken from
the model's report — `od -c` shows exactly `G A T E   L I V E \n`, ten bytes. Gated work
still flows, and the receipt is real.

One earlier attempt (`af208971`) was submitted with a workspace path that did not exist on
the claiming node and blocked on that. Recorded rather than quietly rerun: it was my error,
and it incidentally confirmed the same thing — the card was claimed, so the gate was not
refusing eligible work.

---

## Follow-up: the two control-plane claim paths (20260917190000)

The gate migration named `ctl_pilot_claim` and `ctl_d_ctl_pilot_claim` as still open and
declined to touch them, because reproducing three safety-critical claim bodies by hand is
how a transcription slip reaches the most important function in the system. Closed now, by
removing that risk rather than accepting it: the bodies were **generated**, not retyped —
dumped from a fresh replay of all 108 prior migrations, confirmed md5-identical to
production first, then the clause inserted at a single asserted anchor.

Checked afterwards by diffing the replayed schema before and after: **7 added lines per
function, zero deletions, and no other function in the schema moved.**

### A finding that changes how to read the original item

These paths cannot serve a `code` card at all. The predicate requires
`execution_mode = 'hive'`; the ADR-024 line requires `'local'` for modality `code`. Both
cannot hold. Code cards are the only cards carrying acceptance checks today, so the hole
was unreachable for code work — it was filed as high priority and it was not.

It was still right to close, and not as defence-in-depth hand-waving:
`required_capabilities` is free-form jsonb, so nothing structurally confines `acceptance`
to code cards; and the unreachability rests entirely on that one ADR-024 line continuing to
say `local`. Relax it and the hole opens silently, with no test failing. A guarantee that
holds by coincidence of another clause is not a guarantee. The fixture therefore **asserts**
the unreachability rather than trusting the comment that claims it — if that line ever
changes, that assertion is what says so.

### Verification

Both fixture halves proven capable of failing independently, by stripping the clause from
one function at a time and confirming the failure named that specific function — otherwise
a fixture that tested one function twice would look identical to one that tested both.

Sequencing checked before deploying, not after: zero ready cards, zero open leases, no
gated hive-mode work, so unlike `node_claim_card` there was no starvation risk and no
client rollout was needed first.

All three claim paths now verified byte-identical to fresh replay:

| function | md5 |
|---|---|
| `hive.node_claim_card(text)` | `11fc4e74fdb91a01a944c7d870b62664` |
| `hive.ctl_pilot_claim(text,uuid,uuid)` | `b0179738cbf9b174792acd05dbe374e8` |
| `hive.ctl_d_ctl_pilot_claim(text,uuid,uuid)` | `278665f23804444892827ba4692e12e6` |
