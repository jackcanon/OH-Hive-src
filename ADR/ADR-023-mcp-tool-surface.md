# ADR-023: MCP Tool Surface for Cards (Task #177)

**Status:** Proposed · **Date:** 2026-09-13 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** Task #177 (queued this session during the Hermes Agent survey), refined by ADR-022 S4

## Context

Task #177's original ask: let a card call out to an MCP (Model Context Protocol) server the *member*
has configured on their own machine — a local filesystem server, a local dev-tools server, whatever
the member points it at — as a new tool a card's `required_capabilities` can opt into, alongside the
existing four (`exec_wasm`, `artifact_get`, `artifact_put`, `spawn_child_card`; ADR-006, `crates/
ohhive-core/src/tools.rs`).

The task list's original title, "local-only cards," is refined by ADR-022 S4: "local" for this
purpose means **any node owned by the requesting member**, matching ADR-015 S1's existing rule that
any of a member's own paired machines may claim that member's own `execution_mode = 'local'` work —
not strictly the one machine a tool happens to be configured on. This ADR is the design decision
ADR-022 S4 asked for.

This is a materially different trust problem from every existing tool in `tools.rs`. `exec_wasm` runs
a WASI Preview 2 component under wasmtime (ADR-006 D45-D48, `crates/ohhive-core/src/sandbox.rs`): no
filesystem access beyond a wiped scratch dir, no host process spawning, a fuel/memory budget, network
denied unless both the node and the card opt in. An MCP server is the opposite of that by
construction — it is a real OS subprocess, speaking stdio JSON-RPC, with whatever access the member
who configured it chose to grant it on their own machine (a filesystem root, a local database, a dev
tool). Hive cannot sandbox an arbitrary subprocess the member themselves chose to run; it also
shouldn't try to — the member already has full control over their own machine and has made their own
choice about what that server can touch. **The security boundary this ADR draws is not "what can the
process do" (the member's call, on the member's hardware, running as the member's own OS user) but
"who can reach it and when"** — never a stranger's project, never a node the member doesn't own,
never a server the member didn't explicitly configure and enable, and never on a node whose operator
has opted out of sandboxed tools.

## Decision

### 1. Member-configured server model

A member registers zero or more MCP servers in `hive.member_mcp_servers` (name, transport, command,
args, env, enabled). This is member-owned configuration data, not a card-supplied or model-supplied
value — the same "host-trusted, set when the card/config was created, never something a running model
can invent or redirect at runtime" property `tools.rs`'s existing module doc calls out for every
other tool. A card names *which* server and *which tool on it* to call via `required_capabilities`;
it never gets to name a command/args/env directly. The command a card can ultimately cause to run is
always exactly one the member typed into their own server config ahead of time, on the web app (or,
later, another surface) — never anything embedded in a project's or card's inputs, and never anything
an inference step's own output can widen.

### 2. Trust gate — `tools_level = 'sandboxed_tools'`, plus fleet-scoped ownership, plus explicit enablement

Three independent checks, all enforced server-side in `hive.node_claim_card` (never only client-side):

- **(a) Never on hardware the requesting member doesn't own.** ADR-022 S4's refinement, applied
  literally: regardless of `execution_mode` (`local` *or* `hive`), a card whose
  `required_capabilities` names an `mcp_server_id` may only be claimed by a node whose
  `member_id = <the project's owner>`. This is *stricter* than the existing `execution_mode = 'hive'`
  branch (which lets any funded community node claim the card) — an MCP-requesting card collapses to
  "my own fleet only," full stop, even if the project itself is nominally a community/hive-mode
  project. This is the literal reading of the task's security framing: never let a card that isn't
  the node-owner's own project use this tool, regardless of `execution_mode`.
