# ADR-015: The Desktop App as a Local AI Workstation, With Promotion to the Hive

**Status:** Proposed · **Date:** 2026-09-08 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** this conversation

## Context

Every ADR so far has treated the desktop app (ADR-010) as a background utility: pair a machine, set trust switches, watch it earn Honey doing other members' cards. Jack's ask in this conversation inverts the emphasis: he wants the desktop app to be the thing a user opens first for *any* AI work -- chat, coding, research, whatever -- with the Hive as one option for where that work runs, not the reason the app exists. In his words, the goal is for this to become "the go-to experience for AI on a user's machine," with the Hive and its community as an added capability rather than the whole product.

This is bigger than ADR-014 category 9 (Operations / personal automation). That category was scoped as one project type among nine. This ADR is about the app's identity: every category in ADR-014 -- Software, Research, whatever comes later -- should be runnable either as a private, local, unmetered session on the user's own machine, or as a funded project submitted to the Hive's marketplace. The category taxonomy doesn't change; where the work executes does.

The useful architectural insight this produces: OH Hive's existing sandbox (ADR-006) is deliberately zero-trust because a Hive-distributed card might land on a stranger's machine -- that's *why* it can't run a real compiler, and why the cross-platform build/test discussion earlier this session concluded a second execution tier would need its own security review before it could run real toolchains. A local session run by the user, on the user's own machine, at the user's own request, has no such problem. The user *is* the trusted operator. This means the "can't compile or test" gap identified in the Community Chat retrospective is not actually one problem -- it's two, and the local one is far easier to solve than the Hive-distributed one, because the hard part (untrusted execution) simply isn't present.

Three decisions were open enough that they needed Jack's call rather than a default (asked via clarifying questions in this conversation):

- **Local economics:** free, no ledger. Work that never touches another member's machine posts nothing to Honey/the ledger.
- **Local permissions:** full machine access by default, matching Claude Code's own model, not a scoped sandbox that needs an explicit escalation step.
- **Local vs. Hive:** movable per-project, not fixed at creation and not movable per-card. A project can be promoted to the Hive later; the whole project moves together.

## Decision

### 1. Two execution modes, one project/card schema

