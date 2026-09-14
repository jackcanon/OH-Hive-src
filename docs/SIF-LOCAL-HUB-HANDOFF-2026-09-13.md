# ADR-025 local hub — implementation handoff

**By:** Sif your friendly Codex Agent · **Date:** September 13, 2026

The Rust local-hub package is implemented and ready for Claude's integration review. New projects can be stored in SQLite on the owner's machine and processed in-process or through an authenticated local HTTP hub. Pairing, project/card/checkpoint/output persistence, and activity receipts use no Supabase endpoints. Existing Supabase-backed projects are unchanged and are not migrated.

This is a developer/library surface, with a runnable example. It is not a published desktop feature. A real two-machine hardware test remains pending because SSH to the known Overgaard and Heimdall addresses timed out from this session. Two-client loopback and separate-process smoke tests passed.

## User-approved correction to the original handoff

The proposed 12-method Hub trait did not cover two existing paths that carry project content: coder activity posts and CloudBrain's Supabase Edge Function call. Keeping a direct HubClient alongside LocalHub without changing those routes would have uploaded private work even though card rows lived locally.

Sif asked Jack whether the small routing changes needed to prevent this were authorized, with direct-to-provider cloud coding left for a separate follow-up. Jack answered: **“Yes—keep fully local jobs off Supabase.”**

Accordingly:

- The 12 specified data-plane methods are present. Two explicit additions are `community_client()` (default None) and `post_activity(...)`. SupabaseHub returns its existing account client and forwards activity unchanged; LocalHub and RemoteLocalHub return no community client and write receipts to local SQLite.
- CloudBrain rejects a fully local hub before making any request. Local project creation also rejects cloud-brain code cards. **Fully local coding currently requires a local model.** Direct-to-provider Anthropic/OpenAI/Nous integration is a follow-up; the existing Supabase-backed cloud coding path still works for existing community-backed projects.
- Community artifact fetch/upload tools return an explicit unsupported error on local hubs. Their old HubClient implementation remains unchanged for SupabaseHub. These adapters must not acquire a fallback community client in future work.
- Worker/coder edits are interface/routing changes; their inference/agent step loops were not redesigned. Existing account conveniences, pairing, memory, provider-key management, and regional-server APIs remain on HubClient.

## Source map

Paths relative to `Projects/Apps/OH Cloud-src`:

- `crates/ohhive-core/src/hub.rs`: Hub trait, SupabaseHub concrete name, backward-compatible HubClient type alias, direct forwarding implementation, serializable data-plane response types.
- `crates/ohhive-core/src/worker.rs`: worker hub field uses `&dyn Hub`.
- `crates/ohhive-core/src/coder.rs`: trait reference, local activity routing, pre-request rejection of the community cloud adapter on local hubs.
- `crates/ohhive-core/src/tools.rs`: trait references; explicit community-only artifact boundary.
- `crates/ohhive-core/src/local_hub/mod.rs`: LocalHubStore administration, authenticated LocalHub sessions, SQLite transactions, pairing and Hub implementation.
- `crates/ohhive-core/src/local_hub/schema.sql`: schema version 1, including projects/cards/card_outputs/checkpoints/leases, nodes/local_node_keys, pairing, child relationships, MCP configs and activity.
- `crates/ohhive-core/src/local_hub/transport.rs`: axum server and RemoteLocalHub client for the same trait.
- `crates/ohhive-core/src/local_hub/tunnel.rs`: opt-in reuse of existing cloudflared create/DNS automation with a separate local-hub config and process. Never overwrites the community `~/.cloudflared/config.yml`.
- `crates/ohhive-core/src/local_hub/tests.rs`: local-hub regression/integration tests.
- `crates/ohhive-core/examples/local_hub.rs`: runnable developer entry point, including a worker using the existing local-model backend.
- `scripts/test_local_hub_smoke.py`: reproducible standalone-process smoke test using synthetic data and temporary files.
- `Cargo.lock`, core Cargo.toml and lib.rs: opt-in `local-hub` feature, bundled rusqlite, sha2, rand, axum. Existing builds do not enable SQLite by default.

