# Backup scheduling recovery — 2026-09-19

The general coordinator may be a volunteer, but backup export correctly requires an online HJM server. Requiring both on the same machine stopped backups after Vanaheim won the lease.

Configured HJM servers now run their existing daily backup independently of coordinator status. Volunteer servers remain excluded locally and by the deployed database permission gate. Recipient configuration, encryption, replication and per-server daily state remain unchanged. Ledger archival still uses its existing coordinator gate; it is outside this fix.

Multiple configured HJM servers can produce multiple encrypted snapshots each day. Current retention keeps fourteen snapshots, not fourteen days: do not describe that as a two-week recovery window. Roll out first to one eligible HJM server; review retention before broad rollout. The existing daily marker is written only after the hub records the backup. Existing marker write errors are not fatal, so a disk-write failure can still cause repeated exports; this patch does not claim exactly-once execution.

## Verification and rollout

1. Run `cargo test -p hive-server` and the repository CI gates. Regression coverage includes non-coordinator eligibility, volunteer rejection, recipient validation and persisted daily suppression across restart.
2. On one configured HJM server, record current binary identity/service configuration, preserve a rollback binary, install a release built for that host and restart its service. Do not change the coordinator or relax export authorization.
3. Verify a new pinned backup timestamp, ciphertext hash and replication. A metadata record alone is not a restore test.
4. Verify restore into an isolated disposable database using the owner's existing recovery key; never apply restore SQL to production as a test.

## Freshness probe

`python3 scripts/check-backup-age.py --project-ref pxfbnuxcnerulbvbmowz`

Uses an existing authenticated Supabase CLI session and reads only the latest pinned backup timestamp. Exit 0 means age <=36 hours; exit 2 means stale, missing, invalid or unavailable status. Query failure cannot yield a healthy result. No backup contents or key material are read. Authentication must be provisioned appropriately for the monitoring host; do not put credentials in command lines or repository files.

Connect this probe to an independently hosted scheduler/monitor with failure notifications. The script alone is NOT scheduled monitoring and does not send notifications. A separate monitor is necessary to detect when all backup servers stop. No monitor, server deployment, coordinator change, database migration or restore was performed by this code change.

## Restore drill findings (September 19)

The new backup decrypts with the existing local recovery key. A disposable PGlite PostgreSQL instance replayed the repository migrations and restored all 57 Hive tables (54,218 rows) after the corrections below. PGlite supplies test stand-ins for Supabase services; it is not a complete Supabase recovery exercise.

- Preserve explicit identity IDs with `OVERRIDING SYSTEM VALUE`, then advance identity/serial sequences beyond restored IDs.
- Suspend user triggers during row replay so audit/presence/ledger side effects are not regenerated; keep internal foreign-key checks enabled. Restore each user trigger's previous enablement mode afterward. Always execute the generated SQL as one transaction.
- Fresh migrations seed accounts with random IDs. For full recovery use an empty Hive data set in an isolated destination; merely applying `ON CONFLICT DO NOTHING` over migration seeds can skip the backup's account IDs and break ledger references. The test harness clears only its disposable in-memory tables. The production restore script never truncates automatically.
- **This backup contains only the Hive schema. It does not include Supabase Auth accounts or public profiles.** Those records must be recovered separately before the Hive rows can restore with foreign keys intact. The test's explicit `--auth-placeholders` option supplies ID-only synthetic prerequisites; it cannot recover real profiles, login identities, sessions, or passwords. Never use that test shortcut as production recovery.
- The protected restore directory contains sensitive plaintext and SQL; keep it private, remove it when no longer needed, and never commit it. Existing output directories are rejected rather than overwritten.

Run the local drill with `npm ci --prefix scripts/migration-replay --ignore-scripts`, then `node scripts/test-backup-restore.mjs /private/path/backup.json --auth-placeholders`. The harness never connects to production and does not print row contents. Without the flag it intentionally exposes missing external account dependencies. It verifies a second replay and future sequence values, as well as exact restored rows. A full Supabase restore and offline recovery-key custody remain outstanding.

## Existing platform recovery coverage verified September 19

Read-only `supabase backups list --project-ref pxfbnuxcnerulbvbmowz` returned eight completed physical backups, September 12–19, with the latest at **2026-09-19 14:34:23.252 UTC** (07:34 Phoenix). WAL-G is enabled; PITR is not enabled. This is observed inventory, not proof that a restore has been exercised.

Supabase's [restore-to-new-project documentation](https://supabase.com/docs/guides/platform/clone-project) says physical restores include all database schemas/data and Auth user data. Therefore the missing Auth/public-profile records in the portable Hive export have a documented platform recovery source; do not describe all account data as unbacked-up. The latest platform snapshot precedes the recovered Hive export by about 49 minutes: restore a consistent platform snapshot first, rather than blindly combining mismatched snapshots.

A hosted clone was not started. It creates a billable project and copies scheduled jobs/extensions that may begin external operations immediately. A future drill needs a reviewed isolation plan and explicit project/cost approval. Database recovery also does not restore Storage object bytes, Edge Functions, or all Auth/service configuration. The existing offline key-copy question and end-to-end hosted recovery test remain open. No account contents or backup download links were retrieved during this inventory check.