- **(b) Never a server the member didn't explicitly configure and enable.** The named
  `mcp_server_id` must resolve to a row in `hive.member_mcp_servers` owned by that same member with
  `enabled = true`. Checked twice: once at claim time (`node_claim_card`'s eligibility query — so a
  card that shouldn't be claimable never gets leased in the first place) and again at run time
  (`hive_member_mcp_server_get_node`, which the Rust worker calls to fetch the actual command/args/env
  right before spawning — closing the gap where a member disables or deletes a server *between*
  claim and execution).
- **(c) `tools_level = 'sandboxed_tools'` on the claiming node.** Exactly the same gate `exec_wasm`
  already uses (ADR-006 D48), applied here for the same reason the task calls out: running a
  member-configured local process is at least as sensitive as running a WASI component, so a
  contributor who opted their node into `inference_only` must never have an MCP subprocess spawned on
  it either — even though, unlike `exec_wasm`, this can only ever happen on the member's own hardware
  in the first place (check (a) above already ensures that; this is defense in depth on top of it,
  respecting a node operator's explicit choice regardless of whose project it's running).

All three are combined into `node_claim_card`'s existing single `where` clause with the same `or`
structure the pre-existing `tools_level` check already uses (`<field not requested> or <condition
holds>`) — see the accompanying migration for the exact SQL. No new RPC, no separate schedule-time
check: this reuses the one function that is already both eligibility-check and lease-grant for every
other tool.

### 3. Transport: stdio subprocess, hand-rolled JSON-RPC

v1 supports exactly one transport, `stdio` — the obvious first choice, and the shape of every MCP
server a member is likely to already run locally (`npx @modelcontextprotocol/server-filesystem
/some/path`, or an equivalent dev-tools server). The Rust worker:

1. Spawns `command` with `args` (a plain argv array — **never** through a shell, so nothing in `args`
   or `env` can be reinterpreted as shell syntax; this is a direct `exec`, not `sh -c "..."`).
2. Speaks newline-delimited JSON-RPC 2.0 over the child's stdin/stdout (the MCP stdio transport's
   actual wire format — one JSON object per line, no embedded newlines; not LSP-style
   `Content-Length` framing).
3. Runs `initialize` → `notifications/initialized` → `tools/list` → `tools/call` in that order, once,
   per tool call: `tools/list` first, so an unknown tool name fails with a clear
   `NoSuchTool` before ever sending `tools/call`, rather than surfacing whatever error shape the
   third-party server happens to return.
4. Kills the child process once the call completes (success, failure, or timeout) — there is no
   persistent MCP session across steps or across cards in v1, matching `exec_wasm`'s own "one call,
   fresh state, every time" model. A card cannot yet hold a conversation with an MCP server across
   multiple tool calls; see "Deferred" below.

**Hand-rolled, not a third-party MCP client crate.** `Cargo.lock` has no existing MCP dependency
(confirmed by grep before writing this). The protocol surface actually needed here — one JSON-RPC
request/response pair per call, over a pipe, no batching, no streaming, no resources/prompts — is
small enough that hand-rolling it (~150 lines) is more auditable than taking on an early-stage,
security-adjacent external dependency (parsing untrusted-ish JSON-RPC from a subprocess the member
configured is exactly the kind of code where "we can read every line of it" is worth more than not
reinventing a wheel). This can be revisited if the surface grows enough that a maintained crate
becomes clearly worth the trust transfer.

### 4. Card contract

A card opts in via three `required_capabilities` keys, all host-trusted, following exactly the shape
`tools.rs`'s module doc describes for the existing four tools:

- `mcp_server_id` (uuid, required to use this tool at all) — which of the member's configured servers
  to call.
- `mcp_tool_name` (string, required alongside it) — which tool on that server.
- `mcp_tool_args` (object, optional, default `{}`) — arguments passed through verbatim to `tools/call`.

Same rule as every other tool here: nothing a running inference step outputs can change which server
or which tool gets called — only what was declared when the card was created.

### 5. v1 scope vs. deferred

**In v1 (this pass):**
- Schema: `hive.member_mcp_servers`, RLS deny-all + member-JWT CRUD RPCs (list/create/update/delete).
- `node_claim_card`'s three-part gate (S2 above).
- A node-key-authenticated config-read RPC (`hive_member_mcp_server_get_node`) for the worker to
  fetch command/args/env at run time, re-checking ownership + `enabled` independently of the claim
  check.
- `crates/ohhive-core/src/mcp.rs`: the stdio JSON-RPC client (spawn, `initialize`, `tools/list`,
  `tools/call`, kill).
