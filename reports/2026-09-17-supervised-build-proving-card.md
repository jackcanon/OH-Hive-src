# Proving card for the supervised build — 2026-09-17

## What this was for

Work item `da8b6721`: two builds each had a real claim and they were not the same code. `9813b40` had
run live cards twice; `9c6f899` (current main) added the supervisor, the `last_seen` work and the
FFI/Swift wiring, and had never run a card. Rolling either one to the fleet blind was the thing to
avoid. This is step 1 of that item — prove current main runs a card correctly — done before any
rollout decision.

## Conditions

Not arranged: the machine had just been through a restart and an upgrade to **macOS 27.0 (26A428)**.
The Hive app was not running afterwards. `HIVE_WORKER_ENABLED=1` was still on disk from before.

Launching the rebuilt app was therefore also an unplanned test of the supervisor's own reason for
existing. **Midgaard checked itself back in within seconds, with no toggle touched** — the second
confirmation of resume-at-launch, this time across an OS version change rather than a same-session
relaunch.

## The check was built to be capable of failing

The failure mode filed as `4c67a5fd` is an acceptance check that would pass on an untouched
workspace. Two precautions:

- `--check-json`, not the shorthand, so whitespace splitting cannot silently change what the check
  means (`4c67a5fd` mistake 2: `grep -q pub fn add src/lib.rs` became a grep for `pub` across three
  files that did not exist, and exited 0).
- The check was **run against the untouched workspace before submitting** and exited 2.

```
{"name":"wrote-the-file","command":"grep","args":["-qx","SUPERVISED BUILD PASSED","result.txt"]}
```

`grep -qx` requires the whole line to match, so a file with extra content fails.

## Result

Card `de8123fa-8e40-4f28-b616-3d663e568a5b`, 17:08:53 → 17:09:19 UTC (26 seconds), three turns.

| what | value |
| --- | --- |
| status | `review` |
| acceptance receipt | **PASSED**, `wrote-the-file` exit 0 |
| model_id | `anthropic/claude-sonnet-4.6` |
| usage on the card | 5,716 in / 317 out |
| `hive.code_brain_usage` | 3 turns, **5,716 / 317**, `$0.021903` |
| target | `--node` Midgaard (`f1cc4f2b…`), claimed by Midgaard |
| `--expect-acceptance passed` | exit 0 |
| after the card | Midgaard `checked_in`, **0 open leases** |

The token totals are the part worth dwelling on: the node self-reports the card's usage and the Edge
Function records its own server-side from the provider's response. They were arrived at
independently and they agree exactly.

## What this covers, and what it does not

Exercised in one run: node targeting (`20260917041000`), host acceptance checks (`1369a32` +
`cc31584`), usage through `BrainTurn` (`d8dbf13`), the resolved model label (`9813b40` + the v8 Edge
deploy), settings-based nous pricing (`074cd2c`), the CLI receipt and `--expect-acceptance`
(`e625119`), the cost line (`2a1a147`), and the supervisor not disturbing the claim/lease/report path
(`548db35` + `9c6f899`).

NOT covered, and worth being explicit rather than letting the green result imply more than it shows:

- No backoff or restart path was exercised. Nothing failed, so nothing retried. That behaviour is
  covered by the nine supervisor unit tests and by a live idle-liveness check, not by this card.
- One card, one provider, three turns, a trivial task. This says the path works; it says nothing
  about a 40-turn card on a real repository.
- The fleet has not been rolled. Every other node still runs an unsupervised worker.

## Recommendation

Unchanged from `da8b6721` and now evidenced rather than argued: roll `9c6f899`, one node first,
watched across a check-in, a card and a deliberate stop, then the rest with backups and graceful
service handling. Preserve deliberate checked-out status; do not broaden any node's internet
permission as part of a rollout.

Loki
