# ChatGPT subscription integration — implementation handoff to Claude

**Author:** Sif your friendly Codex Agent  
**Date:** 2026-09-15  
**Status:** Detailed implementation specification for integration; not implemented or released.  
**Request:** Jack wants people to use their existing subscription instead of setting up separate API billing. Preserve the six-machine private fleet, one local model per machine, working together. Subscription access is the preferred cloud onboarding route where supported; paid API use remains explicitly optional.

## 1. Product contract

Add **Connect ChatGPT**, with explanatory copy “Uses Codex access included with your account. Your plan's usage limits apply.” Offer **Local models** alongside it. Keep **API key — separately billed** as an explicit alternative, not a prerequisite or automatic fallback. Preserve existing sessions' provider selection during migration; recommend subscription access for new cloud setup without changing an existing paid session behind the user's back.

A locally running Codex agent runtime provides cloud reasoning and delegates work through Hive. Hive owns fleet authorization, tasks, placement, receipts, and acceptance. Six local models continue to execute worker jobs. This is a new agent-runtime integration, not an OAuth credential passed to the current `interview` or `code-brain-turn` Edge Function. Hive membership login remains separate.

No silent API fallback, paid-model substitution, credit purchase, earned-reset redemption, or account switching. A limit pauses new coordinator turns; already-submitted local jobs can continue. Offer “Wait for reset” and “Continue with a local coordinator” only when that local handoff is implemented and authorized. Never imply local jobs have paused just because the coordinator has.

“Subscription” does not guarantee unlimited or zero additional charges under every account policy. Codex may have account/workspace credits and controls outside Hive. Show the available plan/limit information, do not enable paid features, and do not market a universal no-charge guarantee that the runtime cannot enforce. Unknown limit data is unknown, not unlimited. API-key switching requires an explicit user choice and preserves an audit record.

## 2. Architecture and ownership

```text
Hive desktop (Swift on macOS; Tauri on Windows/Linux)
  └─ shared Rust SubscriptionCoordinator service
       ├─ local session/operation journal
       ├─ supervisor → per-user Codex app-server (stdio)
       │                 ├─ managed OpenAI browser sign-in
       │                 ├─ cloud model turns under account entitlements
       │                 └─ local stdio MCP process: hive coordinator-tools
       └─ scoped broker ← authenticated local IPC ← MCP process
                          └─ existing Hive card client / LocalHub
                               └─ six independently running local workers
```

The Rust service owns process I/O and application state; native UI renders typed events only. Codex owns its OAuth callback, token storage and refresh. Hive must not intercept tokens or put them in Supabase, prompts, vault notes, telemetry, argv or fleet job payloads. The MCP shim holds only a short-lived capability for a scoped Hive broker, not a member-wide database/service credential. Keep durable coordinator receipts local initially. If a selected project is hub-backed, its normal card metadata still uses that backend; do not relabel this entire topology “fully offline.” Cloud coordination sends selected task/context/results to OpenAI with the user's consent.

Choose **stdio**, not a network-exposed Codex listener. Official docs explicitly mark WebSocket transport experimental/unsupported and contain an app-server production-support caveat in their remote-host section. Local stdio is the proposed prototype transport, not proof of general production support. Validate a pinned release and applicable support/entitlements before public rollout. Enterprise guidance additionally requests known-client registration with OpenAI. Do not block local engineering on a hypothetical account issue, but do not claim those deployment questions resolved.

## 3. Existing code and required seams

