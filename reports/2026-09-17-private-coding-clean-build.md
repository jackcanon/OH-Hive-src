# Private coding: clean committed build and Overgaard staging

Source: **ad3046309bd0941e217f609fab0470570f074854** (`ad30463`). Jack confirmed regular commits and continuation after reviewing the shared-tree build concern.

Built from a detached worktree `/private/tmp/lokis-den-release-ad30463`. Only generated compiler-cache symlinks were temporarily excluded from source status during compilation; they were removed afterward. The source checkout now has empty `git status --porcelain`. The app's `OHHiveSourceCommit` is exactly `ad30463`, without `-dirty`.

## Verified artifacts

- Swift production app, matching Rust bindings/library, Developer ID signatures and bundle engine-load probe passed. `/private/tmp/hive-clean-ad30463-build.log`.
- CLI `hive`, built with `--features bots` from the same checkout, version/help checks and Developer ID signature verification passed. `/private/tmp/hive-clean-ad30463-cli.log`.
- Earlier final shared-tree build also passed (`/private/tmp/hive-wiring-app-final.log`), but retained the dirty marker because of the untracked avatars; the isolated artifact supersedes it for rollout.
- Local package: `/Volumes/10TB JBOD/Agents/Claude/Artifacts/LokiDen-ad30463/`.
- Overgaard staged package: `/Users/jack/Downloads/LokiDen-ad30463/`, including extracted `Loki's Den.app`, matching `hive`, `manifest.json`, ZIP and setup notes.
- After transfer, Overgaard verified both artifact hashes, source stamp and code signatures. Nothing was launched or installed over an active binary.

SHA-256:

| Artifact | SHA-256 |
|---|---|
| `hive` | `3fdcb5060c9f15707815993ca99b91cf49cf52cb8151f6c4113d90f01037aa48` |
| `Lokis-Den-ad30463.zip` | `da945ce33592a7b777caf0ad8ddcefe307341c92db73e24dfc05b4d476b1fe6c` |

## Read-only machine findings

Midgaard's live vault schema is **21**. Overgaard is on macOS **27.2**, its local vault schema is **14**, and `media.happyjack.hive-worker` is running. No Den/Hive app was found under `/Applications` or `/Users/jack/Applications`; no saved native `private-primary.json` exists.

Before launching the app against Overgaard's vault, coordinate the matching CLI update for any processes using that vault and take a SQLite-consistent backup. Do not let an old CLI reopen a database upgraded by the new app. The running community worker has not been stopped/replaced. A launchd listing is not proof that no additional manually launched process uses the vault.

## Remaining product acceptance

Install/start the coordinated versions, owner-approve Overgaard's connection to Midgaard through lokisden.app, connect target-local GitHub if needed, and explicitly enable the target coding worker. Then verify discovery, preparation, actual model execution, acceptance checks, output and Stop/Retry in the real desktop flow. Models belong on Overgaard; no Midgaard inference is authorized by this packaging work.

No real model, live database migration, GitHub push, service replacement or community membership change occurred in this increment. See `docs/SIF-PRIVATE-CODING-DESKTOP-WIRING-2026-09-17.md` for the implemented flow.
