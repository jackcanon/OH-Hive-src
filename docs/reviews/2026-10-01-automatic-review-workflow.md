# Bounded automatic coding and independent review

An owner can choose **Run with independent review** for a named private coding task with required acceptance checks, select a different checker and model, and authorize zero to three correction rounds. The hub persists the request and advances preparation, execution, independent review, correction and fresh review using the existing target-pulled protocols. Polling from an authenticated worker advances the chain even when the controller view is closed.

The original coder's assignment, model, reasoning policy, turn limit, checks, instructions and protected repository baseline remain fixed. Checker instructions are saved at authorization. A checker receives a frozen source package and prior host test receipts; it cannot edit or run tests. Every correction receives a fresh checkout and every review binds the exact saved package. Passing review is a result about that snapshot, not permission to merge or publish.

## Stop and recovery boundaries

- The owner authorizes a maximum of three correction rounds and a thirty-minute deadline; default is one round.
- Pass ends the chain. Uncertainty, invalid evidence, unavailable authority, failed/interrupted execution and exhausted correction limits require owner review.
- Stop fences queued/running execution through existing cancellation receipts. Cancelled preparation is excluded from worker queues; files and evidence remain.
- Repeated requests and serialized polling do not duplicate dispatch. Reopening a controller reads the stored workflow.
- Up to sixteen active workflows per owner. One workflow per original task preserves the recorded decision; another attempt requires a separate task.
- There is no blind replay after a crashed worker and no authorization derived from generated text. This is a bounded private coding chain, not the general project graph coordinator.

## Verification

Focused state-machine checks cover reconnect/replay, correction staging, pass/inconclusive/limit outcomes, cross-owner rejection, conflicting requests, malformed review, deadline and queued-run cancellation, failed execution and preparation queue release. Full core, workspace lint, native compilation and installed live results are recorded in the continuity log and readiness artifacts after completion.

## Operational requirements

The primary hub and assigned native coding workers must remain available. Workers need the original repository connection, selected models and coding tools. Model inference is tested on Helheim; Midgaard is a controller. A stopped workflow never reports overall project completion. Larger tasks, cross-host failure injection and graphical acceptance remain separate verification work.
