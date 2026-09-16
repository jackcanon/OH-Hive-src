# Swift cloud replies — implementation handoff

Follows Loki's `LOKI-FFI-CLOUD-RUNNER-HANDOFF-2026-09-16.md` priority, discovered before starting migration reconciliation.

The native FFI drain now attaches CloudTurnRunner using the session's whoami-verified owner and retained hub/node-key pair, after account-change validation. Private-only sessions and remote-primary viewers do not borrow a community node key from unrelated configuration. Remote-primary execution remains unsupported, as the app already states.

The executor can now have no local runner. In that mode it leaves local deliveries unclaimed, reports local readiness honestly, and can still answer BYOK agents. The existing constructor remains compatible with CLI/Tauri callers and fan-out stays off. Invalid local endpoint/model setup cannot prevent a valid cloud runner from running. Failed cloud construction does not prevent local replies.

HTTP 404 (undeployed function), 409 (missing provider key), and 429 leave cloud work pending with the existing bounded retry delay. Authorization failures and other runtime errors remain failures. Route notices now describe service/key availability rather than claiming BYOK replies are unimplemented. These notices are diagnostic suggestions, not a determination of the exact outage cause. Swift worker status refers to agents/providers as well as local models.

Scope crosses into executor.rs and two BYOK strings in local_hub/bots.rs only to satisfy the handoff's no-local-model and honest-failure requirements; no change to delivery claims, routing authority, budgets or automation. No public FFI signature change. Bindings/app rebuilt together to avoid stale UniFFI checksums.

Verification: 8 FFI tests, 15 executor loop-safety tests (including new cloud reply with no local model and a local delivery remaining pending), 6 route-notice tests, and 6 cloud-runner tests including mocked 404/409 retry passed. All 7 Swift BotsModel tests also pass. ARM64 release library built, bindings regenerated/copied, and the existing build-app.sh completed an ad-hoc signed Hive.app. The existing external-dylib packaging weakness is not repaired by this wiring change; library and app now match locally. Tests use local fixtures/mock services, not paid provider calls. Live mixed-room acceptance remains pending deployment of bots-turn and use of a configured provider; no production deploy or claim of live replies in this handoff. Missing service/key has been verified with mock HTTP responses, not by removing live credentials.

Next: deploy/review bots-turn with the deployment owner, then exercise an actual mixed room and a cloud-only installation. See Loki's newer handoff for the following settings T-1 priority; database replay/reconciliation remains a separate outstanding deployment prerequisite for the SQL changes.
