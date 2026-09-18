# Overgaard Bots window constraint crash

Jack reported a crash at 2026-09-18 07:57:13 -0700 when selecting Bots on Overgaard. Attached incident: 2390A25F-BE4A-4436-92AB-62F9FF63667A, Hive-bin PID 38895, macOS 27.2 (26B5086k).

Read-only unified logs confirm NSGenericException: the window requested more Update Constraints passes than there are views. Counts reached 275; recorded window size was 900 x 753. Stack includes NSHostingView.SizeConstraints.update and SplitViewChildController.hostingView. This is a UI layout crash; the report does not establish any enrollment failure or model memory exhaustion.

Candidate mitigation: replace BotsView's nested HSplitView with HStack and a 220-point agent list plus divider, retaining the flexible conversation panel and inspector. This removes the inner native splitter and its minimum/maximum width negotiation. Outer app navigation remains unchanged; the agent list is no longer drag-resizable. User-confirmed trigger plus logs justify the narrow mitigation, but runtime acceptance on Overgaard is still required; do not claim a proven fix from compilation alone.

Source commit 7b9c4f7 on branch sif-bots-layout-crash-20260918, based on installed ad30463. Does not include newer Bots backend changes. Shared main's concurrent changes untouched. Swift-only candidate reuses the verified ad30463 FFI binary and matching generated bindings; no Rust API changes.

Validation: Swift release build passed (37.73 seconds), git diff check passed, Developer ID nested/app signing and deep strict verification passed. Zip SHA-256 `1069886f314dde2fc071e999f6305c1ea532ea4c4fa294b77cac6cc8b8df4c93` matched on Overgaard. Installed at `/Users/jack/Applications/Loki's Den.app` after verifying no Hive-bin process was running. Previous app preserved at `/Users/jack/Library/Application Support/ohhive/backups/pre-bots-layout-20260918-080207/Loki's Den.app`. No app/model/worker launched and no account/database changes. Build log `/private/tmp/sif-bots-layout-build.log`; artifact `/Volumes/10TB JBOD/Agents/Claude/Artifacts/LokiDen-7b9c4f7/`. User must reopen Bots to confirm crash resolution. Pairing remains separately unverified.


## Adaptive layout follow-up

Jack reopened Bots and supplied a screenshot showing clipped agent details. Commit `c54fc5a` extracts the agent list/conversation into separate views; uses available detail width to show a 220-point list at widths >=700, otherwise an Agents and rooms popover; removes the conversation's 380-point minimum; and presents details on demand in a separate sheet, initially closed. New main windows default to 1100 x 760 with content-minimum resizing. Existing saved macOS window dimensions are not reset. The layout does not update state from geometry measurements, avoiding size/constraint feedback. User confirmed the Overgaard app is quit for replacement. Runtime resize, agent selection, details editing, and reopen checks remain user acceptance work.

Adaptive follow-up validation: final Swift release build passed (24.68s), strict signature checks passed, transfer SHA-256 matched (`8c54fc7291f10f08b696112935550483990c573790d4ff2c17bf18250a0874eb`). Installed c54fc5a on Overgaard while closed; backup `/Users/jack/Library/Application Support/ohhive/backups/pre-bots-adaptive-20260918-080835/Loki's Den.app`. Build log `/private/tmp/sif-bots-adaptive-build.log`. No runtime acceptance yet, no app/model launch or account changes.
