# Local-folder vault ingestion milestone

2026-09-14 — Sif your friendly Codex Agent

Implemented in `crates/ohhive-core/src/local_hub/vault_folder.rs`, with schema migration 3 in
`vault_folder_schema.sql` and developer entry point `crates/ohhive-core/examples/vault_folder.rs`.
This builds on the previous storage/search milestone; it does not add desktop UI or cloud sync.

## Behavior

- Owner explicitly attaches a folder to a new empty vault. Source path, root identity, scan generation,
  last successful index timestamp and generic failure status are persistent, host-local metadata.
- Read-only Markdown scans use cap-std directory-contained access. Visible symlinks cause a failed
  scan; hidden entries (including .git/.obsidian) are excluded. No files are created/modified in the source.
- Shared `vault::path_ok` validates indexed relative paths. Limits: 4 MiB per note, 64 MiB text per
  corpus, 10,000 notes, 20,000 visited entries, depth 32. Invalid UTF-8 or unreadable notes fail the scan.
- Filesystem traversal, title extraction and revision hashing occur outside the DB lock. Two full
  snapshots must agree before publication. One transaction replaces document/FTS rows and publishes
  ready state. This is a consistent indexed snapshot, not a filesystem-wide transactional snapshot;
  later edits are picked up by the next scan.
- Missing/replaced root, partial scan, quota failure or invalid data marks unavailable and preserves
  the old index. Failure advances the generation so an older concurrent scan cannot publish over it.
  Success clears the error. A changed generation rejects an obsolete reconciliation.
- Plain source files carry no embedded Hive IDs. Same-path edits and atomic replacement retain the
  stored ID. A unique removed/new pair with exactly matching content is treated as a rename. Copies
  receive new UUIDs. Ambiguous moves or rename-plus-edit may receive new IDs; no claim of universal
  rename tracking or full replica identity. Document reads still require UUID plus exact revision.
- Direct owner put/remove calls reject folder-managed vaults, preventing bypass of scan generation.
- A portable polling watcher runs an initial scan and recurring full recovery scans. The example uses
  five seconds. Normal shutdown, sender loss and task abort mark unavailable; a cancellation lock
  prevents a detached scan from publishing ready after the watcher is dropped.

## Running the developer host

Use existing LocalHub enrollment/pairing to obtain a reader node ID. With one store/host process per DB:

```
cargo run -p hive-core --features local-hub --example vault_folder -- attach <hub-db> <folder> <reader-node-id>
cargo run -p hive-core --features local-hub --example vault_folder -- serve <hub-db> <vault-id> <private-bind-address:port>
```

Stop the host before changing owner configuration using a separate process. Keep the private DB
outside the notes folder. The authenticated LocalHub HTTP reader methods serve this same index.
There is no automatic scan of home directories or any selected personal folder in this work.

## Verification and remaining gates

`cargo test -p hive-core --features local-hub --lib --example vault_folder`: **42 tests passed**;
example compiled. Existing unrelated worker dead-code warning remains. After tightening failed-scan
generation invalidation, all **9 folder tests passed again**. `git diff --check` passed.
Tests cover edits/rename/delete/copies, rapid atomic saves, invalid text, limits, hidden metadata,
symlink escape, absent/replaced source with recovery, old-generation rejection, v2 migration,
persistent identities after restart, normal shutdown and task abort. All fixtures are synthetic.

Remaining: physical two-machine acceptance; Windows/Linux execution and removable-filesystem tests;
UI/UniFFI/Tauri integration; scale/latency measurements. Native OS event watching is not implemented:
polling provides change detection and recovery together. Freshness is bounded by scan interval plus
scan duration; host availability is not a guarantee of the latest file edit having been indexed.
Root identity uses device/inode on Unix and creation time elsewhere; non-Unix identity behavior needs
hardware validation before claiming equal platform readiness. Full index replacement is intentionally
simple and bounded, but its transaction duration needs measurement on larger corpora. Use one active
watcher per vault and one LocalHubStore instance per database; opening a second instance still resets
availability as documented in Claude's review. No cloud provider compatibility, replica sync or
personal-note access has been introduced. No commit/deployment performed.

Claude: please review the cancellation/generation handling and conservative rename policy before
wiring the desktop lifecycle. Keep notes treated as untrusted data in later agent prompt integration.
