# ADR-036: Git workspaces and GitHub workflows

**Status:** Proposed — drafted 2026-09-15 on Jack's go-ahead; content not yet reviewed by him.
**Date:** 2026-09-15
**Author:** Claude (Loki).
**Deciders:** Jack (pending review).

## Problem

Sif's review of existing ADR coverage found a real gap: today, `coder.rs`'s `prepare_workspace` either uses an explicit local `workspace_path` or clones `spec.repo_url` with raw shell `git clone` into a fresh directory that gets wiped and re-cloned on every retry (`clone_dir_for`). There is no authenticated GitHub account connection, no repo picker, no credential lifecycle, no task-scoped branches, and no PR review/merge flow — "today it's raw shell git only." This blocks private-repo access without hand-configured local git credentials, and blocks any PR-based review step for work a card produces.

It also duplicates a problem ADR-034 already has to solve: the GitHub Copilot adapter needs its own Hive-owned GitHub App with device-flow login (ADR-034 §3, from Sif's implementation plan). Building two separate GitHub connections — one for Copilot's SDK auth, one for git operations — would be two logins asking for the same thing.

## Decision

**One GitHub connection, two consumers.** A single Hive-owned GitHub App (device flow, no client secret in the desktop binary, matching ADR-034's Copilot account-setup design exactly) is what both `coder.rs`'s git operations and the Copilot adapter authenticate through. Settings shows two distinct "Connect" affordances (git workspaces vs. Copilot) even though they can share one underlying token when scopes overlap — a member may want authenticated git access without wanting a Copilot coordinator, or vice versa, and the two consent screens should say what they're actually for.

Once connected:
- **Repo picker.** List the connected account's accessible repositories (paginated) so a project/card binds to a picked repo instead of a hand-typed clone URL.
- **Persistent clone + worktrees, not wipe-and-reclone.** `prepare_workspace` gains a mode that keeps one persistent local clone per repo and creates a `git worktree` on a dedicated branch (e.g. `hive/<card-id-prefix>`) per card, instead of a full fresh clone every run. Real efficiency win on top of the workflow win: no re-fetching the whole repo on every retry.
- **PR review/merge.** Once a card's work passes acceptance, offer to open a PR from its task branch using the stored token. Never auto-merge without an explicit user action. The PR URL becomes part of the task's durable receipt (same receipt shape ADR-030/032 already use).
- **Credential lifecycle.** Token stored in OS-secure storage, isolated per Hive install the same way the Codex/Copilot runtime homes are isolated (ADR-033 §4, ADR-034). Disconnecting immediately blocks new git operations through this path; it does not delete existing local worktrees/branches — a member's in-progress work doesn't vanish because they revoked a token.
- **Nothing about today's behavior is removed.** `workspace_path`/`repo_url` with plain local git credentials keep working exactly as they do now — connecting GitHub is optional, mirroring the BYOK-vs-subscription split everywhere else in the app (local-only members are never required to connect anything).

## Consequences

Real private-repo support and a described git story instead of raw shell calls with no account model. Registering the actual GitHub App is Jack's action, not code (org/account-level, can't be done from an agent session). Worktree-based workspace prep is a real behavior change to `prepare_workspace` and needs care not to regress the existing `workspace_path`/`repo_url` paths or ADR-032's coordinator card flow.

## Open questions (defaults noted, override if wrong)

- Exact OAuth scope set for the App (repo contents read/write, PR create, on explicitly granted repos only) — default: narrowest scope that satisfies the flows above, expand only if a specific flow needs more.
- Whether worktree cleanup after a PR merges/closes is automatic or user-triggered — default: user-triggered, matching "PR review/merge is never automatic" above.

## Related records

ADR-030 (submissions), ADR-031 (external adapter), ADR-032 (coordinator cards, whose task branches this feeds), ADR-034 (shares the GitHub App / device-flow mechanism for Copilot).

Claude (Loki).

## Research review — 2026-09-15 (Sif)

Status remains **Proposed**. Jack requested a fresh code/plan audit focused on low-friction GitHub publication. See [Git/GitHub workflow audit](../docs/SIF-GIT-GITHUB-WORKFLOW-AUDIT-2026-09-15.md) for evidence, official sources, user flow, staged implementation and acceptance tests. Recommended amendments: durable workspace recovery/publication journal; shared identity with separate repository/Copilot capabilities; device-token refresh and optional installation broker; task branch publication policy separate from merge approval; exact-SHA receipts and fleet publisher fencing. These are recommendations, not implemented or accepted changes to this ADR's decision.

Sif your friendly Codex Agent

## Amendment — 2026-09-16: "no client secret" was a GitHub property, not a universal rule

**Decided by Jack, 2026-09-16.** The Decision above says "device flow, no client secret in the
desktop binary." That phrasing was written with GitHub in front of us and then applied to
Google, where it is not achievable. This amendment says what the rule actually is, so it stops
being over-read.

### What forced this

Sif's live test reached the loopback callback after Google consent and the token endpoint
refused the code exchange:

```
invalid_request / client_secret is missing
```

The reasoning that produced the original wording — "this is a Desktop client, PKCE protects the
code, therefore no secret is needed" — conflates two separate things. **PKCE protects the
authorization code against interception. It does not stand in for client authentication.**
Every vendor combines the two differently, and generalising from one to another is what went
wrong here (twice in one day: the same inference was made about GitHub's web flow before Sif
checked the docs and corrected it).

All three Google paths were checked before amending, specifically looking for the device-flow
escape hatch that works for GitHub:

| flow | client secret | scopes |
|---|---|---|
| Desktop app + PKCE (our registration) | **required** — proven by live test | any |
| Web application | required, genuinely confidential | any |
| Limited-input device ("TV") | **required** at the polling step | `drive.file` only; **no Gmail scope exists** |

There is no secretless Google flow. GitHub's device flow is the exception, not the pattern.

### The rule, restated

The intent of Decision 1 is, and always was: **the desktop binary ships nothing that grants
server-side authority on its own.** Per vendor:

- **GitHub** — device flow, no client secret, no private key. Unchanged, and it keeps the
  stronger property because GitHub's device flow allows it.
- **Google** — the installed-app client ID **and** client secret ship in the binary as build
  configuration, because Google's token endpoint requires it. PKCE is retained.

This is consistent with Google's own threat model, which does not treat that value as
confidential:

> "Installed apps are distributed to individual devices, and it is assumed that these apps
> cannot keep secrets."
> — Google, *OAuth 2.0 for Mobile & Desktop Apps*

The installed-app secret grants nothing on its own: a user must still complete consent, and the
resulting token lands in that member's own Keychain. Hive never sees member data or member
tokens. This is what `gcloud`, `rclone` and every other desktop Google client do, because the
flow requires it.

### Constraints that come with this

- Both values live in `apps/desktop-swift/config/oauth-clients.sh` as **build configuration**,
  named as such. They are not stored anywhere that implies confidentiality — no Keychain, no
  secret store, no `.env` that looks like it holds real secrets. Mislabelling them would invite
  someone to treat a future real secret the same way.
- **The honest risk, recorded rather than waved away:** anyone can extract the ID+secret pair
  from a shipped binary and build an application that displays **"Loki's Den"** on Google's
  consent screen. That is a phishing surface and it is the actual reason Google asks for the
  parameter. It is an accepted, industry-wide condition of shipping a desktop Google client, but
  it is not zero risk.
- **The escape hatch, if that risk ever becomes real:** a hosted confidential client, where Hive
  brokers the code exchange with a genuinely server-side secret that can be rotated and revoked.
  It was rejected *now* — not on difficulty, but because it routes every member's Google token
  exchange through Hive infrastructure, which contradicts the "your own machine, your own data"
  line ADR-023 and ADR-026 draw, and makes Hive an availability dependency for a member
  connecting their own account. It is a different registration and a different architecture, not
  a proxy patch.
- Scope tiering under ADR-026 §3 is unchanged: `drive.file` and `gmail.send` only.
  `gmail.readonly` and full `drive` remain a separately decided v2 with a CASA assessment.

Full analysis, including the flow-by-flow check:
[`docs/LOKI-GOOGLE-CLIENT-SECRET-DECISION-2026-09-16.md`](../docs/LOKI-GOOGLE-CLIENT-SECRET-DECISION-2026-09-16.md).

Claude (Loki), on Jack's decision.
