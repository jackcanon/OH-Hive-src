# ADR-036: primary-Mac private coding task controls

The Coding projects section now offers Tasks. Users can save a task against the project's frozen repository, prepare a private checkout with their GitHub connector, and explicitly run the selected task on the primary Mac's local model. A bounded result preview, workspace path, status refresh and Stop control are included. Saving or preparing does not run the model. Nothing commits, pushes or opens a PR automatically.

## Execution contract

Typed FFI calls derive the node/key from verified Private Fleet identity. They reject a selected remote primary; callers cannot supply a target identity, repository URL, capabilities JSON or cloud provider. Mutations share the existing primary-selection gate, which stays held during execution; status polling remains available. Preparation passes the connector token only through the existing Git preparation path, never into the card or model. Completed checkout recovery can run without a token.

The runner creates a fresh private LocalHub session, checks local model availability and coding-tool permission, and scopes claims to the selected prepared task. It uses the existing OS-user execution permit, coding worker, workspace receipt/lock and heartbeat machinery. Stop signals the worker and waits for its existing lease-release path. A busy execution slot leaves the task queued. No community HubClient is constructed by this runner.

## Verification

Full workspace excluding Tauri: 368 passed, 2 ignored. Strict combined CLI/FFI Clippy, formatting and diff checks pass. New regression checks selected-card-only dispatch, missing-card no-op, and staged-task status isolation by project/node. Existing workspace tests cover worker heartbeat, stop/release and checkout ownership/recovery. Logs: `/private/tmp/hive-private-run-{tests,clippy,fmt,build}.log`.

Native release build, regenerated Swift bindings, app assembly/signing and isolated bundle engine-load probe passed. No live GitHub download or model execution through the new screen was performed. The full workspace included concurrent Bots transport changes; their missing DateTime qualification was repaired without taking ownership of those files.

## Remaining work

- A deliberate live task through the screen, including stop and output review.
- Acceptance-check entry (new tasks currently have no checks and results must be reviewed).
- Recovery controls for interrupted or failed tasks; no automatic requeue added.
- Remote worker targeting, credential delegation and fleet execution. This surface is primary-Mac-only.
- Child/dependency orchestration: selecting one task does not drain the remaining queue.
- Commit/push/PR publication journal, exact-commit verification and Integrator workflow.
- Empty-repository bootstrap remains unsupported by the preparation layer.

Sif your friendly Codex Agent
