# Hive code efficiency and improvement audit

Date: 2026-09-15. Author: Sif your friendly Codex Agent.

## Assessment and scope

Hive has useful foundations, but its next efficiency gains should come from bounding work, fixing task lifetimes, and avoiding repeated full-data retrieval. A rewrite is not warranted by this review. The highest-priority problems can interrupt active work or make a library unavailable; optimizing those matters more than shaving small amounts of CPU from ordinary requests.

Reviewed the current shared checkout of `OH Cloud-src`, HEAD `2c8fefe678c734e462497cf162beefb6651fe57d` **plus its uncommitted changes**, including my recent vault maintenance implementation. Followed execution through worker/coder, hub HTTP, local SQLite/vault curation and maintenance, regional live updates/storage/replication/backups, web refresh behavior, and CI configuration. Sampled the latest checked-in board SQL and cloud coding endpoint; this is not a complete security audit of every SQL migration or a native UI audit. Earlier private-visibility/payout and CLI/skills fixes recorded in continuity are not being reopened as unresolved findings.

Two new executable probes were run without contacting production: the actual TypeScript live hook reproduced a post-cleanup polling leak; the actual schema-6 archive table and quota SQL reproduced a full-table query plan in synthetic SQLite data. Other findings below are source-level conclusions or clearly identified scaling risks. No production profiling, provider calls, migration deployment, or application changes occurred. The previously recorded 160 core tests and CLI check are prior validation, not new tests run by this audit. Linux/Windows runtime behavior was not tested here.

Priority: **P1** = correctness/resource problem to address before expanding unattended use; **P2** = scaling or operability improvement for the next iteration. These priorities do not imply an observed production incident.

## Findings

### 1. P1 — Heartbeat awaits suspend dispatch progress

Evidence: `crates/ohhive-core/src/worker.rs:1122`, especially the `tokio::select!` branch at 1138. The dispatch future is polled alongside a heartbeat timer, but `send_heartbeat(...).await` happens **inside the selected branch**. During that await, this loop does not poll dispatch again. This is narrower than the comment's claim that neither activity can delay the other. The earlier improvement allows heartbeats during long cards, but a slow heartbeat can still freeze progress through an active card's Rust future. An already-spawned OS child may continue while its supervision is paused.

Recommendation: give heartbeat and dispatch separate long-lived futures polled concurrently (or supervised tasks), with coordinated cancellation and a bounded heartbeat deadline. Avoid spawning an unbounded series of heartbeat requests. Test a never-resolving mock heartbeat while a mock card advances, checkpoints, and stops; verify no heartbeat accumulation and prompt shutdown.

### 2. P1 — A reader open mutates availability for every vault

Evidence: `crates/ohhive-core/src/coder.rs:1006` (`open_vault_reader`); `local_hub/mod.rs:113–147`; `local_hub/vault.rs:45`. Each coding vault tool opens the store, which runs `UPDATE vaults SET state='unavailable'`. It then republishes only manual vaults. Folder-backed vaults remain excluded until their reconciliation path restores readiness. This is shared persisted state, not a harmless reader-local flag. Repeated agent reads can invalidate a folder vault even while its host is running, as well as repeat SQLite writes and credential setup. The code comment acknowledges fresh-open cost but understates the folder-availability side effect.

Recommendation: distinguish host startup/recovery from opening a reader. Let the host own source readiness, and reuse an authenticated session/store handle within a worker session, or call the owning LocalHub service. Continue checking grants and revisions per operation; caching must not cache authorization indefinitely. Test two connections: maintain a ready folder vault, issue repeated coder searches, and assert availability stays ready until the source actually becomes unavailable. Also resolve configured vault UUIDs, rejecting ambiguous names instead of silently choosing the first match.

### 3. P1 — File and directory response limits do not bound work

Evidence: `coder.rs:619` reads an entire file with `tokio::fs::read` before returning at most `READ_FILE_MAX_BYTES` (200 KB). It also returns the complete byte vector for the exact-read skill accounting hook. A multi-gigabyte input can therefore allocate/read gigabytes for a small response. `list_dir_tool` around 674 collects, inspects, sorts, and returns every entry. `run_git` around 516 captures command output without the bounded-drain approach used for normal commands.

Recommendation: bounded reads of limit + 1 bytes, offset/range support, regular-file validation, bounded directory pagination and bounded git output. Preserve the skill fix: mark usage only for the exact complete skill content actually shown, never a truncated read or a separately re-read file. Metadata may report size but must not substitute for enforcing the read bound. Test sparse large files, huge directories, binary inputs and noisy git failures; assert memory/output budgets and accurate truncation.