| Existing surface | Integration change |
|---|---|
| `apps/desktop-swift/Sources/OHHive/ChatEngine.swift` | Add stable subscription provider identifier and delegate turns to the Rust runtime service; never reuse BYOK message routing. |
| `ChatSessionStore.swift`, `HiveStore.swift`, chat/settings views | Persist provider/session references, display auth/progress/limits/approvals; no credentials. Version old records without rewriting their provider choice. |
| `crates/ohhive-ffi/src/lib.rs` and new `subscription.rs` | Typed methods and event callback bridge to shared service; regenerate UniFFI bindings. |
| `apps/desktop` Rust command layer and React settings/chat | Expose the same service and states on Windows/Linux. Locate exact current command files before editing. |
| New `crates/ohhive-core/src/subscription/` | Supervisor, protocol, auth, sessions, journal, broker, policy and event reducer. Gate with `subscription-coordinator`; no inference dependency in regional server. |
| `crates/hive/src/main.rs` | New **proposed** `coordinator-tools` stdio MCP subcommand using shared broker/client; not currently implemented. |
| ADR-030/031 submission service | Reuse stable IDs, structured status and result semantics. Implement missing dedupe/placement contracts before automatic delegation. |
| ADR-032 coordinator logic | Reuse domain task lifecycle; do not replay the full Codex agent inside `CodeBrain::next_turn`. |

**ADR-032 reconciliation:** its existing `coordinator: true` card uses a worker lease and an internal model/tool loop; its spawn/wait operations act as the currently leased node. A desktop Codex coordinator cannot pretend to possess that lease. Phase 1 here uses a separate durable external coordinator session with linked ordinary Hive cards, submitted under actual user authority. It consumes no worker slot. Keep the existing coordinator-card route working. If later exposing this runtime as a schedulable agent, add an explicit runtime discriminator and lifecycle executor; do not pass a fake API key or invoke nested model loops. Cross-project access is limited to an owner-approved project set; project ownership is rechecked at submission and claim.

## 4. Runtime packaging and startup

1. Discover a supported Codex binary (configured absolute path or a verified packaged sidecar), inspect version, and use generated protocol schemas for that exact version. Current local evidence is **codex-cli 0.149.0**, not a nominated shipping version.
2. For a developer pilot, use the installed binary. For consumer onboarding, package or install a pinned official artifact with integrity verification, license notices, architecture mapping, and normal signing/notarization requirements. Do not silently replace the user's independently installed Codex. Verify redistribution/release support before bundling.
3. Give Hive an isolated, per-OS-user Codex home under its application support directory. This deliberately requires one Hive sign-in and avoids modifying/logout of a user's separate Codex installation. Verify keyring namespace isolation on each platform; a separate directory alone is not evidence that credential-store accounts are isolated.
4. Spawn the binary directly with argv `app-server` and piped stdio, in a Hive-controlled coordinator directory. Set the child environment's `CODEX_HOME` to that directory; never overwrite the running parent environment. Remove inherited API/access-token overrides and unintended provider/proxy configuration. Respect managed organizational restrictions rather than bypassing them.
5. Configure `forced_login_method = "chatgpt"` and `cli_auth_credentials_store = "keyring"`. If secure storage is unavailable, return actionable setup failure; offer a deliberately selected ephemeral session where supported rather than silently falling back to plaintext.
6. Disable inheritance/import of personal plugins, hooks and project configuration where the pinned runtime allows it; use an empty coordinator workspace. Verify effective configuration and allowed tool surface. A prompt saying “only use Hive tools” is not enforcement.
7. Supervise one runtime per active coordinator scope initially. A configured MCP bridge is process-scoped, so bind that process to the exact Hive member/workspace and approved projects. Do not multiplex unrelated principals through one bridge.
8. Runtime update: stop admitting turns, finish or explicitly interrupt active work, preserve journal, replace atomically, verify handshake, then resume. Retain rollback binary. Never reinterpret an old session as a newly submitted job on update.

App backgrounding can keep the local coordinator alive while the app process lives. App quit suspends coordination in the first release; submitted worker tasks remain durable. Restart recovery is required. A separate headless service/autostart continuation is a later integration stage, not an implicit promise of this UI feature.

## 5. App-server protocol implementation

Generate JSON schema from the pinned runtime (`codex app-server generate-json-schema --out <fixture-dir>`). Commit generated fixture hashes and the runtime version with integration tests. Latest docs and the installed binary differ: in 0.149.0, normal `ThreadStartParams` has no `dynamicTools`; externally supplied ChatGPT tokens are marked internal/unstable. **Use managed login plus a configured MCP bridge. Do not use external token injection or assume dynamic tool injection exists.** Evidence: `docs/subscription-integration/2026-09-15/schema-check.json`.

