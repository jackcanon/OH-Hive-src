# Targeted Nous live test — 2026-09-17

Passed: card `69ac83c5-4bb1-4bcc-9a71-4fe55569bc92` reached review with a passing host acceptance receipt. Submitted through the rebuilt Rust CLI, explicitly targeted to Midgaard (`f1cc4f2b-9820-4955-9912-449e2f52d93c`), using Nous, cloud consent and a three-turn limit. Request ID `7d570ac9-0936-42ca-9991-d5f5d82c1727`.

Task: write `result.txt` containing exactly `HIVE LIVE CHECK PASSED` followed by a newline. The host ran an independent Python file-content assertion; separate filesystem inspection confirmed it too. Workspace `/private/tmp/hive-live-target-20260917`. SHA-256 `77c61cf14ab84ffc2ae203babc53f7a83c3d830c22503013a0eda953f1aa3643`.

Production card output identifies Midgaard, matching the declared target. Card usage is 5,822 input / 315 output tokens. Three Nous meter rows between 14:30:55 and 14:31:09 UTC sum to those exact totals and an estimated **$0.022191**. All record `anthropic/claude-sonnet-4.6`, the existing Nous default. This is the server's configured-price estimate, not an invoice. The meter has no card-id column; attribution uses the isolated test window and exact matching token totals.

Preflight: no ready/running cards; Midgaard checked out, no lease, internet already allowed, Nous already configured. Built current CLI, temporarily ran `hive work --poll 1`, then stopped it with Ctrl-C after completion. Verified Midgaard checked out and no lease afterward. Existing GUI app and installed workers were not replaced. No settings or migrations changed.

## Remaining reporting bug

`card_outputs.model_id` is NULL and the CLI reports unknown model, despite the meter identifying the model. The handler resolves and records the provider model but omits it from its response; BrainTurn carries usage but no resolved model; worker completion uses `spec.model_id`, which is absent for the cloud-provider default. Propagate resolved model metadata from the handler through the brain/session result into completion; do not label a cloud job with the local fallback model or guess from current settings. Add a regression using an omitted requested model and a server-resolved model. Token counts, targeting and host acceptance are verified independently of this unresolved label.

Machine-readable evidence is in `2026-09-17-targeted-nous-live.json`. Temporary execution logs: `/private/tmp/hive-live-worker.log`, `/private/tmp/hive-live-await.json`, `/private/tmp/hive-live-build.log`.

Sif your friendly Codex Agent
