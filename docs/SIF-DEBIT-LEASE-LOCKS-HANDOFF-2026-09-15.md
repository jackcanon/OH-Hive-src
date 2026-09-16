# Audit S-6: serialize ledger debits and lease mutations

Forward migration `20260915180000_debit_and_lease_locks.sql`. Implemented and tested against a focused PostgreSQL fixture; no production deployment or multi-session contention test performed.

## Changes

`split_debit` now locks the debit account with `FOR NO KEY UPDATE` before reading source balances. It must be VOLATILE rather than STABLE: a STABLE function cannot perform this locking operation and cannot promise fresh balance reads. Every existing funding/provider/storage path using the helper participates without duplicating changes in each caller. Locks remain until transaction end, not helper return.

`post_txn` also locks debit accounts, in UUID order, so callers supplying raw or previously assembled debit entries cannot bypass serialization. After inserting balanced entries, it checks earned/grant/purchased source balances for debited member-wallet/project-fund accounts. A negative source bucket raises and rolls the whole posting back, including credits. This catches stale plans and prevents a positive aggregate balance from concealing an overdrawn source bucket. Existing 0.0000005 rounding tolerance is retained.

Account rows use NO KEY UPDATE rather than UPDATE because incoming credit ledger rows acquire foreign-key KEY SHARE locks. NO KEY UPDATE permits those harmless credits while serializing competing debits; blocking credits would introduce funding/completion lock cycles. Multi-account raw postings acquire debit locks in sorted order. Callers assembling several split_debit calls in one transaction must acquire all debit accounts in that same order to avoid deadlocks.

`fund_project` locks its wallet before its preliminary balance check. Both direct and delegated pilot `node_complete_card` lock the owned lease before creating outputs/settling, and lock the project fund before reading/clamping its balance. Both checkpoint variants lock the owned lease before writing checkpoint state, and their lease extension also includes node_id in its predicate.

Release already starts with an owned-lease DELETE, which takes the row lock and returns no_lease when no row was deleted. It therefore serializes with the new completion/checkpoint locks without a redundant preceding SELECT. Direct failure gained the matching ownership guard in S-1; this migration also applies that guard to delegated pilot failure, which had retained the old unchecked delete behavior.

The shared ledger helpers permit READ COMMITTED (the RPC default) or SERIALIZABLE, but reject REPEATABLE READ. Waiting on a row lock alone cannot refresh a fixed Repeatable Read snapshot of append-only ledger entries. Serializable callers must handle normal serialization failures by retrying the whole transaction; no internal partial retry is added.

## Preserved scope

Existing payout calculation, provider source/budget policy, treasury exceptions, notifications and anonymous contribution attribution are retained. Completion's existing ledger-error handler still records completed work with zero payout if posting raises, instead of orphaning a running lease; this task does not replace that business rule. Treasury/provider/storage-pool negative balances are not newly prohibited. This is not the separate S-8 treasury/payout-policy repair, nor a repair for previously overwritten local-mode/no-ledger contracts. Existing account_sources/archive semantics remain unchanged.

CREATE OR REPLACE preserves existing grants. No new public wrapper, privilege grant, table, or production data mutation is introduced. Locking provides transaction safety, not lease-generation fencing for a stale worker using the same node key after reassignment; that remains a separate protocol concern.

## Verification

`scripts/test-debit-lease-locks.mjs` runs the actual new function bodies with real PostgreSQL/PGlite and pgcrypto, using fixture tables and identity helpers. Checks successful/insufficient funding, stale prebuilt debit rollback, per-source overdraft rejection despite a positive total, direct/delegated completion, duplicate completion, checkpoint-after-completion rejection, foreign delegated failure refusal, release-before-completion refusal, function volatility/lock clauses and Repeatable Read rejection. Static migration guards and diff checks pass.

**PGlite queues requests; it cannot prove simultaneous lock contention.** No native PostgreSQL binary or running Docker daemon was available here. Before rollout run the following against a disposable PostgreSQL database with separate sessions:

1. Give a wallet 100 units. In two transactions concurrently attempt funding 80. Hold the first transaction before commit. Second must wait; after first commits it must fail for insufficient funds. Final debit must be 80, not 160.
2. Repeat with two different completed cards sharing a small project fund. Aggregate payout must not exceed its spendable source balance.
3. Concurrently complete the same leased card. Exactly one output/notification/payout succeeds; the waiter gets no_lease (or a serialization failure under SERIALIZABLE).
4. Race checkpoint, release, failure, reaper and completion on one lease. No checkpoint should succeed against a deleted lease, and a losing completion must create no output/payout.
5. Run funding and worker completion in opposite directions between wallet/fund to verify incoming-credit FK locks do not create a lock cycle.
6. Under SERIALIZABLE, retry an entire aborted transaction; verify no duplicate ledger rows. Under REPEATABLE READ, verify the explicit rejection.

Review live function definitions before applying: the migration is based on the latest checked-in direct and delegated bodies. Full schema replay, production-only drift and real concurrent transactions remain rollout validation. Next queue item: S-7 private snapshot filtering and owned-URL validation.
