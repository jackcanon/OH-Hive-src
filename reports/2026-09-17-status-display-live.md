# Status display live verification — 2026-09-17

Sif your friendly Codex Agent

Five active code workers remain checked_in with acceptance=true, recent last_seen (1–23 seconds at inspection), and zero leases. No new Claude assignment in continuity; older night queue items are already implemented. Concurrent delegated-path acceptance migration/tests/workflow remain Claude-owned and untouched.

Closed status-parser follow-up 385998e by rebuilding release CLI and querying existing real records. Failed card 4a48208e-76bc-4b43-abd1-b83844e5cdfa now parses as blocked with usage null and displays its FAILED host receipt. Successful card c18651d0-2253-4724-8747-79a4adb610d3 remains review, passed receipt, 4681 input / 336 output tokens. No new card or provider call.

Removed obsolete CLI explanation that token counts are dropped before completion (that bug was already fixed). Missing usage now explicitly says unavailable; numeric usage is labelled usage rather than monetary cost; zero totals do not establish a free job. Does not invent counts or backfill failed outputs. Final release build passes and both live status queries assert the expected stderr text and JSON values. Existing parser regression covers empty, complete and invalid partial usage records. Formatting and diff checks pass.

Local executable target/release/hive includes both parser and display changes. Installed fleet workers were not replaced for this CLI-only follow-up; no database deployment or remote git push. Evidence /private/tmp/hive-status-{failed,passed}.{json,stderr}; build /private/tmp/hive-status-fix-build-final.log.
