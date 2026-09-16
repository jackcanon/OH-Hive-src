# Live schema recovery and reconciliation

Jack explicitly approved the previously blocked read-only schema-definition export. Recovered the missing production objects locally, completed the live/replayed schema comparison and added repeatable CI coverage. No production migrations, ID repairs, data writes, deployment, commits or pushes performed.

## Recovered definitions

`20260913221000_recovered_live_schema.sql` restores 19 functions (11 internal functions and 8 public wrappers), three tables (`geocodes`, `chat_link_codes`, `presence_events`), `members.home_geocode` and its foreign key, and four triggers for node/server presence and notification delivery. Columns, constraints, indexes and policies were recovered from the approved catalog export. The earlier notification subscription/delivery baseline remains part of the chain.

The three reported missing app RPCs are present: `hive_presence_recent`, `hive_member_create_link_code`, and `hive_node_projects_overview`. Recovery also includes member node listing/checkout and home geocode selection, plus bridge link redemption/unlinking and notification fan-out.

Live bridge link/unlink functions had broad execution grants. The recovered local baseline intentionally grants only the public bridge wrappers to `service_role`, with internal functions inaccessible to ordinary clients. User-facing member endpoints retain authenticated access; the node endpoint validates its node key. This limits who may change linked identities without changing Hive community project visibility. It is a pending local correction, not a claim that the live grants are fixed.

## Runtime corrections found during recovery

`20260916050300_recovered_node_projects_visibility.sql` corrects three concrete failures:

1. Node project listing was STABLE despite calling a key verifier that updates key usage; it is now VOLATILE.
2. That listing included local/private projects. It now lists only community (`execution_mode='hive'`) projects, preserving private-fleet separation and community-wide visibility.
3. Member checkout used text CASE results for the presence enum. Actual execution failed. Explicit enum casts now allow checkout to work.

These are separate from the reference snapshot so the distinction between live behavior and intended changes is reviewable.

## Verified result

- All **93 migrations** replay from an empty PGlite database.
- All **1,070** captured structural catalog records are represented; no missing live table, column, constraint, index, policy, trigger, view, enum, sequence configuration or function signature remains in that comparison.
- All **326 live Hive functions** are accounted for: **311** retain the same normalized body; **15** differ intentionally. The manifest maps each difference to account display, lease/ledger/security hardening, speech reservations or the recovered-RPC corrections. No unexplained function-body difference remains.
- New runtime tests execute the actual recovered functions after full replay: node project privacy/invalid keys, member identity, link-code creation/service-only redemption and replay rejection, presence events, notification delivery and checkout. No member/app rows were exported for the fixtures.
- Speech accounting/reservation tests, static guards (53 Hive tables), and whitespace checks pass. CI now runs replay, baseline comparison and accounting/Private Fleet regressions. This is local execution of those checks, not a remote CI run.

Reference artifacts live in `docs/schema-baseline/2026-09-16/`. PGlite's external auth/storage/vault/cron interfaces remain explicit platform stand-ins; the tests do not prove production scheduler, network bridge or cloud-service behavior, nor independent PostgreSQL-session contention.

## Deployment handoff to Claude

Use `SIF-MIGRATION-VERSION-RECONCILIATION-2026-09-16.md` to review repository-versus-production version names. Production timestamps still differ from the repository: no automatic repair or push was run. Compare each historical deployment record before marking a version applied, and treat the recovered baseline as an additive backfill, not a replacement that erases the deployed migration history.

The old proposed labels on the control pilot and card submission were stale: production metadata confirms both applied. Keep them in the chain. Deploy the pending Private Fleet schema/client changes together. Run genuine PostgreSQL concurrency and platform integration acceptance before rollout; choose the speech tariff and finish its submission/cap wiring separately. Geographic lookup rows and service configuration are intentionally outside this schema-only baseline.

Sif your friendly Codex Agent
