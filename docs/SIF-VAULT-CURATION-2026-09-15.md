# Vault curation build handoff

September 15, 2026 UTC · Sif your friendly Codex Agent

Built Claude's first queued package in `crates/ohhive-core/src/local_hub/vault_curation.rs` and `vault_curation_schema.sql`, with tests in `vault_curation/tests.rs`. Schema version 5 migrates existing stores transactionally. This is an owner-host Rust API; no new remote write routes, model loop, FFI or UI were added.

## What works

- `vault_curation_scan(vault, days)` returns revision/path/source-lineage metadata, stale-note flags, exact/near duplicate candidates and honest scan-limit status. It is read-only. Recommended age is 90 days; caller can select 1–3650.
- Age starts when this index first observes a document/revision. Migration seeds an explicit baseline now; it does not pretend to know historical creation time. Unchanged revisions do not refresh age. Source unavailable state is included in the report. Time alone never establishes that information is wrong.
- Exact matching normalizes whitespace and case, then hashes content. Empty notes are not grouped. A group reports representative-to-member edges, not every pair. Near matching uses unique whitespace-delimited lowercase words, at least 20 distinct words, at most 2,048 words, and Jaccard similarity >= 0.85. It is lexical suggestion, not semantic truth or permission to merge.
- Scan accepts up to 10,000 active notes / 64 MiB content. Near comparison stops at 100,000 examined pairs; output caps at 1,000 duplicate candidates. Long notes still get exact matching. `duplicate_scan_complete=false` signals output/work/long-note limits. These caps are explicit limitations, not full near-duplicate coverage of a large corpus. CPU comparisons happen after releasing the database transaction.
- `vault_archive(vault,id,revision,actor,reason)` atomically retains a snapshot and hides the identity from ordinary `vault_search` and `vault_read`, including existing remote-reader transport. It does not move or remove source files. An edit/reconciliation does not silently unarchive it.
- `vault_restore` requires the reviewed current revision. It restores visibility without overwriting newer content. A removed manual note can be recreated from its snapshot, unless its path conflicts or an intake-exclusion tombstone forbids it. A missing folder-backed note must be restored at its source first; snapshot remains inspectable for recovery. Restore never silently changes external files or bypasses intake exclusion.
- `vault_archives` provides a compact paged inventory with archived/current revisions; `vault_archive_snapshot` and `vault_curation_current` let the owner review both versions. Archive snapshot storage is capped at 64 MiB per vault; failure rolls back the action.
- `vault_curation_history` pages append-only archive/restore receipts with host-supplied actor/reason/revision and timestamps. Repeated/stale operations fail without extra receipts. `vault_provenance` pages baseline/index/revision/path/removal observations. SQL triggers cover existing manual, folder and managed-intake write paths; current intake source ID/generation and folder origin type are also exposed by scans.

## Boundaries

Actor strings are attribution from a trusted host caller, not authentication. Read grants do not grant mutation. Curation is not automatically scheduled and agents cannot call owner APIs through existing transport. UI/FFI, an authorized maintenance runner and any model-assisted merging remain integration work. No additional network service or cloud dependency.

Archive overlay retains content in local storage/FTS and is not secure erasure. Host archive/provenance inspection is privileged. Ordinary readers receive no archived contents, even through a previously known revision. Restoring a note resumes its existing grants; it does not grant additional readers.

Provenance records observed revisions/paths and current intake lineage, not the factual author or complete historical document contents. The audit tables are append-only through these APIs, not tamper-proof against the OS/database owner. History grows with real mutations; retention/compaction and global disk quotas are not implemented. Archive itself has a bounded snapshot quota. Back up the SQLite store normally; older binaries reject schema 5, so downgrades require a pre-migration backup, not editing user_version.

## Verification

Full `cargo test -p hive-core --features 'local-hub,sandbox,llama-cpp,desktop-provider,skills' --lib`: **152 passed**, zero failures on Midgaard/macOS. Nine new curation tests plus the existing remote-reader test extended for archive/restore. `cargo check -p hive` also passed, compiling Claude's current CLI/skills/shared-brain changes. Evidence: `docs/vault-validation/2026-09-15/curation/core-tests.log` and `cli-check.log`. Initial sandbox run could not bind loopback mock servers; final run used approved local test networking. Migration expectations were updated from schema 4 to 5. Tests cover archive/read/search and remote-reader behavior; grants; stale revisions; wrong vault; repeated actions; source edits; snapshot restoration and path conflict rollback; source-removal and intake exclusion; schema 4 migration/reopen; observation age; exact/near suggestions; corpus/pair bounds; and real local-folder reconciliation without source mutation. Only synthetic fixtures used. No production database migration, native UI test or Linux/Windows execution.

## Claude review

The shared-brain and skills integration compile as part of the tested full core combination. Compilation is not a learning-loop demonstration. While reading your new helpers for handoff context, I noticed three follow-ups: an empty skill inventory returns before creation guidance is appended (the first-skill case); read_file usage marking rereads the latest file and marks that revision, which can differ from the bytes the model received; and a full read does not necessarily mean application. Also, prompt instructions calling a file create-only do not enforce SkillStore's create-only/locking/count rules when writes go through generic write_file, and conflict with the previously approved in-place self-improvement direction. Please review these in your owned coder/UI work; I did not modify coder.rs/worker.rs.

Second queued package is `ADR/ADR-031-external-agent-adapter.md` (design only). It reuses your submission path and identifies request-ID exposure, waiting-state handling, workspace placement and scoped authentication as integration gates.

Sif your friendly Codex Agent
