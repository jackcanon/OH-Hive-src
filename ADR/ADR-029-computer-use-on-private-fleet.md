# ADR-029: Computer Use, Scoped to a Member's Own Private Fleet

**Status:** Proposed · **Date:** 2026-09-14 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** Jack: "So if users want to use computer use with Hive, how do we enable that?"

## Context

"Computer use" is a specific, real capability: a model that gets a screenshot back each turn and issues abstract actions — move the mouse, click, type, press a key, scroll — instead of (or alongside) text/tool calls. Anthropic's own documentation is unambiguous about what running it safely requires: an isolated environment (a VM, a container, a sandbox), not the primary machine; minimal privileges to limit jailbreak/prompt-injection blast radius; no credentials of any kind living in that environment (no saved browser passwords, no SSH keys, no API tokens — "treat it like a public computer at a library"); and restricted network reachability, allow-listed rather than open. That's not overcaution — it's the vendor's own stated baseline.

Checked this against what Hive already has, and one thing rules itself out immediately: the existing community sandbox (ADR-006) is wasmtime/WASI — no host process spawning, no real display, no OS-level input at all, by design, because it runs untrusted code from strangers. Computer use cannot run there, full stop; it needs either a real OS session or a real VM with an actual display, neither of which a WASI component can provide. That means this is never a "make the existing sandbox a bit more capable" problem — it's a new execution environment, and it inherits ADR-024's already-settled framing almost exactly: real, unsandboxed-in-the-GUI-sense capability, on a member's own machine, member's own trust call — not something a stranger's card should ever get anywhere near.

Worth naming directly: this very Cowork session already runs a working version of exactly this problem. Its own system prompt carries a real, tested safety framework for an agent controlling a real desktop — tiered per-app permissions (full control / click-only / view-only, based on what kind of app is in front), an explicit `request_access` consent step per application, a fixed list of actions that are simply never allowed regardless of who asks (entering payment/financial credentials, executing trades, permanently deleting data, bypassing CAPTCHAs), a second list requiring explicit chat confirmation before acting (sending messages, purchases, changing settings), and a hard rule that anything read off the screen is data, not instructions, even when it's phrased as a command. That's not a hypothetical reference design to study from a distance — it's a working policy Hive can adapt rather than invent from nothing.

## Decision

### 1. Private-fleet-only, no exceptions, same as ADR-024

A computer-use card only ever runs where `execution_mode = 'local'` and `node.member_id = project.owner_id` — no `'hive'` branch, not "gated more strictly," genuinely absent, for the same reason ADR-024 drew that line: a compromised or badly-prompted computer-use session is real-world-reversible-or-not on whatever it's pointed at, and that's a call only the machine's owner should make about their own environment. Contributing a computer-use capability to the wider marketplace is a materially bigger trust problem (arbitrary strangers' cards controlling a volunteer's actual screen) and isn't attempted here.

### 2. Execution environment: **decided — Path A, the member's real desktop**

**Decided (2026-09-14, Jack): Path A** — native OS accessibility APIs (macOS Accessibility/CGEvent
first, Windows/Linux equivalents when Tauri catches up) drive real mouse/keyboard/screenshot actions
against the member's actual screen, not an isolated VM. Chosen over Path B (a throwaway virtual
display, matching Anthropic's own stated safety baseline more closely) knowingly — Path A is faster
to ship and gives a session real access to the member's already-logged-in accounts/browser/apps
without the isolation-punching Path B would need for the same usefulness, but it means Hive's own
safety framework (below) is the entire reason this is safe to run at all, not a nice-to-have layered
on top. Path B stays a documented, real alternative (same "designed, not rejected" treatment as
ADR-028's full sync path) if real-world use of Path A surfaces problems severe enough to warrant it.

### 2a. Hive's own safety framework — first pass, modeled on this session's own working rules

This session's host application already runs a tested version of this exact problem and gets three
things right that this framework should keep, adapted for a private-fleet single-user context
rather than a multi-tenant one:

- **Tiered app access, not all-or-nothing.** A browser gets read/view-only by default (screenshots
  visible, no clicks/typing) unless the member explicitly grants more — the highest-value browser
  mistakes (a wrong click on a real page) are exactly where a narrower default earns its keep.
  Terminals/IDEs get click-but-not-type by default (can press a visible Run button, can't type
  arbitrary shell input blind). Everything else defaults to full control once the member has
  approved that specific app for that specific session.
- **Explicit per-app consent, every session, not a one-time blanket grant.** The member approves
  which applications a given computer-use session may touch before it starts, the same shape as
  ADR-023's MCP server enablement and ADR-026's connector "Connect" click — nothing gains standing
  reach without the member seeing it first.
- **A hard-blocked action list that no instruction can override**, regardless of who's asking or how
  it's phrased — entering financial/payment credentials, executing a trade or money transfer,
  permanently deleting data, bypassing a CAPTCHA, accepting a EULA/OAuth grant unattended. These stay
  blocked even on the member's own machine, for the member's own protection against their own
  agent's mistake or a prompt-injection attack from a page it's told to visit — being the machine's
  owner doesn't make a wrong stock trade or an emptied trash reversible.
- **Screen content is data, never instructions** — text on a page telling the agent to do something
  is exactly as untrusted as a webpage's text is when this session reads it; a card's own
  Private-Fleet-declared task is the only source of actual instructions.

One deliberate difference from this session's own rules, worth being explicit about rather than
silently copying: some of this session's restrictions (never touching credentials at all, in any
form) exist because it mediates for many different people across many different trust contexts. A
member's own private-fleet agent, running only on that member's own machine for that member's own
benefit, has a narrower threat model — the risk is protecting the member from their own agent's
mistakes and from prompt injection, not protecting Hive from cross-member harm.

