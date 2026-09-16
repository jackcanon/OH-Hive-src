# Shared OAuth decision and worker refusal handoff

Sif your friendly Codex Agent

Read new commit `9d32307` before proceeding. It supersedes prior connector notes retaining personal OAuth client setup. Connector ownership remains Sif's; Claude owns CLI advertisement changes in `crates/hive/src/main.rs`.

## Worker assignment implemented

`Worker::run_card` now dispatches only text, code and speech to their implemented paths. Every other modality, including unknown strings, calls fail_card once, emits Failed and returns before model selection, tools or Draft state construction. It does not release/requeue unsupported work. Existing feature-gated speech/code behavior remains.

`worker_modality_tests.rs` uses a backend that panics if invoked and a Hub double that permits only failure reporting. Image/video/music/unknown/empty/case-mismatched inputs each produce exactly one failure event; inference, completion/payment, checkpoint and release would fail the test. This changes new binaries only; older deployed binaries still require update/server gating. No deployment performed.

## Google shared client implemented

Members no longer see client ID/secret fields. GoogleAuthManager reads only the public publisher ID from signed bundle Info.plist via SharedConnectorConfiguration; it sends no client_secret and does not read legacy personal-client fields. Existing tokens must match the new oauthClientID binding or the member reconnects. Build script accepts `HIVE_GOOGLE_OAUTH_CLIENT_ID`, validates it and inserts `HiveGoogleOAuthClientID` before signing. Missing configuration yields a disabled Connect button and honest explanation. No ID was invented; publisher registration/configuration and actual OAuth consent verification remain required. Bare swift run has no configured bundle ID and therefore does not offer a working connection by default.

Source: [Google native app OAuth](https://developers.google.com/identity/protocols/oauth2/native-app) marks the client secret optional for this flow. This is documentation-based implementation, not live Google acceptance.

## GitHub exception requiring a flow decision

Shared Hive client is accepted; the technical assertion that PKCE removes GitHub's secret requirement is not supported by current docs. [GitHub best practices](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/best-practices-for-creating-an-oauth-app) explicitly requires a client secret except for device flow. [Authorization documentation](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps) confirms the browser exchange requirement. PKCE is additional protection.

Asked Jack to choose a Hive-hosted sign-in exchange service or device-code flow. No answer at writing; dependent implementation is pending. Removed the personal OAuth credential fields from GitHub Settings and disabled new connection setup with shared-sign-in configuration copy. Existing GitHub manager browser implementation remains internal pending replacement; do not treat its old BYOK implementation as approved shared-client wiring. Existing saved GitHub sessions can still perform the prior read-only action; a client-binding migration is needed with the selected new flow. No secret was embedded or moved to a new location.

Google email-header and Keychain-write bugs cited in the new handoff were already addressed in preceding work. Binary Drive uploads remain separate; current exports are text/Markdown only.

No commit, push, bundle rebuild or deployment. Production protection, interactive account acceptance and real Google/GitHub actions remain unverified.

Verification: worker refusal regression passes with `--features hub` and `--all-features` (targeted tests, not the entire Rust suite); all 18 Swift tests pass; shell syntax and whitespace checks pass. Hub-only build reports the pre-existing unused spawned_card_id warning. Actual OAuth/Keychain/UI and production behavior not exercised.


Update: GitHub flow choice resolved by ADR-036/Claude `75ef7f0`; shared GitHub App device flow now implemented. See the latest section in `SIF-GITHUB-CONNECTOR-C2-HANDOFF-2026-09-16.md` for configuration, verification and remaining live acceptance.
