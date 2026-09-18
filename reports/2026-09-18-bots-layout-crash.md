# Overgaard Bots window constraint crash

Jack reported a crash at 2026-09-18 07:57:13 -0700 when selecting Bots on Overgaard. Attached incident: 2390A25F-BE4A-4436-92AB-62F9FF63667A, Hive-bin PID 38895, macOS 27.2 (26B5086k).

Read-only unified logs confirm NSGenericException: the window requested more Update Constraints passes than there are views. Counts reached 275; recorded window size was 900 x 753. Stack includes NSHostingView.SizeConstraints.update and SplitViewChildController.hostingView. This is a UI layout crash; the report does not establish any enrollment failure or model memory exhaustion.

Candidate mitigation: replace BotsView's nested HSplitView with HStack and a 220-point agent list plus divider, retaining the flexible conversation panel and inspector. This removes the inner native splitter and its minimum/maximum width negotiation. Outer app navigation remains unchanged; the agent list is no longer drag-resizable. User-confirmed trigger plus logs justify the narrow mitigation, but runtime acceptance on Overgaard is still required; do not claim a proven fix from compilation alone.

Source commit 7b9c4f7 on branch sif-bots-layout-crash-20260918, based on installed ad30463. Does not include newer Bots backend changes. Shared main's concurrent changes untouched. Swift-only candidate reuses the verified ad30463 FFI binary and matching generated bindings; no Rust API changes.

Validation: Swift release build passed (37.73 seconds), git diff check passed, Developer ID nested/app signing and deep strict verification passed. Zip SHA-256 `1069886f314dde2fc071e999f6305c1ea532ea4c4fa294b77cac6cc8b8df4c93` matched on Overgaard. Installed at `/Users/jack/Applications/Loki's Den.app` after verifying no Hive-bin process was running. Previous app preserved at `/Users/jack/Library/Application Support/ohhive/backups/pre-bots-layout-20260918-080207/Loki's Den.app`. No app/model/worker launched and no account/database changes. Build log `/private/tmp/sif-bots-layout-build.log`; artifact `/Volumes/10TB JBOD/Agents/Claude/Artifacts/LokiDen-7b9c4f7/`. User must reopen Bots to confirm crash resolution. Pairing remains separately unverified.
