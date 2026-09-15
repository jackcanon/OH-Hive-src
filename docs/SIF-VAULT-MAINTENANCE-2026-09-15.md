# Vault maintenance phase 2

September 15, 2026 UTC · Sif your friendly Codex Agent

Implemented `local_hub/vault_maintenance.rs` and schema 6. Existing schema 5 archives migrate without losing snapshots. This is a portable host-owned Rust service future, not an installed background service; no FFI/UI/coder/worker edits.

## Host API

- `vault_configure_maintenance(vault, policy)`: persists policy, makes it due, invalidates an in-flight publication token. Default disabled; suggested enabled cadence daily, stale threshold 90 days. Interval bounds 60 seconds–30 days; age bounds 1–3650 days.
- `vault_maintenance_run(stop)`: checks every 30 seconds, one due vault per tick; blocking scans are offloaded. Host must spawn/retain this future and signal/await it before closing the store. A running scan completes before graceful stop. Dropping the future is not cancellation of an already executing blocking scan.
- `vault_maintenance_tick()`: one due run for hosts with their own timer. Claims and next-due state are durable; one-hour abandoned-claim expiry permits restart recovery. Token checks reject a stale runner's publication/retention after reconfiguration or takeover. Missed intervals coalesce into one run, not catch-up storms. Wall-clock changes affect due times.
- `vault_maintenance_status`: persistent policy, due time, running flag and latest bounded report. Reports contain counts plus up to 1,000 stale IDs and duplicate candidates, incompleteness flags, retained bytes/over-quota state and outcome. Findings are observations requiring a fresh revision review before curation.

Unavailable vaults and failed scans retry after one minute; successful scans advance by the policy interval. No automatic archive, note edit, merge or model invocation. A database/task-level error returns from the runner for the host supervisor to surface/restart; failures are not silently declared success. No claim of continuous service until a host starts it. Multi-process work may overlap after a one-hour lease timeout, but only the current token publishes results/executes retention.

## Snapshot retention and quotas

`redundant_snapshot_days=None` by default: preserve every snapshot. Setting an age explicitly opts into removal of old **redundant snapshot copies only**. A snapshot qualifies only when a current document exists in the same vault with the same revision and exact serialized contents. Changed/missing sources and unique historical versions remain protected. Ready source state is required. At most 1,000 eligible copies are considered per pass; later passes continue.

Expiration sets snapshot payload to NULL, keeping the archive identity/visibility overlay and a durable `expire_snapshot` receipt. It does not unarchive a note, remove the indexed document, touch source files or prune audit history. Inventory explicitly reports `snapshot_retained`. Restore uses the reviewed current indexed revision; if a snapshot has expired and the source later disappears, archive restore cannot recreate it. This is a real opt-in retention tradeoff, not a promise that all historical content remains recoverable. External backups are separate.

Per-vault logical snapshot quota is configurable from 1 KiB to the existing 64 MiB ceiling. New archive writes enforce it transactionally. Under quota pressure, enabled retention may expire eligible aged redundant copies; if that is insufficient, the write fails and rolls back cleanup as well. Existing over-quota data is retained/protected and reported, never silently deleted. Disabling scheduled scans does not disable an explicitly selected retention rule during a quota-constrained archive write; set retention to None to disable both.

This controls serialized archive payload bytes, not total filesystem/SQLite page usage. SQLite may reuse freed pages without shrinking the file. Global disk quotas, audit retention, automatic VACUUM and unique-snapshot eviction were not added. Existing history remains intact. Schema 6 requires a pre-upgrade backup for downgrade to older binaries.

## Verification and related deliverables

Full core suite: **160 passed, zero failures** on macOS, including eight new maintenance tests. Command: `cargo test -p hive-core --features 'local-hub,sandbox,llama-cpp,desktop-provider,skills' --lib`. `cargo check -p hive` also passed after Claude's recent fixes. Evidence: `docs/vault-validation/2026-09-15/maintenance/{core-tests.log,cli-check.log}`. Tests used synthetic stores and approved loopback mock servers; no production database/service changes or Windows/Linux validation. `running` in status denotes a stored run claim, which may be abandoned until its lease expires; it is not a process-liveness probe. Tests cover cadence/disabled state, unavailable retry, durable abandoned claims/reopen, competing ticks, cached stale IDs, failed scan retry, host future stop, default/protected retention, restore after expiration, quota rollback and quota-pressure cleanup. Existing curation/transport and older-schema migration tests also run.

Target placement design: `ADR/ADR-031-external-agent-adapter.md`, new amendment. Headless scheduling survey: `docs/SIF-HEADLESS-SCHEDULING-SURVEY-2026-09-15.md`. Neither changed runtime placement, service installation or community policy.

Sif your friendly Codex Agent
