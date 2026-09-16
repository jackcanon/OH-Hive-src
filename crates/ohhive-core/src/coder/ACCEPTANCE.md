# Executable acceptance checks

A code card's `required_capabilities.acceptance` is an optional array of explicit
checks: `name`, `command`, optional `args`/`cwd`/`expect_exit`/`required`. Defaults
are empty args, workspace root, exit 0, and required. Older cards omit the array
and remain runnable with an explicit `unverified` receipt. Unknown check fields
(including `shell`) are rejected. No checks are inferred from project files.

Checks run only after the brain finishes with text. Turn-limit, expired-lease and
child-wait stops skip them. Declared checks appear in the system prompt with cwd,
expected exit and required/advisory status. The host uses the same direct subprocess
capture implementation as run_command, with a 15-minute per-check ceiling further
limited to the remaining lease. Progress and result events are posted per check.
Each output stream retains its last 4096 bytes rather than its beginning. At most
16 checks are accepted; check names and command arguments are bounded.

A required nonmatching exit or timeout fails acceptance. Spawn/cwd failures are
reported as errored. Advisory failures, including launch errors, remain in the
receipt without blocking completion. Successful checks are separate from unverified
and skipped outcomes. Structured results are included in ToolOutcome.data, and a
JSON acceptance receipt is appended to its summary because complete_card/fail_card
persist report text. Worker finalization invokes fail_card for required failures or
errors, keeping the existing child-pause and lease-expiry handling ahead of it.
No model retry, judge model, shell wrapper, or inferred command is introduced.

Known inherited limitation: kill_on_drop/start_kill owns the immediate child, not
its descendants. Reader drain is bounded, but a grandchild retaining pipes can
outlive a timed-out command and prevent output drain. This is not process-tree
cleanup or a sandbox. Descendant cleanup remains a separate platform task.

Tests use real direct Python subprocesses on Unix, controlled model replies, and a
real LocalHub database. Both positive and negative terminal decisions are tested;
the negative card reaches blocked via fail_card even when the model claims success.
These tests do not consume cloud credits or deploy a fleet worker.

Live integration follow-up: scripts/cloud_card.py currently exposes no acceptance
parameter, and the code-session creation RPC builds required_capabilities without
this array. The harness/RPC submission path needs to carry explicit checks before
using that harness as live acceptance evidence. No migration/deployment or live
cloud-card run is included in this implementation.

Sif your friendly Codex Agent
