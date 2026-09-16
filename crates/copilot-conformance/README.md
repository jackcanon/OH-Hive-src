# Hive Copilot P2 compatibility fixture

This isolated crate compiles Hive's intended account/session boundaries against the
**official github-copilot-sdk 1.0.14**. It is not part of the shipping workspace and
adds no login button or production adapter. Cargo.lock pins transitive dependencies.
The SDK's bundled-runtime manifest pins **Copilot runtime 1.0.85**; the optional
`bundled-runtime` feature uses the SDK's downloader/hash verification.

## Run

```sh
cargo test --locked --manifest-path crates/copilot-conformance/Cargo.toml
cargo clippy --locked --manifest-path crates/copilot-conformance/Cargo.toml --all-targets -- -D warnings
cargo run --locked --manifest-path crates/copilot-conformance/Cargo.toml --features bundled-runtime --example handshake
```

The last command downloads the official runtime through the SDK, starts it with
an intentionally invalid fixture token in a separate temporary directory, pings,
and stops. It performs no login or model turn and does not read Hive's Keychain.
A failure is a failed compatibility check, never permission to use another account.

## Integration findings

- Explicit GitHub App user tokens and `use_logged_in_user(false)` exist in the
  official Rust API. This fixture rejects installation tokens and other credential
  types. Prefix validation is a routing guard, not proof of token validity or identity.
- `ClientMode::Empty` disables ambient CLI behavior and SDK code disables the CLI
  system Keychain integration. Supply a separate owner-protected base directory
  and no shared CLI credentials. Production must enforce directory permissions.
- The SDK injects `COPILOT_SDK_AUTH_TOKEN` **before** applying `env_remove`.
  Removing that variable erases the selected token. The fixture removes inherited
  settings while preserving that explicit injection and the SDK's home/keychain
  controls. Platform process essentials remain inherited. This is configuration
  isolation, not an OS sandbox or proof of every runtime behavior.
- Create/resume both install an explicit deny-all permission handler and empty
  tool allowlist. The stdio MCP configuration also explicitly exposes zero tools.
  Never ship with an absent permission handler assuming it means deny: the SDK
  documents that it sends `requestPermission: false` when none is provided.
- The compiled lifecycle surface includes auth observation, models, create,
  observer-before-send, abort, disconnect and resume. These functions are compile
  contracts, not an implemented durable delivery loop. Tests do not call them.

## Remaining gates before enabling Copilot in Hive

1. Wire the **existing app-owned GitHub connection** to the core through the real
   app, with owner/account binding, token expiry/refresh and cancellation rules.
   Do not extract credentials with another standalone Keychain helper.
2. Verify an eligible real account, no inherited account fallback, available models,
   and one explicit harmless response. This fixture's handshake is not account proof.
3. Implement typed domain events and terminal reconciliation; durable turn intent,
   session writer fencing, cancellation, reconnect and delivery-unknown handling.
4. Bind the scoped Hive broker with selected tools; enforce approvals and deny
   unsupported permission/input requests on every create/resume.
5. Test revoked/expired credentials, quota/policy denial and crash recovery without
   duplicate jobs. No BYOK fallback, automatic credit purchase or silent provider switch.
6. Package/version-check the runtime on macOS, Windows and Linux and run the same
   acceptance matrix. This initial local check is macOS only.

No SDK or runtime redistribution decision is implied by this spike. MIT SDK license
and runtime distribution terms/version provenance must be retained in packaging.

Sources checked 2026-09-16:
- https://github.com/github/copilot-sdk/tree/main/rust
- https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/authenticate
- Published 1.0.14 source: `src/lib.rs` (process environment and ClientOptions),
  `src/mode.rs`, `src/handler.rs`, `src/types.rs`, and `cli-version*.txt`.

Sif your friendly Codex Agent

## Observed result — 2026-09-16, macOS Apple Silicon

Four unit tests pass. Clippy with warnings denied and formatting pass. The bundled
SDK/runtime started, answered ping and stopped successfully with the invalid fixture
token and Empty mode. No real credential was accessed; no model request was made.
Logs: `/private/tmp/hive-copilot-conformance.log`, `/private/tmp/hive-copilot-clippy.log`,
`/private/tmp/hive-copilot-handshake.log`. This is partial P2 acceptance: the remaining
real-account and cross-platform gates above are still open.