- `tools.rs::run_mcp_tool_call` and `worker.rs` wiring into the existing pre-`Draft` tool step,
  alongside the other four tools.

**Explicitly deferred, not built in this pass:**
- **Web UI for managing servers.** The CRUD RPCs exist; nothing in `apps/web` calls them yet. A
  member cannot configure an MCP server from the web app until that UI ships.
- **FFI/Swift wiring.** `crates/ohhive-ffi` and the Swift app are untouched — MCP tool calls only run
  through the Rust `hive` CLI / headless worker path in this pass, not the desktop app's UniFFI
  bridge.
- **Non-stdio transports** (`http`, `sse`, or similar remote MCP transports). The `transport` column
  and its check constraint only accept `'stdio'`; a future pass can widen both.
- **Multi-call / looping tool use.** Same limitation `tools.rs`'s existing module doc already states
  for `exec_wasm`: a card gets one Act→Observe pass before `Draft`, not the full multi-turn ReAct loop
  ADR-006 describes. A card cannot yet call a second MCP tool, or call the same server twice, mid-run.
- **MCP resources/prompts.** Only `tools/call` is implemented; `resources/read` and `prompts/get`
  are not.
- **Per-tool allow-listing within a server.** If a member enables a filesystem server, a card that
  names that `mcp_server_id` can call *any* tool the server advertises (by name) — v1 does not let a
  member restrict "this server, but only these tools." The member already trusts the whole server by
  enabling it; this is a reasonable v1 simplification, not a gap in the ownership/enablement gate.
- **Secrets handling for `env`.** Values are stored as plain `jsonb` text, visible to the member who
  owns them (same as `command`/`args`) and to nothing else (RLS deny-all + SECURITY DEFINER RPCs).
  No encryption-at-rest beyond what the rest of `hive.*` already gets from Supabase; no masking in a
  future web UI is designed yet.
- **Configurable working directory / resource limits for the child process.** The subprocess inherits
  the node process's own cwd and gets no CPU/memory/wall-clock cap from Hive (see Consequences —
  Negative). A timeout on the JSON-RPC round-trip is enforced by the client, not a hard process
  limit.

## Consequences

### Positive
- Extends the existing tool surface without inventing a new trust primitive: reuses `tools_level`
  exactly as `exec_wasm` already does, reuses `node_claim_card` as the single claim-time gate, reuses
  the "host-trusted `required_capabilities`, nothing the model says can widen policy" property every
  other tool already relies on.
- Correctly narrows "local" per ADR-022 S4 without introducing a new `execution_mode` or a new
  scheduling concept — a member's fleet becomes reachable for this tool the same way it's already
  reachable for `execution_mode = 'local'` claiming (ADR-015 S1).
- No new sandboxing subsystem to build or maintain: the member's own machine is the trust boundary,
  and Hive's job is limited to access control it already knows how to do (ownership checks,
  `tools_level`, SECURITY DEFINER RPCs).

### Negative
- **This is Hive's first tool that runs an unconstrained host process.** Every other tool either runs
  inside wasmtime (`exec_wasm`) or does no code execution at all (`artifact_get/put`,
  `spawn_child_card`). A bug in a member's own MCP server, or a server the member didn't fully vet
  before enabling, can do anything that OS user can do — Hive provides no fuel budget, no memory cap,
  no filesystem confinement here. This is by design (see Context), but it is a real, qualitatively
  different risk sitting in the same `required_capabilities` surface as the sandboxed tools, and needs
  to be described to members as such wherever server configuration is surfaced (not framed as
  "sandboxed," even implicitly, in any future UI copy).
- A misconfigured server (wrong command, missing binary, hung process) fails a card's tool step but
  is otherwise invisible until the JSON-RPC timeout fires — no health-check or "test this server"
  action exists yet.
- Widens the already-noted ADR-022 S6 concern: a compromised or misconfigured node is now able to
  spawn arbitrary member-configured subprocesses, fleet-wide, not just run wasmtime components.

