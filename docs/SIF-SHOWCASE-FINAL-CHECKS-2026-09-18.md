# Showcase readiness, 18 September 2026

## Known working path

The installed desktop builds are a651283. The reconciled source is 463b49f, including Claude’s named migrations and Fenrir v2. A source merge is not an installed release.

Read-only authenticated check from Overgaard after the presentation build: primary reachable, desktop-host agent `Jack’s Mac Studio` found, shared user profile and agent bio APIs available, archive safeguard endpoint available. The latest two delivery states are done, with an older failed delivery retained. This is connectivity/profile evidence, not a fresh inference test.

## Short demonstration

1. Keep the primary app on Midgaard open and sharing. Keep the secondary app on Overgaard open. Use Overgaard for inference; do not load a model on Midgaard.
2. In Bots, select the registered desktop-host agent `Jack’s Mac Studio`. Do not confuse it with the older CLI agent `Overgaard`; they are separate identities with separate histories.
3. Show the right-side bio panel and avatar. Confirm the model selection on Overgaard before sending one short greeting from Midgaard.
4. Demonstrate a small task in a test repository with one concrete acceptance check. The check must fail before the task writes the expected file. Avoid the earlier broad control-surface task as the live proof.
5. Explain what was verified: cross-computer conversation or a specific completed task. Reliable unattended fleet autonomy remains a target, not a demonstrated general guarantee.

## Rehearsal reset boundary

Archiving an agent does not revoke the computer’s pairing. Do not delete vaults, credentials or prior conversations to simulate onboarding. A remove/rejoin rehearsal needs a named secondary computer and a preserved backup. Leave Midgaard’s primary intact. No reset has been performed.

## Fallback

Use the real development screenshot in the presentation if the live connection fails. Do not start changing primary roles or repairing accounts on stage. The deck labels the screenshots as development previews.

## Handoff

The shared worker’s local-model tool support preflight is being completed separately in Sif’s isolated branch. Private-run UI paths already probe tool support. Unknown support from a compatible server is not proof of support; explicit refusal should stop before workspace preparation.
