# Hive feature parity audit: Hermes Agent and Buzz

September 15, 2026 UTC · Sif your friendly Codex Agent

## Assessment

Hive is not yet at functional parity with either product. Its strongest foundations are distributed card execution, local inference, owner-controlled local storage, and community compute. The largest remaining gaps are a complete general-purpose agent loop, external-agent integration, automatic knowledge/skill maintenance, recovery for coding sessions, and an equally capable interface on all three desktop platforms.

The queued skill-file package is complete: 40 standalone and 139 full core tests passed on macOS in that build. It supplies storage and usage metadata; it does not turn on automatic learning. A final log check found Claude's new shared-brain refactor and his ownership of the next card-submission CLI slice; it did not assign another package to Sif.

The best route is to finish Hive's fleet orchestration and integrate existing agent runtimes, while extending Hive's own worker where it provides clear value. Literal parity with Buzz would also mean building a collaboration suite and Git forge. That is a separate product expansion, not a prerequisite for Jack's fleet goal.

## Baseline and method

Compared the current official feature documentation and repository descriptions against Hive source, ADRs 020–030, prior handoffs, and the continuity record. Release pages resolved to [Hermes v0.21.3 / v2026.9.14](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.9.14) and [Buzz Desktop v0.5.23](https://github.com/block/buzz/releases/tag/desktop-v0.5.23). Documentation is the live main-branch baseline and may include work newer than those packages; this is not a claim that every documented feature exists in those binaries.

Hive checkout HEAD: `2c8fefe678c734e462497cf162beefb6651fe57d`, including concurrent uncommitted work. This is a broad source/documentation feature audit, not a live comparative benchmark, security certification, or exhaustive review of every plugin. Neither competitor was installed or exercised. Live backlog and separately deployed messaging services were not inspected. No parity percentage is assigned: weighting a theme, a secure scheduler, and a complete agent runtime equally would be misleading.

Statuses below apply to Hive: **Present** = inspected implementation; **Partial** = meaningful implementation with an identified gap; **Foundation** = isolated library/design/test support without the complete user flow; **Missing** = no corresponding implementation found in the inspected paths; **Unverified** = evidence insufficient to judge delivery. Present does not mean tested on every OS or deployed. Competitor claims are documented capabilities unless explicitly marked roadmap.

## Evidence map for Hive

Paths below are relative to this report's repository root, `OH Cloud-src`.

| ID | Evidence inspected | What it establishes |
| --- | --- | --- |
| E1 | `crates/ohhive-core/src/worker.rs`, `tools.rs`, `coder.rs` | General Draft/Critique/Revise worker; fixed pre-draft tools; separate actual coding tool loop; child-card waiting; coding resume explicitly absent. |
| E2 | `crates/ohhive-core/src/mcp.rs` | Outbound stdio MCP: initialize, list, one call, teardown. No HTTP, persistent conversation, resources or prompts in this implementation. |
| E3 | `crates/ohhive-core/src/skills.rs`, `skills/tests.rs`; `docs/SIF-WORKSPACE-SKILLS-2026-09-15.md` | Bounded skill files, compact listing, create-only publication and revision-bound usage. No caller in coder's loop yet. |
| E4 | `crates/ohhive-core/src/local_hub/{mod,transport,vault,vault_folder,vault_intake,vault_intake_folder}.rs` | Local data plane, transport, FTS5, grants/revision checks, bounded folder import, managed intake receipts and deterministic filing. |
| E5 | `apps/desktop-swift/Sources/OHHive/VaultView.swift`, `HiveStore.swift`; `crates/ohhive-ffi/src/local_hub.rs` | Local native note editor/search/import UI and bindings; not a complete cross-machine editing/sync experience. |
| E6 | `crates/ohhive-core/src/desktop/`; desktop handoff reports dated September 14–15 | Simulated policy, receipts, recovery, limits, direct Anthropic proposal adapter. Native capture/input and worker integration remain open. |
| E7 | `supabase/functions/{interview,code-brain-turn}/index.ts`; Swift `ChatEngine.swift`, `ChatSessionStore.swift`; `20260913010000_chat_memory.sql` | Provider-backed chat/planning and coding turns; persistent chat history and automatically updated bounded member memory. Not a fleet-wide autonomous project coordinator. |
| E8 | Swift `GoogleConnector.swift`, `ConnectorsSettingsView.swift` | OAuth/refresh/Keychain, Drive file creation and Gmail send methods, connection UI. Searches found method definitions but no callers for either action. |
| E9 | `apps/desktop/src/App.tsx`; Swift `ContentView.swift`, `PrivateFleetView.swift`, `KanbanView.swift`; `apps/web/app/`; `crates/hive/src/main.rs` | Unequal client surfaces. Tauri tabs still Setup/Node/Server/Earnings/Settings/About; CLI still Probe/Run/Models/Status/CheckIn/CheckOut/Work/Set/Pair. |
| E10 | `crates/hive-coordinator/src/lib.rs`, `coordinator_hub.rs`, `crates/hive-server/src/`; ADR-022/025/030 | Placement, regional transport, replication/backup infrastructure; these are not a model-driven project coordinator. External card CLI is proposed. |
| E11 | `nodeconfig.rs`, `capability.rs`, `sandbox.rs`, backend modules; node schedule migrations | Capability/permission configuration, WASI isolation, local inference/media backends, availability ingredients. No complete configurable private/community arbitration demonstrated. |
| E12 | Continuity entries and ADR amendments | Latest ownership and verification limits. Earlier private-visibility/payout defects were reported fixed and live-tested; do not reopen them as known-current defects based on the September 13 review. |

## Hermes parity matrix

### Agent execution, coordination and development

| Feature | Hermes reference | Hive status and remaining work |
| --- | --- | --- |
| General iterative tool use | [Tools overview](https://hermes-agent.nousresearch.com/docs/user-guide/features/overview) | **Partial, E1.** Coding can choose repeated tools; general cards still execute fixed pre-draft actions. Unify execution without losing existing checkpoint behavior. |
| Terminal and file work | [Project overview](https://github.com/NousResearch/hermes-agent/blob/main/README.md) | **Present/partial, E1/E11.** Real shell, read/write/list and repo preparation exist. Need consistent approval, process cleanup, artifact and recovery behavior across runtimes. |
| Isolated concurrent subagents and completion delivery | [Delegation](https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation/) | **Partial, E1.** Child-card creation/wait/dependency output exists; dynamic model-selected agent delegation, messaging and session lifecycle are not equivalent. |
| Programmatic orchestration of tools | [Code execution](https://hermes-agent.nousresearch.com/docs/user-guide/features/code-execution) | **Missing as a structured agent capability.** Running arbitrary Python via shell is not a scoped tool-RPC environment with compact returned results. |
| Alternative execution environments | [README](https://github.com/NousResearch/hermes-agent/blob/main/README.md) | **Partial, E11.** Native worker and WASI exist. No unified Docker/SSH/serverless/sandbox backend selection comparable to Hermes' documented terminal backends. Distributed Hive nodes are a different capability. |
| Working-directory checkpoints and rollback | [Rollback](https://hermes-agent.nousresearch.com/docs/user-guide/checkpoints-and-rollback) | **Missing for coding edits, E1.** Worker inference checkpoints and desktop simulated receipts do not restore modified project files. Add revisioned workspace snapshots and reviewable undo. |
| Coding session continuation/recovery | [Dashboard sessions](https://hermes-agent.nousresearch.com/docs/user-guide/features/web-dashboard) | **Partial, E1/E7.** Chat persistence exists; reclaimed coding cards explicitly start over. Persist attempts, tool receipts and recovery decisions before claiming resumable coding. |
| Context-file discovery | [Context files](https://hermes-agent.nousresearch.com/docs/user-guide/features/context-files) | **Missing in inspected coder.** Automatically load applicable project instructions with provenance, bounds and scope; do not mistake the agent's ability to read a file for automatic discovery. |
| File/folder/diff/URL references in chat | [Context references](https://hermes-agent.nousresearch.com/docs/user-guide/features/context-references) | **Missing as a unified reference mechanism, E7/E9.** Add expandable source references rather than pasting everything into each turn. |
| Semantic code diagnostics | [LSP](https://hermes-agent.nousresearch.com/docs/user-guide/features/lsp) | **Missing as native tools.** Shell-accessible compilers are useful existing capacity; structured symbol navigation and diagnostics are additional work. |
| Multiple-model answer aggregation | [Mixture of Agents](https://hermes-agent.nousresearch.com/docs/user-guide/features/mixture-of-agents) | **Missing as a defined workflow.** Draft/critique and multiple workers do not establish independent candidate generation plus synthesis. Lower priority than dispatch/review reliability. |
| Batch evaluation and trajectory export | [Batch processing](https://hermes-agent.nousresearch.com/docs/user-guide/features/batch-processing) | **Partial.** Cards and Halo benchmarks provide useful ingredients; no equivalent reusable batch/trajectory product surface found. Add repeatable fleet quality evaluation before optimizing model placement. |
| ACP editor integration | [ACP](https://hermes-agent.nousresearch.com/docs/user-guide/features/acp) | **Missing, E9/E10.** Neither an ACP host for installed agents nor Hive exposed as an ACP agent was found. These are separate directions. |
| General agent HTTP endpoint | [API server](https://hermes-agent.nousresearch.com/docs/user-guide/features/api-server) | **Missing as an equivalent, E7/E10.** Hive's internal provider-turn endpoint and hub RPCs are not a public agent-session API. |

### Memory, skills and automatic curation

| Feature | Hermes reference | Hive status and remaining work |
| --- | --- | --- |
| Agent-maintained bounded user memory | [Memory](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory/) | **Present/partial, E7.** Hive already updates member MEMORY/USER content. Gap: consistent project/agent scoping and use across local coding, remote agents and cloud coordination. |
| Searchable past conversations | [Memory/session search](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory/) | **Partial, E4/E7.** Saved chats and vault FTS exist separately. No unified agent tool for searching and paging historical conversations found. |
| Progressive skill discovery and loading | [Skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills) | **Foundation, E3.** Metadata inventory/read API complete; prompt selection, token budget and actual loop use remain Claude's integration. |
| Automatic skill creation and improvement | [Skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills) | **Foundation, E3.** Automatic behavior is approved in ADR-027; storage alone does not perform post-task judgment, creation or improvement. |
| Skill archive, pin, consolidation and rollback | [Curator](https://hermes-agent.nousresearch.com/docs/user-guide/features/curator) | **Missing beyond usage tracking, E3.** Hermes documents automatic stale/archive maintenance, recoverable archives and opt-in model consolidation. Add provenance, pinning, reversible updates and maintenance scheduling. |
| Skill catalog and reusable packages | [Catalog](https://hermes-agent.nousresearch.com/docs/reference/skills-catalog) | **Missing as product, E3.** File format support is not a bundled useful library, installer, update policy or supporting-file package validator. |
| Inspect/edit/remove learned knowledge | [Learning journey](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory/) | **Partial, E3/E5/E7.** Note editing exists. Skills visibility/delete and a joined learned-memory review surface remain open. A decorative graph is optional; correction and undo matter first. |
| External memory providers | [Memory providers](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory-providers) | **Missing as adapter contract.** Keep Hive's local knowledge store; allow explicitly enabled external backends later without confusing them with local-only storage. |
| Rich document extraction | [Document extraction](https://hermes-agent.nousresearch.com/docs/user-guide/features/document-extraction) | **Partial, E4.** Markdown intake works; document-format extraction with citations and bounded indexing remains a gap. |
| Joint human/agent library curation | Hermes' [curator is skill-focused](https://hermes-agent.nousresearch.com/docs/user-guide/features/curator) | **Partial, E4/E5.** Managed intake is deterministic and explicit. Continuous ingestion, deduplication, topic organization, agent write/undo policy and fleet-wide editor access remain open. This broader library requirement exceeds simple skill parity. |

### Tools, models and everyday use

| Feature | Hermes reference | Hive status and remaining work |
| --- | --- | --- |
| Persistent and remote MCP | [MCP](https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp) | **Partial, E2.** Stdio one-shot works. Add HTTP transport, persistent sessions, per-agent tool discovery/filtering and iterative invocation. Hive-as-MCP-control-server is separate work. |
| Plugin and lifecycle-hook ecosystem | [Plugins](https://hermes-agent.nousresearch.com/docs/user-guide/features/plugins) | **Missing as an installable extension system.** MCP is one useful extension route, not a replacement for lifecycle hooks, packaged skills, memory providers and extension management. |
| Scheduled autonomous jobs | [Cron](https://hermes-agent.nousresearch.com/docs/user-guide/features/cron) | **Missing equivalent, E11.** Computer check-in schedules are not recurring agent tasks with results, edits, pause/resume and missed-run handling. |
| Browser navigation/form interaction | [Browser](https://hermes-agent.nousresearch.com/docs/user-guide/features/browser) | **Foundation at most, E6.** No integrated browser session capability found. Simulated desktop clicks and screenshot proposals are not working browser automation. |
| Web search and fetched-source tools | [Tool Gateway](https://hermes-agent.nousresearch.com/docs/user-guide/features/tool-gateway) | **Missing integrated agent experience.** Configured MCP or shell might reach the web; provide discoverable tools, citations and explicit network policy. A Nous model key alone does not wire its Tool Gateway. |
| Local and cloud model selection | [README](https://github.com/NousResearch/hermes-agent/blob/main/README.md) | **Present/partial, E7/E11.** Local backends and cloud provider paths exist, including Nous. Needs a coherent coordinator/worker selection flow and reliable capability checks. |
| Provider priorities and fallback | [Routing](https://hermes-agent.nousresearch.com/docs/user-guide/features/provider-routing), [fallback](https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers) | **Partial.** Existing routing/overflow is not a user-configured, privacy-aware per-agent fallback policy with auxiliary-task routing. Never silently move local-only work to cloud. |
| Credential pools/rate-limit rotation | [Credential pools](https://hermes-agent.nousresearch.com/docs/user-guide/features/credential-pools) | **Missing as equivalent.** BYOK storage exists; no pooled credential scheduler found. Lower priority than clear account/limit errors. |
| Prompt caching and context management | [Configuration](https://hermes-agent.nousresearch.com/docs/user-guide/configuration), [Buzz agent context handling](https://github.com/block/buzz/blob/main/VISION_AGENT.md) | **Partial, E1/E7.** Individual output bounds exist; no coding history compaction policy found. Add token accounting, bounded retrieval and context recovery; verify provider-specific cache support separately. |
| Shared messaging gateway | [Messaging](https://hermes-agent.nousresearch.com/docs/user-guide/messaging/) | **Unverified/partial.** ADR-020 and notification SQL exist; bridge runtime was not located in this checkout. Verify deployed Telegram/Discord/Slack flows before assigning missing work. Broader channel and cross-channel session parity is not established. |
| Voice conversations and TTS | [Voice mode](https://hermes-agent.nousresearch.com/docs/user-guide/features/voice-mode) | **Partial.** Whisper transcription and native Transcribe UI exist; no complete bidirectional spoken-agent loop found. |
| Wake word and vision/image interaction | [Feature overview](https://hermes-agent.nousresearch.com/docs/user-guide/features/overview) | **Partial/missing.** No wake-word flow found. Desktop adapter accepts screenshots but is not general multimodal chat. Audit attachment support separately before promising model-independent vision. |
| Image generation | [Feature overview](https://hermes-agent.nousresearch.com/docs/user-guide/features/overview) | **Present/partial.** ComfyUI backend, image function and native UI exist. Agent-selected generation, provider breadth and delivering results across clients need integration verification. |
| Unified settings, profiles and remote control | [Dashboard](https://hermes-agent.nousresearch.com/docs/user-guide/features/web-dashboard) | **Partial, E9.** Multiple Hive surfaces exist, but no unified agent-profile administration across machines. Windows/Linux native workflow coverage lags Swift. Hermes also documents a Windows limitation for its embedded dashboard TUI; do not assume universal competitor parity. |
| Subscription-backed access | [Hermes dashboard](https://hermes-agent.nousresearch.com/docs/user-guide/features/web-dashboard) and provider-specific documentation | **Missing supported Hive runtime integration.** Jack wants subscriptions plus keys where supported. Choose official runtime/auth flows and revalidate provider terms at implementation; do not treat a subscription as a generic API credential. |

## Buzz parity matrix

Buzz's [README explicitly separates working features from work in progress](https://github.com/block/buzz/blob/main/README.md). The table respects that distinction. Its implementation model also differs: [architecture describes a relay as the source of truth, not peer-to-peer replication](https://github.com/block/buzz/blob/main/ARCHITECTURE.md). Hive does not need Nostr internally to match a user outcome.

| Feature | Buzz baseline | Hive status and remaining work |
| --- | --- | --- |
| Channels, threads and DMs | Working, README | **Partial, E9.** Fleet activity/community surfaces exist. A complete permissioned human/agent workspace with those conversation types is not demonstrated. |
| Collaborative canvases and media | Working, README | **Partial/missing, E5/E9.** Local note editor and media tools exist; shared canvas editing, contextual media discussion and collaborative permissions are separate work. |
| Unified search and audit history | Working, README; architecture | **Partial, E4/E7/E10.** Vault, chat and card records remain separate. Provide one authorized search surface over decisions, conversations, artifacts, attempts and approvals. |
| Agent-first CLI and ACP harness | Working, README | **Missing equivalent, E9/E10.** Implement ADR-030 submit/status/await, then adapter lifecycle and events for Hermes/Codex/Claude Code. |
| Native agent with concurrent sessions | [Agent design's “What We Built”](https://github.com/block/buzz/blob/main/VISION_AGENT.md) | **Partial, E1.** Coding loop exists; independent agent profiles, session management, compaction and standardized protocol lifecycle need work. |
| Human/agent identity and scoped participation | [Architecture](https://github.com/block/buzz/blob/main/ARCHITECTURE.md) | **Partial.** Member/node identity exists; first-class agent identity with channel/project membership, distinct credentials and revocation is a gap. |
| Message/reaction/schedule/webhook workflows | Working, README | **Missing equivalent.** Cards/dependencies provide execution units, but no comparable user automation definition, event triggers and run history found. |
| Workflow approval gates | Being wired, README | **Not a shipped-parity requirement.** Hive should still build durable approval decisions for its own execution needs; simulated desktop approvals are not global workflow approval infrastructure. |
| Git event integration and hosting | Working, README | **Missing equivalent.** Repo checkout and shell git are not hosting, code review or branch-linked collaboration. Start with GitHub/forge adapters and artifact links; owning a forge can wait. |
| Branch-as-room, integrated CI/review/release story | [Projects vision](https://github.com/block/buzz/blob/main/VISION_PROJECTS.md) | **Roadmap comparison, not fully verified delivery.** Hive has Kanban and outputs, but needs typed review evidence, patch integration and release controls. Do not mark the entire Buzz vision shipped. |
| Self-hosted workspace isolation | [Architecture](https://github.com/block/buzz/blob/main/ARCHITECTURE.md) | **Partial, E4/E10.** LocalHub now exists; finish onboarding, device pairing, recovery, remote consent and supported packaging. Supabase independence must be assessed by selected execution path. |
| Desktop clients | Working, README | **Partial, E9.** Hive has Swift/Tauri but unequal product flows. Complete Projects/Fleet/Library/Agents/Automations/Settings on all three desktop OSes. |
| Mobile, push, huddle lifecycle, cross-relay reputation | README lists unfinished/planned portions | **Not current shipped-parity blockers.** Track mobile companion under ADR-021 and validate voice/huddle components individually; do not promise either product's complete vision as present. |

### Additional customization and discovery coverage

| Feature | Hermes reference | Hive status and remaining work |
| --- | --- | --- |
| On-demand tool search | [Tool Search](https://hermes-agent.nousresearch.com/docs/user-guide/features/tool-search) | **Missing equivalent, E1/E2.** Current coder advertises its small fixed set. A larger connected tool library needs searchable metadata and lazy schema loading to avoid prompt growth. |
| Independent profiles and personalities | [Profiles](https://hermes-agent.nousresearch.com/docs/user-guide/profiles), [Personality](https://hermes-agent.nousresearch.com/docs/user-guide/features/personality) | **Partial.** Member/node settings are not isolated agent profiles with separate model, memory, credentials and identity. Add these to the agent registry; themes are secondary. |
| Explicit skill invocation, bundles and source learning | [Skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills) | **Missing product flow, E3.** Users need a way to invoke procedures directly and teach from references, alongside automatic learning. Package composition and external-directory trust require explicit rules. |
| Plan-before-execution mode | [Skills documentation's plan mode](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills) | **Partial, E7.** Interview-generated planning exists; a consistent read-only planning mode across agent runtimes and clients is not demonstrated. |

## Important corrections and scope boundaries

0. **Concurrent update included:** Claude logged a new `brain.rs` shared message/tool interface during this audit. Its own scope explicitly excludes desktop provider/loop/worker wiring, and its build was not compiler-verified in that handoff. Credit the shared vocabulary as implemented; do not mark computer use complete. Claude also claimed the next ADR-030 CLI slice. Do not start a duplicate implementation. The 139-test result above predates this refactor.
1. **The coordinator name is misleading if read without code.** `hive-coordinator` places jobs; `CoordinatorHub` transports regional control. Neither proves a cloud agent can plan a whole project, delegate, inspect results, request repairs and finish independently of an open chat window.
2. **Google connection is not agent tool access.** The methods and connection UI exist, but no action callers were found. Scopes are `drive.file` and `gmail.send`; they do not provide inbox reading or arbitrary Drive library browsing. The original handoff explicitly said the Swift build and real OAuth round trip were unverified, despite a later queue summary calling the connector verified. Preserve the narrower evidence until a real test is recorded.
3. **The vault is already more than a plan.** There is tested FTS5, grants, revision checking, bounded intake and a basic native editor. Remote-reader transport exists. Full replica sync and a seamless shared editor are not complete. Keep the chosen central-query/local-folder direction; do not introduce iCloud/Drive database syncing as a shortcut.
4. **Computer use is still a foundation.** Policy, mock execution, budget and provider tests do not mean an agent can operate a real Mac, Windows PC or Linux desktop today.
5. **Old security findings are not current feature gaps by default.** The continuity record reports private visibility and payout fixes. This audit does not reproduce those defects or revalidate the live fixes. Regression verification remains a release requirement, not a claim the old bugs persist.
6. **Secrets need their own facility.** Existing Google Keychain and server-side BYOK storage deserve credit. A user-controlled cross-platform secret broker with scoped grants, revocation, redacted receipts and agent access was not found. Keep secret values out of Markdown, full-text indexes, embeddings and automatic curation. This is Jack's requirement, not a claim that either competitor has a universal secrets vault.
7. **External tools are not inherently absent.** Hive has real MCP calls and host tools. The gaps are persistent/general agent access, cross-platform connection management and external agents controlling Hive.

## Recommended delivery order

Recommendations for Claude and Jack to review; not newly assigned implementation work.

| Order | Build | Acceptance demonstration | Existing anchor |
| --- | --- | --- | --- |
| 1 | Complete skills integration and visibility | A task creates a useful skill automatically; a fresh session discovers and uses it; last-used changes only on actual use; user can inspect/remove it. | ADR-027; Sif storage complete, Claude loop/UI remaining. |
| 2 | External Kanban submission plus one agent adapter | A supported external cloud runtime submits a Mac build, gets an attempt ID, follows progress and retrieves a real artifact/test result. Expired auth and unavailable node produce clear states. | ADR-030 plus ADR-022; no Cmd Work dependency. |
| 3 | Durable cloud project coordinator and agent registry | Coordinator dispatches to two owned machines, inspects artifacts, requests one correction, integrates results, survives UI closure and reports completion against acceptance criteria. | ADR-022; extend current card/hub contracts. |
| 4 | Recovery, workspace isolation and unified tool loop | Kill a coding worker after a file edit and during a command; restart without blindly replaying uncertain effects. Demonstrate patch review/rollback and bounded context. Add persistent MCP/browser support behind explicit capabilities. | ADR-024/029 and worker checkpoints. |
| 5 | Complete fleet-wide library and reversible curation | Agent on machine B searches allowed library on A using compact snippets and source revisions. New local-folder documents are indexed under policy; duplicate/stale content can be curated with provenance and undo. | ADR-028; keep central-query first. |
| 6 | Native automatic jobs and community arbitration | Scheduled task runs with UI closed. Private work arriving during community work follows the user's selected finish/pause/stop behavior. No private files, memory, secrets or cloud allowance leak into donation. | ADR-020/022, availability foundations. |
| 7 | Equal desktop experience and integration verification | Clean install on Mac/Windows/Linux: pair, select model/agent, run project, inspect artifact, use library, pause contribution and recover failure without terminal setup. Verify one messaging channel and Google actions end to end. | Shared core plus Swift/Tauri/web. |
| 8 | Breadth after the core loop | Expand messaging, voice, document formats, plugin catalog, provider fallbacks, collaboration and forge integration according to actual demand. | Hermes/Buzz matrix above. |

These are dependency milestones, not a request to postpone all Windows/Linux work until the end: each new contract and feature should be implemented portably and validated on all three platforms as it lands. Likewise, secrets and permission boundaries must accompany features that consume them.

Suggested first supported external runtime is one with a documented lifecycle protocol and the user's supported account flow. Hermes' ACP support and Buzz's ACP direction make a common adapter contract attractive, but test the exact runtime versions. Do not assume every adapter can resume or preempt. Record those as capabilities and show the actual behavior to users.

Hive's distinguishing promise remains personal fleet management plus optional community capacity. Halo pooled inference is a capacity option, not a parity shortcut: a larger distributed model does not supply browser tools, durable sessions, curation or project orchestration by itself.

## Claude handoff and verification

Please reconcile these rows against your newest work and any separately deployed bridge service. In particular, review the connector caller/build gap, skill-loop/UI ownership, coding recovery, and the distinction between regional coordination and project coordination. Add evidence when closing a row: implementation path, tested user flow, supported platforms and deployment version. Avoid duplicating existing tasks solely because an ADR still says Proposed.

Audit actions were read-only source/documentation review plus this report and continuity/index updates. No application code, credentials, services or deployments changed. Existing test results are attributed to their build handoffs; tests were not rerun for this documentation-only audit.

Sif your friendly Codex Agent
