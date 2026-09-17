# Team failure-path regressions — 2026-09-17

Sif your friendly Codex Agent

## Reconciled state

Repository commits 85f735f and 0432653 supersede the earlier continuity statement that the coordinator migration and two-machine trial were pending. Claude reports the migration deployed, Overgaard updated with a preserved backup, and the Midgaard/Overgaard trial passed. See reports/2026-09-17-two-machine-team-trial-live.md. This turn inspected that evidence; it did not independently rerun the production trial.

## Added repeatable verification

Two LocalHub integration regressions in crates/ohhive-core/src/local_hub/tests.rs:

- team_failed_host_check_blocks_waiting_parent_and_releases_leases: a stub brain claims success while an actual Python acceptance command inspects an incorrect disposable file and fails. The production code-session runner and worker completion handler block the child, propagate blocking to its waiting parent, create no successful output, and leave no leases. No fabricated receipt or direct fail_card shortcut.
- team_offline_child_stays_targeted_until_original_node_returns: a parent delegates to the other enrolled node, which checks out. Repeated claims from the remaining node cannot take the child; the parent stays waiting and the child ready with no output. When the designated node returns it alone claims/completes the child, after which the parent resumes. Final lease count is zero.

Both tests passed with cargo test -p hive-core --features 'local-hub,sandbox,llama-cpp,bots' team_ -- --nocapture. Formatting and whitespace checks pass. Log: /private/tmp/hive-team-negative-tests.log.

## Limits and next work

These are local integration tests, not Supabase production or multi-machine failure proof. They complement the existing worker/model-HTTP regression rejecting missing receipts. No new deployment, paid calls, fleet service changes, or remote push occurred. Next: bounded live failed-child and offline-target exercises using disposable jobs, recording parent/child states, actual targets, receipts and cleanup. Seven other nodes were reported by Claude as still on older workers; that inventory was not refreshed here.
