# Workspace skill-file layer (ADR-027)

2026-09-15 UTC — Sif your friendly Codex Agent

## Delivered

Opt-in `skills` feature exposes `hive_core::skills`. This is a local filesystem/parser layer only: no coder/worker integration, model calls, skill execution, tool grants, UI or cross-machine sync. New files: `crates/ohhive-core/src/skills.rs` and `skills/tests.rs`; feature and module registration in Cargo.toml/lib.rs, dependency lock update.

Layout:

```text
<existing workspace>/.hive/skills/
  build-project/
    SKILL.md
    .last-used.json       # created only when mark_used is called
```

Skill IDs are 1–64 ASCII lowercase letters/digits/hyphens, beginning with a letter. IDs choose directories and are independent of the human-readable frontmatter name. Each skill uses its own SKILL.md; flat `.md` files at the skills root are reported as invalid entries rather than treated as an alternate format. Supporting files inside a skill directory are left untouched and are not loaded by this layer.

## API

- `parse_skill(source)` -> name, normalized one-line description and unchanged procedure body. Requires YAML frontmatter with nonempty string name/description and a nonempty Markdown body. Quoted/folded scalars, CRLF and UTF-8 BOM are supported. Unknown metadata is tolerated but not returned/interpreted. No metadata, including allowed-tools, grants authority.
- `SkillStore::open(workspace)` -> anchored store. Workspace must already exist; creates `.hive/skills` if needed.
- `write_new(id, source)` -> summary/revision. Validates first, writes the exact original bytes, create-only publication; never overwrites an existing skill. Editing remains the existing write_file path per ADR-027, not a new tool in this slice.
- `list()` -> sorted compact summaries plus explicit per-entry issues. Summaries contain id/name/description/revision/last_used_unix_ms, not procedure bodies. Caller/UI must display issues rather than silently hide malformed entries. Excessive top-level counts fail the whole inventory rather than returning a silently truncated list.
- `read(id)` -> summary and full procedure. Reading/listing does not imply use.
- `mark_used(id, revision)` -> records system UTC milliseconds when the caller actually uses the selected procedure, rejecting a stale revision. Caller integration decides when use occurs; this layer does not infer it from file access.

Typical flow:

```rust
use hive_core::skills::SkillStore;
let store = SkillStore::open(workspace)?;
let inventory = store.list()?; // metadata for selection, plus diagnostics
let document = store.read("build-project")?;
// After actual selection/use, separately from merely browsing:
store.mark_used("build-project", &document.summary.revision)?;
```

## Last-used semantics

Sidecar version 1 stores only the source SHA-256 revision and last-used UTC timestamp. This avoids changing procedure mtime or conflating edit time with use time. Timestamps never regress for the same content revision. Missing sidecar means no recorded use. If external write_file changes SKILL.md, the old sidecar no longer matches: last-used becomes None for the new version, and attempting to mark the old revision fails. An identical restored version can legitimately match its prior use record. Malformed sidecars produce inventory issues; metadata is not silently fabricated. No raw procedure is copied into usage metadata.

## Filesystem and limits

- 128 KiB per skill; 16 KiB YAML header; name 128 bytes; normalized description 1,024 bytes.
- 256 non-hidden skill-root entries and 1,024 total root entries (including hidden metadata/lock entries). Resource directories inside a skill are not recursively enumerated. Effective maximum skill text is bounded; external files are still validated at read time.
- 1 KiB usage sidecar.
- cap-std directory-relative access anchors operations to the opened workspace/skill directories. Checked roots/directories/files reject symlinks; capability resolution prevents following an escape outside the anchored directory. Unix reads also reject multi-linked files. No absolute/parent-traversal IDs.
- A cooperative `.write.lock` serializes API reads/listing/writes/usage updates across instances/processes. Busy is an explicit error, never an automatic lock steal. Normal returns drop/remove it. A crashed process may leave a stale lock: host recovery must establish no writer remains before removing it. There is no PID-based or timed automatic recovery in this first slice.
- Skill creation writes and syncs a temporary file, then uses a create-only hard link and removes the temporary name. Filesystems without hard-link support return an error rather than using a partial/non-atomic fallback. Usage metadata uses temp-file rename. This is atomic visibility, not a claim of power-loss durability of directory entries.
- Existing workspace directories must remain under owner control while handles are in use. Cooperative locking does not fence arbitrary external editors/write_file or a malicious same-user process. Bounded reads detect size/mtime changes but are not an adversarial snapshot protocol. A usage sidecar always retains its exact revision, preventing a stale record from being mistaken for a changed version.
- Failed creation may leave an empty skill directory (visible as an inventory issue); no pre-existing directory/content is recursively deleted. No general edit/delete API added.

## Parser dependency

Added optional `serde_yaml_ng` 0.10.0 for real YAML scalar/frontmatter parsing, together with existing optional cap-std/SHA-256 dependencies under the skills feature. This supports YAML 1.1 as documented by its upstream; no claim of complete YAML 1.2 compatibility. Errors are reduced to safe categories and do not echo file content.

Upstream checked: https://github.com/acatton/serde-yaml-ng .

## Verification

- `cargo test -p hive-core --features skills --lib`: **40 passed, 0 failed**.
- `cargo test -p hive-core --features 'local-hub,sandbox,llama-cpp,desktop-provider,skills' --lib`: **139 passed, 0 failed** on Midgaard/macOS.
- Ten new tests cover frontmatter compatibility and rejection, exact byte/count boundaries, file round-trip, honest usage/reopen semantics, stale revisions, create-only protection, malformed-entry diagnostics, lock contention/cleanup, symlink paths/hard-linked content, sidecar failure without partial creation, and public clock-based marking.
- Temporary synthetic workspaces cleaned up. No personal/workspace skill files imported or generated outside the test fixtures. Formatting and `git diff --check` passed. No Linux/Windows run or deployment.

## Claude handoff

Enable the skills feature where needed; enumerate compact summaries before a coding session and load procedures only when chosen. Keep descriptions as skill metadata, not higher-priority permission instructions, and budget aggregate prompt size independently of per-file limits. Record actual use via mark_used. Reuse write_file for automatic self-improvement under the existing workspace scope, while accounting for its bypass of this module's cooperative lock. New automatic learning, settings visibility/delete, stale-lock recovery UX and cross-machine sync remain your separate integration work. Per ADR-027 Decision 5, do not ship hidden automatic learning before the visibility surface is ready.
