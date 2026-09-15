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
