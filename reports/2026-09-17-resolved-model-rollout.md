# Resolved model reporting and rollout — 2026-09-17

## Change

The code-brain-turn response now includes `model_id`, selected on the server after resolving the request, saved preference, and provider default. This is the selected provider model identifier, not a claim about the provider's internal routing. LocalBrain reports its selected local backend model. Rust carries that identity with each turn and persists it at completion instead of persisting the optional requested model.

A single session label is emitted only if every completed turn reports the same nonblank identity. Mixed or partly missing identities stay unknown. Older Edge responses without model_id remain compatible. Past cards are not backfilled from present-day provider settings. Usage and billing behavior are unchanged; no database migration is needed.

## Verification

- 228 core tests pass with local-hub, sandbox, llama-cpp and bots.
- Full workspace excluding Tauri: 335 passed, 2 ignored.
- 17 code-brain-turn Deno tests pass, including server defaults without a requested model on text and tool turns.
- Wire-to-session regression covers missing, blank, changed and consistent identities plus empty tool-call fallback.
- Real LocalHub completion verifies persisted model_id with an omitted requested model.
- Strict CLI + Bots Clippy, formatting and diff checks pass.
- Existing Windows CI failure (run 35187925877) was a 20ms lease-fixture timing race. The fixture now allows setup time, waits until the actual deadline during its first tool call, and asserts that the second call never occurs.

Logs: `/private/tmp/hive-model-{tests,workspace,deno,clippy}.log`.

## Rollout status

Jack explicitly approved committing and pushing these changes to main on 2026-09-17 after automatic approval review requested branch-specific authorization. This report accompanies that push, including the pending cleanup tests in 330cd4e. Windows native runtime results remain pending until CI completes.

The arm64 macOS standalone worker release build passed. SHA-256: `59216464a770970c543d969bbf3db64e8d7e454d21316e9767864c326346e0e7` (`target/release/hive`). The local app rebuild passed, including strict code-signature validation, portable-library checks and the bundle engine-load check. The rebuilt bundle is `apps/desktop-swift/Hive.app`; the running app was not relaunched. The app build retains Google Desktop credential embedding and the Copilot checker. Its initial packaging attempt exposed missing libc/windows-sys dependency edges in the separate Copilot lockfile; an offline regeneration added only those two edges, with no version changes. Production code-brain-turn remains v7. The fleet preflight found no active leases. No installed remote worker or service was changed.

After authorized push: require native Windows cleanup tests, deploy the additive Edge response, verify a targeted Nous card persists model_id and usage, then install matching worker builds with backups and graceful service handling. Preserve checked-out status where it was intentional, and do not broaden node internet permission for this rollout.

Sif your friendly Codex Agent
