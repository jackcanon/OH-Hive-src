# Regional control pilot — completed pilot report, 2026-09-13

**By: Sif your friendly Codex Agent**

The authorized Chicago pilot completed all three real cards through the regional control path.
Pilot access is now disabled and cleaned up. **Do not cut over community workers yet.**
Direct RPC remains live and authoritative; Jack's separate cutover decision is still required.
This report supersedes the earlier pending-project/pending-approval status.

## Scope and result

Jack/Claude approved the duplicate project, additive migrations, temporary identities and restricted
login, isolated service/route, and 50 Honey from Jack's existing wallet. No new purchase was made.
The original project `27f794a9-8159-467c-8cb3-d1e534199631` was not edited by this pilot.
Duplicate `6045a447-04ae-4a2d-bf7a-860c3d63257b` retains the three outputs for review.
Both projects now have three cards in review and zero leases.

The existing worker used local `qwen3.5:9b` through an isolated temporary model alias, with Chicago
handling control operations and the authoritative database handling leases/checkpoints/settlement.
The initial alias spelling lacked Ollama's `:latest`; only pilot card requirements were corrected.

| Card | Execution seconds | Output tokens | Honey earned |
|---|---:|---:|---:|
| outline | 180.369 | 3,492 | 7.4019 |
| guide | 182.490 | 3,546 | 9.5442 |
| review-polish | 174.144 | 3,540 | 9.3402 |
| Total | 537.003 | 10,578 | 26.2863 |

The duplicate retains **23.7137 Honey** of the allocated 50 for review/follow-up.
This is a successful transport/workflow test, not an editorial acceptance: the final draft ends
mid-list and contains an unverified installer filename. Review and fact-check before publication.

## Verification and fixes

- Final combined workspace suite: **66 tests passed**, plus SQL rollback checks and Linux build.
- Two temporary clients: eight heartbeats combined into four two-node database batches;
  competing claims granted one lease, checkpoint/release worked, out-of-project release rejected.
- Live three-card execution completed with persisted checkpoints and normal settlement.
- 60-second token test: heartbeat rotated the token, old bearer rejected (401). Default remains
  900 seconds, hard maximum 900. Private migration `20260914020000_control_pilot_token_ttl.sql`
  adds the bounded test TTL; both pilot migrations were applied and recorded.
- Stopping only the pilot produced 502 through its route. After restart, the previous token was
  rejected (401), and fresh authentication/heartbeat succeeded. No direct-RPC fallback occurred.
- Terminating only the pilot's database connection produced heartbeat 503, not a false durable
  acknowledgement. Restart plus fresh authentication recovered service.
- Revoking the second temporary worker key caused heartbeat 503 and fresh authentication 401.
- Initial TLS connection required the official Supabase root CA. Added optional
  `HIVE_CTL_DATABASE_CA`; certificate and hostname verification remain enabled.
- A null schedule initially triggered a Rust row-decoding panic/502. Fixed nullable decoding;
  subsequent schedule probes returned successful null results.
- Initial lifecycle probes immediately after service restart saw 502 before readiness and did not
  establish a saved token. These are retained in the raw log, not counted as passes. Repeated
  checks after verifying the listening socket passed. Add proper readiness reporting before rollout.

## Measured latency

Eight samples per method, sequential calls from this Mac. Small pilot measurements, not a load test.

| Operation | Direct median | Regional median |
|---|---:|---:|
| Heartbeat | 60.38 ms | 1,103.59 ms |
| Get schedule | 57.86 ms | 185.12 ms |

Heartbeat acknowledgement intentionally includes the one-second batching interval. The reduced
statement count trades latency for batching; this does not prove capacity at 150–200 workers.

## Cleanup verified

The pilot service is stopped; its exact tunnel route and server credential file are removed.
Tunnel configuration matches the saved original byte-for-byte. Existing `hive-server` is active;
public `/health` returns HTTP 200. Pilot allowlist disabled; restricted role cannot log in and its
password is cleared; both temporary worker keys and all pilot tokens are revoked. Local alias
removed. Duplicate/output records, additive migrations, disabled service unit and isolated build
files remain for audit/review. Original model and direct-RPC grants/defaults were not changed.
Chicago SSH was reliable via Tailscale `100.100.2.120`; public SSH was intermittent.

## Recommended next work for Claude

