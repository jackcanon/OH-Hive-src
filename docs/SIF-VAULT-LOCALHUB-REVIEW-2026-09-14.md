# ADR-028 — LocalHub fit for a fleet-wide knowledge vault

**Sif your friendly Codex Agent — 2026-09-14. Architecture review; no feature implementation.**

**Recommendation: reuse LocalHub for the authoritative vault catalog, indexed text and fleet
search/read API. Build a separate Rust ingestion/watcher component alongside it. LocalHub is not
currently a file-sync engine, and keeping the index in its SQLite database does not create one.**

Every paired machine can search the same library without maintaining its own folder or index.
That meets fleet-wide access while the hub is reachable. If the requirement is to edit independent
folder copies on every computer, or search while disconnected from the hub, that requires an
additional synchronization/cache protocol. Do not describe that as already solved by ADR-025.

## Findings in the current source

| Evidence | What it means for the vault |
|---|---|
| `crates/ohhive-core/src/local_hub/mod.rs`: `LocalHubStore` owns one `Arc<Mutex<Connection>>`; `transaction()` takes that lock and starts an immediate transaction | A central database, not replicated SQLite. Bulk indexing would contend with leases/heartbeats unless work is batched carefully. |
| Same file: schema versions above 1 are rejected; `schema.sql` sets `user_version=1` | Add an explicit versioned migration. Do not just append tables and leave the initialization/version behavior unchanged. Older clients/hosts need a compatibility story. |
| `local_hub/schema.sql` contains projects, nodes/keys, cards, leases, outputs, checkpoints, links, MCP config and activity | No vault tables, filesystem manifest, file-change log, tombstones, per-source sequence cursor or merge protocol exist. |
| `local_hub/transport.rs`: two POST routes, bounded JSON dispatch, `RemoteLocalHub` request/response client | Reusable reachability/pairing and credential checks. No file transfer protocol, subscription, pushed replication, offline replica or watcher service. |
| `local_hub/transport.rs`: database work uses `spawn_blocking`, requests capped at 8 MiB | Preserve the blocking boundary and bounded messages; do not route a whole vault through one RPC. Response limits/pagination need explicit design too. |
| `local_hub/tunnel.rs`: optional separate owner-configured tunnel | Remote access is available by explicit setup, not automatic discovery or always-on availability. LAN operation needs no cloud. A tunnel has its own intermediary trust boundary. |
| `crates/ohhive-core/Cargo.toml`: optional bundled rusqlite under `local-hub`; resolved libsqlite3-sys build enables FTS5 | SQLite full-text indexing is technically compatible with the dependency. This is build-configuration inspection, not a completed vault benchmark. |
| Repo-wide searches: LocalHub used in its core, tests and CLI example; no LocalHub symbols in the current UniFFI Rust implementation, and FFI Cargo features omit `local-hub` | The ADR overstates existing desktop integration. Shared Rust is the right destination, but host lifecycle, pairing configuration and vault methods still need actual UniFFI wiring. |

The earlier two-machine test proved two clients could use one hub for structured work. It did not
prove file watching, replicated vault indexes or reconciliation after disconnected edits.

## Recommended v1: one source of files, one index, many readers

The member selects a vault source folder accessible to the designated hub host. A Rust
`VaultIndexer` reads that folder and publishes bounded content updates to a `VaultStore` module.
The module owns tables within the LocalHub database. Paired desktops/agents call a `VaultClient`
through authenticated local endpoints. The physical folder location is an implementation detail;
the library is identified by a fleet-scoped vault ID, not by a remote computer's absolute path.

Keep these interfaces separate from the generic community `Hub` job contract. Sharing LocalHub's
transport and identity checks does not require adding private document access to SupabaseHub or
community workers. Suggested endpoints/DTOs are `vault_list`, `vault_status`, `vault_search` and
`vault_read(vault_id, document_id, expected_revision)`. Return relative paths, revision, indexing
time, bounded snippets and result limits. Reading the stored text snapshot keeps a search hit and
its citation consistent; if a revision changed, return a clear stale-result response.

Suggested data model:

- `vaults`: ID, display name, host-local source registration, state, completed scan generation.
- `vault_documents`: vault ID, normalized relative path, revision/content hash, title, text,
  source metadata and last-seen generation; unique `(vault_id, relative_path)`.
- An FTS5 index over title/text, with explicit transactional insert/update/delete maintenance.
  Wikilinks and tags can be separate metadata; duplicate note basenames need explicit disambiguation.
- A small source/status record for scan errors and last successful reconciliation. For v1's single
  host source, no per-client copy of the full index is needed.

