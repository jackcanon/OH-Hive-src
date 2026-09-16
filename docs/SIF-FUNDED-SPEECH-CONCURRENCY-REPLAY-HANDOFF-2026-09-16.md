# Funded speech, SQLite contention and migration replay

## 1. Funded speech — built locally

`20260916050200_speech_reservations.sql` adds source-aware holds per speech card. Claiming a lease locks the project account and reserves the approved maximum across earned/grant/purchased balances. `split_debit` excludes held funds; `post_txn` also enforces the holds for callers that construct their own debit entries. Ordinary work can spend only the unreserved balance.

Both direct and delegated completion lock the fund, validate the full hold, release their own hold within the settlement transaction, charge actual approved-rate usage and credit the worker. Any later failure rolls everything back, including the hold, output and lease. Successful completion releases the unused budget. A lease deletion trigger releases holds on explicit release, cancellation, deletion and expired-lease reaping. Clock expiry alone does not release funds. No rate is seeded and nothing is deployed. Requeue any pre-existing unreserved speech leases before rollout.

Tests cover competing claims that individually fit the balance but collectively do not; raw ledger and split-debit attempts to consume held funds; successful unrelated spending; source splitting; direct/delegated payouts; unused-budget return; injected ledger failure rollback; cancellation; expired-lease cleanup; missing holds and unauthorized mutation. These are executed PostgreSQL semantics under serialized PGlite, not independent PostgreSQL-session contention proof. Preserve the existing real-PostgreSQL deployment acceptance gate.

Fund-as-you-go chunking remains unimplemented. This implements fully funded admitted jobs; it must not be presented as unlimited processing with a payment check afterward. Pricing/cap propagation/submission limitations remain in the speech pricing handoff.

## 2. SQLite contention — built on Claude's fix

Claude committed `325d0d3` while this queue was being checked: timeout 250ms → 5s. Preserve that fix. Further inspection found initialization still used a deferred transaction and read the migration version before acquiring the writer lock. Two concurrent first opens could decide to apply the same migrations.

`LocalHubStore::from_connection` now takes an immediate writer transaction before the authoritative schema-version read. Foreign-key mode is selected outside the transaction as SQLite requires; legacy initialization is checked before commit. Modern opens retain enforced foreign keys without a new whole-database integrity scan. Added regressions holding a competing writer for 600ms (beyond the old timeout) and racing four first opens, in addition to the existing duplicate-room and exactly-one-claim tests.

All 88 local-hub tests pass on this Mac, including localhost transport tests; all-features core library clippy passes with warnings denied. This is not a new Linux/Windows CI result. Existing repository-wide formatting issues are not addressed by a broad reformat in this shared checkout.

## 3. Fresh migration replay — passing; live parity still gated

All **91** repository migrations replay from an empty PGlite database. Added pinned test-only PGlite dependency/lockfile and a CI step that runs replay, speech pricing/reservations and Private Fleet RLS tests. `platform.sql` represents only external Supabase interfaces (auth/profiles, storage, Vault and cron); every Hive definition comes from the actual migrations. The single pg_cron extension declaration is omitted by the harness because PGlite has no scheduler. Cron delivery, Storage and Vault service behavior are not exercised. Final RLS and publication guards pass.

Repairs:

- Added the `node_member_id` prerequisite before its first SQL wrapper, preserving existing migration IDs.
- Recovered missing notification subscription/delivery table columns, constraints, indexes and access metadata from read-only production catalog queries. No rows or function bodies exported. Added the baseline definitions before the historical grant migration. Preserved active RLS and lack of authenticated row policies; the existing service-role delivery grant remains.
- Moved the still-pending Private Fleet table definition into `hive`, updated both enrollment clients and the owner-isolation regression test. Static checks now reject public table creation. Deploy this together with its clients; do not leave a previously installed public table orphaned if deployment state differs.
- Production migration-version metadata confirms both supposedly proposed files were applied: `control_pilot_delegation` is `20260914030000`; card submission is `20260915144511`. Corrected stale headers instead of removing required migrations.
- Added `SIF-MIGRATION-VERSION-RECONCILIATION-2026-09-16.md`. Matching names are only candidates; compare bodies before repairing production history. No repair, push, migration application or deployment was performed.

**Update after explicit approval:** Jack approved the read-only schema-definition export. Recovered the missing live objects and compared all 326 live Hive functions. Replay now passes 93 migrations, with 1,070 catalog objects represented and 15 documented intentional function-body differences. See `SIF-LIVE-SCHEMA-RECOVERY-2026-09-16.md` for the full result. The three previously missing app RPCs and their dependencies are recovered and runtime tested. Production migration ID repair and deployment remain separate, unapplied rollout steps.

Verification: all 91 migrations; all speech accounting regressions; Private Fleet owner/anonymous/write-denial tests; 88 local-hub tests; core clippy; web TypeScript check; static migration guards (50 tables); whitespace check. No production writes, commits or pushes.

Sif your friendly Codex Agent
