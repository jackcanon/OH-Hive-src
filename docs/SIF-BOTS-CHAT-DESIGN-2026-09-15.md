# Hive Bots: shared chat, agent DMs and visible teamwork

2026-09-15 · Sif your friendly Codex Agent · Proposed design and implementation plan.

## 1. Product goal and existing decisions

A member opens Hive and can talk to their whole team, a project team, or one named agent. An agent is a durable teammate with a role, tools, memory scope and a place it runs. It can use a local model or one of the three selected subscription runtimes. Six computers need not mean exactly six identities: a machine can host several roles, scheduled against its actual capacity.

[ADR-022](../ADR/ADR-022-personal-hive-fleet-control-plane.md) already selects a fleet channel with durable receipts, but section 6 deliberately excludes full Bot Mode. [ADR-032](../ADR/ADR-032-durable-project-coordinator.md) anticipates a role-scoped registry; its full registry and cross-project work are deferred. [ADR-035](../ADR/ADR-035-bots-chat-and-agent-collaboration.md) now explicitly extends those boundaries for this request. It does not claim the UI is built.

Observed foundation:

| Existing code | Reuse / gap |
|---|---|
| `apps/desktop-swift/Sources/OHHive/PrivateFleetView.swift` | Existing activity/member-post feed; no per-agent addressing or response dispatch |
| `apps/web/app/fleet/page.tsx` | Existing member-scoped feed with node filter; a filtered node feed is not a DM |
| `crates/ohhive-ffi/src/channel.rs`, `crates/ohhive-core/src/hub.rs` | Existing cloud-channel wrappers; new private chat must not inherit their unconditional cloud route |
| `ChatEngine.swift`, `ChatView.swift`, `ChatSessionStore.swift` | Existing assistant chat and local Swift JSON history; migrate explicitly into shared Rust-owned conversation storage |
| `crates/ohhive-core/src/local_hub/` | Existing SQLite/local transport foundation; add conversation domain and schema here, not a second ad hoc sync database |
| ADR-030/031/032 task submission and coordinator work | Durable execution and receipts; do not turn free-form posts directly into worker commands |
| ADR-027/028 skills and second brain | Bounded reusable knowledge; transcript/memory scopes must be added rather than assuming member-wide memory is safe for all bots |

## 2. Design references

Borrow interaction patterns, not branding or a pixel copy:

