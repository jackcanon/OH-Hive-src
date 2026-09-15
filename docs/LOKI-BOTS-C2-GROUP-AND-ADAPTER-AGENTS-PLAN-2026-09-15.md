# ADR-035 C2 kickoff: group chat, mentions, and adapter-backed agents

Loki, 2026-09-15. Scoping note before writing code — this touches schema, storage, the
delivery executor, and three UIs, and I don't want to half-wire it the way a same-day sprint
wired C1. Grounded in what's actually in the repo today, not assumption.

## Where this sits in the roadmap

Not scope creep. ADR-035 already names this exactly:

> C2 team/project rooms (coordinator selection, mentions, threads, Inbox, adapter-backed
> agents as each ADR-034 provider passes its gates)

C1 (today's work) was deliberately narrow: one recipient per conversation, `AgentDm` only,
local agents only. `crates/ohhive-core/src/bots/types.rs` already has `ConversationKind::Team`
and `ConversationKind::Project` defined — unused by C1's storage/executor, but the schema
anticipated rooms from the start. `AgentRuntimeKind` already has `ChatgptSubscription`,
`CopilotSubscription`, `GrokSubscription` alongside `Local` — the ADR-034 subscription-seat
concept — but nothing for a direct BYOK provider agent (Anthropic-direct, Nous), which is what
Jack actually asked for by name.

## Why Nous and Claude don't show up today

Not a bug, not hidden — they don't exist as Bots agents yet. Two separate concepts currently:

- **BYOK keys** (`ByokKeysStatus.anthropic/.openai/.nous`, Settings): used today only by
  regular one-on-one Chat and the coding agent. Per ADR-008, these live in Supabase Vault,
  hub-side only — a member's device never holds the raw key. Every BYOK turn is a hub
  round-trip (the `interview` Edge Function pattern: Anthropic primary -> member's own OpenAI
  key -> Nous Portal).
- **Bots agents** (`AgentProfile`): a persistent identity with a name, role/instructions,
  memory namespace, conversation history. Today only `AgentRuntimeKind::Local` can be created
  (CLI `agent-register`, Tauri/Swift "Register this Mac"), and its replies come from
  `LocalModelTurnRunner` calling a local Ollama endpoint directly — no hub involved at all.

Nobody has built the bridge: turning "you have a Nous key on file" into "there's a Nous Bots
agent you can DM or @ mention." That bridge is real, net-new work on both sides:

1. **Core**: a new `AgentRuntimeKind` (or two: `AnthropicDirect`, `Nous`) whose
   `provider_account_ref` points at the same BYOK vault entry Settings already writes, plus an
   auto-provisioning step — when a member has a provider key on file, a matching Bots agent
   should exist (or get lazily created) rather than requiring a separate manual "register"
   action per provider.
2. **Delivery**: a cloud turn runner parallel to `LocalModelTurnRunner`, implementing the same
   `LocalBotsTurnRunner` trait but calling a hub Edge Function (new, or `interview` extended)
   instead of a local endpoint, since the key can't leave the hub. This is the one piece that
   needs Supabase/Deno work — I haven't touched that layer this session; Sif's toolchain
   access and the `interview` function are the natural template.

## Group chat + mentions (the bigger piece)

Needs, in order:

1. **Storage**: wire `ConversationKind::Team` in `local_hub/bots.rs` (today only `AgentDm` is
   handled) — multi-member rooms, not one-fixed-coordinator DMs.
2. **Mention parsing**: `@AgentName` / `@everyone` in a message body resolves to a recipient
   list at send time (user -> agent(s), user -> everyone) — new logic, doesn't exist yet
   (`bots/mod.rs` lists "mentions" explicitly as not-yet-built).
3. **Agent-to-agent mentions**: when an agent's own reply body contains a mention, the
   delivery executor needs to detect it and enqueue a delivery to the mentioned agent(s) too —
   this is the part that can loop (agent A mentions B, B mentions A...), which is exactly what
   `HandoffBudgets` (already stubbed in `types.rs`, unused since C1 doesn't need it) was
   designed to bound. Wiring real budget enforcement here isn't optional — it's the guard rail
   that makes agent-to-agent @ mentions safe to ship at all.
4. **UI** (all three surfaces): room creation, @ mention autocomplete against the agent roster,
   multi-author message rendering (already close — `BotsMessageRow`/`TeamChat.tsx` just assume
   one other party).

## Track C: agent inspector panel (status + tool access), added 2026-09-15 after Jack's screenshot

Jack pointed at the empty space right of "Reconnect" in the Bots screen and wants an agent
detail panel there: status, and a way to edit the agent's attributes and which tools it can
use.

What's real already: `AgentProfile.capability_policy_ref` (free-text, opaque reference into a
capability policy store per its own doc comment) and `AgentProfilePatch`/`agents_update` (core
service + LocalHub storage both already implement patching name/preferred_host/
capability_policy_ref/memory_namespace) -- so there's a real backend path to edit an agent's
fields today. It isn't exposed through `ohhive-ffi` or any UI yet, and nothing on the read side
resolves `capability_policy_ref` into an actual tool list -- every agent registered anywhere in
the codebase today is hardcoded to `"default"` at creation, and that string is never read back.

What's not real: agent *tool access*, full stop. `bots/runner.rs` is explicitly "Bounded,
tool-free local reply execution" and hardcodes `tools_level: ToolsLevel::InferenceOnly` --
Bots turns have never called a tool. The tool surface that exists in this codebase
(`tools.rs`: `exec_wasm`, `artifact_get`/`artifact_put`, `spawn_child_card`, `mcp_tool_call`,
`run_code_session`) belongs to Cards (ADR-006), a different system with host-trusted,
creation-time `required_capabilities` -- deliberately never something a running model can
widen at runtime. Giving a Bots agent real tool access means deciding whether it reuses that
same surface (probably yes -- rebuilding a second tool-calling loop would be a mistake) and
building, for the first time, a tool-calling turn loop for Bots deliveries. That's a bigger
architecture piece than the panel itself.

Two honestly separable pieces: (1) the inspector panel + FFI/edit path for name and a raw
`capability_policy_ref` string -- buildable now, no new architecture, useful even before
enforcement exists; (2) actual tool permissioning -- needs the Card tool surface wired into a
new Bots tool-calling loop first, then a real policy store `capability_policy_ref` resolves
into, before the panel's tool toggles would do anything real.

## Proposed order

**Track B (adapter-backed agents) first.** Closes the concrete gap Jack named — Nous and
Claude visible and DM-able — without touching conversation/message schema at all. Self-
contained: new runtime kind, provisioning, one new turn runner. The Supabase Edge Function
piece is the long pole; scoping it for Sif is next.

**Track A (rooms + mentions) second.** Bigger, touches schema/storage/executor and all three
UIs, and agent-to-agent mentions genuinely need the loop-budget work done carefully, not
rushed the way a same-day CLI slice can be. This is the one where "let's ship it today" is the
wrong instinct even by today's own standard of pace.

Checked `supabase/functions/interview/index.ts` (542 lines, Deno) before writing this: it
already has exactly the adapter shape needed — `callAnthropic`/`callOpenAI`/`callNous`,
a shared `Brain { provider, key, byo, model }` type that resolves a member's own BYOK key
server-side, and OpenAI-compatible request/response handling shared across providers (Nous
Portal is OpenAI-compatible). A cloud Bots turn runner doesn't need new provider-calling code
— it needs a new Edge Function (or a new `mode` on `interview`) that accepts a Bots
conversation's message history instead of `interview`'s own chat/plan shape, resolves the same
`Brain`, calls the same `call*` functions, and returns a reply the delivery executor can
persist as a `Message` — same contract `LocalModelTurnRunner` already satisfies for local
agents, different transport underneath.

## Track D: Sif's assignment (given in parallel, 2026-09-15)

Track C piece (1) — the inspector panel's buildable half — is Sif's, running in parallel with
my Track B work. See `docs/LOKI-AGENT-INSPECTOR-SWIFT-HANDOFF-2026-09-15.md` for the scoped
handoff. Track C piece (2) (real tool enforcement) and Track A (rooms/mentions) stay mine to
architect before anyone builds against them — both have a loop-safety or trust-boundary
property (handoff budgets; "nothing the model says can widen policy") that needs a careful
single design, not two people converging on it independently.
