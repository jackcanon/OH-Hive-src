# Coding coordinator recovery — implementation handoff

Implemented locally on 2026-09-17 by Sif your friendly Codex Agent. No production migration, rollout, or paid team test has been run for this change.

A coding parent previously restarted from its original task without child reports. The worker now passes its claim's dependency payload through the coding tool wrapper into the session. For coordinator code cards, an updated hub supplies a versioned child envelope with authoritative parent/child IDs, keys, statuses, declared checks and latest reports. A resumed model sees the existing children and bounded report data. Reports are labelled untrusted evidence, not instructions.

## Behavior

- Initial coordinator claims have an explicit empty child list. Older hubs that omit the envelope are rejected before workspace access or a model call. There is no silent fallback to a fresh task.
- LocalHub and Supabase claims use the same envelope. Ordinary non-coordinator dependency payloads keep their existing shape. This is child recovery, not replay of the full earlier conversation.
- Recovered sessions can review or wait on existing children; spawn calls are rejected so a model cannot replace the batch after restart. New sessions that spawn children must wait before completing. This first implementation supports one batch, not successive delegation waves. Recovery after a partial spawn does not create the missing remainder automatically; that case needs operator review/a new job.
- SQL direct and delegated spawn mutations serialize through a parent-row lock and return the same child ID for the same parent/key/payload. Changed payloads or another parent's key conflict. Existing lease authorization still runs before replay. LocalHub already had same-payload replay; its tests remain in use.
- Parent completion requires each known child to be review/done with an output. Code children additionally require a final host acceptance receipt matching their declared checks, including required expected exits. Missing/malformed final receipts cannot fall back to an earlier quoted success. A child without declared code checks remains unverified and blocks completion.
- A related existing bug is corrected: `finish_code_session` now rejects `ok:false`, not only `acceptance_failed`. Setup/model/turn-limit errors cannot be marked ready for review. Paused/expired leases still take their existing separate paths.
- Context rejects wrong parent/version, duplicate identities, more than 16 children or over 256 KiB. Individual model-visible reports truncate at a UTF-8 boundary after 8 KiB, with an explicit notice. Validation uses the full bounded receipt before prompt truncation. Large reports fail or truncate explicitly; they are not silently considered fully reviewed.

## Verification

Real Worker::tick → LocalHub claim → LocalBrain HTTP test server demonstrates both child IDs/reports in the first resumed request, rejected replacement spawn in the next request, exactly two children afterward, successful completion with valid receipts and blocked completion when reports merely claim success. Additional tests cover legacy missing metadata before any brain call, malformed final receipt, wrong expected exit, missing/pending/failed results, duplicate child IDs and mismatched parent ID.

Full workspace excluding Tauri passed: 347 tests, two ignored, including 295 core tests. Feature-specific core suite passed 239 tests before the final missing-context regression was added. CLI+Bots strict Clippy passed. Full SQL replay: 107 migrations; node-targeting, funding, coordinator context/idempotency/conflict/lease-loss/latest-failure/legacy-shape tests pass. Delegated mutation body is compared against the tested direct mutation body, allowing only the existing credential-resolver difference; this is not a new live delegated-authentication test. Schema baseline preserves all 1,070 catalog objects and accounts for 326 functions / 30 intentional body changes.

Logs: /private/tmp/hive-resume-{tests,workspace,clippy,sql}.log. SQL snapshots: /private/tmp/hive-resume-{catalog,functions}.json. Native Windows CI for this revision and the real two-machine trial remain rollout validation.

## Integration and deployment

Migration: `supabase/migrations/20260917170000_code_coordinator_resume.sql`. It replaces only `hive.card_dep_outputs`, `hive.spawn_child_card` and `hive.ctl_d_spawn_child_card`, retaining their ACLs and signatures. No new table or public RPC. It does not change child internet inheritance, billing, or owner/target-node policy.

Loki's concurrent FFI/Swift supervision edits are preserved and not part of Sif's change. Before deployment, recheck continuity and live function definitions. Coordinate this migration with the integrated worker build; avoid running coordinators across old/new workers during the transition. Deploy the hub metadata before enabling updated coordinator workers. Then run the prepared two-machine trial and its negative acceptance case. The client fails closed against an old hub, so publishing only the client would intentionally block coordinator jobs.

The prepared kit is in `docs/SIF-TWO-MACHINE-TEAM-TRIAL-2026-09-17.md` and `scripts/fleet-team/prepare.py`. Its former source-level blocker is implemented here; rollout and live evidence are still outstanding. No claim of production autonomous teamwork yet.
