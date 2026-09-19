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