1. Review this report and the code changes before proposing any cutover. Build automatic database
   reconnect/backoff, explicit readiness, and client rediscovery/re-authentication with safe lease
   recovery. Current database disconnect needs an operator restart.
2. Replace raw worker bootstrap keys retained in regional memory with narrowly scoped delegation.
   The pilot reuses existing node-authenticated database functions; a regional process holding
   those keys is not yet an appropriate trust boundary for arbitrary volunteer-operated servers.
3. Test multi-modality renewal, session/lease fencing and recovery, and realistic concurrent load.
4. Establish output completeness/fact-check acceptance independently of transport success.
5. Present measured results and rollback criteria to Jack for the separate community cutover decision.

Evidence: `docs/control-plane-reports/2026-09-13/` contains final workspace/SQL logs, live probe,
real worker output, latency samples, lifecycle failures and successes, server journal, final DB
state and cleanup evidence. Raw lifecycle traces include intentionally rejected requests.
No credentials are included in these report files.

## Earlier implementation notes (historical)

The following preserves the implementation detail and pre-live checks from the earlier handoff.
Any pending authorization, project selection, execution or cleanup language below is superseded
by the completed report above.

## Implementation

- `crates/ohhive-core/src/coordinator_hub.rs`: third `Hub` implementation, explicitly constructed
  against one HTTPS origin. A bootstrap key is exchanged once; later operations use signed
  short-lived tokens. Calls serialize token rotation. Failed writes never replay through direct
  RPC. Account/artifact access is possible only through an explicitly supplied community adapter;
  the pilot worker supplies none.
- `crates/hive-server/src/control.rs`: `/hive/ctl/1/auth` and `/hive/ctl/1/rpc` HTTP surface.
  Uses a certificate-verified PostgreSQL connection held only by the regional server, the existing
  `hive_coordinator::place()` capability matcher, and authoritative database claim/completion rules.
  No arbitrary RPC forwarding or SQL endpoint. Requests capped at 2 MiB.
- `hive-control-pilot` binary: separate, opt-in regional process on loopback (8791 by default),
  intended for a narrow HTTPS proxy route. It does not run coordinator election or replace
  `hive-server`. PostgreSQL/crypto dependencies are behind the `control-pilot` server feature.
- `supabase/migrations/20260914010000_control_plane_pilot.sql`: additive allowlist and
  `hive.hub_tokens` tables, restricted database gateway, a scoped copy of the live claim function,
  and set-based heartbeat writes. No public PostgREST wrappers, no changed direct-RPC grants.
  `ctl_pilots` binds the actual database login (`session_user`) to one server, one Hive-mode
  project, and an explicit worker-node list; its default is disabled.
- `crates/ohhive-core/examples/coordinator_pilot.rs`: bounded existing-worker runner using a
  specified key file/model endpoint/scratch directory/card count. Separate heartbeat task; a
  failed heartbeat signals the existing worker stop flag. Does not load or rewrite `node.env`.

## Token and heartbeat behavior

Tokens are HMAC-SHA256 signed, contain node/server/project/current lease IDs, and expire within
15 minutes. The database stores token IDs and scopes, not raw bearer tokens or raw node keys.
Raw bootstrap keys remain in the regional process's memory so existing node-authenticated DB
functions can be reused; they are never logged. The process signing key is ephemeral, so a
restart invalidates its old signed tokens. Reauthenticate to recover scope for still-live pilot
leases; explicit release/checkpoint is available, but automatic worker-session recovery is not
implemented in this package.

The server batches pending heartbeats, with at least one second between flushes. Success is
acknowledged only after the database accepts the batch; outages/revocation return errors.
Each batch rechecks keys, token expiry/revocation and the pilot allowlist. It updates presence
and renews only unexpired pilot text leases named by those tokens. It cannot resurrect expired
leases or renew another project's work. Other modality renewal is not claimed for this text pilot.
RTT is measured by the client, including batching delay, and emitted in pilot logs; the pilot
currently does not overwrite existing direct-RPC RTT history with this differently routed metric.

The database preserves funding, dependencies, model/tool/internet checks, output storage, and
Honey completion behavior. Card-changing operations additionally require the token's lease scope,
a matching live lease, and the configured project. Completion ambiguity is reported rather than
automatically retried. No default endpoint selection, failover election, libp2p transport, or
community-wide rollout is implemented.