No Supabase schema, production web app, installed fleet worker, or existing project data was modified by this package. Sif made no Git commit. Claude made concurrent consolidation commits during the session; review the current working tree rather than assuming a clean starting commit or attributing every tracked diff to Sif.

## Storage and execution behavior

`LocalHubStore` is a trusted local administration handle. It creates projects/cards, configures MCP servers, enrolls the owner node, creates pairing codes, revokes node keys, and inspects results. None of these administration operations is exposed over HTTP except redeeming a valid pairing code.

Every worker connects with its own node key and worker-session UUID. Each mutation revalidates the key. Claiming uses an immediate SQLite transaction, and unique constraints enforce one current lease per card and node. Checkpoints, completion, release, failure and child creation require that node's unexpired lease and session. A new worker session cannot finish an earlier session's lease. Same-session identical completion retries return the previous result; conflicting repeats fail. Local completion produces a review result and zero Honey.

Claims honor modalities, installed model IDs, internet/tools capability, dependency outputs, optional memory requirements and target_node_id, and enabled local MCP configuration. Repo-based code cards created through `add_card` require internet even if omitted in the input card. Capability discovery/check-in must occur before claiming.

Generic expired leases return to ready and carry the saved checkpoint. **Expired or released code sessions become blocked**, since the current coder cannot resume its side effects safely. They are not automatically replayed. Recovery of arbitrary side-effecting commands remains a separate concern; this package does not claim exactly-once external tool execution.

Child creation is idempotent by project key and parent relationship; conflicts are rejected. Waiting releases the parent lease. Completed children make waiting parents eligible again with dependency output and checkpoint context; child failures propagate blocked state. The existing review/done dependency convention is retained, not changed into a new approval workflow.

Local MCP configuration is read from SQLite and requires tools-enabled node capabilities. It never falls back to the member's cloud MCP configuration. The database holds private project data and MCP environment values, so it belongs in the owner's protected data directory.

## Pairing and transport

- Node keys: `hive_nk_` plus 24 random bytes encoded as 48 hex characters; only SHA-256 hashes are stored. Raw keys are returned once to the joining owner and saved locally by the example.
- Pairing: one active eight-digit code, five-minute expiry, five attempts total, one successful redemption. Wrong attempts commit their counters. Generating a new code invalidates the old one. The raw node key is generated on successful redemption, avoiding storage of a recoverable pending raw key.
- A paired node is trusted as part of the single owner's fleet. This is not multi-tenant or per-project authorization between mutually untrusted users.
- HTTP exposes POST `/local/v1/pair` and bearer-authenticated POST `/local/v1/rpc`. There are no anonymous project reads, cookie auth, request-body logs, or CORS permissions. RPC dispatch is an explicit method allowlist. Requests are size-limited and SQLite work runs off the HTTP executor threads.
- `serve` refuses wildcard/public binds. Default example bind is `127.0.0.1:8787`; LAN service requires an explicit private interface address. Plain HTTP is supported only on numeric private/loopback/link-local origins (or localhost). **Plain HTTP assumes a trusted LAN**; it does not encrypt pairing credentials or job data. Use an owner-managed HTTPS tunnel for other networks.
- The remote client disables environment proxies and redirects, so credentials and project data do not silently follow a proxy or redirected destination.
- Optional Cloudflare setup is never invoked by local storage, serving, or pairing. It reuses existing tunnel creation/DNS functions but writes a separate JSON/YAML-compatible config targeting the selected loopback port. An explicitly chosen relay carries traffic through that relay; this is distinct from Supabase storage and from direct LAN operation. No tunnel/DNS change was made or tested live in this session.
- Unix database/credential files are created with mode 0600; existing overly permissive database files and database symlinks are rejected. Windows callers must supply a user-private directory with appropriate inherited ACLs. This session did not run Windows/Linux builds or ACL checks.

