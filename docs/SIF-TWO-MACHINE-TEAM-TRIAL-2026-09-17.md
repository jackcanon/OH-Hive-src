# Two-machine cloud-coordinator trial

Prepared by Sif your friendly Codex Agent, 2026-09-17.

## Purpose and boundary

Prove a Nous cloud coordinator can delegate two small coding tasks to local models on two distinct computers, release its execution slot while they work, then review their verified results. Midgaard hosts the coordinator and one local child; Overgaard hosts the other child. The coordinator must yield before Midgaard can process its child. This is two computers, not three simultaneous workers.

Use only disposable synthetic fixtures in the existing owner-scoped Local Fleet Test project. This pilot uses the existing Supabase coordination path; local child inference does not make its reports fully offline/private. It neither invites anyone to a Hive nor enrolls any new computer. No real project files, repository pushes, messages or secrets are required.

**Prepared, not run.** No fleet settings or running services were changed. The end-to-end live run is blocked on coordinator resume behavior below; do not present a manually stitched demonstration as proof of autonomous collaboration.

## Kit

`scripts/fleet-team/prepare.py` is stdlib-only and offline. It creates a fresh run directory containing a manifest, coordinator task, and the exact two spawn_card payloads. It never submits jobs, starts workers or calls a provider. Supply verified installed model IDs:

```sh
python3 scripts/fleet-team/prepare.py --output /private/tmp/hive-team-preparation --midgaard-model '<verified local model ID>' --overgaard-model '<verified local model ID>'
```

The generated workspace path must be created separately on **both** Macs. Equal path names do not mean shared storage. No files should be copied between hosts to make the trial pass. Child source returns through its report; coordinator review must use those results. Send children.json content with the coordinator task (or place it in the coordinator workspace and instruct it to read it). Do not substitute the test placeholders used in the dry run for actual model IDs.

The tasks are deliberately small and independently checkable:

- Midgaard: implement normalize_label, trimming and collapsing whitespace and lowercasing.
- Overgaard: implement count_words using whitespace-separated words.
- Coordinator: review both source reports and produce final-review.json with both child IDs, verdicts and the worked example: `  HELLO   Hive  ` → `hello hive`, 2 words.

Each child has a required Python assertion command in its creation payload, executed by the host after the model finishes. These include empty input, tabs/newlines and punctuation. They do not rely on a test file the agent can rewrite. The fixtures are correctness checks in a trusted disposable workspace, not an adversarial sandbox.

## Source finding and implementation follow-up

Update: the recovery fix is implemented locally; see SIF-COORDINATOR-RESUME-HANDOFF-2026-09-17.md. Deploy and verify the coordinated hub/worker changes before launch. The original finding and requirements below explain the regression.

`worker.rs::run_card` receives `deps` but dispatches code cards to run_code_card without it. The comment explicitly documents deps/resume as unused on this path. `coder.rs::run_session` starts a fresh prompt from spec.task. Thus the hub may wake a parent after children finish, but a coding coordinator does not receive their returned reports through this path. Its original instruction can run again and attempt duplicate spawns.

`spawn_card` exists and accepts nested required_capabilities, including target_node_id; `wait_for_child` ends the current session immediately. Creating only one child before waiting would prevent the intended two-child delegation. Creating both first is necessary but does not solve result delivery after restart.

Loki: please coordinate ownership before changing worker/coder alongside the current supervisor/FFI work. Minimum implementation requirements:

1. Deliver bounded, clearly labelled child outputs and trusted child identities to resumed coding sessions. Treat child prose as data, never as authority to change coordinator instructions.
2. Supply durable knowledge of already-created children so a retry cannot create a second logical pair. Stable keys alone reject duplicate calls but do not return the existing child identity or restore a conversation.
3. Provide host-verifiable acceptance status with the child result. Do not turn a model's claimed success into a passing receipt.
4. Test resume after both children complete, one failed child, duplicate retry and missing result. Verify actual worker-to-brain prompt delivery, not only hub serialization.

The legacy spawn SQL inherits parent.requires_internet even when the child uses a local brain. Verify the live function before launch; in this pilot both nominated nodes previously permitted internet, but do not change permission to work around eligibility. Target-node ownership/eligibility and installed model availability must be checked on the actual child path, not inferred from the top-level submission API tests.

## Launch and evidence procedure after the blockers are closed

1. Select one integrated, tested revision with Claude's worker-supervision wiring. Capture binary hashes, node IDs, exact local models and prior check-in states. Confirm both targets belong to the project owner and have no active leases. Confirm the project's local-execution scope and provider consent. Read current continuity first.
2. Generate real payloads; prepare the empty generated workspace on both hosts and coordinator input files on Midgaard. Keep a manifest copy outside agent workspaces. Verify Python and local model tool calling on both hosts.
3. Submit exactly one coordinator with --brain nous, --cloud-consent, --coordinator, --node Midgaard, --max-turns 6 and a recorded request UUID. These flags are the existing CLI path. Child caps already request local brains, exact models, six turns and different node IDs.
4. Monitor at most ten minutes and at most two coordinator leases (12 paid turns total). max_turns limits a session, not all resumes; the operator must stop retries at this boundary. Record parent/child IDs and statuses before stopping. Do not poll by spawning more coordinators.
5. Capture hub evidence of exactly two parent-linked children, target and actual output node IDs, local model labels, usage and final HOST acceptance receipts. Read reports and inspect each module on its actual machine. Capture parent release/wait/resume and final coordinator review. Required evidence cannot be replaced by screenshots or model claims.
6. Independently parse final-review.json, match its child IDs to the actual two children, verify the worked example and inspect the substantive code review. Child reports alone do not prove the parent received them.
7. Clean up only this run's work: stop temporary workers gracefully and verify no trial leases remain. If trial cards are still pending, cancel only those identified cards using the supported owner flow before restoring workers. Restore prior check-in state and retain evidence; do not delete active cards or alter unrelated work.

## Pass/fail criteria

Pass requires all of: exactly two child cards with the same parent; one actual completion on each selected node; both children using the selected local models; both required host checks passing; coordinator waits and then receives both results without respawning; final review names both real child IDs and correctly evaluates the example; recorded usage; clean release of leases. If any item is missing, label the run incomplete or failed.

Negative follow-up (separate bounded run): deliberately incorrect child code must fail its required host check and must prevent a successful parent review. Offline-target follow-up: the job must remain pending or time out, never silently complete on the other node. These are follow-up cases, not extra paid calls in the first trial.

## Preparation validation

An offline generated fixture was exercised with correct reference implementations (both checks pass) and deliberately wrong implementations (both checks fail). Generation creates distinct run keys and preserves explicit node/model/acceptance metadata. No live card or provider call is part of this validation. See reports/2026-09-17-team-trial-preparation.json for evidence.