### 4. P1 — Lease and command cancellation stop at incomplete boundaries

Evidence: `coder.rs:1542–1579` checks lease expiry only at a turn boundary, then awaits a model and executes every returned tool call. A response arriving after expiry can still initiate multiple side-effecting tools. The comments explicitly describe this limitation. `run_command_tool` around 739 uses `kill_on_drop` and timeout `child.start_kill()`, without process-group/Windows job ownership. Killing a direct child does not establish cleanup of its descendants. Timeout also discards captured diagnostics.

Recommendation: propagate a session deadline/cancellation token through model calls and tool execution; check authority before each side effect and cap tool-batch length. Define finish-versus-interrupt behavior explicitly, including community reassignment fencing. Own the command process tree using platform-specific supervision, preserve capped diagnostics on timeout, and report incomplete cleanup honestly. Test expiry during a model response and during a multi-tool batch, plus a child that spawns a long-lived grandchild on all three supported platforms. This is cleanup and duplicate-work prevention within the accepted private-host execution model, not a claim that the current tool is sandboxed.

### 5. P2 — Model context grows with the complete transcript

Evidence: `coder.rs:1512–1580`: every tool result is appended to `messages`, and every turn sends that history again through `brain.next_turn`. A turn limit is not a byte/token budget. Several 200 KB file/command responses quickly dominate useful context; repeated serialization and retransmission increase latency, memory and provider usage. The exact cost depends on model, tokenization and provider caching and was not measured.

Recommendation: enforce per-result, per-turn and whole-session budgets; prefer search/excerpts and durable artifact references; compact older tool material while retaining decisions, current constraints and error evidence. Keep enough recent raw context to audit the next action. Record input/output tokens and useful-task completion before tuning. Test a many-turn noisy repository task against a fixed context budget without losing acceptance criteria.

### 6. P1 — Hub operations lack explicit local deadlines and response bounds

Evidence: `crates/ohhive-core/src/hub.rs:420–460`: client construction, `.send().await`, then full `.text().await`; adjacent Edge Function helper has the same full-response pattern. No explicit operation deadline or response-size budget appears in this path. Rejected/bad JSON errors also include the response body. This amplifies finding 1 and can stall claims, events, status, or shutdown under a slow server.

Recommendation: centralized client policy with connection deadlines, operation-specific total deadlines, bounded streaming response collection, and bounded/redacted error excerpts. Model generation needs a different allowance from heartbeats. Retry only operations whose request IDs or semantics make retry safe; do not automatically retry side effects. Test stalled headers, stalled bodies, oversized errors and ambiguous submission failures. Credit: the ordinary HubClient already reuses a client, so retain connection reuse.

### 7. P2 — Local SQLite serializes reads as write-intent transactions

Evidence: `local_hub/mod.rs:154`: one `Arc<Mutex<Connection>>`, with `TransactionBehavior::Immediate` for the shared transaction helper, including read APIs. Within a handle, queries serialize; separate handles still contend for SQLite write reservations. Reader-open mutations from finding 2 compound this. This is a reasonable small-store starting point, but long maintenance operations can delay interactive search and lease work.

Recommendation: measure lock-wait and transaction duration first. Split read-only operations from writes, shorten write transactions, and consider a bounded read connection strategy after establishing migration/lifecycle ownership. Evaluate WAL only for a supported local-disk database; do not put the live database into a cloud-synced folder or assume WAL eliminates writer serialization. Test concurrent search, imports, archive writes and maintenance with p95/p99 foreground latency and correctness assertions.

### 8. P2 — Archive maintenance repeatedly scans payloads under that lock

Evidence: `local_hub/vault_maintenance.rs:81–150`; schema 6 in `vault_maintenance_schema.sql`; `vault_curation.rs:249`. Quota accounting sums snapshot payload lengths repeatedly, including before/after possible expiration and another archive cap check. The archive table has a document primary key but no vault-first index. Retention collects up to 1,000 snapshot/current-content pairs and serializes comparisons within the transaction. This can approach substantial fractions of the 64 MiB per-vault snapshot cap, in addition to current content and allocations. The cap prevents unlimited snapshot payload growth, but does not make these scans cheap.

**New evidence:** `efficiency-audit/2026-09-15/archive-plans.json` records `SCAN vault_archives` for the current quota SQL over 10,000 synthetic rows/100 vaults. A candidate `(vault_id, archived_ms, document_id)` index changes this to a vault-filtered index search, with the same 51,200-byte sum. This is a query-plan result, not a measured speedup. Inventory's document cursor may benefit from a different `(vault_id, document_id)` index; choose using actual workload plans.

