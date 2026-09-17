# Live team failure checks — 2026-09-17

Sif your friendly Codex Agent

## Failed child blocks its coordinator — PASS

Submitted coordinator b29bc79c-cb49-4f4a-b6c7-4d7028ca4163 through the normal CLI, Local Fleet Test project. Nous coordinator, explicitly targeted Midgaard, maximum four turns per session; exactly one local child, maximum two turns, mistral-small3.2:24b verified installed before submission. Child 6745c649-7b58-451e-8bc1-2f441d1fdd2b deliberately runs /usr/bin/python3 -c 'raise SystemExit(17)' with expected exit 0. No file changes requested.

Observed parent waiting_on_child and child running, then both blocked. Persisted child host receipt reports failed, required true, exit_status 17. Parent output records spawned-child failure. Both have zero leases. This exercises real child failure and database propagation; it is not a live forged/missing-receipt case, which remains covered locally. Failure outputs do not preserve a model label or useful usage totals in the queried rows; no cloud cost estimate asserted.

## Offline target does not fall back — PASS

Verified Overgaard had zero leases and its existing launchd service definition was available. Temporarily booted out that service, then submitted card 45c5b516-9a44-46ac-84d2-b2af72386fd0 targeted exclusively to Overgaard. After a 25-second observation interval, the target was checked_out and the card remained ready with zero leases and zero outputs. Other fleet workers remained available. No routing or permissions changed.

Restored the existing service in a finally cleanup block. Verified running PID 5756, and database presence checked_in. Job completed to review on the exact target UUID ce95c14d-db8d-4a88-90c8-277c067fc57c, mistral-small3.2:24b, 1070 input / 5 output tokens; zero leases. This disposable routing probe asked only for a short response and declared no acceptance command, so its UNVERIFIED receipt is expected and is not a correctness proof. This is a top-level targeted job, not a live offline-child cascade; the latter is covered by the LocalHub regression.

## Evidence and remaining work

Machine-readable database snapshots: reports/2026-09-17-team-failure-live.json. Temporary manifests: /private/tmp/hive-team-negative-manifest.json and /private/tmp/hive-offline-manifest.json. Disposable workspaces retained for inspection. Three test cards retained with their receipts; no unrelated cards touched. No deployment, worker replacement, config edit, or remote push in this turn. Overgaard's original service is restored. Concurrent acceptance-capability work remains untouched.

Next: integrate and verify Claude's acceptance-capability eligibility safeguard, then continue staged rollout to the remaining workers. A live missing-receipt child and offline-child parent lifecycle remain distinct from the cases proven here.
