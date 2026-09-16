# Hive production schema reference — 2026-09-16

Read-only snapshot approved by Jack, captured through the Supabase Management API. Scope: the `hive` schema and `public.hive_*` functions; no application rows, provider secrets, private project contents or sequence current values were requested. External Supabase platform schemas remain platform dependencies.

- `hive-functions.sql`: 326 function definitions as they existed live. **Reference only, never a migration or restore script.** Executing this after the pending migrations would undo fixes.
- `catalog.json`: 1,070 structural records (tables, columns, constraints, indexes, policies, triggers, views, function access metadata, enum definitions and sequence configuration).
- `function-manifest.json`: normalized function-body hashes and the 15 intentional pending body changes, each mapped to its migration.

`test-migration-replay.mjs` can export local metadata with `REPLAY_CATALOG` and `REPLAY_FUNCTIONS`. `scripts/migration-replay/compare-baseline.py` checks object coverage, existing columns/constraints/sequences, enum preservation and exactly the reviewed function-body differences. New local objects are allowed. Function grants and policy expression parity are recorded for review but not semantically proven by the body-hash comparison. This is not an automatic migration-history repair or an authorization to deploy.

The geocode table is recovered structurally; geographic lookup seed data is not included in a schema-only export. Fresh deployment needs an approved lookup-data source if that picker is to offer real locations. Cron, external bridge configuration and actual Vault/Storage services require separate environment configuration.
