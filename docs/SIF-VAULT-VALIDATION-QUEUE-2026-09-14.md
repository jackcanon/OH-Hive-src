# Vault validation queue and cross-machine core handoff

2026-09-14 — Sif your friendly Codex Agent

Completed Claude's five validation items on the available Mac/Linux hardware. Windows was not
attempted. Worker/coder, Swift/FFI and real desktop effects were outside scope and left untouched.

## Results

- **58 core tests passed on Midgaard/macOS** with `cargo test -p hive-core --features local-hub --lib`.
- **58 core tests passed on Heimdall/Linux** using an isolated source-only temporary checkout and
  its existing Rust toolchain. Heimdall is reachable at **192.168.1.201**, supplied by Jack after
  the stable log's `.50` timed out. Cargo exists at `~/.cargo/bin/cargo`, outside the SSH default PATH.
- **Physical Midgaard → Overgaard vault access: PASS**, 12 checks. The Rust host bound
  `192.168.1.203` on an ephemeral port; the independent Python reader ran on Overgaard `.202` via
  SSH and made real LAN HTTP calls. No loopback substitute and no preexisting account credentials.
- Added exact-production-limit and one-over tests for corpus bytes (64 MiB), note count (10,000),
  entry count (20,000), and depth (32). These exercise real filesystem traversal, not scaled-down
  replacement constants. Both platforms passed.
- Added normal desktop sequence test (Allow → fresh Observe → Allow) and PNG-envelope exact/over
  dimension and byte limits. Header/envelope validation remains distinct from actual PNG decoding.

The physical test checks fresh pairing, no implicit vault grant, explicit grant, search, exact
revision read, unchanged-content rename retaining identity, stale revision rejection, missing-source
503, recovery, grant removal, node-key revocation and unreachable-host errors. Host-offline testing
checks the error path, not a packet-capture proof about arbitrary future adapters. Existing core
`RemoteLocalHub` loopback tests additionally exercise the Rust reader/client implementation.

Synthetic folder mutation and scan triggering in the hardware harness are explicit stdin admin
commands. Polling-watcher rapid-save/shutdown behavior is exercised by the unit tests on both
platforms. The hardware test does not claim to simulate unplugging an actual removable drive.

## Scale measurements

Three full scan/reconciliation runs and searches at each size. Debug builds, file-backed SQLite,
synthetic Markdown of 1,017 bytes per note containing repeated common tokens. Figures below are
medians, rounded. Corpus files are created before measurement. Scans include two filesystem walks,
hashing and complete transactional index replacement. Searches return 10 matches.

| Notes | Text size | Mac scan | Linux scan | Mac search | Linux search |
|---|---:|---:|---:|---:|---:|
| 100 | 101,700 bytes | 16.66 ms | 24.88 ms | 0.67 ms | 0.76 ms |
| 1,000 | 1,017,000 bytes | 163.69 ms | 191.42 ms | 1.68 ms | 2.37 ms |
| 10,000 | 10,170,000 bytes | 1,738.68 ms | 1,993.72 ms | 15.92 ms | 23.90 ms |

These are illustrative measurements, not production SLOs or a representative natural-language
benchmark. No release-build, cold-cache, RSS, concurrent-search stall or maximum-64-MiB indexing
latency claims. The maximum-corpus boundary test walks that corpus but does not benchmark its FTS
publication. Full replacement at every poll has visible cost: the current five-second sleep occurs
*after* a scan, so the 10,000-note scan cycle here is about seven seconds, not a fixed five seconds.
Recommend measuring lock-hold/concurrent read latency before optimizing to incremental publication;
keep failed-scan preservation and generation guards intact through any optimization.

## Core-only cross-machine plumbing: present and now verified

The requested core seams already exist. Avoid a parallel protocol or new privilege-bearing API:

1. Host opens **one** `LocalHubStore`, attaches folders, owns their watchers and calls `serve` with
   that same store. The host's explicit shutdown must stop watchers and await server completion.
2. Owner obtains a short-lived `pairing_code`; reader uses `RemoteLocalHub::pair(origin, code, name)`.
   Pairing creates a node credential, not a vault grant. Persist the key privately on the reader;
   never send it to the model or log it. Remote reader does not open a shared SQLite file.
3. Owner uses `vault_grant(vault_id, node_id, enabled)` locally. This is deliberately absent from
   the HTTP RPC allowlist. The grant UI remains Claude's Swift work.
4. `RemoteLocalHub::new` plus `vault_list`, `vault_status`, `vault_search`, `vault_read` provide the
   reader surface. Reads require document ID and exact revision. Pairing/grant/revoke behavior is
   covered by the physical test and existing Rust HTTP tests.
5. Grant removal returns denied; revoked node credentials return unauthorized; unavailable source
   returns 503; offline host returns a transport error. UI should distinguish these and never turn
   any into an empty successful search or silently select another host/cloud route.

No new runtime API was needed for that core-only scope. Added reusable validation harnesses:
`crates/ohhive-core/examples/vault_validation.rs` and `scripts/vault_two_machine_validation.py`.
The example's administration is stdin-only and synthetic-only, not an exposed remote admin service.
Claude can wire the existing protocol into the per-device Swift flow using this acceptance evidence.

## Evidence and cleanup

Raw result files under `docs/vault-validation/2026-09-14/`:
- `two-machine.json`
- `scale.jsonl`
- `linux-tests.log`
- `linux-scale.jsonl`, `linux-scale-build.log`

The temporary Mac host exited and removed its synthetic folder/database. The remote Python reader
exited; ephemeral node credentials existed only in its memory and were revoked before stopping.
Heimdall's isolated `/tmp/hive-vault-validation.q6CYwHnm` checkout/build directory was removed and
absence verified. The local source tarball was removed. Normal Cargo dependency caches remain;
no existing Hive checkout/service or personal notes were changed. No deployment or Git commit.

Outstanding beyond this queue: actual Windows hardware; production scale/read-contention measures;
Swift grant/reader integration; real removable-volume behavior. Desktop worker/coder integration and
the dedicated native-permission pilot remain Claude/Jack's separate work.

Sif your friendly Codex Agent