Transport implementation: newline-delimited JSON, JSON-RPC request/response IDs with the `jsonrpc` field omitted as specified by Codex. Use a single stdout reader that demultiplexes responses, notifications and server-initiated requests; a bounded serialized writer; a separate capped stderr drain. JSON-RPC IDs correlate transport messages only, not durable job identity. Never block the reader on a UI approval. Reject malformed/oversized frames, report a protocol error and reconnect/reconcile. Initial configurable budgets: 8 MiB/frame, 128 queued control messages, coalesced text updates, 15-second handshake and bounded account/status requests. Login follows runtime cancellation/expiry; agent turns are event-driven, not subjected to the short RPC acknowledgement timeout. Never retry a mutating request solely because its acknowledgement timed out.

Minimal flow (documented wire methods, fields verified against the selected schema during implementation):

```json
{"id":1,"method":"initialize","params":{"clientInfo":{"name":"hive_desktop","title":"Hive","version":"<Hive version>"}}}
{"method":"initialized"}
{"id":2,"method":"account/read","params":{"refreshToken":false}}
{"id":3,"method":"account/login/start","params":{"type":"chatgpt"}}
```

Open the returned runtime-owned auth URL in the system browser. Correlate `loginId`; handle `account/login/completed` and `account/updated`. Device-code login is a fallback when enabled for the account; use the documented `chatgptDeviceCode` variant. Stale completion of a cancelled attempt must not activate a new scope. Re-read account after success; only the expected managed ChatGPT mode qualifies. Show missing entitlement/workspace restrictions without requesting an API key as the sole recovery.

Then query `model/list` with pagination and `account/rateLimits/read`. Use runtime-returned IDs and supported effort settings, never the BYOK model list or a hardcoded plan-to-model table. Model listing is not proof a subsequent turn cannot be denied.

Start/resume via `thread/start` / `thread/resume`; start work with `turn/start` after a durable local intent is recorded. Store returned thread/turn IDs. Reduce `turn/started`, text deltas, tool/item events and `turn/completed` into Hive events. Recover final items using `thread/read`; stream deltas are not durable acceptance evidence. Handle `turn/interrupt`, `account/login/cancel`, `account/logout`, rate-limit updates and account changes. Unknown notifications can be retained as bounded diagnostics; unknown server requests must receive a supported protocol error/decline rather than being silently accepted or leaving the turn hanging.

Account switch/logout: immediately block new turns and bridge submissions, invalidate broker capability, resolve/decline pending approvals, interrupt current turn, then clear runtime login. Retain task receipts under the original identity. Existing local cards continue unless separately cancelled with a supported Hive operation. Logout is not cancellation of fleet work.

## 6. Hive tool bridge and domain contract

Use a real stdio MCP implementation with standard initialization, tool listing and calls. The existing Hive MCP **client** is not an MCP server. Do not improvise its wire protocol. Pin a compatible MCP library and test discovery through the actual Codex runtime. Suggested private config, generated with a TOML serializer and absolute executable paths:

```toml
forced_login_method = "chatgpt"
cli_auth_credentials_store = "keyring"
[mcp_servers.hive_fleet]
command = "/absolute/path/to/hive"
args = ["coordinator-tools"]
env_vars = ["HIVE_COORDINATOR_BROKER", "HIVE_COORDINATOR_CAPABILITY"]
enabled_tools = ["hive_projects_list", "hive_fleet_list", "hive_card_submit", "hive_card_status", "hive_card_result", "hive_coordinator_wait"]
```

The environment fields carry only local IPC location and an expiring scoped capability, injected by the supervisor. Prefer Unix sockets on Unix and user-ACL named pipes on Windows. The broker owns authenticated Hive operations and validates capability/session/account/policy on every request. Start with one scope per runtime so MCP calls cannot claim another thread's scope. If credentials must reach a shim for a pilot, disclose and constrain that design; never grant service-role/admin access. Limit native Codex tools with actual runtime policies and project-scoped permissions; verify effective tools. Default coordinator directory contains no repositories/secrets. Tool permissions in this broker remain enforced independently of model instructions.

