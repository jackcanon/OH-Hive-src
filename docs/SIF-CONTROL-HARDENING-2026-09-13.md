> Completed: migration applied, bounded WAN validation passed, temporary access cleaned up. See [WAN results](SIF-DELEGATION-WAN-RESULTS-2026-09-13.md). Earlier pending-approval text below is historical.

> Update: Claude logged approval. Reviewed SQL moved unchanged to `supabase/migrations/20260914030000_control_pilot_delegation.sql`; execution was rejected by automatic approval review pending direct user confirmation. See `SIF-DELEGATION-LIVE-APPROVAL-2026-09-13.md`. No live migration or setup ran.

# Task #202 — control-plane recovery and scoped delegation

**Sif your friendly Codex Agent — 2026-09-13**

Implementation and isolated verification are ready for Claude's review. The proposed delegation
schema is **not deployed**. The previous pilot remains disabled, and direct RPC remains live and
authoritative. This does not authorize or recommend community cutover.

## What changed

`crates/hive-server/src/control.rs` now binds its HTTP service even while PostgreSQL is unavailable.
`GET /hive/ctl/1/ready` reports 503 until the configured database gateway works, and 200 when ready.
A single connection supervisor reconnects automatically with exponential backoff and jitter
(125–250 ms initially, capped at 15–30 seconds). Connect/statement/lock/health checks are bounded.
Connection loss never replays a database operation. Supervisor, batch and connection tasks are
cancelled when the service stops.

`crates/ohhive-core/src/coordinator_hub.rs` checks readiness at the explicitly configured trusted
origin. It does not accept a redirect or silently switch servers. Expired/restarted sessions
re-authenticate automatically; a 401 received before dispatch permits one authenticated retry.
Heartbeats and read-only recovery/schedule requests can retry transient outages. Other writes
never retry after a timeout, 5xx, or malformed response because their outcome may be uncertain.
`recover_leases()` retrieves only authoritative, unexpired scoped leases and checkpoints; it
neither claims work nor resurrects expired leases. An already running worker can keep its lease
through a bounded outage. Restarting an entire worker still requires its caller to inspect the
recovery result and decide whether to resume local execution; this change does not replay work
or supply the broader session/lease-fencing system deferred from #202.

Regional authentication rejects full `hive_nk_` worker keys. Sessions retain only `hive_dg_`
delegations, scoped to one node, project, regional server and restricted database login, with a
one-hour maximum lifetime. Short session tokens are bound to that delegation and cannot outlive
it. Revoking the original node key, delegation, session, or pilot allowlist prevents continued use.
Re-authentication invalidates old sessions and retrieves only still-live lease scope.

`AuthorityDelegation` runs on the worker and obtains refreshed credentials from an explicitly
configured trusted authority; the raw node key is sent only there. The bounded example supports
this through `HIVE_CTL_AUTHORITY_URL`, `HIVE_CTL_AUTHORITY_ANON_KEY`, `HIVE_CTL_SERVER_ID`, and
`HIVE_CTL_PROJECT_ID`; its credential file then contains the raw local key. Without those settings,
the file must contain a preissued delegation, which can re-authenticate only until its expiry.
The optional existing community adapter remains explicit and is never a fallback for failed writes.

## Proposed database change — approval required

Review `docs/proposed-migrations/20260914030000_control_pilot_delegation.sql`.
It is deliberately outside `supabase/migrations/` to prevent an ordinary migration push from
applying it before Jack approves, as requested in Claude's continuity handoff.

The proposal adds a protected delegation table and a delegation reference on pilot session tokens,
an explicit trusted-authority issuance RPC, and the private readiness/recovery/delegated gateway
functions. It preserves the existing normal node-key verifier and all existing direct-RPC function
bodies/grants. The regional login still needs only its explicit gateway/heartbeat grants plus
EXECUTE on `hive.ctl_pilot_ready()`; it receives no direct table/helper access. The proposal itself
does not enable that role, change the allowlist, create worker identities, fund a project, or open
a route.

Private copies of the current node operations preserve settlement/checkpoint behavior while
accepting only scoped delegation. These copies are not new public account APIs; future fixes to
the original operations must also be considered for these private copies. This duplication is
intentional isolation for the pilot, not a claim that it is the final general-purpose architecture.
The delegated gateway rejects MCP account-secret retrieval. Supporting sandboxed MCP jobs will
need a separate narrowly scoped secret-delivery design; text inference remains supported.
Delegation permits the listed control operations for its scope, including node presence and
activity events; it is not an arbitrary RPC or SQL forwarding credential.

## Verification

- **71 workspace tests passed**, with loopback permission for HTTP tests. The first sandboxed
  attempt was rejected at socket binding; the permitted rerun passed. No code defect was hidden.
- HTTP tests cover token rotation, raw-key rejection before regional authentication, restart
  re-authentication, exactly one dispatched write after pre-dispatch 401, checkpoint recovery,
  transient heartbeat recovery, and no retry of uncertain writes/no community fallback.
- Real Supabase database tests applied the complete proposal and synthetic fixtures inside a
  single BEGIN/ROLLBACK transaction. Tests passed for role/server/project/node binding, expired
  and revoked delegation, issuing-key revocation, session invalidation, bounded session expiry,
  recovered checkpoints, exclusion of expired leases, completion and duplicate rejection,
  out-of-project and missing-card rejection, no account-secret access, private helper grants,
  and preserved direct claim RPC access. No new schema, data, or settlement persisted.
- Chicago Linux build passed. Using an extracted PostgreSQL 16 runtime (no system package install),
  a disposable TLS database bound only to 127.0.0.1:55439, and a test pilot on 127.0.0.1:8792:

| Check | Observed time |
|---|---:|
| Startup without database reports 503 | 0.242 s |
| First database connection becomes ready | 0.407 s |
| Stopped database reports 503 | 0.001 s |
| Restarted database becomes ready automatically | 2.022 s |
| Missing gateway function reports 503 | 0.003 s |
| Restored gateway function reports ready | 0.002 s |

The pilot PID stayed unchanged through database recovery. These measure a local TLS fixture on
real Chicago hardware, **not** WAN recovery against deployed Supabase delegation functions.
Both disposable processes were stopped in the test's finally block. The production Hive service
remains active, the previous pilot service inactive, and no test listeners remain. Extracted
runtime/database test artifacts remain under `/opt/hive-control-pilot/reconnect-test` for audit.
No new public route, Honey allocation, or community job execution occurred.

The bounded example compiles with `hub,llama-cpp,sandbox`. Evidence lives in
`docs/control-plane-reports/2026-09-13-hardening/`, including full test outputs, rollback result,
hardware results/server log, build output, and final-state checks. The reproducible hardware test
and fixture setup scripts are included there. They create only the isolated test database, not
Hive's production schema; do not rerun the initialization script over its existing data directory.

## Claude handoff and remaining decision

Please review the code and proposed SQL, then obtain Jack's approval for the delegation migration.
After approval, move the reviewed SQL into the normal migration directory, apply/record it, and
provision a bounded validation using the previous Chicago/duplicate-project scope. Grant readiness
only to the explicitly provisioned pilot role, issue worker delegations directly at the trusted
authority, verify the combined WAN authentication/recovery path, and tear down temporary access.
No additional funding is needed for authentication/readiness/recovery checks.

Do not report the deployed combined path as verified yet. This proposal still needs that approval
and integration pass. Multi-modality renewal, broader lease fencing, fleet-scale load testing,
editorial review, and the separate cutover decision remain outside this task's current scope.
Sif did not commit, deploy, or alter Claude's concurrent Swift UI work.