### Risks & mitigations
- **A malformed `mcp_server_id` in `required_capabilities` must never crash `node_claim_card`.**
  `node_claim_card` is a live, frequently-called function. The gate compares
  `hive.member_mcp_servers.id::text = required_capabilities->>'mcp_server_id'` — casting the *known*
  uuid column to text, never casting the arbitrary card-supplied text to uuid — specifically so a
  malformed value (not a valid UUID) simply fails to match instead of raising a cast exception that
  would abort the whole claim query for every node. Verified in the accompanying migration.
- **Runaway or hung MCP server process.** Mitigation: the JSON-RPC client enforces a timeout on every
  request/response round-trip (separate, shorter timeout for the `initialize`/`tools/list` handshake
  than for `tools/call`, since a real tool call may legitimately take longer); the child is killed
  (`kill_on_drop` plus an explicit `Drop` impl) whether the call succeeds, fails, or times out, so a
  card can never leave an orphaned process running past its own tool step.
- **A member's fleet expands the "MCP-configured" blast radius without them realizing it** (ADR-022
  S4's own consequence). Mitigation: `enabled` is a first-class, always-visible column and the
  run-time re-check (`hive_member_mcp_server_get_node`) means disabling a server takes effect
  immediately for any card not yet executed, not just future claims.
- **Shell injection via `args`/`env`.** Mitigation: the child is spawned directly (`argv` array),
  never through `sh -c`, so nothing in a member's own `args`/`env` values is reinterpreted as shell
  syntax. (A member can still misconfigure their own server to do something harmful to their own
  machine — that is squarely inside the trust boundary this ADR draws, not something Hive is
  positioned to prevent.)

## Open questions
- Should `hive.member_mcp_servers` eventually support a `cwd` column, or per-server resource hints
  (expected max call duration, to tune the `tools/call` timeout per server rather than one fixed
  value)? Open — no evidence yet that the default is wrong for real usage.
- Does a future web UI need to run a live "test this server" round-trip (spawn, `initialize`,
  `tools/list`, report back) before a member can flip `enabled = true`? Open — would meaningfully
  reduce the "silently broken until a card tries to use it" failure mode noted above.
- Should per-tool allow-listing (a member enables a server but restricts which of its tools a card
  may name) become a v1.1 column, or is "the member already trusts the whole server" durable enough
  in practice? Open — flagged, not decided.
