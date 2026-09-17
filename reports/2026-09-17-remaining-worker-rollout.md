# Remaining active coding-worker rollout — complete

Sif your friendly Codex Agent, 2026-09-17

Five active code-capable nodes now report acceptance=true and checked_in: Midgaard, Overgaard, Odin, Jotunheim, Heimdall. Final snapshot has zero leases on all five. Retired checked-out pilot nodes were not re-enrolled. Regional/non-code nodes were not changed.

## Attribution and concurrent work

Odin had already been updated by the other session. Jotunheim changed concurrently during preflight: staged Mac worker and installed worker both matched e43c2a951a98e8331fdf432967479280e1cb6726e086ad10f039d70d3f5f5752, and the live capability changed to true. Sif did not replace/restart Jotunheim after detecting that. Midgaard app control timed out; requested a user relaunch, and subsequent database observation confirmed true. App signing check passed. Overgaard was completed in the preceding pilot.

The claim safeguard was also deployed concurrently. Saved live pg_get_functiondef(hive.node_claim_card(text)) and compared it exactly against the tested migration loaded into PGlite: identical. Sif did not redeploy it. Database update occurred before Heimdall/Midgaard finished advertising capability; this temporarily made checked jobs ineligible on those two nodes. All active coding nodes now qualify.

## Heimdall installed by Sif

Built exact git archive 66738c0 in isolated /tmp/hive-build-66738c0, using its existing Cargo cache without changing the source checkout. Linux x86_64 release build and dynamic library resolution pass. Binary version 0.4.1.

New SHA-256: 687ae6b7af92df231edd3df422404c9760b9aa3b2df11e057b45f0e935641a8f.
Old SHA-256: 8b458766bc991fcd5557a6c64095f70a81964c15a4a7e3e0b81d9bd2a2cfe444.
Backup: /home/jack/.local/bin/hive.bak-preAcceptanceCapability-20260917.
Installed: /home/jack/.local/bin/hive. Existing user hive-worker service stopped idle and restored via cleanup trap. No persistent config changes.

First proving card 4a48208e-76bc-4b43-abd1-b83844e5cdfa failed correctly: model wrote the requested text without its required newline. Independent hex inspection confirmed the omission; host assertion exited 1 and blocked the card. Do not describe this as a passed model task.

Second proving card c18651d0-2253-4724-8747-79a4adb610d3 used a fresh workspace and required answer.txt containing decimal 19+23 (whitespace allowed). qwen3.5:4b completed, host check passed, independent remote file check confirmed 42. Usage 4681 input / 336 output tokens, local inference, zero leases. Neither test used a paid cloud model. Both receipts retained in adjacent JSON.

## Follow-up discovered and fixed locally

Failed outputs contain usage={}, which made CLI status/await reject the whole response as missing tokens_in. CardStatus now reads an empty usage object as unavailable (None), not measured zero; partial malformed records still fail validation and complete records preserve counts. Four relevant tests pass; formatting/diff checks pass. This small parser fix is committed locally but not rebuilt into fleet binaries or pushed in this turn. Installed workers remain the tested 66738c0 source build; their task execution does not depend on the CLI status parser.

Full fleet rollout means the five currently active coding workers above. It does not claim the dormant pilot, non-code regional services, or every machine running every Hive subsystem was upgraded. No remote git push. Claude's untracked coordinator report preserved.
