# ADR-036: native project repository controls

The Private Fleet Projects page now contains a Coding projects section, distinct from the existing local idea board. It lists real LocalHub execution projects and can create a project, connect/change/clear its GitHub repository, and choose an optional branch/tag/commit. The editor uses the existing GitHub connector's accessible repository list or a manually entered GitHub HTTPS URL.

## Data path and authority

- Swift `RepositoryProjectsView` → HiveStore → typed UniFFI project methods → LocalHubStore repository defaults.
- Project listing joins projects with optional repository bindings in one SQLite transaction. No community API or Hive membership is involved.
- All three FFI operations hold the Private Fleet selection lock across the operation, including the blocking database work. They run in owned runtime tasks so caller cancellation does not release that lock while a database write continues.
- A selected remote primary causes an explicit error before any local store is opened. There is no local fallback. Remote project administration is not implemented in this slice.
- Local administration requires this computer's verified Private Fleet identity. No caller-supplied owner/member identity is accepted.
- The sheet edits a snapshot, persists only on Save/Disconnect, and prevents duplicate submissions during writes. Errors remain visible. Creating a project and assigning its repository are separate explicit actions.

## Scope

These are executable-project records, not automatic conversion of idea-board cards. There is no new task-submit control in this slice. The existing add_card path snapshots repository defaults at task creation; existing tasks remain unchanged.

Repository selection does not grant worker Git credentials. Private clone/fetch still needs the connector-to-Git credential bridge. The interface states this limitation; it does not claim cloning or publishing succeeded. No child inheritance, push/PR/merge, remote project editing or fleet rollout was added.

Opening the new binary uses schema 14 introduced by 9d2d257. Older host binaries cannot reopen that upgraded database. No live host database was opened for this verification.

## Verification

Core repository regressions include joined project listing and stored binding round trip. Rust FFI check and native app build validate the typed bridge and Swift controls. Logs: /private/tmp/hive-project-controls-{tests,ffi,clippy,build}.log. See continuity entry for final check outcomes. Interactive UI and live private-repository Git execution are not claimed.

Sif your friendly Codex Agent
