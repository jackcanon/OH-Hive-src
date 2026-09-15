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

## Addendum 2026-09-15: will ChatGPT/Copilot/Grok show up here too?

Jack asked, after Track B's BYOK provisioning landed, whether the ADR-034 subscription
coordinators (ChatGPT/Codex, Copilot, Grok -- "sign in with" flows, not API keys) will show up
as Bots agents the same way. Checked before answering:

- **Yes by design, not yet by code.** `AgentRuntimeKind` already reserved `ChatgptSubscription`/
  `CopilotSubscription`/`GrokSubscription` before today's work (unused until now, same as the
  BYOK variants were). The `ensure_provider_agents` pattern generalizes directly: same idea,
  different status source (a coordinator connection state instead of `member_key_status`).
- **Claude specifically is excluded from this path, on purpose.** ADR-034: "Claude itself is
  out of scope: Anthropic's Agent SDK terms permit third-party subscription auth only 'unless
  previously approved,' and Hive has no such approval." So there will never be a "sign in with
  Claude.ai" coordinator -- the "Claude" Bots agent stays the Anthropic-BYOK one built today,
  permanently, not a placeholder for something fancier later.
- **Only Codex/ChatGPT has any scaffold at all** (`subscription/account.rs`, P0, partial, per
  ADR-034's own status line -- "Everything below 'Implementation contract' is still to be
  built"). Copilot and Grok adapters don't exist yet.
- **Real wrinkle, worth deciding explicitly rather than assuming it mirrors BYOK:**
  `AccountConnection` in `subscription/account.rs` is a local child-process connection to a
  `codex` binary on one machine -- "Managed ChatGPT account connection only. No model turns or
  Hive dispatch," no hub reference in that file at all. BYOK keys are hub-resolved, so a
  Claude/Nous Bots agent is the same agent from any of a member's paired devices. A ChatGPT
  agent provisioned the same way would, on the current design, likely exist only on whichever
  Mac ran the sign-in -- not automatically visible hub-wide the way Claude/Nous are, unless
  connection state gets synced to the hub as part of building the other two coordinators. Not
  resolved here; flagging it so whoever builds the Copilot/Grok provisioning path (probably me,
  alongside that work, not a separate open question left for later) makes that call on purpose.

## Track E (new top priority, 2026-09-15): Bots needs to be fleet-wide, and today it isn't

Jack, after the ChatGPT/Grok question, stated the actual requirement plainly: create an agent
on Midgaard, open Hive on Overgaard, see the same agent -- "it doesn't matter where a local
agent is because it connects to the chat and can then be useful through the chat." This is the
whole point of Bots as a management surface, not a nice-to-have.

**This was always the design intent, not a new ask.** `docs/SIF-BOTS-CHAT-DESIGN-2026-09-15.md`
section 5.1 says it outright: "Use the selected LocalHub as authority for private
conversations... Clients submit to that authority; do not invent multi-master writes or sync
live SQLite files through Drive/iCloud." One authoritative LocalHub per member, every device
talks to it -- that's what "selected LocalHub" has meant since before C1 was built.

**It isn't implemented.** Checked every C1 entry point (CLI `hive bots`, `ohhive-ffi`'s
`BotsSession`, the Tauri app's `bots.rs`) -- every single one calls `LocalHubStore::open(...)`
against `vault-host.sqlite3` on its own local disk, unconditionally. There is no code path
anywhere that opens a *different* device's Bots data. Register an agent on Midgaard today and
it lives only in Midgaard's local SQLite file. Open Hive.app on Overgaard and it's a
completely separate, empty store -- not "the same fleet, seen from elsewhere."

**The good news: this isn't a from-scratch build.** `crates/ohhive-core/src/local_hub/
transport.rs` already has exactly this mechanism, already working, already tested, already
used by real (not example-only) app code: `router()`/`serve()` run a small JSON-RPC-over-HTTP
server (`/local/v1/rpc`), `dispatch()` matches on a method name and calls the matching `LocalHub`
method, and `RemoteLocalHub` is the client side -- pair once (`RemoteLocalHub::pair`, reusing
the same pairing/credential flow every node already has), then call `vault_list`/`vault_read`/
etc. over the network exactly like a local call. `ohhive-ffi/src/local_hub.rs` already exposes
this for Vault to the real Swift/Tauri apps, with a real cross-device consent step. `dispatch()`
already routes more than vault, too -- `claim_card`/`complete_card`/`checkpoint` go through the
same switch. It has just never had `bots_*` added to it.

**What's needed:**
1. Add `bots_agents_list`/`agents_create`/`agents_update`/`conversations_*`/`messages_*`/
   `message_send` (the same surface `BotsService` already defines) as cases in `dispatch()`,
   and matching async methods on `RemoteLocalHub`, mirroring the existing `vault_*` methods
   exactly.
2. Decide what "selected LocalHub" means operationally -- simplest default, consistent with
   the design doc and requiring no new concept: the first node where a member opens Bots
   becomes their Bots-authoritative LocalHub; other paired devices connect to it remotely via
   the same pairing flow Vault already uses. Worth confirming with Jack rather than assuming,
   since it's a real product decision (what happens if that Mac is asleep/offline -- the design
   doc already answers this: "drafts/outbox entries remain visibly pending until accepted,"
   not silently queued elsewhere).
3. Every C1 entry point (CLI, FFI `BotsSession`, Tauri `bots.rs`) needs to pick local-file vs.
   `RemoteLocalHub` based on which node is authoritative, instead of hardcoding local -- same
   choice `examples/local_hub.rs` already makes ad hoc (`if target.starts_with("http")...`).

**Reprioritized: this comes before Track A (rooms/mentions) and before finishing Track B's
cloud turn runner.** Group chat across agents that only half the fleet can see isn't useful,
and a cloud-backed Claude/Nous agent provisioned on one Mac being invisible from another
defeats the point Jack just stated -- BYOK credentials are already hub-wide, but the *agent
identity itself* is stuck local until this lands. Starting here.

## Addendum 2026-09-15: is this LAN-only, and does Hive have tunnels?

Jack asked whether Track E's remote transport is LAN-restricted, whether Hive (not just Halo)
has tunnel/Tailscale-style support, and what's needed to reach his agents from off his home
network. Checked before answering rather than guessing:

- **Not fundamentally LAN-only.** `local_hub/transport.rs`'s `RemoteLocalHub::new` requires
  HTTPS for any non-LAN/loopback host -- plain HTTP is the thing restricted to LAN, not remote
  access itself. `serve()`'s own doc: the listener refuses to bind a public address directly
  (safety -- it must bind loopback or private LAN), "TLS may be terminated by the existing
  private tunnel setup." The design already assumed a tunnel sits in front of it.

- **Hive has real tunnel infrastructure today, and it's Hive's, not Halo's.** `hive-core` has
  a first-class `tunnel` Cargo feature, both desktop apps bundle a real `cloudflared` binary,
  and tonight's own live Tauri run proved it: `hive_server: serving listen=0.0.0.0:8790
  public_url=https://midgaard.ohghive.com`. That's ADR-013 D74's regional-server tunnel --
  real, running, public, used today for the community compute marketplace role (card
  claiming, artifact serving).

- **There's a second, separate, purpose-built tunnel specifically for LocalHub, already
  written, already tested, wired to nothing.** `crates/ohhive-core/src/local_hub/tunnel.rs`:
  `provision()` reuses the exact same authenticated `cloudflared` login the regional-server
  tunnel already has (`crate::tunnel::create`/`route_dns`), but writes its own separate config
  and hostname (`{name}-local`) so exposing a private LocalHub never touches or collides with
  the public regional-server tunnel -- deliberate, per its own doc comment ("NEVER overwrites
  the community regional server's config.yml or process"). It has unit tests
  (`local_hub/tests.rs`) exercising `write_config`. It has zero FFI exposure and zero UI --
  nothing in Tauri, Swift, or the CLI calls `local_hub::tunnel::provision` anywhere. Built,
  correct-looking, completely unreachable from any app today.

**What "open Hive away from home and still reach my agents" actually needs**, on top of
Track E's core dispatch/RemoteLocalHub work:

1. FFI + Settings UI for `local_hub::tunnel::provision()` -- a "make my private LocalHub
   reachable remotely" action, mirroring the `tunnel_snapshot`/`tunnel_login` pattern
   `ohhive-ffi/src/tunnel.rs` already has for the *regional-server* tunnel, but pointed at
   this separate module so it gets its own private hostname.
2. Track E's `bots_*` dispatch/`RemoteLocalHub` methods (already planned) -- the tunnel gets
   you a reachable address, dispatch is what actually answers `bots_*` calls at the other end.
3. A device away from home needs to know to use that hostname rather than a bare LAN address
   or local file -- same "which LocalHub is selected, and how does a client find it" question
   Track E already raised, now with a concrete answer for the off-LAN case: the private
   tunnel hostname, once (1) exists.

Folded into Track E rather than split into a new track -- it's the general case of the same
"pick the right authority" problem, not a separate one.
