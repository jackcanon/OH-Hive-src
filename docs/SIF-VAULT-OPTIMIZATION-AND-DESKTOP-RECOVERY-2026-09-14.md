# Vault optimization and simulated desktop recovery

2026-09-14 — Sif your friendly Codex Agent

Completed the two approved work packages without changing worker/coder, Swift/FFI, native input,
or the database connection architecture. No commit or deployment performed.

## Vault implementation and verification

First implemented complete-snapshot equality using stored document revisions, skipping document/FTS
writes when unchanged while still validating generation and updating availability/scan metadata.
Benchmarked this stage on the Mac before proceeding: near-limit median publication 1.48 ms,
worst concurrent search 10.68 ms, versus the earlier 1,051.87 ms / 1,304.46 ms baseline.

Then generalized publication to preserve unchanged rows and replace only inserted/changed/deleted
identities. Compute the change set outside the lock. Inside one transaction, check generation,
delete changed/removed rows before inserting replacements (avoids transient path uniqueness
collisions), then publish source metadata/availability. FTS triggers see only affected rows.
No separate connections, WAL change, extra store handle or schema migration.

Retained: two matching complete scans, shared path validation, failed-scan preservation, generation
invalidation, watcher cancellation, existing conservative rename identity, and revision-checked reads.
The initial old-row read still loads content under the shared mutex; this has not become a lock-free
reader. Same-path content swaps retain the existing same-path identity policy.

New regression tests install mutation-audit triggers to prove unchanged scans do not rewrite notes,
and that a mixed edit/delete/rename/add preserves an untouched row and revision while updating FTS
correctly. Unchanged scans also restore availability and advance generation; stale publication is
still rejected. Existing missing-source, restart, cancellation and boundary tests pass.

## Measured result

Same release harness and generated near-limit corpus: 1,934 notes / 66,062,240 base bytes. Added a
single-note-edit mode that appends a short update to one note before each scan; the base corpus size
reported excludes these few appended bytes. Three scan trials each, one concurrent searcher with a
10 ms delay after each response. No errors or wrong result counts in any concurrent trial.

| Near-limit workload | Mac publication median | Linux publication median | Mac worst search | Linux worst search |
|---|---:|---:|---:|---:|
| Earlier full replacement | 1,051.87 ms | 1,776.23 ms | 1,304.46 ms | 2,049.66 ms |
| Final unchanged snapshot | 0.76 ms | 6.96 ms | 10.16 ms | 27.76 ms |
| Final single-note edit | 3.13 ms | 6.41 ms | 21.84 ms | 27.04 ms |

Publication medians come from initial-plus-two-rescan measurements without the concurrent reader;
worst search comes from separate concurrent trials. First initial indexing still must insert the
whole corpus. These improvements concern polling and small changes, not a claim that first import
or changing every note has become cheap. Synthetic repeated prose, warm caches, few samples and
closed-loop readers remain limitations; these are not production p95/p99 guarantees.

Final 100 / 1,000 / 10,000-note scale scan medians: Mac 5.32 / 37.26 / 433.84 ms;
Linux 12.41 / 32.66 / 305.98 ms. Original release baseline: Mac 5.61 / 56.81 / 654.48;
Linux 10.58 / 69.16 / 747.63 ms. Not every small timing improves across separately loaded runs.
At near-limit, full-scan medians are now about 275 ms Mac / 246 ms Linux for unchanged notes.

Stage-one benchmark was Mac-only; final stage-two release benchmarks and regression checks ran on
both Mac and Heimdall. Source and fixture temporary directories were removed afterward. Raw data:
`docs/vault-validation/2026-09-14/optimized/stage1/`, `stage2/`, and `optimized/summary.json`.

**Recommendation:** keep the single-connection architecture for now. The dominant idle/small-edit
stall is removed in these measurements. Revisit separate readers/WAL only if realistic workloads
show material remaining waits; that architectural change remains unapproved.

## Desktop Rust-only recovery slice

Added `desktop/journal.rs` and extended `FakeDesktop`:

- Versioned serializable journal snapshots with session, action/call identity, sequence, observation,
  policy revision, target identity, action kind, receipt state and dispatch time. No typed content,
  screenshots or secret values are copied into receipts.
- Simulated dispatch first records an uncertain receipt, then marks it applied after the simulated
  effect. `mark_uncertain` supports lost-ack fault injection. The snapshot can be serialized and
  reconstructed without replaying any stored action.
- Recovery validates version, bounded receipt count, sequence continuity, duplicate call IDs,
  session consistency and absence of actions after unresolved dispatch. Recovered sessions pause
  for local review even when the last receipt was successful.
- Human interruption blocks subsequent execution; every new action still evaluates current grant,
  lease/authority expiry, policy, target and observation. There is no cached blanket batch approval.
- Resolving an uncertain result requires a current authorized observation newer than the last receipt
  and an explicit trusted local finding (effect observed or not observed). The old call remains
  consumed in either case. Further work needs a new action ID/sequence; no automatic retry of the
  uncertain action occurs. Known-effect counts are reconstructed from reviewed receipt state.

Tests cover mid-sequence grant expiry, interruption/resumption, serialization without typed text,
restart, missing/stale review observations, explicit uncertainty resolution, replay rejection and
malformed journal order. Existing focus/process-identity and consent-binding tests remain green.

This is the approved **simulation/contracts scope**, not a durable native journal or authenticated
consent UI. No CGEvent, AX action, screen capture or provider call was added. Public Rust review APIs
assume a trusted local caller and must not be exposed as model tools. Journal snapshots are trusted
local persistence input, not evidence signed by a human.

**Native-side questions for Claude:** choose the durable write-ahead storage/fsync boundary and its
failure policy; bind actual reviewer identity/time and policy changes to a persisted review record;
ensure native grant revocation bumps policy revision and invalidates queued approvals; establish how
physical interruption and focus changes invalidate observations. Current tests simulate these inputs;
they do not resolve TCC/XPC/native event timing or guarantee actual exactly-once effects.

## Verification summary

- Incremental vault suite: 60 passed before the journal additions.
- Combined core suite on Mac: **62 passed**.
- Combined release core suite on Linux: **62 passed**.
- After the final receipt-action metadata and reviewed-effect counter refinement: all **13 desktop
  tests passed again on Mac**. That small refinement was not re-run on Linux; the Linux 62-test run
  preceded it. Vault benchmark code/behavior was unchanged by that refinement.
- Final benchmark examples compiled and completed on both platforms. A harness parameter compile
  error was corrected before final measurements; no failed build result is presented as a benchmark.
- `git diff --check` passed. Existing unrelated worker dead-code warning remains.

Claude: please review both packages. Vault optimizations are built and measured; simulated desktop
recovery is ready for your shared-executor/native-contract integration review. No native pilot gate
is implicitly waived by these simulator tests.

Sif your friendly Codex Agent
