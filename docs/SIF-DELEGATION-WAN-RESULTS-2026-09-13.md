# Task #202 — deployed delegation/WAN validation complete

**Sif your friendly Codex Agent — September 13 Phoenix / September 14 UTC, 2026**

Jack directly approved execution after automatic review required direct confirmation. The exact
reviewed `20260914030000_control_pilot_delegation.sql` was applied and recorded in the live migration
history. SHA256: `843ed121c8cdf13a032912c45f5b5ce8bd8ce9195dca330673baa3e33e6b5900`.
The initial PROPOSED comment is historical; the contents were not changed before application.

The combined real worker client → Chicago HTTPS control endpoint → authoritative Supabase database
path passed its bounded authentication/recovery validation. Temporary access is now torn down.
Direct RPC remains authoritative. No community cutover, new Honey allocation, or job execution.

## Observed results

The real Rust `CoordinatorHub` used `AuthorityDelegation`, sending its temporary raw worker key
only to the trusted Supabase issuance endpoint. Chicago received the scoped delegation. The same
client survived a database outage and a subsequent regional process restart without manual client
replacement. New reusable example: `crates/ohhive-core/examples/coordinator_recovery_probe.rs`,
explicitly gated by `hub`; it makes no claims/completions or funding calls.

| Measurement | Result |
|---|---:|
| Initial authority issuance + regional auth + empty-lease recovery + heartbeat | 1,326 ms |
| Heartbeat/recovery spanning an intentionally held database outage | 6,413 ms |
| Heartbeat/recovery after regional restart, including fresh authority issuance/auth | 2,571 ms |
| Natural pilot DB disconnect, trial 1 | 1.317 s |
| Natural pilot DB disconnect, trial 2 | 1.480 s |
| Natural pilot DB disconnect, trial 3 | 1.320 s |

Natural-disconnect timings start when the database administration call acknowledges terminating
only the pilot connection and end at the first successful public readiness response. All three
observed 503 before 200. Median **1.320 s**; this is a small timing sample, not an SLA. Compare the
previous isolated Chicago TLS database result of **2.022 s**: both demonstrate automatic recovery,
but they are not an apples-to-apples network benchmark (connection termination versus full database
restart, polling phase, backoff jitter, admin acknowledgement and public HTTPS overhead differ).
Do not claim that WAN recovery is intrinsically faster.

For the held outage, only the pilot login was briefly set NOLOGIN and its connection terminated;
a 503 was observed, the client was released to retry, and login was restored. The pilot PID stayed
unchanged through database recovery. Readiness returned 200 at the first poll 0.186 s after the
restore command returned; that figure excludes administration-call time and is not full recovery
latency. The client measurement above includes the deliberate hold and retry work.

Separate WAN probes verified:

- Full worker key rejected at Chicago (401).
- Scoped delegation issued directly at trusted authority and accepted at Chicago.
- Heartbeat rotates a 60-second session token; old bearer rejected (401).
- Recovery returns no leases, accurately matching the completed project's state.
- Out-of-scope release rejected (409), without modifying any card.
- Revoked delegation rejected at auth (401) and heartbeat (503).

The previous 71-test workspace suite and rollback-wrapped database tests cover active checkpoint
recovery, expired-lease exclusion, scope binding and settlement behavior. **This live pass used
empty lease recovery**, not a new in-flight job or replayed completion. It proves the combined WAN
authentication/recovery plumbing; active work restart/fencing and fleet-scale behavior remain
separate tests. No core behavior was changed during this validation; the added probe compiled and
ran against the actual deployed services.

## Cleanup verified

- Pilot process stopped; exact temporary HTTPS route removed; tunnel configuration restored
  byte-for-byte to its pre-validation backup.
- Server credential file removed, temporary worker keys/delegations/tokens revoked, allowlist
  disabled, restricted login disabled and password cleared.
- Final database check: zero unrevoked pilot keys, delegations or session tokens.
- Original Hive service active, public health HTTP 200; existing direct claim RPC grant retained.
- Both original and duplicate projects retain three review cards and zero leases. Balances remain
  30.8378 and 23.7137 Honey respectively. No funding or job changes occurred.
- Applied migration and audit records remain. Local temporary credential files were removed after
  evidence collection; no credential content is included in committed-intended reports.

Evidence: `docs/control-plane-reports/2026-09-13-delegation-wan/`. Contains migration/setup results,
actual client log, outage orchestration/probe results, server journal, final-state/health and cleanup
outputs, and test orchestration scripts without credentials. A deployment does not imply a Git
commit: Claude should consolidate the relevant files along with the earlier #202 changes. Sif did
not touch Claude's Swift/ADR work or make a commit.

**Claude: please review the completed result.** This supersedes pending-approval/deployment wording
in earlier #202 documents. Leave the pilot disabled until a separately scoped next test is agreed;
community cutover remains Jack's separate decision.
