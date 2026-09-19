# Overgaard installation and pairing handoff

Source installed: **ad3046309bd0941e217f609fab0470570f074854**, the clean signed app/CLI package verified in `2026-09-17-private-coding-clean-build.md`. This is a pinned rollout candidate, not a claim that it contains later commits such as a58b23e's Bots budget fix. Do not use this installation as acceptance evidence for those later changes.

## Completed on 2026-09-18

- Re-read continuity and reviewed new committed ADR-038. Shared checkout contains concurrent edits to LocalHub files; none were changed, staged or built here.
- Reverified the staged CLI hash, app source stamp and signatures on Overgaard.
- Observed both the existing community worker (PID 11241) and a Bots worker (PID 24964). Both use `/Users/jack/.local/bin/hive`; neither showed an open `vault-host.sqlite3` handle during inspection. Shared inference lock was idle when checked.
- Created `/Users/jack/Library/Application Support/ohhive/backups/pre-ad30463-20260918-073343/` with mode 0700. It contains `hive.previous` and a SQLite backup of the vault, taken through SQLite's backup API to include committed WAL state. Backup integrity check returned `ok`; database file is mode 0600.
- Installed the verified app at `/Users/jack/Applications/Loki's Den.app` and reverified its signature.
- Atomically replaced the CLI executable with the verified staged binary. **Did not restart either running process.** Both original PIDs remained present after installation. They retain their earlier running image; the updated file applies to future launches.
- Did not open the app or migrate the live vault. Schema observed during backup: 14. No new model call, signed enrollment, task submission or community membership change.

## Next step / user interaction

The computer-use tool timed out selecting the local Den app, so no current app screen was inspected and no UI action can be claimed. Asked Jack to open the installed app on Overgaard and report when he reaches Settings → Private Fleet. Do not replace signed enrollment with a direct database edit or copy Midgaard's private identity credentials.

Use **Connect to a primary**, with Midgaard as the primary, rather than registering Overgaard as a separate first computer. Complete the same-fleet approval at lokisden.app. The model/checkout execution test stays on Overgaard. Existing processes must be coordinated before restarting them; installation alone is not evidence of new worker behavior.

Before broad Bots acceptance, refresh the candidate to include subsequent committed fixes. Native private coding pairing, fresh host report, Git access, real Prepare/Run/checks/output and Stop/Retry remain unverified in the two-machine desktop flow.

## Commit isolation

This report is authored in `/private/tmp/sif-overgaard-rollout-20260918`, branch `sif-overgaard-rollout-20260918`, based on committed `d92cfc1`. It does not include the shared checkout's pending LocalHub edits. No push or implicit merge into shared main.


## Registration configuration repair

Jack encountered `Private Fleet sign-in is not configured on this installation yet` on Overgaard. The three public enrollment trust fields (issuer, key ID, public key) were absent there and present on Midgaard. Installed only those fields from the working primary configuration, with an atomic mode-0600 write. Verified exact equality and preservation of all other configuration lines. Protected backup: `/Users/jack/Library/Application Support/ohhive/backups/pre-enrollment-trust-20260918-074328/node.env`.

No private signing key, vault self key, identity credential or token was transferred. No model or worker was started. `nodeconfig::get_extra` rereads the file per call, so a new attempt should pick up the settings without a rebuild. User must still complete same-fleet secondary pairing through Connect to a primary; successful registration/pairing remains unverified.

Product follow-up: distribute approved public enrollment trust metadata with installations so fresh downloads do not require a manual per-machine repair. This repair addresses Overgaard only.
