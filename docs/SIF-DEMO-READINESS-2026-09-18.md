# Loki’s Den: two-Mac demo readiness

The intended demonstration uses Midgaard as the private primary and Overgaard as the model host. Do not start local inference on Midgaard. Keep historical CLI agents and conversations intact; the desktop-owned agent is currently named Jack’s Mac Studio and bound to the new signed pairing identity.

## Included in the candidate

- Primary sharing intent is saved only after successful start. The app restores that exact private address on launch and retries unavailable networks at 15-second intervals. Explicit Stop sharing clears the preference. It never promotes a secondary or silently changes authority. A changed IP still needs address recovery; internet relay and account-wide discovery are not implemented.
- Bots refreshes the fleet roster automatically, including after initial connection recovery. The current secondary registers an agent for its authenticated host if absent.
- Each recent message shows recipient reply state, including failed, waiting, running, completed, unknown or held. Status is read from the primary with conversation-owner checks; up to 500 recent delivery rows are displayed. Failure text offers a next step but does not contain a detailed backend exception.
- Local model prompts explicitly identify an AI software agent and supply the configured model ID. Hardware identity is not inferred from the computer name. No tool access is claimed. Generated wording still needs live verification.
- About you is shared through the primary, with preferred name and optional background, available to future Bots turns.
- Models offers a named Download and use action and a link to Ollama’s library. This is not an embedded searchable catalog.
- Committed main through d92cfc1 integrated, including the a58b23e budget fail-closed fix. Concurrent uncommitted main files remain untouched.

## Installation and live acceptance

1. Build/sign one consistent release FFI + generated Swift bindings + Swift executable; verify signatures and transfer checksum.
2. Coordinate quitting both apps. Preserve current apps and consistent SQLite backups before replacing. Preserve all credentials and model settings. Since the currently installed build cannot persist sharing intent, carry forward Midgaard’s explicitly enabled and verified address into the new preference during installation; never seed sharing on Overgaard.
3. Reopen Midgaard then Overgaard. Verify listener comes back without Make available and roster appears without manual Refresh. User setup may require macOS privacy/keychain dialogs; do not bypass them.
4. Test a short message to the current desktop agent. Confirm the new host’s delivery becomes done rather than relying on the historical CLI Overgaard agent. Verify reported model against actual selected/available model.
5. Enter preferred name through About you and verify it on both Macs and a subsequent reply. Do not insert personal background on the user's behalf.
6. Verify failed historical message stays marked failed while new successful messages show replied. Verify explicit stop sharing survives relaunch with isolated preference tests; avoid disconnecting the live showcase simply to repeat this check.

The new release must be installed before these runtime behaviors can be claimed as live. Live screenshots and read-only delivery evidence are stronger than generated claims of success.

## Live verification — 2026-09-18, approximately 10:11 Phoenix

Signed build dac8380 is installed and launched on both Macs. Midgaard restored its saved sharing address automatically: Hive-bin listens on 192.168.1.7:8787; historical CLI listener at 192.168.1.203 was preserved. Overgaard authenticated against that desktop primary using its existing registration. Both profile and delivery-status RPCs responded.

One benign test DM (sequence 4) targeted the desktop-owned agent Jack’s Mac Studio through the signed Overgaard host identity, not the historical CLI Overgaard agent. Its delivery progressed pending → running → done. Reply: “Good morning! I am Jack’s Mac Studio, and my exact configured model is **qwen3.6:27b**.” The allowlisted HIVE_MODEL value and Ollama loaded-model response both confirmed qwen3.6:27b. Existing failed delivery remains failed. No inference ran on Midgaard.

Backups: Midgaard `~/Library/Application Support/ohhive/backups/pre-demo-20260918-100825/`; Overgaard `~/Library/Application Support/ohhive/backups/pre-demo-20260918-100430/`. Both include the previous app and consistent database backup.

Remaining visual/user acceptance: roster appearance without manual Refresh, preferred-name entry through Settings → General → About you and subsequent personalized greeting, and Models download UI. No profile data was invented or entered. No new model was downloaded during this smoke test. Internet relay remains future work.
