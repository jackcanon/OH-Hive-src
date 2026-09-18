# Agent profiles, avatars and removal

The right-hand agent inspector opens on direct agent selection. It now offers a name, one of the 21 supplied avatars, biography, instructions, Save profile, and Delete agent. Connection metadata is folded beneath the editable fields. Avatar artwork is bundled from the provided 128-pixel GIF collection and rendered as native static images; selecting artwork does not invent an agent role or instructions.

Profiles live on the selected private primary in additive `bots_agent_bios` storage, with owner checks on local and remote reads/writes. Names and profile content save together in a transaction. Revision checks reject stale saves; Reload saved profile retrieves the current version explicitly. Limits are 200 UTF-8 bytes for names, 4,000 for bios, 16,000 for instructions. Avatar identifiers are allowlisted, never file paths or remote URLs. The profile revision and role revision advance on save.

The executor fetches the current bio and instructions before each turn and includes them in context for local and supported cloud Bots runners. If the profile cannot be read, it fails that turn rather than ignoring instructions. Saved instructions affect future turns, not a response already in flight. These fields do not grant tools, change model selection or extend the separate coding/New chat prompts. Historical transcript text is unchanged.

Delete is an archive operation with confirmation: removed from the active roster; old messages and profile are retained. A response already running can finish. Models and computer registration remain unchanged. Automatic desktop and provider provisioning checks archived records and does not recreate the deleted agent. Explicit Register this Mac can create a new identity. No existing agent was deleted as part of development.

Verification includes remote owner isolation, same-owner persistence, stale saves, invalid avatars and size limits, deleted-host provisioning suppression, and a runner test that asserts configured instructions actually reach the reply request. Both Macs need the new build before exercising these new RPCs.
