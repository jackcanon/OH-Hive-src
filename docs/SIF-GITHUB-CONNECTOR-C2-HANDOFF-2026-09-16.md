# C-2 GitHub connector implementation

Sif your friendly Codex Agent

Implemented locally; live account acceptance is NOT complete. No OAuth registration, token reading, account login, repository mutation or issue publication was performed by this session.

## Verified provider facts

GitHub's current [OAuth authorization documentation](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps) explicitly permits a loopback redirect port different from the registered callback port and supports S256 PKCE. It also now documents optional expiring OAuth App access tokens and refresh-token rotation. This corrects the handoff's uncertain fixed-port requirement and historical non-expiring-only assumption. JSON token responses require Accept: application/json.

## Built

- `GitHubConnector.swift`: deliberate provider-specific manager, browser + random-port loopback callback, state and PKCE, callback path/method/duplicate-parameter validation, timeout. Registered callback: `http://127.0.0.1/oauth2callback`.
- Separate Keychain service `media.happyjack.hive.github`; checked writes/readback. Tokens/expiry stored together as one session value. Google service unchanged. Both expiring and non-expiring token responses supported; expired sessions refresh before actions and identity is checked after token exchange/refresh.
- App-owned instance injected into all scenes. Settings has connect/disconnect, progress/errors, and an actual read-only API caller: load up to 100 public repositories, open validated GitHub HTTPS links. Uses [authenticated repository listing](https://docs.github.com/en/rest/repos/repos#list-repositories-for-the-authenticated-user) with visibility=public.
- Requests `read:user`, no repo write scope. This is not git push authentication, private-repository access, automatic bug mirroring, or model tool access. First action was chosen conservatively while an optional user preference question was unanswered. No publication destination invented.
- Existing user-supplied client setup retained pending the shared-client product decision. Desktop client secrets cannot be treated as confidential. No embedded service credentials.

## Checks and limits

Swift app/test build passes, all 17 tests pass. Four GitHub tests cover optional token lifetime decoding, repository-link validation, Keychain status/readback validation, callback route/method/duplicate-state rejection. Existing Google and Bots suites pass. Whitespace check passes.

Not verified: real OAuth callback/exchange, refresh against GitHub, API repository result, actual Keychain provider disconnect isolation, UI interaction, or live network error/revocation flow. Service strings are distinct in code; that is not a substitute for the requested two-provider live disconnect test. Loopback transport follows existing Google implementation and still reads a bounded single receive; fragmented HTTP handling/full network lifecycle tests remain follow-ups. Token scopes may reflect prior grants to the same OAuth app; no broad scope requested here. UI lists only public repositories and caps at 100.

No bundle rebuild, commit, push, deployment or remote CI result. Google C-1a–d and GitHub C-2 now have code, but connector queue acceptance requires registered clients and real account tests. Optional image export remains gated behind C-1 verification. Older S-4 Honey follow-ups and S-5 held-chain UI remain open.