- **Grok Bot:** named bot conversations, group work, mentions, visible handoffs and results near messages. [Official chat guide](https://docs.x.ai/grok-bot/chat-and-collaboration)
- **Hermes:** persistent bot profiles, roster, role/tool configuration and a durable relationship with each bot. [Bot Mode](https://hermes-agent.nousresearch.com/docs/user-guide/bot-mode)
- **Buzz:** agents as participants with scoped sessions and separate agent/tool protocols. [Agent architecture](https://github.com/block/buzz/blob/main/VISION_AGENT.md)

Hive's distinction is that the user sees where each task actually runs and can coordinate their own local machines from the same place. No dependency on Grok Bot's hosted computers, Hermes gateway or Buzz's identity network is implied.

## 3. Information architecture and visual plan

Add **Agents** as one main workspace. Keep **Private Fleet** as the machine/activity view, reachable from Agents. Avoid two competing generic “Chat” destinations: keep old chats in an explicit history section until migrated.

Desktop layout: adjustable left sidebar (~260 px), flexible transcript, optional details drawer (~320 px). At narrow widths, roster and details become separate panes. Use Hive's existing fonts/colors, comfortable line length, readable status text and restrained avatar accents. No color-only meaning, no mandatory animation. Keyboard navigation, screen reader author/status announcements and reduced motion are first-slice requirements.

```text
┌────────────────────┬──────────────────────────────────────────┬───────────────────┐
│ AGENTS   + New      │ Launch website       Private · Local hub │ Team / work       │
│ Search              │ Coordinator: Sif · ChatGPT · Midgaard   │ Sif · planning    │
│                     ├──────────────────────────────────────────┤ Odin · working    │
│ Inbox           2   │ You                                      │ Heimdall · queued │
│ My Team             │ Build the demo; have Heimdall review it. │                   │
│ Projects            │                                          │ Tasks       2 / 3 │
│  Launch website     │ Sif                                      │ Build → Review    │
│                     │ Odin will build; Heimdall will review.   │                   │
│ DIRECT MESSAGES     │ [Build task · Odin · Working · Open]     │ Files             │
│ ● Sif               │                                          │ demo/             │
│ ● Odin              │ Odin                                     │ review.md         │
│ ○ Heimdall          │ Ready for review. [Changes] [Test result]│                   │
│                     ├──────────────────────────────────────────┤                   │
│ Private Fleet       │ Message team… @agent  Attach  Send       │                   │
│ Connection settings │ [Stop coordinator]  [View all activity] │                   │
└────────────────────┴──────────────────────────────────────────┴───────────────────┘
```

This wireframe is illustrative sample content, not current fleet status.

- **Inbox:** questions, approval requests, failures and completed results needing attention, each linked to its originating conversation. Read state is per user, not per agent.
- **My Team:** owner-private fleet conversation with one selected coordinator. Machine receipts are visible in an Activity filter; keep every underlying event, as ADR-022 requires, without forcing every lifecycle update into the conversational reading flow.
- **Project rooms:** project-scoped team, task threads, goal, acceptance criteria and selected coordinator. “Private” and “Community” are distinct labeled scopes, never a quiet toggle.
- **Agent DMs:** one canonical owner↔agent conversation, with topic threads when needed. Pin, rename or archive agents; opening a DM does not start inference. A bot identity survives rename or model change.
- **Details drawer:** role, runtime/provider, actual machine, tools, knowledge scope, active tasks and pause controls. Advanced settings stay behind disclosure.

## 4. Essential user flows

### Create or connect an agent

New Agent asks for name and job, then “Runs on”: one of the user's available local machines or a connected subscription runtime. Provide role templates (Coordinator, Builder, Reviewer, Researcher, Librarian); templates propose tools but cannot grant unavailable permissions. Preview authorized projects/folders and whether context goes to a cloud provider. Save a stable ID and revisioned profile. No hidden cloud fallback for a local role.

A deterministic avatar or existing node avatar is sufficient. Adding a cloud connection does not create three mandatory bots or three parallel coordinators. Connecting an existing external runtime maps a Hive identity to that runtime; adapter capability limits remain visible.

### DM an agent

Select the agent and send a message. The header shows role, model/provider and machine. A busy agent offers a queued reply or an explicit “Interrupt current work”; status questions use cached task receipts when possible. An offline agent accepts a durable queued message only when that is useful, with “Waiting for Odin” shown. It never silently runs on another machine/provider. The user can cancel a queued request.

DM context belongs to that DM. Sharing a fact/result to a project is an explicit share action or an already-authorized workflow, with destination and context visible. Another agent does not gain access to the DM just because it shares a room or belongs to the same member.

### Work together in a room

A normal user message goes to the room's coordinator. An `@agent` chip addresses that participant; multiple chips deliberately fan out one request per selected agent. Plain quoted text containing `@` does not route messages. With no coordinator, show “Choose an agent” rather than invoke an arbitrary model. `@everyone` is off by default; enabling it is explicit and bounded.

The coordinator turns a work request into a small visible plan and linked durable tasks. Standing authorization permits routine delegation within the configured scope without repeated approval. Show separate requests only for new permissions or actions requiring consent. Agents may post relevant findings without creating new jobs. Work receipts include assignee, actual machine, timestamps, artifacts and verification result.

### Stop and change course

“Stop coordinator” blocks new dispatch and interrupts its current runtime turn. Existing worker tasks continue unless the user selects task cancellation. “Stop team work” requests cancellation of linked tasks where supported and reports unsupported/offline cases truthfully. Neither button undoes completed actions. A user correction supersedes pending work only through revisioned cancellation/replanning, not by deleting history.

## 5. Identity, storage and API contract

Separate `agent_id` (teammate), `node_id` (machine), `provider_account_id` (credential reference), `runtime_session_id` (provider conversation) and `conversation_id` (Hive history). UUID identity is immutable; display names are not routing authority.

Proposed tables in the selected hub's conversation domain:

| Entity | Essential fields / invariants |
|---|---|
| `agent_profiles` | owner, id, name, role/instruction revision, runtime kind, preferred host, capability policy, memory namespace, archived flag; credential references only |
| `conversations` | id, owner, kind (`team`, `project`, `agent_dm`), project, coordinator, storage scope, policy revision |
| `conversation_members` | conversation, principal kind/id, allowed actions, join/history boundary; unique principal per room |
| `messages` | id, conversation, thread/root, authenticated author, server sequence, client request ID, kind, body/attachment refs, timestamp, task/turn/source event refs |
| `message_revisions` | original ID, replacement/tombstone, author and time; source receipts remain immutable |
| `agent_deliveries` | message+recipient unique key, pending/running/done/failed/cancelled/unknown, lease generation, retry deadline, bound runtime/turn |
| `runtime_bindings` | conversation/thread+agent+account+policy revision to runtime session; one fenced writer |
| `conversation_read_positions` | user+conversation, last seen sequence; notifications derived separately |
| `handoffs` | request ID, requester/assignee, project/task, accepted state, artifact refs, budgets and terminal receipt |

Create idempotent server-assigned sequence order per conversation; client timestamps are display metadata, not ordering authority. Use an atomic message+outbox transaction so restart cannot lose dispatch. Delivery is at-least-once with deduplication, not an exactly-once external-side-effect promise. Reuse the subscription journal for runtime turns and existing task IDs for jobs; do not create a competing scheduler in these tables.

Proposed shared service methods: `agents_list/create/update/archive`; `conversations_list/create/join`; `messages_list(before/after, limit)`; `message_send(client_request_id, expected_policy_revision, recipient_ids)`; `conversation_mark_read`; `handoff_create/status`; `delivery_cancel`; `conversation_search(scope, query, cursor)`. Authenticate before reading; derive author server-side. Neither a node nor model can submit arbitrary `author_id=another_agent`. Use scoped runtime capabilities tied to the agent/session, not generic member-wide node keys for new agent posting.

### Local-first and multi-machine rules

Use the selected LocalHub as authority for private conversations, with paired transport and local durable outbox/cache. Clients submit to that authority; do not invent multi-master writes or sync live SQLite files through Drive/iCloud. If the authority is down, drafts/outbox entries remain visibly pending until accepted. Remote private access needs existing authenticated transport, not a new public runtime port.

Hub-backed conversations use the same domain contract, with owner/project membership enforcement and migration/RLS tests. Never route fully local chat, attachments or memory through the existing cloud-only `channel.rs` methods. Storage-local plus a cloud coordinator still sends authorized context to that provider; show those two facts separately.

Push/event subscription is an optimization; durable cursor replay is recovery. Where unavailable, use one adaptive delta poll per visible account/view, not a five-second full-history request per bot. Virtualize transcripts and page messages. Do not enable Supabase Realtime implicitly against existing architectural policy.

## 6. Agent teamwork without uncontrolled conversation loops

One coordinator owns each project work plan. Agents can ask each other bounded questions or request specialist work through a structured handoff:

```text
handoff(request_id, source_agent, target_agent, project, task_or_question,
        acceptance_criteria, artifact_refs, allowed_tools, parent_run,
        reply_to_thread, max_followups, deadline)
```

The service validates membership, permissions and budgets, then records accepted delivery before waking the recipient. No acceptance means “sent/pending,” not “started.” Result messages link to actual artifacts/tests. Reviewers can reject a deliverable and request a correction; the coordinator integrates results and checks the user's acceptance criteria. A reviewer is not independent evidence if it simply repeats the builder's claim.

Suggested first defaults, configurable rather than user decisions: one active turn per agent, one coordinator per room, two active specialist handoffs per run, two correction rounds, and depth two. Exhaustion produces a concise blocker for the user. The six workers can still execute distinct tasks concurrently when their scheduler capacity allows; conversation-turn caps do not redefine hardware capacity.

Prevent loops structurally:

- Agent informational posts and lifecycle receipts do not wake every participant.
- Only addressed requests, relevant completion events or user messages enter a delivery queue.
- Deduplicate by source request + target + workflow step; carry causation IDs and hop counters.
- Reject cyclic wait graphs; queue busy workers and surface timeouts rather than infinite reply chains.
- Do not invoke a cloud coordinator just to say “still working.”
- Use existing node leases/capacity admission; a DM must not start a second model job over an occupied local worker slot.

For code, assign separate worktrees/branches or nonoverlapping files, then one integration owner. Peer messages do not grant merge/publish credentials. Multiple bots using the same model are distinct roles, not proof of independent verification. Community check-in uses the user's existing private-vs-community interruption preference; it never exports private DM history, vault secrets or provider credentials. This design does not authorize pooling subscriptions for other members.

## 7. Memory, search and privacy boundaries

Keep the durable transcript separate from working context. Each invocation receives: role/policy, authorized project brief, relevant thread window, outstanding handoffs and a bounded set of retrieved references. Do not inject the entire team feed or every DM. Summaries cite message IDs/ranges and carry a revision/watermark; originals remain retrievable. Context budgets are per runtime capability with a conservative configurable maximum.

Search first with scoped full-text indexes (SQLite FTS5/local hub; equivalent authorized server search). Return snippets and stable references, then fetch selected passages. Vault documents remain the knowledge source; chat can propose/save useful facts through ADR-028 curation with provenance. Shared project facts, agent memory and personal DM facts are distinct namespaces. Do not reuse `chat_memories` member-wide injection unmodified for the bots system.

Enforce membership and context policy during retrieval, delivery and tool execution—not only in UI. New participants receive a user-selected history boundary; joining a room never grants another room's DM context. Revoking access invalidates future retrieval and runtime bindings; it cannot erase content already sent to a cloud provider. Secret references can enable a trusted tool without revealing secret values to the model or conversation.

## 8. Migration and phased delivery

1. **C0 contracts:** add profile/conversation/delivery schemas, service interfaces and permission tests. Implement LocalHub first; define cloud parity without sending private data there.
2. **C1 useful single-agent DM:** agent roster, local worker conversation executor with existing capacity admission, persistent DM, offline queue, stop and task receipts. No cloud dependency to test the UI.
3. **C2 team and project rooms:** coordinator selection, mentions, threads, Inbox, task linkage and searchable history. Adapter-backed agents join as each of the three providers passes its own gates.
4. **C3 collaboration:** structured handoffs, reviewer/correction workflow, loop budgets, scoped retrieval and multi-machine cursor recovery.
5. **C4 polish/parity:** native Swift and Tauri builds tested equally, migration UX, keyboard/accessibility, export, retention/archive controls and performance. Web/mobile remote control follows host-availability and authorization gates; ADR-020 bridges are later clients of this domain.

Import Swift JSON chats only on user selection, with stable origin IDs and an idempotent migration ledger. Preserve original file/provider labels; do not relabel old generic assistants as a new bot. The old cloud fleet feed remains a read-only activity source during migration; do not automatically mirror its contents into a local hub or duplicate old posts into every room. Maintain explicit source IDs for receipts. Rollback disables new dispatch, retains data and restores old navigation; it never reruns messages.

## 9. Acceptance scenarios

- A local-only user creates two agents, sends a DM and gets a response with all network cloud routes disabled.
- A DM to Odin never appears in Heimdall's context/search; adding both to a project does not change that.
- Rename/move an agent and its DM identity/history remains stable; changing provider requires a new scoped runtime session.
- One team request creates one plan and bounded tasks; duplicate delivery/restart creates no duplicate jobs.
- A builder completes, reviewer finds one issue, builder corrects, coordinator links actual verification and closes the task.
- An unmentioned agent and a receipt-only message trigger zero model turns.
- Two desktop clients sending simultaneously preserve server sequence and one runtime writer; lost connection recovers by cursor.
- Offline/busy agents, quota pauses and unsupported cancellation are distinct visible states.
- A malicious message cannot impersonate an author, read another DM, grant tools or turn quoted mentions into execution.
- Community transition respects the configured preemption behavior and exports no private context.
- A long-history fixture stays paginated; first paint does not load every transcript or start every agent runtime.
- macOS, Windows and Linux have equivalent creation, room, DM, approval, stop, search and recovery flows.

This is a design handoff, not a new coding queue authorization or a claim of shipped functionality. Claude should review the storage/identity/dispatch contracts alongside ADR-034 before implementation. No application code or database was changed while drafting it.

Sif your friendly Codex Agent