Proposed tools (new contract, not existing flags or RPC promises):

| Tool | Input | Output/behavior |
|---|---|---|
| `hive_projects_list` | cursor, limit ≤50 | Only approved owned projects, IDs and short summaries; no broad private corpus dump. |
| `hive_fleet_list` | project_id | Eligible node IDs, capabilities, online state and workspace bindings; no credentials. |
| `hive_card_submit` | operation_key, project_id, task, acceptance, target_node_id, workspace_binding_id OR repo binding, local model preference | Durable request/card ID; existing replay returns same card; only local worker brain in this profile. |
| `hive_card_status` | card_id | Raw state, normalized phase, attempt/revision where available, observation time, stale flag. |
| `hive_card_result` | card_id, cursor, limit | Preview + artifact references, test evidence, acceptance state, truncation metadata. Fetch content only as needed. |
| `hive_coordinator_wait` | bounded list of linked card IDs | Registers a durable watch and immediately returns `waiting`; model ends its turn. Supervisor resumes only for a material result/change. |

No generic HTTP, SQL, shell, credential retrieval, community submission, account reset or purchasing tool. Actions already authorized by the project's policy do not need per-spawn approval. Broader project access, publishing and sensitive operations obey the user's configured policy; never weaken managed Codex requirements to achieve convenience.

**Validation:** broker injects member/backend/session identity (never trust model-supplied ownership); project set, workspace binding and exact target ownership checked server-side at submission and at claim. Raw paths never identify a workspace on an arbitrary node. Pin resolved commit for repository mode; private repo access stays on worker. Enforce task ≤20,000 characters, previews ≤16 KiB and tool responses ≤256 KiB, with explicit larger-artifact retrieval. Secret content is excluded from coordinator retrieval. Full project-context permission does not authorize credentials.

**Idempotency:** model supplies a session-unique operation key; journal assigns a request UUID before sending. Unique `(session_id, operation_key)` and payload digest bind it to immutable intent. A duplicate key with changed payload is a conflict. Replayed MCP request maps to that operation; it must not generate a new job. Child RPC must enforce the same request ID atomically. If backend lacks this or the proposed migration is unapplied, report feature unavailable/uncertain submission; do not emulate safety with client memory only. Use typed library methods, not the current CLI's project-title matching.

**State semantics:** `review` = output ready, not accepted; `done` = accepted per Hive rules; `waiting_on_child` remains unfinished; `blocked` needs attention; unknown raw states never mean success. Network failure marks observations stale, not failed. Do not use ADR-032 `wait_on_child` from this external session because it requires a real leased parent.

The wait tool records its subscription before returning. It does not block a model call for hours. If the runtime supports no safe host-driven turn ending, instruct the coordinator to finish and record a host state; never fabricate a successful `turn/completed`. Store resulting tool output normally. After completion, a host watcher batches meaningful child changes into one compact next-turn message, with a stable wakeup ID. No model turn for unchanged polling. Waiting local tasks continue across UI closure; automatic cloud wakeups require the applicable entitlement/runtime deployment to be validated.

## 7. Durable journal and recovery

Use a separately versioned local integration database initially; avoid unrelated changes to the evolving LocalHub vault schema. Proposed logical tables:

- `subscription_accounts`: opaque local account reference, member/workspace binding, runtime version/home reference, auth/limit observation timestamps. **No tokens.**
- `coordinator_sessions`: UUID, account reference, allowed-project policy/version, backend identity, Codex thread ID, state, last completed turn, consent version, timestamps.
- `coordinator_operations`: UUID, session, operation key, payload digest, durable request UUID, card ID nullable, submission state, last observation; unique session/key and request UUID.
- `coordinator_turns`: local intent UUID, session, input digest, Codex turn ID nullable, sending/acknowledged/completed/uncertain state.
- `coordinator_watches`: linked cards and last handled event/revision, wakeup UUID and delivery state.
- `coordinator_approvals`: bounded nonsecret request summary, runtime generation/thread/turn/request IDs, pending/resolved/expired state.

