# Private coding: desktop controller and execution worker

Midgaard keeps project/task history. Overgaard performs repository preparation, model work and checks. No model is launched merely by opening the task view or choosing a computer.

## Setup and use

1. On Midgaard, register the private fleet identity and start sharing in Private Fleet settings. Create a pairing code.
2. On Overgaard, install the rebuilt Loki’s Den app, select **Connect to a primary**, enter Midgaard’s private address and pairing code, and complete the signed approval on lokisden.app for the same fleet. This is independent of community Hive membership.
3. On Overgaard, connect GitHub for private repository access, enable coding tools, and make a tool-capable model available in its configured model server.
4. In Overgaard’s Private Fleet settings, choose **Start coding worker on this Mac**. This is explicit for each app launch. It remains active when Settings closes; quitting the app stops the process. This first desktop integration is not an unattended launchd coding service.
5. On Midgaard, open the coding project’s Tasks, choose Overgaard and an advertised model, add instructions/checks, and save. Choose **Prepare on Overgaard**, then **Run on Overgaard** when preparation finishes. No GitHub push is performed.
6. Stop, results and retry are available in the task row. Retry preserves files/checks and explicitly authorizes another run. Interrupted preparation requires owner recovery followed by restarting the target worker; it never authorizes a model run.

## Boundaries and implementation

- The primary UI never defaults to itself or discovers/invokes its local model server. Execution host selection is explicit and immutable per saved task.
- `private_dispatch.rs` in core provides an authenticated project overview and target-bound pending-work query. No schema change beyond v21. Existing durable preparation/run/stop/retry/recovery methods remain authoritative.
- Native FFI exposes typed task records, submission and command methods, plus `PrivateCodingWorker`. Opening the worker validates the saved signed primary selection. The worker cannot open without a selected remote authority; it cannot silently create a local database or fall back.
- The existing fleet-selection mutex protects preparation/execution from authority changes. A per-session mutex prevents simultaneous ticks. Run cancellation uses the existing cooperative stop path.
- Git credentials are obtained by the execution app only when preparation is queued. They are passed to host checkout preparation and never sent to the authority or task data.
- A separate task advertises readiness every 15 seconds while execution can continue. Authority timestamps expire at 45 seconds. Metadata probing never invokes inference. Disabling publishes an unavailable report; crashes rely on expiry.
- Worker failure pauses processing until explicitly restarted. A process restart does not adopt an abandoned preparation session. Recovery remains an explicit owner decision.
- `PrivateCodingTasksView` keeps optional checks/turn settings collapsed and shows host names on every action. `HiveStore` owns `PrivateCodingWorkerModel` for app lifetime. Worker controls are shown only on secondaries.
- Existing local-only FFI methods remain for compatibility; the task screen now uses the fleet dispatch path. Old tasks assigned to Midgaard retain their target and are not silently migrated to Overgaard.

## Verification / remaining acceptance

Core LocalHub suite: 111 passing including real HTTP and Git checkout fixtures with a mock model server. Added assertions cover controller overview, target-only pending work, stopped-operation exclusion, session ownership, foreign owners and revoked targets. Strict FFI Clippy passes. The bounded metadata stream now uses owned model records so its future is Send-compatible with the native runtime.

Native signed build and live machine acceptance are recorded in the continuity log. A read-only Overgaard check found macOS 27.2, no saved `private-primary.json`, and no Loki’s Den app under `/Applications`; actual pairing is therefore still required. No real model was run during development of this wiring. Do not treat the mock-host tests as live Overgaard acceptance.