Recommendation: transactional stored snapshot byte lengths and reconciled per-vault totals; measured indexes; smaller bounded retention batches. Where comparisons move outside a write transaction, recheck revision/content identity before deleting a payload. Preserve protected-snapshot and rollback semantics. Add latency/allocation tests near quota across many vaults, plus counter-rebuild and concurrent-update tests. This finding includes my newly built maintenance code and should be addressed before large-library rollout.

### 9. P2 — Bounded duplicate scans can revisit the same incomplete prefix forever

Evidence: `vault_curation.rs:143–224`: source rows ordered by document ID, complete normalization/hashing every run, then nested near-match comparisons capped at 100,000 pairs or 1,000 findings. The incomplete flag is correct. However, no continuation cursor advances subsequent runs past the same ordered prefix. A duplicate pair outside that prefix can remain unexamined indefinitely in an unchanged corpus. For perspective, 10,000 eligible notes have about 50 million possible pairs; that is a combinatorial calculation, not measured runtime.

Recommendation: cache fingerprints/token signatures by document revision plus algorithm version; use candidate buckets to avoid all-pairs comparisons; persist coverage/continuation and invalidate it deliberately when inputs change. Keep duplicate suggestions reviewable and disclose skipped long notes. Test a planted late-order duplicate across multiple runs and require eventual coverage, bounded memory, and no automatic deletion. Optimize repeated whitespace passes only after fixing coverage and incremental work.

### 10. P1 — Web live polling leaks after cleanup; quiet fallback is incomplete

Evidence: `apps/web/lib/live.ts:37–69`. If cleanup happens while the initial poll/server/session awaits, the `closed` condition calls `startPolling()`, which does not check `closed`; an interval is installed after cleanup has already cleared timers. Socket error scheduling has a similar missing guard. Quiet detection starts only after a message and runs a one-shot poll, rather than enabling the documented recurring fallback; a socket that opens but never sends a frame starts no quiet timer. Interval polls can overlap.

**New reproduction:** `efficiency-audit/2026-09-15/live-cleanup.cjs` transpiles and executes the actual hook with mocked React/auth/timers. Cleanup during the initial poll leaves **one live interval and one extra poll**. The saved JSON records this. This is not a browser rendering test and deliberately asserts current buggy behavior; change it into a no-leak regression assertion when fixing the hook.

Recommendation: effect-generation/closed guards before all callbacks and scheduling, cancellation of outstanding requests, single-flight polling with recursive timeout, quiet timeout initialized on open, and bounded reconnect with jitter. Test unmount/project switches at each await, zero-frame sockets, stale callbacks and slow responses.

### 11. P2 — Live updates suppress transmissions after paying the full query cost

Evidence: `crates/hive-server/src/live.rs:25,93–110`: each active room fetches the full board every two seconds, serializes and hashes it, then suppresses unchanged frames. Latest checked-in `supabase/migrations/20260913231336_fix_private_visibility_and_payout_cap.sql:80` builds all cards with inputs, acceptance and latest output content, with no page bound. Sharing a poller per room is good, but hashing afterward does not reduce hub query or hub-to-regional traffic.

The configured cadence models **30 full-board queries/minute per active room per regional server** when calls finish on time; 100 rooms would model 3,000/minute. This is arithmetic, not observed load. `apps/web/app/fleet/page.tsx:65` also refreshes every five seconds; desktop polling and events can trigger duplicate refreshes.

Recommendation: lightweight project revision/change cursor, incremental card summaries and on-demand outputs, visibility-aware single-flight clients, and adaptive fallback cadence. Preserve authorization on both revision and payload endpoints. Obtain PostgreSQL `EXPLAIN (ANALYZE, BUFFERS)` on synthetic/staging large boards before choosing indexes or rewriting correlated lookups. Do not infer production query plans from SQL text alone.

### 12. P1 — Blob publication uses a shared temporary pathname

Evidence: `crates/hive-server/src/store.rs:34–49`: same hash maps to the same `.part` path; existence check and publication are not serialized. Two writers can both pass the check. One can truncate the shared temporary file while another publishes it, exposing incomplete data or returning rename failures. Content addressing alone does not make concurrent publication atomic. This is a source-derived interleaving, not a reproduced production corruption.

