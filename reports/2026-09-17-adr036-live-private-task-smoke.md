# Live private coding task smoke — 2026-09-17

Result: PASS for the core private worker path. User authorized creating a test repository. Created private `jackcanon/hive-private-task-smoke` with an initial README. The initial remote/local/base commit was and remains `7174f451b6d2b50c1d1c5e9ddb52ed31d030b713`.

Ran the new opt-in `private_task_smoke` example against the real GitHub repository and local Ollama-compatible endpoint at 127.0.0.1:11434, model `qwen3.5:4b`. The example creates a separate LocalHub SQLite database and enrolled test node, freezes the repository on a staged private task, prepares an authenticated checkout, and runs that exact card through Worker::tick_with_heartbeat using the normal execution permit. It checks review status and exact file contents. Model budget: six turns, with a 180-second stop watchdog. No community client is used.

Task `46d99e9b-6cf9-43c3-9bb5-d959e7200242` reached review and created `hive-smoke.txt` containing `Hive private task smoke passed`. Git status reports only that untracked file. HEAD, preparation receipt base, and GitHub HEAD match. Git config has no persisted extraHeader. No task commit/push/PR occurred.

Artifacts retained at `/private/tmp/hive-live-private-29016936-de04-4eb9-bd09-bbd84e93bc65`; run evidence `/private/tmp/hive-live-smoke-run.log`, build/lint evidence `/private/tmp/hive-live-smoke-{build,clippy}.log`. Example builds and strict Clippy passes. This opt-in live example is not run by ordinary tests.

## Limits and next check

Native app inspection timed out twice. This test uses GitHub CLI authentication passed through stdin to the preparation helper, not the app's connector token. It therefore verifies real private Git download, preparation, scoped LocalHub claiming, local model tools, and output/status, but does NOT verify native buttons, FFI identity/gating, connector installation permissions, or Stop via the UI. No user application database was modified. The app still needs a manual screen-level test: Coding projects → create test project → connect the repository URL → Tasks → save task → Prepare → Run on this Mac. If the GitHub App uses selected repositories, grant it the new test repository first.

Next implementation work: acceptance-check editor and interrupted-task recovery. Fleet targeting and publication remain later steps.

Sif your friendly Codex Agent
