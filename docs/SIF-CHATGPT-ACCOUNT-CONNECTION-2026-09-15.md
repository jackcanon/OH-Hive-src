# ChatGPT account connection implementation

Owner: Sif. Requested by Jack on 2026-09-15. This implements the account connection portion of ADR-034 and the subscription handoff. Claude retains Bots/LocalHub work.

## What is implemented

Hive now has a real local Codex app-server account service, exposed through UniFFI to native Mac Settings → ChatGPT and through Tauri to its Settings screen. Controls cover Connect ChatGPT, device sign-in, reopening the browser page, cancel, status polling, account email/plan display and disconnect. The service starts on an explicit account action; opening Settings alone does not start Codex. Clicking Connect after an app restart resumes an existing managed account if present.

The preview requires an installed **Codex 0.149.0** executable, discovered automatically or selected by full path under Advanced. It does not install or bundle Codex. Windows users need the actual executable, not an npm .cmd wrapper. Browser login uses the runtime-provided official OpenAI URL; device login offers the runtime-provided code. A connected account does not establish model access, subscription entitlement for every workload, or a successful inference turn.

## Code map

- `crates/ohhive-core/src/subscription/account.rs`: process ownership, version check, initialization, managed account RPCs and status projection.
- `crates/ohhive-core/src/subscription/fixtures/account_server.py`: deterministic protocol fixture with stale events, delayed account visibility and cancellation races; no network or credentials.
- `crates/ohhive-ffi/src/subscription.rs`: async UniFFI account record/method, hosted on the existing Tokio runtime.
- `apps/desktop-swift/Sources/OHHive/ChatGPTSettingsView.swift`: native connection UI; HiveStore and SettingsView integration.
- `apps/desktop/src/ChatGPTConnection.tsx` and `apps/desktop/src-tauri/src/lib.rs`: matching Tauri account UI and command.
- `subscription-coordinator` now enables `dirs` and is included by both desktop bridges.

## Account boundary and behavior

The runtime uses a dedicated `dirs::data_local_dir()/OHHive/subscriptions/chatgpt` home and empty workspace. It never reads Hive API keys or copies the user's existing Codex credentials. Configuration forces ChatGPT login and keyring storage, with no plaintext fallback selected. The child environment is allowlisted, excluding inherited provider API keys and proxy settings. Unix directories are owner-only, and direct symlinks at the home/workspace/config are refused.

OpenAI's [pinned credential storage implementation](https://raw.githubusercontent.com/openai/codex/rust-v0.149.0/codex-rs/login/src/auth/storage.rs) namespaces its direct keyring entry by a hash of the canonical Codex home. A dedicated home therefore isolates Hive from the usual Codex account in that backend. Actual credential write/restart verification still needs a completed user sign-in; source review and signed-out probing do not substitute for it.

Requests are serialized, correlated and time-bounded. Login completion must match the active attempt; the UI stays pending until account/read confirms a managed ChatGPT account. API-key accounts are refused. Cancellation clears the pending attempt and logs out to handle a completion race; logout is checked with account/read. Old browser pages cannot reactivate the Hive UI after cancellation. Runtime errors reset the process and visible state, and raw provider diagnostics/tokens are not logged or surfaced. Unknown server requests are declined; no model turns or tools are allowed by this account service. Browser callback failure can be retried with device sign-in. Device sign-in availability is controlled by the provider/account.

No inference, coordinator selection, rate-limit UI, agent executor or fleet dispatch is wired in this slice. Do not label it full ChatGPT-powered agent support yet.

## Verification

- `cargo test -p hive-core --features subscription-coordinator,bots --lib --quiet`: **57 passed, 1 opt-in real-runtime test ignored**.
- Opt-in `real_signed_out_handshake`: **passed** against installed Codex 0.149.0; initialization, signed-out read, browser login start with URL validation, cancel, logout and signed-out confirmation. No completed login and no inference. Sandbox callback binding was denied; rerun with permitted local callback access passed.
- `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --quiet`: passed on this Mac.
- `pnpm build` in `apps/desktop`: passed, including TypeScript and Vite.
- Release FFI build for aarch64-apple-darwin, regenerated UniFFI bindings and native Swift release build: passed. `apps/desktop-swift/Hive.app` was assembled and ad-hoc signed. The existing build script produces a developer bundle linked to the checkout’s FFI dylib; this is not a portable release artifact. The running Hive session was left untouched; no visual or authenticated end-to-end test is claimed.

## Next verification and integration

1. Launch the rebuilt app, open Settings → ChatGPT and complete the browser flow as the account owner. Confirm the correct account and plan, quit/reopen and reconnect, then disconnect and confirm the separate Codex login is unchanged.
2. Exercise device sign-in, denied/expired login, missing/locked keyring and missing/wrong runtime version on each supported OS. Windows and Linux have not been run here. No completed browser/device authentication or authenticated keyring persistence has been claimed.
3. After account gates are proven, connect this runtime to the coordinator through the existing supervisor protocol: model/capability discovery, limits, explicit project-context consent, one bounded turn, cancellation, approvals and disconnect cleanup. Do not launch a competing auth runtime with a second account state. Private/community scheduling remains a separate scope.
4. Packaging still needs a reproducible runtime installation/update policy before calling this a turnkey end-user feature.

## Separate feedback for Claude: Bots C1 compiler check

I read Claude's new C1 entry and ran `cargo check -p hive-core --features bots,local-hub --quiet`. It fails at `crates/ohhive-core/src/local_hub/bots.rs:729`: `Option<u64>` from `page.after` / `page.before` does not implement rusqlite `ToSql`. Convert cursors to checked `i64` values and reject overflow before binding (avoid unchecked casts). There are also unused imports at lines 40–43. I left this Claude-owned file untouched. Please review and fix, then rerun the feature combination; subscription-only/Bots-C0 tests do not cover C1 storage compilation.

Sif your friendly Codex Agent
