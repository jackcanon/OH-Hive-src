# Sif → acceptance checks. Pivot at this boundary, not the next one.

**Loki, 2026-09-16 night.** Jack put the call to me. Here it is, with the reasoning, because a
pivot without reasoning is just churn.

---

## First: the premise we were both working from was wrong

Earlier tonight I told Jack you were "on the Copilot spike," and he said reasonably enough:
*"Let's finish her spikes, and then we can do the acceptance checks."* That implied a short
remaining distance.

Reading your last four entries, you are not on a bounded spike. You are deep in the **ADR-033/034
subscription-coordinator thread** — fenced turn runner, durable replies, offline restart recovery,
Copilot SDK adapter wired to the runner, and now `publish_to_bots` with an atomic LocalHub
transaction. That is a substantial architecture, and your own "Next:" names another layer after
it: app account lifecycle, refresh and revocation, subscription executor, FFI and UI wiring,
recovery of abandoned delivery claims.

So "finish the spike then switch" was never a small ask. It is several more days. That changes
the decision, and Jack should hear that rather than have me quietly act on it.

## Why this boundary is the right one

Not "stop whatever you're doing" — **stop here specifically**, because here is clean:

- **Your tree is empty.** Nothing half-edited. `git status` is zero files.
- **`45ea551` is a complete unit**: the publication API, 203 core tests passing, clippy clean
  with warnings denied, formatting clean, four new real-disk tests.
- **What comes next in your thread is a new layer**, not a continuation of the edit in front of
  you. FFI and UI wiring does not resume a half-written transaction.

Pausing at the end of a layer costs a context reload. Pausing three files into the next one costs
a lot more.

## Why acceptance now

1. **Gate 1 is one item from done, and Friday 11:00 is the target.** Acceptance checks are the
   only thing left in it — the model-fit gate is node scheduling and CI hygiene is hygiene, on the
   gate's own wording. Everything else in Phase 1 has landed.
2. **CI is green for the first time.** Your `cargo fmt` and the Windows clippy fix cleared the
   last two reds; the Swift, Tauri and Python jobs I added are all passing. Acceptance checks
   touch `coder.rs`, `tools.rs` and `worker.rs` — three load-bearing files — and **landing that
   against a green board means any breakage is unambiguously yours to see.** Against a red board
   it would have been guesswork. This window is worth using.
3. **The gap is now demonstrated, not theoretical.** Five cloud cards have run tonight. All five
   produced correct work. In every one, the Hive had no way to know that: the card completed
   identically whether the code worked or not, and the only reason we know it worked is that I
   read the diffs and re-ran the tests by hand. PR #29 is literally a change to this repo whose
   sole verification was a human reading it.
4. **The subscription thread serves Phase 5** (the fleet running itself), which is three gates
   out. Acceptance is Gate 1. Finishing a Phase 5 layer before a Gate 1 blocker is the ordering
   we agreed not to do.

## One experimental result that changes the spec before you start

I ran the §6 claim rather than leaving it as an assertion, and **it went against me.** Two cards,
same workspace, same bug, only the task text differing; the test suite instrumented to write
`RAN.log` on execution so "did it run the tests" is a fact on disk rather than a claim in the
model's own report.

Both arms ran the tests unprompted. Both fixes were correct. **So do not build §2–§5 on the
strength of §6** — the agent verifying itself is a better starting position than I assumed. The
spec is amended with the caveats (n=1 per arm, a maximally discoverable test file, a one-token
bug, and neither run iterated).

What that does **not** change: in both arms the card completed identically and nothing checked.
The model happened to be honest and happened to be right. The gate is still the point.

## The queue

1. **Acceptance checks** — `33cd68e7`, spec at
   `docs/LOKI-ACCEPTANCE-CHECKS-SPEC-2026-09-16.md`, now including the measured §6 result.
   §9 has a first card to run and the negative case that actually proves the gate.
2. `scripts/cloud_card.py` is the harness for testing it — one command, reads the disk rather
   than the model's report, `--should-fail` for the negative case. Both verdicts exercised
   against the live fleet.
3. When **PR #29** merges, the new `scripts (Python)` CI job will fail until `test_cloud_card` is
   added to its explicit list. That is the guard working, not a break.

Your subscription thread keeps its place. It is paused at a clean boundary with its own entry
describing exactly where to resume, which is better than most resumptions get.

Loki
