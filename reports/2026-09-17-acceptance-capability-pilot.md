# Acceptance-capability rollout pilot — PASS

Sif your friendly Codex Agent, 2026-09-17

Validated commit 66738c0. Dedicated SQL fixture passes old/new workers, ungated jobs, malformed input and strict boolean capability. Full 108-migration replay passes, including targeted jobs waiting for acceptance-capable workers. Three capability tests pass with local-hub,sandbox,llama-cpp,bots enabled. Initial bare-core test invocation failed because existing worker tests import feature-gated hub dependencies; corrected feature selection passed. Release CLI build passed after waiting for Claude's FFI build lock.

## Installed on Overgaard only

Architecture arm64 confirmed; no active lease before stop. Staged binary SHA-256 matched local build before installation:

e43c2a951a98e8331fdf432967479280e1cb6726e086ad10f039d70d3f5f5752

Prior SHA-256: 05a882fa0cd3a932aea8a87d11008c7e9e09d9ed47ed7ea2598fe6ef04ba27a2.
Backup: /Users/jack/.local/bin/hive.bak-preAcceptanceCapability-20260917.
Installed: /Users/jack/.local/bin/hive.
Original launchd service booted out and restored using a cleanup trap. No persistent configuration changed. Production subsequently reports acceptance=true and checked_in for ce95c14d-db8d-4a88-90c8-277c067fc57c.

## Proving job

Card ebac5358-7ab8-4f3e-a4fa-cef154ed7cfb in Local Fleet Test, explicitly targeted Overgaard, local mistral-small3.2:24b, max four turns. Required Python assertion checks exact newly written result.txt bytes. Completed to review with a passed host receipt; independently read the remote file and verified CAPABILITY VERIFIED followed by newline. Zero node leases afterwards. Machine-readable database evidence is the adjacent JSON report. No cloud provider used.

## Rollout boundary

Only Overgaard was updated by this turn. Migration 20260917180000 was NOT deployed; Claude explicitly requires workers to advertise the capability before gate activation. Remaining eligible workers, including the running Midgaard app, need matching updates and live capability verification first. Do not infer their readiness from source or local app rebuilds. Existing binary backup supports rollback, but if rolling back after gate activation that node will become ineligible for checked jobs until upgraded again.

No remote git push, database deployment, or changes to Claude's concurrent app/report work. Logs: /private/tmp/hive-capability-{build.log,tests-supported.log,replay.log,pilot-await.log}.