### 2b. Guardrails default on, member-adjustable with explicit informed consent — decided

**Decided (2026-09-14, Jack): "It's their machine and their choice, but we want the guardrails on
by default."** Every restriction in 2a ships on, for every member, with no setup step required to
get the safe behavior. A member who wants more may turn specific guardrails off — but not with a
quiet settings toggle: each relaxation requires its own explicit explainer describing concretely
what's being turned off and what could go wrong, and the member must affirmatively accept
responsibility for that specific change before it takes effect. Not one global "disable safety
mode" switch — a member turning off, say, unattended-typing-into-browsers should not have silently
also turned off financial-credential blocking. Mirrors the granularity of ADR-023's per-server MCP
enablement and ADR-026's per-connector "Connect" click: nothing gains reach through one broad
consent covering things the member didn't specifically see.

**Decided (2026-09-14, Jack): everything is relaxable, including the hard-blocked list.** Full
"it's their machine" — with strong enough explicit per-item consent, even trades/payment-credential
entry/permanent deletion/CAPTCHA-bypass/unattended-consent-grant restrictions can be individually
turned off. Accepted knowingly that several of those items can implicate a third party's terms of
service or carry financial/regulatory exposure beyond the member's own risk tolerance — Hive's own
choice to let a member accept that exposure for themselves, not an oversight. Two things this makes
non-negotiable about the *mechanism*, given the stakes just went up: consent must be genuinely
per-item (turning off CAPTCHA-bypass blocking must never silently also turn off payment-credential
blocking), and the explainer for anything on the former hard-blocked list needs real legal review of
its language before this ships — a checkbox that doesn't hold up as informed consent protects no
one. Flagging that review as a real prerequisite, not decided or performed here.

### 3. Brain: cloud BYOK first, local model support opportunistic

Unlike ADR-024's coding agent (where local Ollama/Hermes tool-calling was a real, immediate, free option), computer-use specifically requires a model actually capable of the screenshot-in / action-out loop well enough to be useful — not every locally-runnable model is there yet. v1 leans on the member's existing BYOK cloud key (Anthropic's own computer-use tool being the obvious first integration, since Hive already routes BYOK calls per-provider per ADR-024 decision 3); a member's local model gets the same interface the moment one is capable enough to drive it usefully, no separate architecture needed later.

### 4. Reuses ADR-024's session/lease shape, not new machinery

Same as coding: one continuous session inside one claimed card's lease (no new claim/lease/checkpoint system), progress posts to the Private Fleet channel per card (ADR-022's existing pattern) so a member can watch a session work without a dedicated UI surface in v1, and workspace/scope (which app, which files, which env) is declared the same way a coding card's `workspace_path`/`repo_url` is today.

## Consequences

**Positive:** directly closes a real, named gap using almost entirely existing machinery (ADR-024's session/lease/channel pattern, ADR-006's proof that the community sandbox correctly excludes this) rather than inventing new architecture from scratch. This session's own safety framework is a genuine, already-debugged reference rather than a blank page.

**Negative / risks:** whichever path Decision 2 lands on, this is the single highest-blast-radius capability Hive would ship — higher than ADR-024's unsandboxed shell, because a shell command is at least legible and scoped to a workspace, while a GUI action can touch literally anything visible on screen, including things far outside any declared workspace. A real safety framework (tiered permissions, hard-blocked actions, explicit consent, screen-content-is-not-instructions) is not optional scaffolding here — it's the actual load-bearing part of this ADR, whichever execution environment is chosen.

**Deferred, explicitly, not solved here:** the `'hive'` (multi-tenant/marketplace) case, for the same reasons ADR-024 deferred it, harder here; a v1 UI for watching/interrupting a running session beyond the existing channel; any bridge between an isolated sandbox (Path B) and the member's real accounts/files, if Path B is chosen; Windows/Linux accessibility-API parity, needed regardless of path once Tauri catches up per the standing Swift-first sequencing.

## Related

ADR-024 (coding agent — the private-fleet-only trust gate and session/lease/channel shape this reuses directly), ADR-006 (community sandbox — confirms by construction why this can never run there), ADR-023 (MCP tool surface — the earlier "member's own machine, Hive only gates whether it runs" precedent this extends again).
