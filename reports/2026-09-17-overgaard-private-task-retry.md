# Private coding and acceptance retry on Overgaard — PASS

Following Jack's direction to protect Midgaard RAM, ran the complete isolated backend test on Overgaard: authenticated private Git checkout, model inference, agent tools, acceptance checks and retry. No model inference ran on Midgaard. Existing app projects and the failed Control Surface task were not modified.

Overgaard reported hostname Overgaard.localdomain, arm64, 38,654,705,664 bytes RAM. Its already-loaded qwen3.6:latest advertises tools. Read-only community lease preflight found zero leases. No worker service restart, global model-setting change or model download. Built current private_task_smoke example on Midgaard, copied it to /private/tmp on Overgaard; SHA-256 matched f473ba635bf5bb254d8fbad7b4310a2308674139c0fa9dbc4b2c51ed991eb169. GitHub authentication supplied through encrypted SSH stdin, not arguments/logs/task records.

Task ced29913-78e4-4cf9-8e5a-748fd296c913, repository jackcanon/hive-private-task-smoke, model qwen3.6:latest. First attempt wrote the exact smoke marker but the required /bin/test -f smoke-owner-ready check failed. Fixture then supplied that prerequisite, explicit retry preserved files and check configuration, second attempt reached review with an independently read passed receipt (exit 0). One private_task_retry audit record. Checkout has only two untracked fixture files: hive-smoke.txt and smoke-owner-ready. No commit, push or PR created by the test.

Remote artifacts: /private/tmp/hive-overgaard-private-8ff0ed3d-cc35-4b49-930a-fc83917b5ad9 (separate smoke.sqlite and checkout). Local run/build logs: /private/tmp/hive-overgaard-private-run.log and /private/tmp/hive-overgaard-smoke-build.log. Harness uses isolated enroll_owner developer identity; this test does not verify GUI account enrollment on Overgaard.

## Remaining product gap

Native private_job_context rejects a selected remote primary and derives the target from the current local node. Run on this Mac is therefore still local execution; neither this SSH test nor pointing inference at another server implements desktop remote task dispatch. Next product work should provide explicit execution-host choice, authorized private job transport and host-owned checkout/token access, status/receipt/cancel/retry routing, and compatible installed-model selection with a preflight tool-support check. Never silently fall back to Midgaard. Remote execution must use Private Fleet authorization, not community Hive RPCs or an invitation workaround. Define the bounded protocol and tests before extending these authority-sensitive operations.

Sif your friendly Codex Agent
