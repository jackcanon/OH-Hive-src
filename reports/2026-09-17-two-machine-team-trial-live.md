# Two-machine team trial — live run, 2026-09-17

Run `3e092678-8f45-4986-a65a-e4353cac4fc8`. Executed against Sif's kit
(`docs/SIF-TWO-MACHINE-TEAM-TRIAL-2026-09-17.md`) after its blocker was closed in `7beb3c9` and the
hub half deployed. **PASS on every criterion in that document.**

## What had to happen first

Overgaard was running a worker dated Sep 16 08:17. Checked rather than assumed, and the check was
decisive: `strings ~/.local/bin/hive | grep -c "Acceptance checks:"` returned **0**, as did the
coordinator envelope, as did `--check` on its CLI. That worker predates host acceptance entirely, so
its child would have finished with no receipt and the parent's completion gate would have blocked —
a failure with nothing to learn from.

So Overgaard became the "one node first" rollout from `da8b6721`. Architecture matched (arm64 both),
the node was checked out gracefully with no lease held, the existing binary was preserved as
`hive.bak-20260917-preSupervisor`, the new one was verified to contain the acceptance receipt and
coordinator envelope **before** launchd was allowed to start it. Midgaard was already on current main.

## The run

One coordinator card, `--brain nous --cloud-consent --coordinator --node Midgaard --max-turns 6`,
request id `ee6d3df0-cf1d-4516-8b06-7b5b8db92251`. Card `91d4d1d3-e976-4610-9db4-6e74b2acbad4`.

Observed in order, from the hub rather than from any model's account of itself:

1. Coordinator claimed by Midgaard, `running`.
2. Two children created, both parent-linked. Coordinator went `waiting_on_child` **with its lease
   released** — it yielded its execution slot, which is what makes this two computers rather than
   three simultaneous workers.
3. `normalize` claimed by Midgaard; `count` claimed by Overgaard. Each on its declared target node.
4. Both children reached `review` on `mistral-small3.2:24b`, the model verified installed on each
   host via its own ollama API before the run.
5. Coordinator resumed — **the first time Sif's recovery path has run in production** — and
   completed without creating a third child.

## Evidence

| criterion | result |
| --- | --- |
| exactly two children, one parent | 2, no respawn after resume |
| one completion on each selected node | `normalize.py` on Midgaard, `count_words.py` on Overgaard |
| both used the selected local model | `mistral-small3.2:24b` on both |
| both required host checks passed | receipts `status: passed`, `exit_status: 0`, `required: true` |
| coordinator waited, then received results | `waiting_on_child` with lease released, then resumed |
| final review names both real child IDs | `9718f158…` / `34750703…`, matching the hub exactly |
| worked example correct | `  HELLO   Hive  ` → `hello hive`, 2 words |
| usage recorded | coordinator 8,197/2,019; children 5,014/535 and 3,826/379 |
| clean lease release | 0 open leases afterwards |

**Cost: $0.111576** across 5 cloud turns. The children were local-brain and cost nothing.

## The part worth dwelling on

`normalize.py` exists on Midgaard and is **absent on Overgaard**. `count_words.py` exists on
Overgaard and is **absent on Midgaard**. Checked in both directions on purpose: identical path names
on two machines are not shared storage, and a run that quietly used one filesystem would have looked
identical from the hub.

The coordinator's `final-review.json` cites the **host receipts** as its evidence — "exit_status=0,
passed=true" — not the children's prose. It was instructed never to claim completion from a child's
account alone, and it did not. Its composition note is substantive rather than ceremonial: it
observes that `normalize_label` leaves only single spaces, so `count_words`'s `split()` receives
input in its simplest form, and that the composition is idempotent.

## What this does and does not establish

Establishes: a cloud coordinator can decompose work, delegate to local models on two separate
computers, release its slot, be resumed with trustworthy child identities and host-verified results,
and review them without respawning. That is the first genuine instance of the fleet doing a piece of
work as a team.

Does not establish: the negative case (a child whose report merely claims success must block the
parent) has not been run — it is a separate bounded run in Sif's plan. Nor has the offline-target
case. Two children, one batch, synthetic tasks. Seven of the nine nodes still run pre-supervisor,
pre-acceptance workers.

Loki
