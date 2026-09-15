# Vault release performance and indexing contention

2026-09-14 — Sif your friendly Codex Agent

**Finding:** release builds improve scan speed, but full index replacement still blocks searches
noticeably near the corpus ceiling. All sampled searches returned ten expected hits; the issue
measured here is latency, not failed or partial results. Recommend reducing unnecessary publication
before describing large vaults as smooth interactive use. No indexing optimization was made here.

## Same scale workload, optimized builds

Same 1,017-byte-per-note synthetic corpus as the prior debug benchmark, three scan/search runs per
size. File-backed SQLite in a temporary local directory. Medians:

| Notes | Mac release scan | Linux release scan | Mac release search | Linux release search |
|---:|---:|---:|---:|---:|
| 100 | 5.61 ms | 10.58 ms | 0.20 ms | 0.32 ms |
| 1,000 | 56.81 ms | 69.16 ms | 0.63 ms | 0.97 ms |
| 10,000 | 654.48 ms | 747.63 ms | 6.12 ms | 11.51 ms |

Previous debug 10,000-note scan medians were 1,738.68 ms (Mac) and 1,993.72 ms (Linux). These
separate runs suggest roughly 2.7x improvement; they are not controlled build-only comparisons of
identical machine load/cache state. The finalized measurements were run sequentially per machine,
after compilation and correctness tests. Preliminary overlapping runs were discarded/overwritten.

## Near-ceiling workload and concurrent reads

Generated 1,934 Markdown project notebooks, **66,062,240 bytes (98.44% of the 64 MiB limit)**.
Notes contain headings, varied topic names, numbered project observations, task lists and prose,
rather than artificial byte-padding chunks. This is structurally plausible generated content,
not a representative sample of real user vaults: repeated sentence patterns and vocabulary remain
an explicit limitation. No personal notes were read. About 34 KiB per note on average.

Three scans without a reader (initial indexing plus two complete replacements), followed by 30
baseline searches, then three scans with one concurrent search thread. The thread searches for
`database`, verifies ten results, and sleeps ten milliseconds *after* each response. Both writer and
reader share the same LocalHubStore instance, matching today's mutex-based runtime.

| Metric | Midgaard/macOS | Heimdall/Linux |
|---|---:|---:|
| Full scan median, without reader | 1,353.95 ms | 2,017.93 ms |
| Publication transaction median, without reader | 1,051.87 ms | 1,776.23 ms |
| Baseline search median | 7.57 ms | 11.07 ms |
| Worst observed search during scanning | **1,304.46 ms** | **2,049.66 ms** |
| Concurrent search samples | 45 | 36 |
| Failed/wrong-count samples | 0 | 0 |

Each concurrent run has its own raw publication and scan timing. The local database mutex blocks
reads during the publication transaction, even though scanning/hashing happen outside that lock.
The scanner also reads old rows in an earlier, separately locked transaction. The publication
measurement does not include that earlier transaction or waiting to acquire the mutex.

The new optional `VaultScanStatus.publication_transaction_ms` records elapsed time from entry into
the publication closure through transaction return, including commit and negligible unlock/return
overhead. This approximates the publication lock-hold window; it is not an OS mutex profiler and
excludes transaction-begin time. It is runtime-only, returns None from stored-status reads, and has
serde defaulting for older payloads. No DB migration or API permission change was needed.

The concurrent reader is a closed-loop sampler, so it does not model many queued users or support
production percentile claims. These are warm/local-storage debug-to-release comparisons, not
cold-cache, WAN, p95/p99 SLO, peak RSS, or actual USB-storage measurements. All scan/search raw arrays
are retained; medians must not hide the worst observed wait. No promise of zero future timeouts.

## Recommended next implementation

1. Detect an unchanged complete snapshot and skip document/FTS replacement while still validating
   availability and generation. Today even unchanged polling rewrites everything.
2. Publish only inserted/changed/deleted rows in one transaction; handle rename collisions and
   preserve document UUID/revision semantics. Keep incomplete-scan preservation, stale-generation
   rejection, and cancellation protection. Benchmark again using the same fixtures.
3. If those changes leave excessive pauses, evaluate separate read connections and WAL snapshot
   reads. This is a larger concurrency change: do not merely enable WAL while every operation still
   shares the same mutex, and do not open a second LocalHubStore that resets vault availability.

These are recommendations for Claude's next scoped handoff, not optimizations implemented or
performance guarantees. Avoid silently reducing the published 64 MiB limit to hide the pause.

## Verification, artifacts and cleanup

`cargo test -p hive-core --features local-hub --lib`: **58 passed** after the instrumentation change.
Both release benchmark binaries built and completed their assertions. No additional Linux regression
suite claimed this turn; the previous Linux 58-test result remains in its earlier report.
Existing unrelated `spawned_card_id` dead-code warning remains. `git diff --check` passed.

Harness: `crates/ohhive-core/examples/vault_validation.rs` (`scale` and `near-limit` modes).
Raw results/build logs/test log and recomputable summary:
`docs/vault-validation/2026-09-14/release/{mac-scale.jsonl,linux-scale.jsonl,mac-near-limit.json,linux-near-limit.json,summary.json,mac-build.log,linux-build.log,tests.log}`.

No test services were started for performance measurements. RAII fixtures removed temporary notes
and SQLite databases. Heimdall's `/tmp/hive-vault-release.PMYnE3N6` checkout/build was removed and
absence verified; the local transfer tarball was removed. Normal Cargo caches remain. Worker/coder,
Swift/FFI, desktop permissions and production services were untouched. No Git commit/deployment.
The optional real USB unplug/replug test was not attempted; Jack would need to provide/select a
spare drive before that separate physical test.

Claude: please review the measured stalls and choose the optimization scope above before further
performance work. Native pilot progress remains separate from this vault queue.

Sif your friendly Codex Agent
