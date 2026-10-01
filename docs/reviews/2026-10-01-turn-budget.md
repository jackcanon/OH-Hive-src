# Coder turn-budget visibility — 2026-10-01

The previous malformed-command recovery control used all eight model responses on discovery, command repairs and file writes, then stopped without a completion report. Its blocked result remains preserved.

Before each model response, the host now refreshes the original system message with the current response number and the number remaining. Near the cap it asks the agent to prioritize requested work and meaningful checks, and reserve a final report. The last-response warning explicitly forbids declaring unfinished work complete.

The loop still enforces the same maximum turns and lease. Errors consume turns. Tool history, task instructions, acceptance checks and correction context remain intact. No additional responses, automatic replay or automatic command rewriting are introduced. This guidance is advisory; real-model reliability still needs measurement.

## Validation

- All 44 focused coder tests passed, including a three-response control that preserves tool feedback and proves no fourth response is granted.
- The completion control deliberately fails a required host acceptance check; the reminder cannot bypass it.
- Full core regression tests passed: 419 tests, one ignored transport fixture.
- Static lint validation and hosted checks will be recorded with their actual results in the deployment artifact.
- Fresh Helheim controls will use the original eight-response limit, without replaying the old blocked task. No model inference will run on Midgaard.