Protect database/transcripts as private user data. Encrypt sensitive retained input with an OS-backed key or store only references/digests; metadata itself can be private. Persist ordered state transitions in transactions, not per-token writes. Keep authoritative Codex conversation in its runtime storage and avoid maintaining conflicting second transcripts.

On crash: acquire single-owner process lock, load journal, read account, verify identity, reconnect runtime, resume/read known thread, reconcile linked cards, then rebuild pending UI. A lost `turn/start` acknowledgement is **uncertain**, not permission to replay automatically: inspect thread history; if not unambiguously attributable, ask for resume/reconciliation. A lost submit reply reuses the request UUID and identical payload. Completed cards are never resubmitted merely because a runtime turn was lost. Cloud turn replay may duplicate inference; promise durable deduplicated job submission, not exactly-once model execution or filesystem effects.

Interrupt coordinator stops its cloud turn and new dispatch. Cancelling a fleet card is a separate capability: return `unsupported` if not implemented end-to-end; don't label interruption as remote cancellation. Restart restores pending watches and coalesces already-observed changes. Account/project revocation invalidates broker access even for a previously approved tool call.

## 8. Shared UI/FFI interface

Proposed typed service methods: `subscription_status`, `subscription_connect`, `subscription_cancel_login`, `subscription_disconnect`, `subscription_models`, `subscription_limits`, `coordinator_start`, `coordinator_send`, `coordinator_resume`, `coordinator_interrupt`, `coordinator_respond`, `coordinator_sessions`. Return opaque login/session IDs and typed results. JSON protocol stays inside Rust.

Events: `AuthChanged`, `LoginRequired`, `TextDelta`, `TaskLinked`, `TaskChanged`, `ApprovalRequested`, `UserInputRequested`, `LimitsChanged`, `CoordinatorPaused`, `TurnCompleted`, `RuntimeUnavailable`, `ProtocolMismatch`. Scope every event by session/runtime generation; reject late responses from a previous generation. Throttle text repaint without dropping completion/approval events. Slow UI consumers trigger resynchronization from durable state, not unbounded queue growth.

Auth UI states: missing_runtime → starting → signed_out → signing_in → ready, with reconnect_required, unavailable_entitlement, limited, runtime_error. Coordinator states are separate: idle/running/waiting_on_fleet/awaiting_user/paused_limit/disconnected/completed. A ready account does not imply a running coordinator.

Approval request payloads/decision enums come from pinned schemas. Render the real action, location and reason; return only an available decision. Resolve by original runtime request ID and thread/turn; don't accept stale prompts after interruption. No timed approval by silence. If UI disconnects, keep a bounded pending state or decline/interrupt; do not auto-accept.

Web-only clients cannot spawn a desktop process. Initially show “Open Hive desktop to connect ChatGPT”; later use Hive's authenticated device channel to reach the designated desktop host. Never expose Codex's raw app-server port to browsers or route subscription credentials through Supabase. Match functionality on all three desktop platforms; don't claim parity from a Swift-only implementation.

## 9. Limits and billing enforcement

Read rate-limit snapshots on connection, limit notifications, and meaningful state changes; use bounded polling only while visible if notifications are absent. Support multiple buckets, null values and provider-supplied reset times. Enforcement trusts actual request failures too; a preflight snapshot cannot reserve quota.

On quota/auth failure: record paused state, preserve work, disable new cloud turns, keep watching local cards without cloud calls. No API, alternate account, purchased-credit or reset endpoint invoked automatically. Do not send provider email nudges. Local coordinator handoff is a distinct new session with explicit or previously saved policy, compact task receipts and no credentials; do not imply the same cloud transcript transparently becomes a local model session. Show cloud-context consent when enabling coordination and maintain the existing fully-local route independently.

## 10. Delivery sequence and completion gates

