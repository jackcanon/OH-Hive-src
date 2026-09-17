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

Commands, acceptance checks, and git subprocesses now own their process trees.
On macOS/Linux, a separate process group is created before exec and killed on
timeout, cancellation, error, or completion (including leftover background children).
On Windows, the process starts suspended, joins a kill-on-close Job Object, then
resumes; assignment failure stops the command before it can run. Reader tasks abort
on drop. This is cleanup, not a sandbox: Unix programs that deliberately detach into
a different session/group can escape this ownership mechanism. macOS subprocess tests
exercise timeout/cancellation cleanup and retained output; the Windows/Linux module
is cross-compiled, with native Windows/Linux runtime verification still required.

Coding turns now carry provider-reported Usage through the session, ToolOutcome,
completion RPC, and completed-event token count. Counts accumulate over tool calls and
final text, including turn-limit stops. Cloud spend enforcement remains server-side;
this does not invent token estimates or change honey rates. Operational failures with
no completed session still rely on the server's cloud meter for billing evidence.

Tests use real direct Python subprocesses on Unix, controlled model replies, and a
real LocalHub database. Both positive and negative terminal decisions are tested;
the negative card reaches blocked via fail_card even when the model claims success.
These tests do not consume cloud credits or deploy a fleet worker.

Submission is wired through the deployed acceptance RPC and `scripts/cloud_card.py`.
The Rust CLI also accepts repeatable flags:

```sh
hive card submit --project "My Project" --workspace /path/on/worker --task "Implement the change" \
  --check 'tests=cargo test --quiet' \
  --check-advisory 'format=cargo fmt --check' \
  --check-json '{"name":"script","command":"python3","args":["test with spaces.py"],"cwd":"tests","expect_exit":0,"required":true}'
```

Shorthand splits `NAME=PROGRAM ARG...` on whitespace after the first equals sign;
it does not interpret inner quotes, shell operators, or variables. Use JSON for exact
argument boundaries, a working subdirectory, or a nonzero expected exit. Checks run
on the claiming worker, inside its workspace. Across flag types, required shorthand
checks run first, then advisory shorthand, then JSON checks, matching the Python
harness. Repeat one flag type when order matters. No flags preserves the legacy RPC
request without `p_acceptance`. Invalid checks and the combined 16-check limit are
validated before project lookup/submission; the worker uses the same bounds.

Claude's live pair (`7bfce362` and `57643ff2`) verified passing/review and failing/blocked
outcomes on production. CLI parsing and HTTP transport tests run locally with dummy
credentials; they do not create live jobs or consume provider credits.

Sif your friendly Codex Agent

Explicit worker selection: `hive card submit ... --node NODE_UUID` (also supported by
`cloud_card.py --node`). Migration `20260917041000_code_target_node.sql` adds the
14-argument endpoint, verifies that the target belongs to the project owner, includes
the target in request-id equality, filters normal claiming, and guards all lease writers.
Offline or ineligible selected nodes leave the job ready; there is no fallback. Existing
untargeted requests keep their 12/13-argument behavior. The fully local hub already
honors `target_node_id`. The new migration must be deployed before using this flag
against a production server that lacks the 14-argument endpoint.