The no-Supabase property applies to the built-in local project data plane and its adapters. It is not a firewall around arbitrary user-authorized commands, MCP servers or model endpoints. Their network behavior remains governed by the existing runtime and the owner's configuration.

## Run the developer surface

From the repository root:

```sh
cargo build -p hive-core --features local-hub,llama-cpp,sandbox --example local_hub
./target/debug/examples/local_hub
```

The example prints available commands. Typical sequence (use an existing private directory):

```sh
./target/debug/examples/local_hub init /private/path/local.sqlite /private/path/owner.json
./target/debug/examples/local_hub project /private/path/local.sqlite "My local project" "Requested outcome"
./target/debug/examples/local_hub card /private/path/local.sqlite /private/path/card.json
./target/debug/examples/local_hub work /private/path/local.sqlite /private/path/owner.json http://127.0.0.1:11434 /private/path/work
```

The model endpoint must expose the existing OpenAI-compatible local-model API. `work` explicitly enables the existing private coding tool mode; add `--allow-internet` only when desired. It never reads the community `node.env`. Ctrl-C requests normal worker shutdown.

For a second node, start `serve` on an explicit private address, run `pair-code` on the owner machine, and run `pair <hub-origin> <code> <node-name> <new-credentials-file>` on the joining machine. Then use `work <hub-origin> ...` there. Do not distribute the owner credential file or put raw node keys in logs.

A card JSON uses the existing ClaimedCard shape: id/project_id UUIDs, key, title, modality, inputs, acceptance, optional deps/requires_internet, and required_capabilities. For code cards, include the existing CodeSessionSpec fields inside required_capabilities, set brain to local, and provide the intended workspace. The new desktop project/hub selector and polished local project creation UI remain follow-on work.

## Verification and remaining release work

Verification was performed on this macOS host:

1. Full `cargo build --workspace` and `cargo test --workspace` passed after the trait/routing change, before starting LocalHub implementation, as requested by the handoff.
2. Full workspace build/test with `--features hive-core/local-hub` passed during integration; 59 tests passed in the final whole-workspace pass. The focused 14-test local-hub suite also passed after the last delegated-repository internet-gate correction. Output is filed alongside this handoff under `docs/local-hub-reports/2026-09-13/`.
3. Local-hub tests cover pairing replay/expiry/attempt limits/hash-only storage, revocation, concurrent claims across independent SQLite connections, unauthorized/stale writes, checkpoints/dependencies, children, local MCP, persistence, routing gates, target placement and repository internet requirements. They include the normal Worker with a deterministic backend and coder activity with a synthetic brain.
4. Two HTTP clients paired, competed for one job, checkpointed and completed it; a revoked key was rejected. This is a loopback test, not two physical machines.
5. `python3 scripts/test_local_hub_smoke.py` passed against the actual standalone process: local pairing, competing claims, checkpoint handoff, result persistence after process exit. No cloud endpoint was used. Temporary fixture files were removed.
6. Real two-machine smoke was attempted but unavailable: SSH to Overgaard `192.168.1.61` and Heimdall `192.168.1.50` both timed out. No remote installation or remote filesystem changes were performed.

**Claude: review these changes and the two user-approved routing additions; preserve the no-fallback rule.** Then run the physical two-machine test, review local-network authentication/transport for the intended deployment, check Windows/Linux behavior, and wire the desktop's hub choice and project creation. Direct cloud-provider support for fully local jobs, existing-project export/import, production tunnel deployment and code-session recovery remain explicit follow-ups. Do not advertise existing Supabase-backed “local execution” projects as physically local; only projects created on this new hub have the new storage behavior.

The newly queued control-plane migration is noted in the continuity log as a subsequent package, not part of this implementation.

Signed: Sif your friendly Codex Agent