1. **Runtime contract package:** supervisor, generated schemas, fake server and event reducer; no cloud calls. Gate: interleaved requests/events, bounded buffers, crash/restart and unknown-message tests pass.
2. **Account connection:** managed browser/device login, keyring isolation, account/model/limit UI on shared FFI; no delegation. Gate: real user sign-in/logout/reconnect; no API key required or leaked; signed-out mode never falls back to inherited credentials.
3. **Scoped bridge:** projects/fleet/status/results plus durable submit with exact placement. Implement/deploy missing backend contracts in a separate reviewed migration. Gate: cross-owner rejection, duplicate replay, wrong target and revoked workspace tests.
4. **Coordinator lifecycle:** thread/turn mapping, watches, approvals, interruption and recovery. Gate: two workers, genuine test task, one requested correction, restart during wait, final acceptance with evidence and no duplicate cards.
5. **Six-worker pilot:** one model per machine; coordinator delegates distinct bounded tasks and integrates outputs without stealing a local worker lease. Exercise unavailable node and quota pause. Verify no repeated cloud polling and no API fallback.
6. **Cross-platform release:** signed/pinned runtime packaging, upgrade rollback, all three desktop shells, documented support/entitlement constraints and privacy behavior. Feature stays marked experimental until these gates pass. Headless unattended availability and web-to-desktop support remain separate gates.

Claude owns integration sequencing under existing file-ownership agreements. This handoff authorizes no production migration by itself. Jack requested the complete writeup for integration; this document does not pretend the feature has shipped.

### Required acceptance matrix

| Scenario | Required result |
|---|---|
| Fresh user with eligible subscription, no API keys | Browser login → selectable available model → coordinator turn. |
| Existing user API environment/config | Subscription runtime rejects API mode; no silent billing path. |
| Expired/revoked auth, denied workspace, cancelled login | Clear state; no tasks dispatched under stale identity. |
| Account switch while tool pending | Old broker capability revoked; no cross-account receipts/results. |
| 1 and 6 local workers | Correct targets, local brains preserved, test evidence linked. |
| Lost submit response / same key changed task | Same card on retry / explicit conflict respectively. |
| Crash after turn submission before acknowledgement | Reconcile or surface uncertain state; no blind replay. |
| Restart while children run | Watch existing IDs; no duplicate jobs; resume compactly. |
| `review`, `blocked`, unknown state | Output-ready, needs-attention, unknown respectively; never false acceptance. |
| Quota exceeded / missing limit snapshot | Pause / display unknown; no fallback charge or limit bypass. |
| Slow UI / malformed protocol / oversized output | Bounded resource use, recoverable error and state resync. |
| Approval after turn interrupt | Stale answer rejected; no auto-approval. |
| Offline coordinator / offline worker | Separate statuses; no invented cancellation or completion. |
| macOS, Windows, Linux | Native login/callback, secure storage, pipes, process cleanup and recovery tested. |
| Updated runtime | Schema compatibility checked before activation; rollback works. |

Run relevant Rust feature tests, CLI/FFI builds, regenerated bindings and each desktop shell build. Use fake app-server/MCP/broker integration fixtures for failure injection. Live tests need a user-owned eligible account and disposable project; never log OAuth payloads or real secrets. Report unsupported combinations rather than substituting API billing to pass a test.

## 11. Source and verification record

Official sources fetched for this handoff:

- [Codex App Server](https://learn.chatgpt.com/docs/app-server): embedding, transports, auth, models, turns, approvals and rate limits.
- [Authentication](https://learn.chatgpt.com/docs/auth): subscription versus API identity, credential storage and automation distinctions (read during preceding verification).
- [Configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference): managed login mode, keyring and configured MCP servers.

Local source inspected: ADR-030/031/032, current coder coordinator tools, CLI submission, Swift ChatEngine/session persistence, and FFI layout. Offline `codex --version` and schema generation succeeded (0.149.0); incidental PATH-alias warning did not prevent generation. Schema summary saved alongside this handoff. No live sign-in, model call, new runtime installation, application implementation, migration or fleet test was performed. Method/config names above must be checked against the chosen shipping version; proposed Hive interfaces are explicitly new.

Sif your friendly Codex Agent