## Verification so far

- Whole workspace with `hive-core/local-hub,hive-server/control-pilot`: **66 tests passed**.
- Four focused server tests: signing binds identity/server/project/leases; malformed/expired/
  forged tokens fail; scheduler respects model/memory/internet/presence/busy-worker constraints.
- Three new client tests included in the workspace total: bootstrap exchange and rotated-token
  use, no write replay or Supabase fallback on failure, plaintext remote-origin rejection.
- Live-database rollback-only tests passed: project isolation, unfunded claim rejection,
  successful claim/checkpoint/completion, duplicate completion rejection, expired lease rejection,
  no heartbeat resurrection, revoked token rejection, private gateway grants, direct-RPC grants
  preserved. All fixture projects/nodes/outputs/leases and the test migration were rolled back.
- Linux (Chicago, Rust 1.95): final pilot binary builds and all four focused server tests pass.
- Bounded worker example builds. The final candidate-selection adjustment passed focused tests.
- Evidence: `docs/control-plane-reports/2026-09-13/`.

Not yet verified: real HTTP → regional PostgreSQL execution, simultaneous worker batching through
HTTP, real-project completion, latency versus direct RPC, disconnect/reconnect behavior under a
live workload, Windows build. Those remain release gates for this pilot, not implied by unit tests.

## Deployment preparation and next steps

Chicago's running community service and tunnel were not changed. Sif installed Linux build
prerequisites (`pkg-config`, `libssl-dev`) and an isolated Rust 1.95 toolchain under
`/opt/hive-control-pilot/{cargo,rustup}`. Rust-only source is staged in
`/opt/hive-control-pilot/source`; build log is `/opt/hive-control-pilot/linux-build.log`.
The focused Linux test log was retrieved. A subsequent final-build-log transfer failed with an
SSH timeout; the successful remote build command had already exited 0. Recheck Chicago connectivity
before deployment. No pilot service, login/password, enabled database allowlist, or HTTPS route has been provisioned.
No persistent pilot migration has been applied.

After Jack chooses the project:

1. Recheck Chicago and the approved project. If a copy is authorized, create it separately;
   preserve the original outputs. Use explicit pilot worker identities, leaving existing workers
   running normally. Review/fund the pilot through the normal ledger path, with a bounded budget.
2. Apply only this migration, not every pending shared-repository migration. Provision a separate
   database login with `USAGE` on `hive` and `EXECUTE` only on
   `hive.ctl_pilot_call(text,uuid,text,jsonb)` / `hive.ctl_pilot_heartbeats(jsonb)`; no table grants.
   Add its disabled `ctl_pilots` row, then enable only for the approved server/project/workers.
   Keep its generated password in a mode-0600 server environment file, never in reports or Git.
3. Finish the Linux build, run the pilot process separately with `HIVE_CTL_DATABASE_URL` and
   `HIVE_CTL_LISTEN=127.0.0.1:8791`, and add only a `/hive/ctl/1/` route to the existing HTTPS
   tunnel configuration after preserving its current file. Validate the route and original health
   endpoint. Do not modify the community binary, defaults, election, or RPC privileges.
4. Exercise authentication, two-client batching, token rotation, revocation, scoped claims,
   checkpoints, completion, controlled connection loss, and database outage behavior.
5. Run the approved real project with the bounded example and a real local model. Compare
   direct-RPC and pilot control latency separately from inference time and batching wait. Record
   results, usage, any rejected or ambiguous calls, and database statement counts where measurable.
6. Disable the pilot allowlist after the bounded run; drain/stop the pilot process and remove its
   route, or explicitly agree to leave it available. Recheck original service health and grants.
   Return the evidence to Jack/Claude. Any actual cutover remains a separate decision.

Reproduction on this Mac:

```sh
cargo test --workspace --features hive-core/local-hub,hive-server/control-pilot
cargo build -p hive-core --features hub,llama-cpp,sandbox --example coordinator_pilot
cargo build -p hive-server --features control-pilot --bin hive-control-pilot
```

For SQL validation, concatenate `BEGIN;`, the migration, `supabase/tests/control_plane_pilot.sql`,
and `ROLLBACK;` into one query file. Do not run the test body without that outer transaction.

Signed: Sif your friendly Codex Agent
