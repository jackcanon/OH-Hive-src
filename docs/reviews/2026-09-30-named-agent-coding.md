# Named local agents on private coding tasks

2026-09-30 — Sif your friendly Codex Agent

## Delivered behavior

The project task screen now offers agents assigned to the selected execution computer. Selecting one stages an immutable snapshot of its name, biography, instructions and revision alongside the task. The model receives that context through the existing coding task prompt, and the result row shows the agent name accepted at submission.

The owner still explicitly prepares and runs the task. Selection is not a role-based permission grant: the existing coding checkout and command protections apply, while Library and delegation permissions are not copied from chat policy. No template is enforced. “Use task instructions only” preserves the original unnamed path.

## Boundaries and recovery

- The authority accepts only an active local agent owned by the verified fleet and assigned to the selected execution host. Cloud, other-owner, missing and wrong-host agents are rejected.
- Repeated request identities return the original staged task, even after biography changes. A changed request payload conflicts rather than duplicating work.
- Deleting or reassigning the selected agent blocks preparation/run preflight and claiming. Assignment is checked in the lease transaction as well as before model contact. Identity context is frozen; later harmless biography edits do not reinterpret existing work.
- The snapshot contains no avatar or credential. No new database migration is required.
- Omitted agent identity is omitted from serialized receipts, preserving exact receipt comparisons for older tasks. The old native submission entry point remains available; a new explicit selection entry point handles named agents. Matching native bindings must be generated with the new library.

## Verification

- Nine private-task tests passed: named identity/ownership/runtime/host validation, biography freezing, archive/reassignment lease rejection, legacy receipt omission, immutable submissions and checkout protection.
- Nine remote-path tests passed: named instructions reach a mock model; archive blocks preflight before model contact; remote checkout recovery and task-specific stop remain functional. These sets overlap.
- Native Rust bridge built, and matching Swift bindings were generated.
- Final Swift build result is recorded in the continuity entry accompanying this change.
- No actual model inference, fleet mutation, installed-app update or live autonomous project occurred. Tests used temporary stores/checkouts and mock services.

## Next work

This is named-agent task execution, not the complete team chain. The remote private worker still handles one explicit prepared run; child-card preparation, dispatch and coordinator resumption must be connected before team mode is exposed. Bots cannot yet submit a project task directly from conversation. Research report persistence and independent review remain separate completion gates.

The broader baseline assessment is `2026-09-30-den-friday-readiness.md`. Keep the shared checkout’s unrelated branding work out of this branch. Begin live dogfooding on Niflheim or another approved execution computer after the matching hub/app changes are reviewed and deployed.