Recommendation: unique exclusive temporary files, completed hash/length verification, and an atomic publish-if-absent strategy with platform-tested existing-file behavior. Keep MIME metadata publication coherent; document durability guarantees. Add a barrier-controlled concurrent same-hash write/read test, different-hash concurrency and interrupted-write recovery. Do not merely ignore every rename error.

### 13. P2 — Regional housekeeping scales with full disk walks and whole-buffer transfers

Evidence: `store.rs:94–121` enumerates/stat-checks all blobs; `hive-server/src/lib.rs:228–232` invokes both `used_bytes()` and `count()` each heartbeat (two inventories), and health also calls both at 426. `replicate.rs:54` buffers each entire blob before hash/store; `backup.rs:79–93` retains exported JSON, serialized plaintext, compressed data and encrypted output during one async operation. Compression, encryption and synchronous store I/O are performed directly there. `snapshot.rs:79` constructs a fresh HTTP client for each follower refresh and does not send a conditional ETag request in that path.

Recommendation: one inventory pass immediately, then maintained count/byte accounting with periodic reconciliation; bounded streamed replication with expected length/hash and disk quota reservations; move blocking work off async runtime threads. Design streaming encrypted backups together with a consistent export/restore format—not naive independent table pagination. Reuse snapshot clients and negotiate unchanged snapshots before downloading full bodies. Test on Pi-class storage, large artifacts and restore drills. Peak memory and disk pressure need measurement; no capacity claim is made here.

### 14. P2 — Verification should include lifecycle, feature and performance contracts

Evidence: `.github/workflows/ci.yml` already has three-OS Rust test/clippy/format jobs, ARM64 server cross-build, migration guards and web build. Those are valuable. The shown workflow does not run browser lifecycle tests or explicit `desktop-provider` tests; default workspace feature unification is not a clear feature-coverage contract. The recently recorded full feature test command goes beyond that explicit CI configuration. Dependency denial remains allowed to fail.

Recommendation: retain the lean no-inference server build, add explicit supported feature combinations (not indiscriminate all-features), hook lifecycle regressions, process-tree/lease tests, and representative vault/board load fixtures. Record lock wait, query/response bytes, tool-result bytes, model tokens, maintenance duration/coverage and foreground latency. Keep credentials and note contents out of metrics. Establish baselines before setting regression thresholds; review dependency-check output before making it required. Cross-compiling is not evidence of runtime behavior on ARM or of native desktop behavior on Windows/Linux.

## Delivery order for Claude's review

1. **Worker progress and safe stopping:** findings 1, 4 and 6. Demonstrate that a stalled heartbeat cannot stall a job and expired authority cannot start another tool.
2. **Vault availability and reads:** findings 2 and 3. Fix lifecycle ownership and enforce actual I/O bounds, preserving grant/revision and exact-skill-read guarantees.
3. **Two isolated correctness patches:** web cleanup/fallback (10), blob publication (12). Both merit deterministic regressions.
4. **Library scale:** findings 7–9. Baseline contention, add measured indexes/accounting, then incremental curation with eventual coverage.
5. **Fleet cost and operability:** findings 5, 11, 13 and 14. Budget model context, reduce unchanged full snapshots/boards, stream transfers and enforce verified contracts in CI.

These are reviewable work packages, not automatically assigned implementation or deployment authorization. Prefer small changes with before/after evidence to a combined refactor. Source references reflect this dirty checkout and may move during Claude's consolidation.

## Existing strengths to preserve

The Hub abstraction and local/cloud routing boundaries support targeted fixes. SQLite revision/grant checks, cursor-oriented vault access, FTS search, and non-destructive archive overlays are useful foundations. Maintenance is explicitly opt-in, bounded, token-fenced and honest about incomplete findings. Command output is already drained with a cap in the normal command path. Regional live rooms share polling across subscribers. Blob replication verifies hashes. CI already spans three operating systems and protects the regional server's small dependency footprint. The objective is to complete these contracts consistently across paths, not replace them wholesale.

## Evidence and reproduction

Run from the Hive repository root:

```sh
node docs/efficiency-audit/2026-09-15/live-cleanup.cjs
python3 docs/efficiency-audit/2026-09-15/archive-plans.py
```

The first needs the already-installed TypeScript dependency. The second uses Python's SQLite, version recorded in its output; production Rust uses bundled SQLite, so validate final index changes against that runtime too. Neither probe contacts production or edits the product database. Saved results are next to each script. No new full-suite run was necessary for this documentation-only audit; implementation follow-ups must run their own focused regressions and relevant existing suites.

Sif your friendly Codex Agent
