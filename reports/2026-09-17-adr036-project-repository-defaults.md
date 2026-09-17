# ADR-036: local project repository defaults

Implemented the execution-backend foundation for connecting a GitHub repository to a private project. This is not yet a desktop repository-picker feature.

## Behavior

- Trusted `LocalHubStore` administration exposes `set_project_repository(project, binding)` (including clear) and `project_repository(project)`.
- Binding contains a credential-free GitHub HTTPS clone URL and optional reference. It is metadata only: no OAuth token, installation permission, clone authorization, or repository-visibility assertion.
- SQLite schema 14 adds a foreign-keyed `project_repositories` table. Existing project rows remain unchanged. Older binaries reject a database once it has upgraded to 14; coordinate host upgrades before using this against a real host database.
- `add_card` snapshots the default inside the same transaction that inserts a new code card, then applies normal internet eligibility and card validation. Changing or clearing the binding cannot redirect already-created work.
- Explicit `workspace_path` or `repo_url` wins. A task's explicit `repo_ref` wins over the project default. Text/media cards do not inherit repository configuration.
- This API is trusted local administration, not a newly exposed paired-node HTTP endpoint. No cloud schema or community permission changes.

## Remaining integration

The Swift local idea board persists idea cards independently; it does not represent executable LocalHub projects. Wire a real project selection/creation surface and authenticated owner administration to this API before offering “use repository” in the connector. Do not silently map idea IDs to execution projects.

Child spawning retains its existing explicit-capabilities contract. Do not resolve a mutable project default during child retry: child inheritance should derive from the parent's frozen binding and preserve idempotency. This follow-up is not implemented here.

Private repository Git credentials still require the approved connector-to-Git credential bridge. No private clone, publication, GitHub push/PR, fleet deployment, or app build occurred. Existing per-task worktree receipts remain authoritative for local checkout reuse.

## Verification

Two new regressions cover snapshot preservation after clearing, explicit-folder/repository precedence, internet eligibility, missing projects, credential-bearing/non-GitHub URL rejection, and v13 upgrade with an existing project. Existing older-database migration expectations updated to 14.

Focused tests and full workspace (excluding Tauri) passed. Strict CLI Clippy, formatting and whitespace checks passed. Test/lint output recorded in `/private/tmp/hive-repository-{tests,full,clippy}.log` (format/diff checks run separately).

Sif your friendly Codex Agent
