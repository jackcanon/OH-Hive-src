# Central-query vault v1 — first implementation milestone

Date: 2026-09-14
Author: Sif your friendly Codex Agent

## Built and verified

The shared Rust LocalHub now has a versioned, transactional schema-1-to-2 migration, private vault metadata, explicit per-node reader grants, document IDs, revision-checked reads, and SQLite FTS5 search. Fresh databases and existing databases use the same migration path. Foreign keys are enabled before migration and on every connection. Opening a store marks vaults unavailable until their sources are revalidated.

Host-local administration creates vaults, grants/revokes readers, updates/removes indexed documents and marks availability. Reader APIs are `vault_list`, `vault_status`, `vault_search`, and `vault_read`. They are available on both LocalHub and RemoteLocalHub through the existing authenticated RPC endpoint. No vault administration operation was added to the HTTP allowlist. Pairing alone does not grant access. Every request checks key revocation and vault grants. Unavailable sources and unreachable hosts return errors; there is no cloud fallback.

Identity is an explicit document UUID. A revision hashes that identity, relative path, title and content. Updating or renaming a document with the same ID changes its revision; reading an obsolete revision fails and asks the caller to search again. This implementation stores the current revision only, not revision history or offline replicas. The indexer must supply stable identity; automatic filesystem rename reconciliation is not implemented yet.

Queries are bounded, treated as literal terms and scoped to the granted vault. Paths reject traversal, absolute paths and non-Markdown files. Text is returned as data; agent prompt integration remains to be implemented and must treat note contents as untrusted material.

Verification: `cargo test -p hive-core --features local-hub --lib` passed **33 tests**. Four new tests cover migration/reopening with existing data, grants/revocation, cross-vault identity isolation, stale revisions, rename/edit/delete FTS consistency, path/query bounds, and authenticated HTTP success/unavailable/revoked/offline behavior. HTTP tests ran on loopback; this is not the required physical two-machine acceptance test.

## Remaining implementation, in order

1. Read-only folder ingestion with durable source configuration and document identity reconciliation. Walk/hash/parse outside the database lock; commit a complete bounded reconciliation atomically. The present per-document owner APIs are storage primitives, not a production batch scanner.
2. Watcher hints plus initial/periodic recovery scans, quotas and symlink containment. Missing mounts or failed/incomplete scans must mark unavailable without deleting existing indexed notes. Test rapid saves, rename/delete, restart, missing mounts and permission failures with synthetic notes.
3. Run physical two-machine synthetic acceptance tests, including grant removal, key revocation and unavailable host behavior.
4. UniFFI lifecycle/API wiring and Swift UI, then Tauri bindings/UI using the same Rust API. Coordinate around Claude's concurrent desktop changes.

No user note folders have been selected or scanned. No desktop product flow is wired yet. No deployment, fleet reconfiguration, full multiwriter sync, or Git commit was performed.

Primary files: `crates/ohhive-core/src/local_hub/{mod.rs,vault.rs,vault_schema.sql,transport.rs,tests.rs}`.
