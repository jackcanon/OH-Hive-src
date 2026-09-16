# ADR-026: Personal Connectors — Google Drive/Gmail for the Swift App

**Status:** Proposed · **Date:** 2026-09-13 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** Jack: "we need to figure out plugins and connectors... we need to be able to connect to like google drive and gmail etc."

## Context

Jack's framing was "we already built MCPs" — true, but a materially different shape than what Drive/
Gmail need. ADR-023 (task #177) lets a *member* register an MCP server *they run themselves* as a
local stdio subprocess on their own machine; Hive's trust boundary is "the member's own hardware,"
and the ADR explicitly deferred remote transports (`http`/`sse`) as out of scope. Google doesn't
offer a "run this locally" option for Drive/Gmail at all — there is no subprocess to spawn. Reaching
either one requires a real OAuth 2.0 authorization-code flow against Google's own servers: a consent
screen, a redirect back into the app, and a refresh token good until the member revokes it.

This is also a first for Hive in a different sense: every existing multi-party integration has
deliberately avoided OAuth. ADR-020 (the Telegram/Discord/Slack bridge) chose "linked accounts, not
OAuth-per-platform" on purpose. BYOK chat keys (`byok_keys.rs`) are a member-pasted API key, not an
OAuth grant. This ADR is the first time Hive would actually run an OAuth flow, and it's worth being
honest that this brings real, Google-specific process cost that has nothing to do with Hive's own
architecture — see Decision 3.

Three scope questions were open; Jack decided all three directly (2026-09-13, in chat):

1. **Where does this live?** Swift app only — not the card/project system, not the web app, this
   pass. A card's `required_capabilities` gaining a Drive/Gmail connector option is explicitly out of
   scope here; if that's wanted later, it needs its own ADR extending ADR-023's ownership/trust model
   to remote OAuth tokens rather than local subprocesses, which is a meaningfully bigger problem
   (a compromised or malicious card could exfiltrate through a live Gmail connection in a way it can't
   through a member's own local MCP server, since the member chose what that server can do).
2. **Who hosts the OAuth app?** Hive hosts one shared Google Cloud OAuth client — members click
   "Connect" and approve Hive's own app, not BYOK credentials they'd have to generate themselves in
   Google Cloud Console. Chosen over BYOK specifically because almost no member would actually do the
   Google Cloud Console setup required for a personal OAuth client; a shared app is the only version
   of this that gets used.
3. **Driving need:** foundational plumbing, no specific workflow yet — research and propose an ADR
   before building, same process ADR-023 went through. This document is that proposal.

## Decision

### 1. Client type: Google "Desktop app" OAuth client, embedded directly in the Swift app

Google's own guidance for installed/native apps (as opposed to a confidential web-server client)
issues a client ID and secret for the "Desktop app" credential type and explicitly does not treat
that secret as confidential — it's meant to be embedded in the app binary. Google still requires the
secret *alongside* PKCE for the token exchange (unlike some other providers, PKCE alone can't replace
it for Google's implementation) — so both ship in the app, and neither needs a server to hold them.
This means, concretely: **no backend proxy is needed for the OAuth flow itself.** The Swift app runs
the whole authorization-code-plus-PKCE exchange directly against Google's endpoints via
`ASWebAuthenticationSession` (the system browser sheet, not an in-app WebView — required by Google's
policy for OAuth to Google services since the 2016/2017 embedded-WebView ban, and the correct native
API for it besides). This is a significant scope reduction from what I assumed before checking: no
new Supabase schema, no new Edge Function, no `hive.*` table for this at all in v1 — it's pure Swift
app + Keychain, matching the "Swift app only" scope decision as literally as possible.

### 2. Token storage: local macOS Keychain, per machine, not synced through Hive

Access/refresh tokens are stored in this Mac's own Keychain, scoped to this app, never sent to or
through any Hive server. Consequence, stated plainly rather than discovered later: connecting Google
on one paired Mac does **not** connect it on another of the member's machines — each Mac needs its
own "Connect" click. This is the right v1 trade-off for "foundational plumbing, no specific task yet"
(the simplest version that's still real and useful), not an oversight; **open question below** on
whether a later pass should let a member sync an encrypted token through their own account instead,
once there's an actual cross-machine use case asking for it.

### 3. Scope selection is the real gating decision, not architecture

This is the part of the research that changes the rollout plan. Google classifies OAuth scopes into
tiers with very different verification cost, and **the tier is set by the single most sensitive scope
requested** — mixing one cheap scope with one expensive one makes the whole app expensive:

| Scope | Tier | Verification cost |
|---|---|---|
| `drive.file` (only files the app created, or the member explicitly picked via Google's file picker) | Non-sensitive | Basic app registration only |
| `gmail.send` (send-only, cannot read anything) | Sensitive | Brand verification (logo, privacy policy, domain ownership) — days, not weeks |
| `gmail.readonly` / `gmail.modify` / full `drive` (read/list arbitrary files) | **Restricted** | A Google-empanelled third-party security assessment (CASA Tier 2) — commonly several weeks, **plus mandatory annual re-verification** for as long as the app keeps those scopes |

**v1 ships only `drive.file` and `gmail.send`.** This gets a real, useful connector (save a report to
Drive the member picks the destination folder for; send an email on the member's behalf) into
members' hands without Hive ever going through CASA. **Reading Gmail, or listing/searching arbitrary
Drive files without the member picking them first, is explicitly a v2 decision** — not blocked
technically, but blocked on Jack choosing to start a multi-week Google security assessment process
for an app that, as of this ADR, has no verified production use of restricted scopes yet. That
should be a deliberate call when there's an actual feature needing it, not a default reached by
accident because "Drive and Gmail" sounded like one ask.

### 4. What "connect" actually grants, concretely

- **Drive:** the member picks specific files/folders via Google's own file picker (or the app creates
  a new file directly) — `drive.file` scope means Hive's connector can only ever see files it created
  or the member explicitly chose, never browse their whole Drive.
- **Gmail:** send-only. The connector can compose and send a message as the member; it cannot read
  their inbox, list messages, or search anything. "Summarize my email" is a v2 feature gated on the
  CASA decision above, not something v1 can do even partially.

### 5. Where this surfaces in the app

A new "Connectors" section, most naturally a Settings tab (matching the existing Cloud Keys/Node/
Server tab pattern) with a per-service card: connected/not, "Connect"/"Disconnect" button, and the
same "here's exactly what this can and can't do" framing as Decision 4 — not vague "Drive access"
copy. Whether the on-device chat and the feedback assistant should get tool-calling access to a
connected Drive/Gmail (the way `NodeStatusTool` exposes node status today) is a natural fast-follow,
not part of this ADR's v1 — connecting an account and a model being able to act on it are separable
decisions, and the second one deserves its own explicit look at what a chat session should be allowed
to do unprompted (send an email autonomously is a very different risk than reading node status).

## Amendment (2026-09-13) — "lots of different services," and make it a before-1.0 goal

Same conversation, Jack: "we also want to be able to connect this to lots of different services so
if we need to research how that gets done lets make that a goal for before 1.0." This changes the
framing from "build a Google connector" to "build the *thing that lets you keep adding connectors*,"
with Google Drive/Gmail as the first two, not the only two. Two real paths, not yet decided between:

**Path A — build a small generic OAuth abstraction in-house.** Decision 5 above already points this
way: a `ConnectorProvider` config (client id, authorize/token URLs, scopes, PKCE requirement) plus one
generic `ASWebAuthenticationSession`-driven flow and one generic Keychain-backed token store, so
adding e.g. Slack or Notion later is "write one config + implement that service's specific API
calls," not "rebuild the OAuth dance." Fully in Hive's own control, zero new vendor dependency, but
Hive carries every provider's own quirks (token refresh timing, rate limits, revocation edge cases)
forever, one at a time, as each gets added.

**Path B — use existing connector infrastructure instead of building it.** This exact problem
("many services, one OAuth story") is a solved-for-money category. Two live options as of this
research: **Nango** (open source, self-hostable for free, ~900 pre-built API integrations, geared at
products that ship integrations to customers and want the option to self-host for data control) and
**Composio** (managed, AI-agent/MCP-native, ~500 pre-built tools, explicitly positioned for "personal
automation or internal productivity tool" use cases that stay inside its pre-built catalog — which
is a close description of what this app's connectors are for). Buying this gets dozens of providers
almost immediately instead of one at a time, at the cost of a real dependency: member OAuth flows
(and, depending on the option, token custody) would run through a third party's infrastructure,
which sits awkwardly next to this app's whole personal-fleet/BYOK/on-device positioning so far —
every other trust decision this session has leaned toward "your own machine, your own account,
minimal third parties in the loop" (Private Fleet, BYOK keys, on-device chat). Nango's self-hosted
option is the version of Path B that conflicts least with that — Hive would still run the actual
OAuth broker, just not have hand-written it — but it's still new infrastructure to operate.

**Decided (2026-09-13, Jack): Path B, Nango, self-hosted.** Closest of the two vendor options to
Hive's existing trust posture — Hive runs the actual OAuth broker on its own infrastructure rather
than a third party's cloud holding member tokens; Nango just means Hive didn't have to hand-write the
broker. Confirmed self-hosting needs (2026 docs): everything encrypted at rest with a key Hive owns,
and production deployments should point at an external Postgres rather than the bundled
Docker-Compose database. **Recommendation, not yet decided:** run that Postgres as its own dedicated
instance, separate from the Supabase `hive.*` schema/project — Nango's job is custody of OAuth
tokens for potentially many providers, and keeping that store isolated from the main multi-tenant
RLS-governed schema matches every other isolation-by-design call this session has made (ADR-023's
member-server isolation, the control-plane pilot's separate login/route). Where the Nango service
itself runs (a new dedicated box vs. one of the existing Linode regional servers — Chicago,
Amsterdam, Sydney) is an open infrastructure decision, not resolved here.

Google Drive/Gmail (this ADR's main body) still ships under Path A's small custom flow for v1 — Nango
becomes the path for providers 3 through N once it's stood up, not a blocker on shipping Drive/Gmail
first. Queued for Sif as the next infra research/setup task (self-hosted Nango deployment: where it
runs, its dedicated Postgres, and how the Swift app's connector flow talks to it) via CONTINUITY.md.

## Amendment (2026-09-14) — reversed: Path A (custom in-house), not Nango

Sif's hosting proposal (`docs/SIF-NANGO-HOSTING-PROPOSAL-2026-09-13.md`) surfaced two things the
2026-09-13 Nango decision above didn't account for, and reopened the Path A/B choice:

1. **Feature correction.** Free self-hosted Nango is an Auth/Proxy foundation only — Nango's own
   feature-availability docs mark prebuilt syncs/tools/triggers, tool calls, webhooks, and the MCP
   server as unavailable on the free self-hosted edition; that's Enterprise-only, commercial pricing
   not published. The "~900 pre-built integrations" framing used above (and in Loki's summary to
   Jack) describes that paid catalog, not what self-hosting for free actually gets Hive. Buying Path
   B for free would still mean hand-building every provider's actual actions ourselves — the same
   work Path A always required — while additionally standing up and operating someone else's auth
   broker underneath it.
2. **License.** Nango's root license is ELv2, which restricts offering a substantial portion of the
   software as a hosted/managed service. Not a legal determination that Hive-internal use is
   forbidden, but a connector broker serving Hive's own members is close enough to that restriction's
   shape to need explicit confirmation before production — an unresolved gate Path B would have
   carried.

With the "buy dozens of providers almost for free" case gone, Path B's remaining offer was just "a
broker Hive didn't have to hand-write," at the cost of new standing infrastructure (Sif's proposal:
dedicated VM + separate Postgres/cache, ~$78–88/mo) and an unresolved license question. **Decided
(2026-09-14, Jack): Path A, custom in-house**, reversing the 2026-09-13 Nango decision. No Nango
deployment, budget, or license review proceeds. Sif's proposal remains useful reference (the Swift
connect-flow design — broker-mediated sessions, server-only ownership checks, polling over webhooks,
explicit disconnect/revoke — is sound guidance for how Path A's own flow should work even without
Nango underneath it) but is not being built on top of.

**Open, not yet decided:** whether "providers 3 through N" under Path A extend Decision 2's
per-machine Keychain model (same as Google v1 — simplest, zero new infrastructure, but each Mac
connects separately and there's no story for the web app or a future non-Swift surface), or whether
Hive still wants one small Hive-operated broker service — hand-built instead of Nango, same rough
infrastructure shape Sif costed out — so a connection is made once per member account rather than
once per machine. This determines whether Sif's infra proposal (dedicated VM/Postgres) gets reused
for a hand-rolled broker or shelved entirely in favor of pure on-device Keychain storage.

## Consequences

### Positive
- No new backend surface for v1 — pure Swift + Keychain + Google's own endpoints, genuinely matching
  "Swift app only."
- `drive.file`/`gmail.send`-only means this can ship and actually be used by real members without
  waiting on Google's security assessment queue.
- Establishes the pattern (Desktop OAuth client, `ASWebAuthenticationSession`, Keychain storage) that
  a future Slack/Notion/whatever connector can reuse directly.

### Negative
- Per-machine connection (Decision 2) is a real limitation a member will notice on their second Mac.
- Send-only Gmail and picker-only Drive are meaningfully less capable than what "connect my Google
  account" implies to most people — needs honest UI copy so it doesn't read as a bug.
- Two separate OAuth consent screens' worth of Google Cloud Console configuration (Drive, Gmail) to
  build and maintain, plus normal OAuth-client housekeeping (redirect URI registration, Google's own
  periodic re-consent for long-unused grants) that Hive has never had to do before.

### Open questions
- Should a v1.1 let a member's Google connection sync to their other paired Macs through their own
  Hive account (encrypted refresh token through Supabase, node-key-gated the same way BYOK keys are),
  or does per-machine connection turn out fine in practice? No evidence yet either way.
- Does the on-device assistant (`ChatEngine`/`FeedbackAssistant`) get tool-calling access to a
  connected Drive/Gmail, and if so under what confirmation model (e.g., must the member approve each
  send, the way a human approves an email before it goes out)? Flagged in Decision 5, not decided.
- When does full Gmail read access become worth the CASA assessment? Needs a concrete feature driving
  it, not a "nice to have" — the assessment's several-week timeline plus annual re-verification is a
  standing cost, not a one-time fee.
- Same question as ADR-023's own open list: does a card/project-facing version of this (Decision 1's
  explicitly-deferred option 2) ever get built, and if so does it reuse ADR-023's
  ownership/`tools_level` gate or need something stronger given OAuth tokens are a fundamentally
  different exposure than a member's own local subprocess?

## Related
- ADR-023-mcp-tool-surface (the existing, differently-shaped "connector" work; this ADR is
  deliberately not an extension of it — see Context)
- ADR-020-multiplatform-bridge-notifications-and-chat (S3's "linked accounts, not OAuth-per-platform"
  precedent this ADR is the first exception to, and why)
- ADR-018-native-macos-swift-shell (Keychain-based token storage precedent, decision 7: BYOK's
  Anthropic key "read via a Keychain token provider — never stored in nodeconfig")

## Amendment — 2026-09-16: the shared-client decision is reaffirmed, and the shipped UI is the thing that must change

**Decided by Jack, 2026-09-16, asked and answered directly.**

This ADR decided that Hive hosts one shared OAuth client. The **shipped** Swift UI does the
opposite: `ConnectorsSettingsView` asks each member to paste their own Google client ID and
secret, and the GitHub connector being written on 2026-09-16 followed that same BYOK shape
(`GitHubConnector.swift:12-13`, refusing to proceed without both at `:36`). Nobody had noticed
the codebase disagreeing with its own ADR, so each new connector inherited the wrong model
from the last one.

Jack's decision: **the ADR was right. One shared Hive OAuth client.** A member clicks Connect
and it works. Hive carries the verification burden, including Google's CASA assessment and
annual re-verification for sensitive scopes — that cost was understood and accepted when the
question was put, and it is the price of a consumer-grade product rather than a homelab tool.

Consequences, in the order they bite:

1. **The client secret should cease to exist, not move.** A secret compiled into a desktop
   binary is not a secret. The existing Google flow is already the right shape for this — a
   *public* OAuth client using PKCE with a loopback redirect needs no secret. Every connector
   should follow it.
2. **The two credential fields leave the member-facing UI.** Connectors become a single
   Connect button. This is a visible product change, not a refactor.
3. **Scope discipline from decision 3 of this ADR still governs.** Shared-client makes narrow
   scopes more important, not less: one verification failure now affects every member rather
   than one.
4. **A per-connector exception remains Jack's to make.** If shared-client turns out to be
   wrong for a specific vendor, that is a decision to record here, not an implementation
   detail to settle in code.

One correction to this ADR's own description while we are here: it describes the Google flow
as `ASWebAuthenticationSession`. The shipped implementation is not that — it is
`NSWorkspace.open` plus a one-shot loopback `NWListener` (`GoogleConnector.swift:12-23`), a
deliberate choice documented in that file. Anyone copying "the existing pattern" for a new
connector should copy the code, not this paragraph.

Recorded by Claude (Loki) from Jack's decision of 2026-09-16.
