# Model-label live verification — 2026-09-17

Passed: card `a8d3927a-a12d-4fea-99f0-eb4662c54b17` reached review with host acceptance passed. Request omitted model, selected Nous, targeted Midgaard, and limited execution to three turns. Persisted output independently verified in production: model_id `anthropic/claude-sonnet-4.6`, node_id `f1cc4f2b-9820-4955-9912-449e2f52d93c`, usage 5,822 input / 326 output tokens. File bytes independently match `HIVE MODEL LABEL PASSED` plus newline. This closes the reporting bug found in the earlier live test.

Production was already at code-brain-turn v8 at preflight, ahead of the continuity log's v7 snapshot. Downloaded it to /private/tmp/hive-model-v8-check and compared all three deployed source files byte-for-byte with the tested source: identical. No repeat deployment or production schema change was made.

Used the previously verified 9813b40 release worker (SHA-256 in JSON evidence), not a rebuild including Claude's subsequent supervisor work. Temporarily checked Midgaard in, completed one card, stopped the worker with Ctrl-C, independently confirmed checked_out and no lease. No remote installed workers, app process, or persistent settings changed. First submission failed local argument parsing (file path instead of inline check JSON); corrected before any provider call or card creation. Only one live card created.

Claude's subsequent commits 548db35 and 4ff3e12 introduce supervision and last-seen work; 548db35 explicitly notes FFI/Swift wiring is still a follow-up. Coordinate the next fleet build with that work instead of silently replacing it with the earlier tested binary. Remaining rollout: validate the chosen integrated worker revision, install matching builds on fleet nodes, then run a two-machine team exercise. No claim that fleet rollout has happened.

Evidence: reports/2026-09-17-model-label-live.json. Temporary logs /private/tmp/hive-model-live-{worker.log,await.json,await-status.log,db.json}; workspace /private/tmp/hive-model-live-20260917.

Sif your friendly Codex Agent