- Does this eventually want the finer-grained, per-agent-permission model ADR-022 S6 flags as a
  future reference point (Buzz's model), rather than today's "any of my own nodes, any enabled
  server" trust boundary? Open — same open question ADR-022 already raised, not resolved here.

## Related
- ADR-022-personal-hive-fleet-control-plane (S4: the scope decision this ADR implements; S4's
  "any node owned by the requesting member" language is applied literally in Decision 2(a) above)
- ADR-006-agent-runtime-and-sandbox (`tools_level`/`allow_internet` trust model this ADR extends
  rather than replaces; `crates/ohhive-core/src/tools.rs`'s existing four-tool surface this is the
  fifth addition to)
- ADR-015-local-workstation-and-hive-promotion (the "any node this member owns may claim
  `execution_mode = 'local'` work" rule ADR-022 S4, and therefore this ADR, both build on directly)
- ADR-016-hub-portability-and-local-fleet-independence (the "member's own fleet has none of the
  shared-marketplace's multi-party trust problem" reasoning this ADR's trust-boundary framing relies
  on directly)
- `supabase/migrations/20260913020000_personal_channel.sql` /
  `20260913060000_personal_channel_node_key.sql` (the member-JWT-core-plus-node-key-wrapper,
  RLS-deny-all SQL house style this ADR's accompanying migration follows)
- `supabase/migrations/20260913050000_channel_wiring_claim_and_servers.sql` (the exact prior body of
  `hive.node_claim_card` this ADR's migration extends, preserving every existing condition)

## Amendment — 2026-09-16: stdio MCP is the interoperability layer, and v1 needs a catalog

**Prompted by Jack, 2026-09-16**, on finding that Hermes (Nous Research) ships a community
plugin catalog: "I wonder if this would help us? Should we try to have cross compatible
plugins?"

### What Hermes actually does

Their catalog is one YAML file per plugin in a `plugin-catalog/` directory, submitted by pull
request from the plugin's own maintainer. Each entry declares `name` (the install key), `repo`,
`sha` — **an exact 40-hex commit, and installs check out that pin rather than a branch tip** —
`tier` (`official` or `community`), `category`, `maintainer`, and `capabilities` (declared
tools, hooks, middleware, and required env vars), with optional `requires_hermes`, `platforms`
and `docs_url`. `hermes plugins install <name>` puts a plugin on disk; it must then be
**explicitly enabled** before it loads. The catalog is published as live JSON and re-checked
every six hours, comparing pinned commits against current entries.

The detail that decides our answer: **their plugins are not MCP servers.** Some *bundle* one —
their `touchdesigner` entry ships the twozero MCP server, `snyk` pins `npx -y snyk@<version>
mcp` — but the plugin format itself is their own "Agent Plugins v1", and an MCP server is an
optional component inside it.

### Decision

**1. We do not adopt Agent Plugins v1.** Adopting another product's plugin format means
committing to a spec we do not control, for a surface we would then have to keep compatible
across their versions. The public documentation does not even publish the manifest schema, so
the cost is unknown — and an unknown cost is not a thing to sign up for while ADR-023's own v1
is unbuilt.

**2. stdio MCP is our interoperability layer, and that is enough.** A member-hosted stdio MCP
server already runs under Hermes *and* satisfies this ADR's v1 shape unchanged. That is real
cross-compatibility at zero architectural cost: a member who writes an MCP server for their
Postgres database or their homelab can point either product at it. We should say so explicitly
rather than leave it as a coincidence, and we should avoid any Hive-specific extension to the
stdio contract that would break it. This matters more than it looks: Nous is already a BYOK
provider in this codebase (`AgentRuntimeKind::NousByok`), so Hermes members are plausibly Hive
members.

**3. v1 gains a catalog, and it borrows Hermes's shape.** This ADR specifies a schema, a
claim-time gate and an stdio client, but no registry — there is no UI to register a server and
no way to discover one, which is why the MCP surface is headless-only today. The Hermes design
is a proven answer to exactly that gap and we should copy the parts that carry their weight:

- **one declarative entry per server**, reviewed rather than self-published;
- **pinning to an exact commit SHA, not a tag or branch.** This is the most important borrowing
  and the least optional. ADR-023 already lets a card spawn an arbitrary member-configured
  subprocess, which this ADR itself calls an unconstrained host process; a mutable reference
  turns "the member approved this server" into "the member approved whatever that repo contains
  today". A pinned SHA is what makes approval mean something;
- **declared capabilities** — tools, and required env vars especially — so the member can see
  what a server will touch before enabling it, and so `required_capabilities.mcp_tool_name` can
  be validated against a declaration instead of discovered at spawn time;
- **install and enable as separate steps.** This ADR's §2 already gates on the member's own
  `enabled` flag; Hermes's split is the same instinct and confirms it.

We do **not** borrow their six-hour live-JSON refresh for v1. Auto-refreshing a catalog of
things that spawn processes on a member's machine is a supply-chain surface, and this ADR's
trust posture ("Hive never runs anyone's MCP server") argues for the member deciding when to
move a pin.

### What this does not decide

Remote MCP transports (`http`/`sse`) stay deferred, as in the original decision — though the
vendor-hosted remote MCP servers now shipping from GitHub, Notion and others are a real 2026
trend and will force that question sooner than this ADR assumed. Whether Hive ever *publishes*
plugins into Hermes's catalog, rather than merely being compatible with the same MCP servers,
is a product question and is not decided here.

### Provenance and its limits

Written from a single page of Hermes's public documentation
(`hermes-agent.nousresearch.com/docs/user-guide/features/plugin-catalog`). The manifest schema
for Agent Plugins v1 was not published there. Decision 1 is therefore a decision made under
acknowledged uncertainty: it declines an unquantified commitment rather than judging the format
unsuitable. If someone establishes that the format is small, stable and documented, decision 1
is worth revisiting — decisions 2 and 3 stand on their own either way.

Recorded by Claude (Loki) from Jack's question of 2026-09-16.