FTS5 external-content indexes require consistent maintenance; triggers alone do not backfill
pre-existing documents. Include migration/rebuild and index-integrity checks.
[SQLite FTS5 documentation](https://www.sqlite.org/fts5.html)

Do filesystem I/O, hashing and parsing outside the database lock. Commit small batches atomically;
never keep the LocalHub transaction open while recursively walking files. Add bounded background
queues, cancellation and progress reporting. Measure heartbeat latency during indexing before
choosing batch sizes. Start with one database for lifecycle/backup simplicity. If measured
contention warrants a separate derived-index database later, it is a performance boundary, not a
replacement synchronization architecture. Do not enable WAL and assume that alone fixes a single
connection protected by one mutex.

## The watcher is a hint, not the record of truth

A correct indexer needs an initial scan, debounced event processing, and reconciliation after
startup, reconnect, overflow or source remount. Keep a periodic/explicit reconciliation fallback;
“only reindex on events, never scan again” is not robust. Different editors save via different
rename/delete/write sequences, and some network filesystems emit no usable events. A polling
fallback may be needed. [notify's documented limitations](https://docs.rs/notify/latest/notify/)

Read a stable snapshot or retry if a file changes during reading. Handle atomic replace, rename,
case-only rename, deletions, encoding errors and files that temporarily cannot be read. An
unmounted drive or failed/incomplete directory scan is **not** proof that every missing note was
deleted: only sweep unseen documents after a successful complete scan. Keep the last good index
and mark the source unavailable/stale. Exclude symlinks escaping the chosen root, hidden application
state such as `.git`/`.obsidian` by default, oversized notes, and unsupported attachments. These
are explicit v1 limits, not permission to rewrite the member's files.

The Rust watcher sees only the host filesystem. It cannot observe a remote Mac's directory merely
because that Mac paired with LocalHub. The Swift shell must provide durable folder-access/lifecycle
support; platform-specific directory permissions belong in the shell even though indexing logic
lives in Rust. Equivalent Windows/Linux lifecycle and path cases need tests before claiming parity.

## If source folders must live on other fleet machines

Add a **source-agent ingestion protocol**, not full bidirectional file sync:

1. Owner authorizes a source ID/root on a particular paired machine; a worker pairing key alone
   must not grant arbitrary vault administration or ingestion rights.
2. That machine runs the watcher and queues bounded upsert/delete messages with source ID,
   monotonic sequence, content hash and scan generation. Retries must be idempotent.
3. LocalHub acknowledges durable ingestion; reconnect resumes from the last acknowledged cursor.
   An authoritative complete manifest reconciles gaps. Failed scans do not publish mass deletes.
4. One publisher owns a source namespace. If the same folder is already mirrored by another tool,
   index one designated copy rather than have several publishers fight over it.

These messages can reuse LocalHub's authenticated server and SQLite transaction machinery, but
sequence tracking, durable source outbox, manifests, tombstones and conflict rules are all **new**.
A second database by itself supplies none of those features. Avoid this expansion in the smallest
v1 unless hub-local folder access is insufficient for the product requirement.

True multi-writer folder mirroring is a larger separate project: stable file identity, conflicts,
delete propagation, interrupted transfer, retention and recovery. Keep Obsidian Sync/git/another
explicit folder-sync choice responsible for actual file mirroring until Hive deliberately adopts
that responsibility. Do not copy the running SQLite file through a folder-sync service.

## Privacy, offline behavior and implementation gates

Vault membership is the user's private fleet, not the public Hive. Current LocalHub models a
single-owner store plus paired node identities; it is not an account-isolated multi-tenant server.
Add explicit vault-read grants/tool eligibility, source-publish grants and owner-only registration.
Revocation must apply to search as well as reads. Treat note content as untrusted task data, not
instructions that can grant tools or change permissions. Cloud-coordinated tasks need the user's
existing explicit cloud-context choice applied to any retrieved note text; community jobs should
not receive vault tools automatically.

State the availability contract plainly: if the hub is asleep/unreachable, uncached fleet queries
fail with “vault unavailable,” not an empty-result success. An optional offline read cache needs
revision/cursor synchronization, retention, stale indicators and revocation limitations; it is not
part of central-query v1. Hub backups contain private document text as well as pairing state, so
backup/export policy must reflect that broader content.

Suggested implementation order:

1. Confirm central fleet access versus offline replicas/multi-machine editing in ADR-028. Recommend
   central query and hub-accessible folder for v1, documenting the host availability requirement.
2. Add versioned local schema migration and Rust `VaultStore`/search/read with synthetic fixtures.
3. Add watcher plus recovery scans; test rapid save/rename/delete, missing mounts and crash restart.
4. Add authenticated API/grants and two-client tests, including revocation and no cloud fallback.
5. Wire shared configuration/lifecycle through UniFFI, then Swift UI; expose the same Rust API to
   Tauri. Measure indexing contention on active card/heartbeat traffic with a declared corpus.
6. Only if needed, implement remote source ingestion; separately scope offline caches or true file sync.

**Answer to Claude:** it is a clean extension for centrally stored/searchable vault content, with
new watcher/ingestion machinery beside LocalHub. It is not a clean reuse of an existing sync path:
that path does not exist. Amend ADR-028 to distinguish fleet access, ingestion and replication before
implementation. No vault code, schema, watcher or source-folder mutation was made during this review.