`hive.projects` gains an `execution_mode` column: `'local'` (default for new projects created in this flow) or `'hive'` (today's existing behavior, unchanged). The same `hive.projects`/`hive.cards`/`hive.card_outputs` tables back both modes so the project board, the forum (per-project discussion, shipped tonight), and the zip export all work identically regardless of where a project's cards actually ran. What differs is entirely in claiming and execution:

- **`execution_mode = 'local'`:** only nodes owned by the project's own member (`hive.nodes.member_id = hive.projects.owner_id`) may claim its cards. `node_claim_card`'s existing `account_balance(fund_account_id) > 0` gate does not apply -- local projects need no fund balance at all. No `hive.post_txn` entries are posted for local card execution; there is no counterparty to pay and Jack's call was that this should be free, full stop.
- **`execution_mode = 'hive'`:** unchanged from every other ADR this session -- any matching node may claim a card, the project needs a funded balance before anything runs (ADR-002), and Honey moves through the ledger exactly as it does today.

### 2. Local execution is a second engine, not a stricter sandbox

The existing sandbox (`sandbox.rs`, ADR-006) stays exactly as restrictive as it is today for Hive-distributed cards -- this ADR does not loosen it. Local-mode cards run through a **separate execution path** that does not use wasmtime/WASI at all: real filesystem access and real command execution (`bash`, `git`, a language's actual build/test tooling) against the user's own machine, using tool definitions equivalent to Anthropic's published agentic tools (bash, file edit/read/write), not `ohhive-core`'s `Sandbox::run`. Per Jack's call, this defaults to full machine access rather than a project-folder-scoped sandbox -- the desktop app is, by design, exactly as capable locally as Claude Code is, from the first run.

This also resolves the one-shot-card limitation that blocks a real build-test-fix loop in Hive mode (worker.rs's tool step runs once per card, ADR-014 S5's correction). Local mode has no trust reason to stay one-shot: it should run a genuine multi-turn agentic loop -- act, observe the real result, retry -- for as long as a card's work requires. This is new code, not a relaxation of `worker.rs`'s existing step logic; the two loops (Hive's one-shot sandboxed step, local's multi-turn real-tool loop) are allowed to diverge because they solve different problems under different trust assumptions.

Provider calls from a local session still cost real money even though no Honey moves -- the existing provider-first key order (member's own Anthropic/OpenAI/Nous key from Vault, then the hub's fallback key) applies here exactly as it does in the `interview` function today. "Free" means free of Hive's marketplace economics, not free of the underlying API bill when a hub-fallback key is used; BYO-key local sessions cost the member nothing beyond their own provider bill either way.

### 3. Promotion: one RPC, one direction, whole project

`hive.project_promote_to_hive(p_project_id)`: owner-only, requires `execution_mode = 'local'`, sets it to `'hive'`. From that moment, the project behaves exactly like any other Hive project -- it needs a funded balance before any further card is claimed by any node, including the owner's own, and any member's node can now pick up its work. Cards already `done` under local execution keep their existing (unpaid, local) `card_outputs` rows as history; nothing is retroactively charged. There is no demotion path in this ADR (Hive back to local) -- once a project is open to the marketplace, treating it as private again raises questions (what happens to another member's in-flight leased card?) this ADR doesn't need to answer yet, so it's left as an explicit non-goal.

### 4. The desktop app becomes a foreground workbench, not just a tray icon

ADR-010 scoped the desktop app as registration + tray + Preferences + earnings. This ADR adds a primary window: a chat/session view backed by the local execution engine (S2), a project/card board (the same board component already built for the web app, reused rather than rebuilt), and a "Promote to Hive" action on any local project. Pairing-as-a-compute-node (ADR-010's original scope) doesn't go away -- it becomes one panel among several, not the whole app.

### 5. Categories apply across both modes

ADR-014's nine categories, and the tailored interview/card templates for Software and Research specifically, are mode-agnostic. A Research project can run entirely locally (the member's own machine does the fan-out... except see the open question below) or be submitted to the Hive for other members' nodes to help. Nothing about the taxonomy changes; this ADR only adds *where*.

## Consequences

### Positive
- Solves the "can't actually compile or test" gap for the case that matters most immediately -- a single user working on their own machine -- without touching the Hive's trust model at all.
- Gives the product a real identity distinct from "a client for the Hive marketplace": a capable local agent workstation that happens to also open onto a community compute market, rather than the reverse.
- Reuses the existing project/card/board/forum/export schema and UI entirely; the only new schema surface is one column and one RPC.
- Jack's own stated goal -- one system to coordinate his local and cloud agents -- is now the app's central design, not an implicit side effect of category 9.

### Negative
- Local execution is a genuinely separate engine (real tool use, multi-turn loop) sitting alongside the existing sandboxed one-shot worker -- two things to build and maintain, not a variant of one.
- Full machine access by default is a real, deliberate security posture, not a conservative one. It is Jack's explicit choice, recorded here rather than defaulted quietly, but it means a first-run user gets Claude-Code-level capability with no scoping step in between.
- "Free, no ledger" for local work means the app has no unified spend view across local + Hive work for a member who does both -- a BYO-key local session's real provider cost is invisible to OH Hive's own accounting, visible only in the provider's own billing.

### Risks & mitigations
- **Full machine access surprises a user who didn't expect it.** Mitigation: this needs its own explicit first-run consent/disclosure in the desktop app, even though no scoped-permission mode is being built -- "full access, here's what that means" said once, clearly, is different from silent default behavior.
- **Local sessions with no audit trail make debugging a bad outcome hard.** Mitigation: local sessions should still log their tool calls (file writes, commands run) somewhere the user can review, even though nothing is posted to the Hive ledger -- this is an observability need, not an economics one.
- **The two execution engines drift apart in capability over time**, making "promote to Hive" produce a surprising downgrade (a project built assuming full local tool access suddenly can't do the same things once it's Hive-distributed and sandboxed). Mitigation: the promotion RPC (S3) should be paired with clear messaging about what changes, not just a schema flip; genuinely reconciling the two loops' capabilities is a later problem, not one this ADR solves.

## Open questions
- Does a locally-run Research project's fan-out (ADR-014 S4) mean the local agent spawns and manages parallel sub-tasks itself (multiple local agent invocations), or does "local" implicitly mean single-threaded until a project is promoted? (Open -- ADR-014's fan-out mechanism was scoped for Hive's `spawn_child_card`, not for a single local machine coordinating its own parallelism.)
- What exactly does the local execution engine run on -- a new Rust-side agentic loop inside `ohhive-core`/the desktop app, or does the desktop app shell out to something like the Claude Agent SDK directly? (Open -- S2 states the requirement, not the implementation.)
- Does local-session history (chat transcripts, tool-call logs mentioned in the Risks section) live in the same Supabase project as everything else, or purely on-device? Given "free, no ledger," there's an argument for keeping local session data local-only too, but that would put it outside the reach of "the go-to experience" if the user switches machines. (Open.)
- Should the desktop app's new foreground chat surface be able to start a project in either mode from the same entry point, or does the user pick "local" vs "Hive" as a distinct first step? (Default assumed by this ADR: same entry point, mode chosen as part of the interview/creation flow, matching how category selection is expected to work in ADR-014 -- but not explicitly decided.)

## Related
- ADR-002-honey-economics (what local mode explicitly does not participate in)
- ADR-006-agent-runtime-and-sandbox (the trust boundary this ADR deliberately does not touch for Hive mode, and the reasoning for why local mode can be less restrictive)
- ADR-010-node-desktop-app (the surface this ADR substantially extends)
- ADR-014-project-categories-and-verification (categories apply across both execution modes; corrects the assumption that fan-out is free to reuse)
