# Live acceptance/retry validation and Projects setup guidance

User screenshot showed the correct Private Fleet Projects screen, with `Sign in to Private Fleet on this computer before creating projects` wrapped in a raw FFI error. This is an enrollment prerequisite, not a missing check editor. Added Open Private Fleet setup alongside project-load errors, reusing the existing enrollment and primary-selection views in a sheet. Dismissal refreshes projects. Errors now display their message without the Failed(message:) wrapper. Identity checks remain unchanged. Users with a remote primary are told coding projects must be managed there. The user was directed to Settings → Private Fleet for the current app; the new shortcut requires relaunch.

Native build, matching bindings, signing and isolated engine-load probe passed (/private/tmp/hive-project-setup-build.log). Native app inspection again timed out, and user enrollment remains outstanding; no screen-level or app-connector test claimed.

Extended the opt-in private_task_smoke example with --retry-check (Unix /bin/test fixture). It stages one required check, proves an actual exit-1 failed acceptance receipt blocks the first local-model attempt, creates an owner-controlled prerequisite file, explicitly retries without changing saved checks, verifies existing output survives recovery, and proves exit-0 passed acceptance reaches review on the second attempt. Each model attempt has a 180-second watchdog.

Live run PASS using real private jackcanon/hive-private-task-smoke GitHub checkout and qwen3.5:4b on local endpoint. Task 7a607778-901c-4eb7-943c-a2345f1cdc1c; separate DB/artifacts /private/tmp/hive-live-retry-3069dca3-f05f-4e57-be56-0f5d62dc1cf3. One retry audit entry; host check receipts exactly (exit 1, false), (exit 0, true). Git HEAD remains 7174f451b6d2b50c1d1c5e9ddb52ed31d030b713; only hive-smoke.txt and owner-created smoke-owner-ready are untracked. Model tool log includes file writes and simple test -f commands. No task commit/push or application database changes. GitHub CLI credentials delivered through stdin; app connector was not exercised.

Initial fixture used nonexistent /usr/bin/test on this Mac and correctly remained blocked with a command error. Corrected to verified /bin/test, added explicit receipt exit/status assertions, then reran against a fresh isolated database. Failed-run evidence retained at /private/tmp/hive-live-retry-initial-fixture-error.log and /private/tmp/hive-live-retry-41e00c07-3622-411b-8b8d-d7d6fb0900ca. Final logs /private/tmp/hive-live-retry-{run,build,clippy}.log; strict example Clippy and diff checks pass.

Next: finish user Private Fleet enrollment and native suggested-checks/Run/retry test. Remote fleet and publication are still separate follow-ups.

Sif your friendly Codex Agent
