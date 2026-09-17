# ADR-036 shared repository cache and task worktrees

Sif your friendly Codex Agent — 2026-09-17

## Behavior

New repo_url tasks now share a no-checkout Git repository at code-repositories/<SHA-256 of exact requested URL>, under the node data directory. Each card gets a separate linked worktree at the existing code-workspaces/<card> location and its own hive/<full-card-UUID> branch. Cached Git objects are reused; task files and indexes are separate.

Cache preparation holds a short OS lock, checks the cached origin, fetches for a new task and refreshes the remote default-branch pointer. Branch references resolve through fetched remote branches; tags/commit references resolve to a commit. The task receipt records that exact initial commit and canonical cache location. Task setup uses the resolved commit rather than a moving branch name.

An existing task retry does not fetch or change its branch/commit/files. It verifies recorded identity and Git common-directory ownership, then resumes under its session-wide card lock. Managed standalone clones from the previous implementation remain supported without moving or deleting them. Imported workspace_path behavior remains unchanged.

No destructive cache repair, checkout cleanup, automatic adoption or pruning. Interrupted caches/worktrees without completed metadata remain for inspection. Busy cache preparation returns an actionable retry error rather than mutating concurrently; it does not implement an automatic scheduling retry. Cache keys are exact URL strings: URL aliases can create separate caches. This cache is local to one node/user, not a distributed filesystem.

## Verification

Seven real-Git regressions cover dirty/committed/untracked offline retry, per-task locks, changed metadata/branch and partial recovery, independent tasks, imports, shared common directory with new upstream commits, legacy standalone-clone compatibility, explicit branch/tag references, default-branch changes, and Unix symlink protection. Initial regression exposed macOS /var versus /private/var canonical-path comparison; corrected before final verification. Full suite and strict CLI Clippy results recorded in continuity.

Bounded Git stdout is now returned for identity/ref resolution. Truncated, unavailable or non-UTF8 results cannot silently become receipt metadata. sandbox enables the existing optional SHA-256 dependency; no new package version added.

## Boundaries and next work

No GitHub credential bridge, project repository picker binding, commits/pushes/PRs, automatic merging or repository cleanup. The receipt records initial base, not a complete publication journal. Same-user arbitrary agent commands can reach shared Git metadata: this is workflow isolation, not a security sandbox. Fleet fencing/transfer and publisher authority remain separate work. No Windows runtime proof from this macOS run. No remote push or fleet rollout.

Next useful slice: expose durable task workspace/base information and bind selected repositories to projects, followed by typed verified publication. Keep acceptance tied to the exact published revision when that is implemented; these worktrees alone do not establish that guarantee.
