# ADR-036: durable task checkout foundation

Sif your friendly Codex Agent — 2026-09-17

## Implemented

Repository-based coding sessions no longer delete and reclone their per-card directory on retry. New checkouts create a dedicated `hive/<full-card-UUID>` branch from the requested reference (or cloned HEAD). A versioned receipt records the card, requested repository/reference and task branch in `code-workspace-state/<card>.json`, outside the working checkout. Existing local Git credentials remain the authentication mechanism; no GitHub token wiring in this slice.

A successful retry reuses the checkout without fetching, resetting, checking out another revision, staging or committing. Existing commits, dirty tracked files and untracked files survive even when the source repository is unavailable. This preserves files, not the previous model conversation. A session-wide OS file lock prevents concurrent sessions for the same managed card on this machine; different cards remain independent. The lock file is retained and the OS releases its lock on exit/crash.

Unknown legacy clones, interrupted setup without a completed receipt, identity/reference mismatches, missing Git directories, detached/changed branches and managed-path symlinks fail with recovery diagnostics. They are never automatically deleted or overwritten. Receipt publication follows successful clone/branch setup; an interruption before publication deliberately requires inspection. A new card can be used for a fresh clone while the old directory is retained. There is no automated adoption UI yet.

Explicit `workspace_path` remains an imported folder with its previous behavior; the managed-card branch/lock policy does not silently take ownership of it.

## Evidence

Four real local-Git regression tests cover dirty/committed/untracked preservation with source unavailable, same-task lock exclusion and release, changed identity/branch and missing receipt, distinct cards, imported folders, and symlink preservation (Unix-specific). Strict CLI Clippy passes. Full workspace regression results are recorded in continuity after completion. No live repository, cloud model, push or fleet installation needed for these tests.

## Remaining ADR-036 work

This is a bounded G0 foundation, not the complete workspace/publication design. It still uses one clone per card, not a shared repository cache with Git worktrees. Still needed: exact-base/provenance and publication journal, recovery/adoption UI, imported-workspace ownership policy, fleet fencing and transfer, repository-to-project binding, secure GitHub credential integration, typed commit/push/PR operations, verified remote receipts and explicit merge approval.

The receipt is local recovery metadata, not a security boundary against an agent running arbitrary commands as the same OS user, nor proof of the current remote configuration. No branch publication is enabled here. No claim of cross-machine locking or full conversation replay. No production rollout or main push in this slice.

Files: crates/ohhive-core/src/coder/workspace.rs and coder.rs. Existing data directory layout `code-workspaces/<card>` retained. Existing unrecorded clones now stop safely instead of being destroyed. Source tests use std::fs file locks supported by the repository's Rust toolchain; native Windows execution remains a CI check, not claimed from macOS.
