# Sif: subscription P0 hardening, Bots C0 verification and effort reporting

2026-09-15 — Sif your friendly Codex Agent

Read Loki's newest handoff and effort-method document. This host has cargo 1.98.1 and codex-cli 0.149.0. No provider authentication or cloud inference was performed.

## Built and verified

The original combined subscription/bots feature suite compiled and passed 48 tests. Added six regression tests and fixes; the suite now passes **54 tests**. Both features also received individual `cargo check` runs. Bots C0 remains domain types and service interface only: compilation does not mean DM storage/UI or privacy enforcement is implemented.

Subscription fixes:

- Failed login no longer marks ready; successful login completion waits for account verification. Account updates distinguish managed ChatGPT, logout, unsupported auth modes and malformed data. Stage 2 must still correlate login attempt IDs and establish account/session authority before enabling tools.
- A terminal turn distinguishes completed, failed and interrupted; unknown terminal shapes produce an error rather than false success.
- Server request IDs support schema-defined strings and signed integers and are echoed in unsupported-request replies.
- Reconnect invalidates ready/running state until the new connection is checked.
- Frame limits apply even if an oversized frame and newline arrive in the same read.
- Completed-but-unread results count against admission capacity. Unknown/duplicate replies cannot grow the result map.

Generated a schema snapshot offline using the installed runtime. Selected source JSON and SHA-256 manifest are in `crates/ohhive-core/src/subscription/schema/0.149.0/`. `scripts/generate_codex_contract.py` generates the consumed scalar auth/server-ID types and checks fixture hashes with `--check`. Full turn/usage schemas are reference fixtures. Other protocol payloads remain explicitly opaque; this is not a claim that the entire app-server schema has generated Rust bindings. No runtime version upgrade or shipping-version choice was made.

## Effort reporter

`scripts/codex_effort_report.py TRANSCRIPT [--since ISO_TIME] [--until ISO_TIME]` reads native `event_msg/token_count` cumulative usage metadata. It reports counter differences, not sums of cumulative snapshots or repeated `last_token_usage`. Cached input/reasoning output are subsets, not additive categories. Missing counters remain null. Counter-reset intervals are excluded and flagged; the first observation is a baseline, never assumed zero. Four synthetic tests cover duplicate snapshots, window baselines, resets and missing-data/content isolation.

No transcript text, tool results or credential contents are emitted. No automatic broad session discovery, cost estimate or Anthropic-weight reuse. Recorded wall-clock span includes idle time and reporting delay; a delta can straddle the requested start boundary. It is an observed accounting interval, not exact task-cost attribution. `docs/subscription-integration/2026-09-15/p0/sif-effort.json` is a snapshot for this work taken before the final log/response, so later usage is not included.

## Commands verified

```
cargo check -p hive-core --features subscription-coordinator --quiet
cargo check -p hive-core --features bots --quiet
cargo test -p hive-core --features subscription-coordinator,bots --lib --quiet
python3 scripts/generate_codex_contract.py --check
python3 -m unittest discover -s scripts -p test_codex_effort_report.py
```

## Handoff to Claude

Review these changes before starting real account wiring. Stage 1 now has compiler-backed tests and pinned schema evidence for the exercised subset. Full generated protocol coverage and a real subprocess handshake remain to be integrated; unknown approvals are still declined. Process supervision, managed login, login-attempt correlation, account-read validation, broker capability binding, journal and UI remain the subsequent implementation work. The current in-memory supervisor is not an always-running background host.

Retain the accepted ChatGPT → Copilot → Grok order. Bots storage/executor work stays with Loki unless reassigned. No production database or worker job was touched.

Sif your friendly Codex Agent
