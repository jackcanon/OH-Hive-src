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

## Amendment (2026-09-14) — Sif's macOS implementation design: reviewed and accepted

Sif's design (`docs/SIF-COMPUTER-USE-MACOS-DESIGN-2026-09-14.md`, design only, nothing implemented,
no permissions/production code touched) is thorough, appropriately conservative, and independently
checked against the code it makes claims about. I verified her single concrete claim about code I
own (`crates/ohhive-core/src/worker.rs`): `tick()` receives `lease_expires_at` from the hub
(line ~970) but only logs it — it is never passed into `run_card`/the coder loop — and
`run_forever`'s heartbeat (line ~1016) fires on the *outer* poll loop, strictly between calls to
`tick()`, so a long-running card (a real desktop session, or today's coding agent) blocks the
heartbeat for its entire duration. Confirmed exactly as she described: real, unfixed, and more
consequential for computer use than for coding (continuing to post real clicks past an expired
lease is a materially worse failure mode than a file edit running slightly past a lease a
housekeeping pass will reap).

**Accepting all seven of her requested decisions:**

1. **Terminal/IDE view-only by default, execution as a separate explicit grant** — overrides
   Decision 2a's original defaults table, which listed terminals as merely "click-but-not-type."
   She's right that click-only is not a safe boundary once Run buttons, task lists, shell history
   and links can execute code; this ADR's own default-safe posture requires the stricter reading.
2. **A minimal local Stop/Pause surface ships in the first release, not deferred** — corrects this
   ADR's "Deferred, explicitly, not solved here" list (Decision 4/Consequences), which had folded
   "a v1 UI for watching/interrupting a running session" entirely into future work. A progress
   channel post cannot interrupt a live desktop session during a network stall; a persistent local
   indicator plus Pause/Stop/Escape has to exist before any real input ships, even with no full
   session UI. Scope stays minimal (an indicator and three controls), not a dashboard.
3. **A dedicated desktop tool profile that does not inherit coding tools** — `run_command`,
   AppleScript/JS execution, arbitrary file writes, MCP, connectors and shell-launch URLs must be
   absent from a desktop session by default; requesting one later is a new explicit scope decision.
   Directly closes the bypass Decision 3 (Consequences) didn't yet name: a session with both GUI
   control and an unrestricted coding tool defeats every guardrail in Decision 2a/2b at once.
4. **A direct-provider (BYOK) adapter for desktop image/action calls, not the existing Hub/Edge-
   Function cloud-brain route** — screenshots are materially more sensitive than the text ADR-024's
   `code-brain-turn` was built to carry, and routing them through that path would put desktop
   images somewhere Hive's own infrastructure was never designed to hold. A member configures a
   local key for this specifically; existing paid ChatGPT/Claude subscriptions are not presumed to
   authorize API access.
5. **No automatic replay after an uncertain effect** — a lost network response after a real click
   must never trigger an automatic retry of a Send/Buy/Delete; recovery surfaces "uncertain, needs
   review," never a guess. This is the computer-use-specific sharpening of Decision 2a's existing
   hard-blocked-action posture: the danger here isn't a wrong action so much as an *unknown-whether-
   it-happened* one, which the original hard-blocked list didn't anticipate.
6. **Prototype real TCC/code-signing attribution for the packaged helper before committing to the
   XPC-helper process boundary** — accepted as the concrete next engineering step (build sequence
   phase 2), not a design decision to sign off on in the abstract; if helper TCC proves unworkable,
   fall back to an in-process broker with explicitly documented reduced fault isolation.
7. **Windows/Linux stay later, with their own adapters and tests** — unchanged from this ADR's
   existing Consequences/Deferred section; macOS design doesn't establish cross-platform readiness.

**Also recorded, not a decision but load-bearing for anyone implementing this:** exactly-once GUI
effects are impossible to guarantee across a crash (a click can post and the helper can die before
recording the result) — recovery must show "uncertain" and require review, never assume success or
retry. An app grant binds bundle/signing identity *and* running-process identity (PID + launch
instance), not PID or bundle name alone, and is invalidated by app restart, lock/unlock, logout,
sleep/wake, lease loss, node revocation, or broker restart. Every one-time approval is bound to a
digest of the exact action + target; a policy or target change invalidates it. None of Apple's
Accessibility/ScreenCaptureKit/CGEvent API claims in her design have been verified against real
hardware — that is explicitly the build sequence's phase 2 gate (native read-only pilot, on a
dedicated test Mac), not something a design review can confirm.

**Not yet decided, deliberately left to the build sequence:** the exact pilot limits (turn/time/
byte/action-rate caps) are proposed starting values, not measured; the final process boundary
(signed XPC helper vs. in-process broker) waits on the TCC prototype in decision 6; the legal
review of relaxation-explainer copy (already an ADR-029 prerequisite) is unchanged and still not
performed. Sif proceeds per her own build sequence (§9): contracts/policy engine first, with no
real input until phase 2's native read-only pilot passes its own gates.

## Amendment (2026-09-14) — the lease-propagation/heartbeat finding above: closed

Both halves of the finding this amendment originally flagged as "real, unfixed" are now closed,
in `crates/ohhive-core/src/worker.rs` (ADR-025 ownership, mine):

1. **`lease_expires_at` reaching `run_card`/the coder loop** — this was already fixed earlier the
   same day, before Sif's design review even landed: `tick()` parses `lease_expires_at` once
   (rather than only logging it) and passes it into `run_card` → `run_code_card`/the Draft-
   Critique-Revise loop, which checks it once per step and releases the card rather than
   continuing past it. Confirmed by re-reading the current code, not by memory of the earlier
   patch.
2. **The heartbeat blocking on a long-running `tick()`** — genuinely still open as of this
   morning's review (the old `heartbeat_if_due` was called only between ticks, sequentially, so
   one long card really did starve it for that card's entire duration). Fixed just now:
   `run_forever` no longer calls a heartbeat from inside its own dispatch loop at all. It instead
   polls two independent things concurrently for its whole lifetime — the existing claim/run/
   checkpoint loop (moved into a new `dispatch_loop` fn, unchanged in behavior) and a plain
   `tokio::time::interval` heartbeat ticker — via `tokio::select!` over a pinned dispatch future.
   Neither can delay the other; a card whose `tick()` takes minutes no longer holds up the
   heartbeat for any part of that time. Added a regression test,
   `local_hub::tests::heartbeat_is_not_blocked_by_a_long_running_card`, that drives one card
   through a `Backend` sleeping 280ms (several heartbeat intervals) and asserts the heartbeat
   fired well more than the ~4 times the old sequential design could have managed in that same
   window — it would have failed against the pre-fix code.

Self-verified (brace/paren/bracket balance) but not yet compiler-verified — Jack is running
`cargo test -p hive-core --features "local-hub,sandbox,llama-cpp" --lib worker:: local_hub::tests::heartbeat`
now. This entry will be corrected here and in CONTINUITY.md if that build finds anything wrong.

Not yet touched, from Sif's own list of what she's waiting on from this side: shared executor
extraction, multimodal messages, mixed-version claim/profile rejection, fake-provider integration
and budgets. Lease propagation/refresh — the specific item this amendment covers — is done;
the rest of that list is next.

## Related

ADR-024 (coding agent — the private-fleet-only trust gate and session/lease/channel shape this reuses directly), ADR-006 (community sandbox — confirms by construction why this can never run there), ADR-023 (MCP tool surface — the earlier "member's own machine, Hive only gates whether it runs" precedent this extends again).
