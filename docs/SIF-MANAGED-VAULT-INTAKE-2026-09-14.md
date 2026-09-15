# Managed library intake — phase 1

Date: 2026-09-14
Author: Sif your friendly Codex Agent

## Delivered

Owner-local `LocalHubStore::vault_intake(vault, &IntakeItem)` and a runnable developer adapter (`examples/vault_intake.rs`). Each approved source becomes one generated Markdown document, using existing `vault_documents`, FTS5 triggers and reader grants. Source identity is stable within its destination vault. No third-party editor dependency, new search engine, cloud calls, model writes, UI or source-folder writes.

Filing is deterministic: `Intake/<project-or-inbox>/<type>/<stable-id>.md`. A trusted source adapter supplies an optional project slug. Explicit first headings such as `# Decision`, `# Test report`, `# Research` or `# Procedure` select a type; everything else stays in notes. This is a conservative first pass, not intelligent topic discovery or semantic classification.

The generated document includes source identity, generation, label and SHA-256; up to three exact prose excerpts of at most 200 Unicode characters each with original line numbers; and the complete original Markdown. These are extractive previews, clearly labeled as not independently verified, rather than model-written factual conclusions. Fenced code and headings are omitted from previews, but approved original content remains searchable. Code-fence skipping is a simple heuristic, not a full Markdown parser or security filter.

## Authority and lifecycle

Only host-local owner code can submit intake; no HTTP write route, coder tool, FFI or UI action was added. The adapter must approve the entire Knowledge input for indexing, including title and source label. It must persist a stable source UUID and monotonically increasing positive generation. Inputs are data and cannot change permissions. Different vaults get different document IDs even for the same source ID; summaries and originals share the existing vault-level read boundary.

An atomic transaction publishes document, FTS changes and receipt. Same-generation exact retries do not rewrite documents; conflicting or older deliveries are rejected. A small schema-4 `vault_intake` table stores generations and fingerprints, reusing the existing document/index schema for all content. Excluded sources accept no text payload, remove an existing unchanged managed document from ordinary reads/search, and retain a tombstone to reject older deliveries after restart. Re-admission requires a newer owner-approved Knowledge generation.

If a managed document was edited or removed outside intake, automatic replacement and exclusion stop with a review-required error. This protects human work rather than silently inventing phase-2 overwrite policy. If the conflict concerns sensitive content, the owner should disable the affected vault/read grants until resolved. Exclusion is index removal, not secure erasure from SQLite pages or backups. There is no secret detector or credential storage here; adapters must exclude sensitive sources before submission. Never send credentials as Knowledge and expect automatic filtering.

Limits: 1 MiB source Markdown per item, 512-byte labels, 80-byte ASCII project slug, 10,000 source receipts per vault including exclusions, 64 MiB indexed document content per destination vault on intake publication. Corpus limits include generated headers/excerpts and existing documents. Processing is outside the database lock except for bounded transactional validation/publication. Content-equivalent distinct sources remain distinct to preserve provenance; unchanged content with a newer generation updates provenance.

Use a separate managed vault from an attached folder vault. The latter remains authoritative external state and rejects intake writes. Intake itself never changes availability or reader grants. Existing source folders and curated documents remain in place. No physical Markdown export/mirroring was added: the generated Markdown lives in the existing SQLite document store, with its logical path.

## Developer use

Build:

```sh
cargo build -p hive-core --features local-hub --example vault_intake
```

Send one JSON item on stdin to:

```sh
target/debug/examples/vault_intake /path/to/disposable-hub.sqlite3 new
```

Use the returned vault UUID instead of `new` for later deliveries. Example input:

```json
{
  "source_id": "c6058e83-60b6-49f5-a094-acb38992050a",
  "generation": 1,
  "content": {
    "disposition": "knowledge",
    "title": "Halo test findings",
    "source_label": "Approved test report",
    "project": "halo",
    "markdown": "# Test report\n\nMeasured throughput was 12 tokens per second."
  }
}
```

Exclude that source with generation 2 and `"content": {"disposition":"excluded"}`. The adapter prints IDs/status only. Explicit reader grants are still required to search through the existing reader API. Run one host process per database; this standalone example reopens manual vaults and must not be used against a live desktop store concurrently. Invalid intake in `new` mode can leave an empty vault; the receipt is never partially published.

## Verification

- Full actual worker feature set: `cargo test -p hive-core --features 'local-hub,sandbox,llama-cpp' --lib`: **100 passed, 0 failed** on Midgaard/macOS.
- Nine new intake tests cover source preservation, filing, grant enforcement/revocation, exact-retry no-op auditing, changed FTS content and stale revisions, exclusion/restart tombstones, strict excluded-payload parsing, human-edit protection, bounds/path rejection, folder-vault isolation, scoped identities, Unicode preview bounds, schema migration, concurrent duplicate delivery, availability preservation, and rollback when receipt publication fails.
- Developer example built with `local-hub`; it emitted one existing `spawned_card_id` dead-code warning in worker.rs under that narrower feature set.
- Real CLI subprocess checks across restarts verified creation, filing, source excerpts, FTS, retries, exclusion and stale replay rejection. Temporary synthetic database removed.
- `git diff --check` passed. No Linux/Windows run, real library import or deployment.

The first test run caught Serde accepting unknown fields on a unit Excluded enum variant; changed it to a strict empty-struct variant and verified rejection. No weakening of the test.

## Claude handoff

Review the intake source-generation contract and schema-4 migration first. Older schema-3 binaries reject schema-4 databases; coordinate the next build rather than opening a live store with this example. No migration was run against production data.

This completes the scoped Rust foundation with a working host adapter. Source connectors still need to submit approved source events and persist their generations; no continuous background watcher, durable producer queue, scheduled curator or source discovery was introduced. Phase 2 remains your separate agent-write/review/undo design. Phase 3 remains the editor. Semantic retrieval, model classification, topic pages, secret handling, general document history/undo and broad duplicate merging remain future work. No coder/worker/Swift/FFI files changed in this task.
