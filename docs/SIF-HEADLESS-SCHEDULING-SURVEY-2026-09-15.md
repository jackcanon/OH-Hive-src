# Headless scheduling: what Hive already has

September 15, 2026 UTC · Sif your friendly Codex Agent

**Finding:** background worker startup already exists for macOS and Linux. The missing work is not simply another launchd/systemd template. Weekly node availability, executing queued cards, and creating recurring agent jobs are three different functions; they are not yet joined.

## Inspected source

| Function | Actual implementation | Gap |
| --- | --- | --- |
| Headless execution | `crates/hive/src/main.rs`: `hive work`; shared `worker.rs` claims and runs cards; CLI signal shim handles shutdown. | Does not fetch/enforce weekly schedules. It checks in on startup and re-checks in when a claim reports NotCheckedIn. |
| Weekly availability | `hive check-in --stay`: fetches `get_schedule`, compares weekday/time on the machine's local clock, checks in/out, heartbeats. | This is presence management only. It does not execute cards or create jobs. Initial check-in happens before schedule evaluation. |
| macOS background startup | `packaging/media.happyjack.hive-worker.plist`: `hive work`, RunAtLoad/KeepAlive. `scripts/service-mac.sh worker` installs via `launchctl bootstrap gui/<uid>`. | User GUI LaunchAgent, not a system LaunchDaemon. Do not promise execution while logged out just because `docs/JOIN.md` says it survives logout/reboot. No service was installed/tested in this survey. |
| Linux background startup | `packaging/hive-worker.service`: `hive work --poll 5`, restart policy, user service; documented `loginctl enable-linger`. | Linger/service setup must actually be installed and enabled. Still no weekly policy in the worker. |
| Desktop launch-at-login | Tauri autostart plugin and Swift `SMAppService.mainApp`. | App startup is not an independent daemon or recurring agent-job scheduler. Closing a window, quitting an app and logging out are different tests. |
| LocalHub schedule | `local_hub/mod.rs`: `get_schedule` returns None. | Weekly schedule transport/storage is not implemented for fully local jobs through this method. |
| Folder refresh | `vault_folder.rs`: host-owned portable polling watcher. | Its future needs a living host process; it indexes files, not recurring projects. |
| Curation maintenance | New `vault_maintenance.rs`: persisted due time, host-owned async runner, scan/report and opt-in retention. | No FFI/UI or host startup call added under this task's scope. Not a general agent scheduler. |
| Recurring agent work | No definition/occurrence system found in inspected CLI/core paths. | No equivalent to “every morning create this report,” with deduplicated occurrences, task edits, missed-run policy and result delivery. |

Sources also include `docs/JOIN.md`, `supabase/migrations/20260912270000_node_schedules.sql`, and `apps/web/public/service-mac.sh`. This is source inspection, not a live service inventory or cross-platform execution test.

## Why running two existing loops is not the fix

Starting `check-in --stay` alongside `work` would create competing authorities over presence. The first can check out outside a window; `Worker::tick` explicitly re-checks in after `Claim::NotCheckedIn`. Thus service setup alone cannot demonstrate enforcement. The current weekly loop also turns schedule-fetch errors into “no schedule” (`.ok().flatten()`), which is unsuitable as proof that an unavailable policy prevents new work.

The existing week/time representation has no named timezone. Same-day windows are evaluated with start inclusive/end exclusive; overnight windows need splitting. Timezone changes, daylight saving and malformed/fetch-failed schedules need defined behavior if this becomes an execution policy. Those are availability semantics, not recurrence semantics for generating new cards.

## Recommended next work, survey only

1. Keep existing Mac/Linux packaging. Correct the Mac logout claim and test actual lifecycle behavior. Survey Windows service/login options separately; a comparable headless worker service template was not found here.
2. Put one availability decision in the shared worker before claim/re-check-in. Preserve manual pause; distinguish unknown policy from intentionally unrestricted policy. Reuse weekly schedule storage rather than creating a duplicate. Claude owns worker changes; none made here.
3. If Jack wants recurring tasks, design a separate persisted task definition and occurrence record: timezone, due occurrence ID, project/task specification, missed-run rule, overlap policy, pause/edit and result destination. Deduplicate the same occurrence after restart and submit via the existing card service. A service manager keeps the scheduler alive; it does not supply these semantics.
4. Integrate curation's new runner into whichever LocalHub host is chosen, including orderly shutdown and error visibility. It requires no model calls and should not depend on a desktop window remaining open.

Acceptance checks: worker starts at login/boot as actually supported; outside schedule it cannot re-check itself in and claim; policy fetch failure is reported; quitting UI leaves an independently installed service working; a recurring task survives restart without duplicate cards; a missed scheduled time has a documented outcome. Measure these independently.

Community/private arbitration and data-sharing boundaries were explicitly excluded from this survey and are not changed. We should not build that policy by inferring it from weekly availability.

Sif your friendly Codex Agent
