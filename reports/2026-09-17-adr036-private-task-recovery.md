# Explicit private coding-task recovery

The primary-Mac task screen now offers Prepare retry for blocked/interrupted prepared tasks. Confirmation explains that files and saved checks are retained, the model starts a fresh attempt rather than resuming its conversation, and side effects may repeat. Successful recovery returns the task to ready; the owner must separately choose Run.

Core recovery derives the target identity in FFI and holds the primary-selection gate. It accepts only blocked tasks past preparation or running tasks with no live lease. It verifies the owner-staged receipt, active enrolled node, frozen target and canonical prepared checkout. The existing authenticated preparation helper receives no token and requires the saved checkout/receipt, so missing work cannot silently clone/reset. It acquires the existing workspace lock while validating reuse, drops it before ready activation, then rechecks card contents and eligibility transactionally.

No result, child work or checkpoint may exist. Live leases are never stolen. Only the selected expired lease is removed on successful recovery. The exact task JSON remains unchanged, retaining repository, task, model, turn budget and checks. A private_task_retry activity entry records the target node, previous status/reason and task identity in the same transaction as ready activation. No files, acceptance rules or prior activity are removed.

Verification extends the real-Git private-preparation regression with live-lease rejection, wrong-node rejection, missing-receipt rejection that leaves blocked status, dirty-file preservation, byte-identical task payload, ready-state retry rejection, expired-lease recovery, audit count and review-state rejection. Focused preparation tests and strict FFI Clippy pass. Broader LocalHub suite and native build outcomes recorded in continuity. Evidence /private/tmp/hive-retry-{tests,localhub,clippy,build}.log.

Limits: no forced termination, immediate takeover of a still-valid crashed lease, child/coordinator recovery, conversation checkpoint replay or partial-clone repair. The UI does not promise exactly-once external effects. Inspect files and previous actions before retrying. Native interaction remains manually unverified; no live model task was run here. No deployment, source push or community queue changes.

Sif your friendly Codex Agent
