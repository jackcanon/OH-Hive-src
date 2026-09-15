# ADR-035: Bots chat and agent collaboration

**Status:** Accepted — full 5-phase scope approved by Jack 2026-09-15 ("full send"). Nothing beyond today's C0 contracts work is built yet; this ADR authorizes the roadmap, not a claim of shipped functionality.
**Date:** 2026-09-15
**Author:** Sif your friendly Codex Agent (design), formalized by Claude (Loki).
**Deciders:** Jack, confirmed directly 2026-09-15 (Cowork session, Loki).

## User requirement

Jack asked Sif to update Hive's process around Private Fleet chat. Sif came back with a full redesign: a member opens Hive and can talk to their whole team, a project team, or one named agent — a durable teammate with a role, tools, memory scope and a place it runs (a local machine or one of the three ADR-034 subscription runtimes). Six computers need not mean six identities; a machine can host several roles.

## Decision

Adopt Sif's design in `docs/SIF-BOTS-CHAT-DESIGN-2026-09-15.md` in full, extending ADR-022 §6 (which deliberately excluded Bot Mode) and ADR-032 (whose full agent registry and cross-project work were deferred — this ADR is that deferral coming due). Key shape, detail in the design doc:

- **Information architecture:** a new "Agents" workspace (Inbox, My Team, Project rooms, Agent DMs), with Private Fleet kept as the machine/activity view reachable from it. Old assistant chats stay in an explicit history section until migrated, never silently relabeled.
- **Identity model:** `agent_id` (teammate) is separate from `node_id` (machine), `provider_account_id` (credential), `runtime_session_id` (provider conversation) and `conversation_id` (Hive history) — immutable IDs, display names are never routing authority.
- **Storage:** nine new tables in the selected LocalHub's conversation domain (`agent_profiles`, `conversations`, `conversation_members`, `messages`, `message_revisions`, `agent_deliveries`, `runtime_bindings`, `conversation_read_positions`, `handoffs`) — local-first, with hub-backed parity for project rooms under the same contract. Never routes fully local chat through the existing cloud-only `channel.rs`.
- **Collaboration without loops:** one coordinator per room, structured `handoff()` requests between agents (bounded budgets: one active turn per agent, two active specialist handoffs per run, two correction rounds, depth two by default), deduplicated by source request + target + workflow step, no reply chains that wake every participant on every lifecycle event.
- **Privacy:** a DM's context belongs to that DM — another agent never gains it just by sharing a room. Community check-in never exports private DM history, vault secrets or provider credentials. This governs *chat* content specifically; the separate community-arbitration privacy boundary (native automatic jobs, "fully isolated" — see Decision log 2026-09-15) governs what a donated *job* may see, and is the stricter of the two where they overlap.

## Delivery phases (from the design doc, unchanged)

C0 contracts (schemas, service interfaces, permission tests, LocalHub-first) → C1 useful single-agent DM (roster, local executor, offline queue, stop, task receipts) → C2 team/project rooms (coordinator selection, mentions, threads, Inbox, adapter-backed agents as each ADR-034 provider passes its gates) → C3 collaboration (handoffs, reviewer/correction, loop budgets, scoped retrieval) → C4 polish/parity (native builds, migration UX, accessibility, retention). "Full send" means all five phases are now active work, not that they ship simultaneously — C1 has no cloud dependency and is the first thing a member can actually use.

## Consequences

A large, multi-week product surface: new domain/storage layer, new Swift/Tauri UI, a migration path off today's local-only `ChatEngine`/`ChatSessionStore`, and real interaction with all three ADR-034 adapters as they land. Acceptance scenarios (a local-only user with all cloud routes disabled still gets a working DM; a DM to one agent never appears in another's context even in a shared project; no duplicate jobs on restart; zero model turns for an unaddressed message) are in the design doc §9 and are the actual bar for "done," not just code existing.

## Related records

ADR-022 (private fleet control plane), ADR-027/028 (skills, vault — memory-scope precedent), ADR-030/031/032 (submission, adapter, coordinator), ADR-034 (the three runtimes agents can run on). `docs/SIF-BOTS-CHAT-DESIGN-2026-09-15.md` is the full design; this ADR is the accepted summary, not a replacement.

Claude (Loki), formalizing Sif's design and Jack's 2026-09-15 direction.
