# ADR-032: Durable Cloud Project Coordinator and Agent Registry

**Status:** Proposed (Phase 1 built, self-verified only) · **Date:** 2026-09-15 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** Sif's Hermes/Buzz parity audit (`docs/SIF-HERMES-BUZZ-FEATURE-PARITY-AUDIT-2026-09-15.md`), recommended-delivery-order item 3; scoped in conversation with Jack the same night, same three-question pattern ADR-030 used.

## Context

The audit's item 3 named a real gap and bundled two things under one row:

> Durable cloud project coordinator and agent registry — coordinator dispatches to two owned machines, inspects artifacts, requests one correction, integrates results, survives UI closure and reports completion against acceptance criteria.

Read without code, "cloud project coordinator" suggests a new, centrally-hosted, always-on service — genuinely new infrastructure. Reading the schema instead told a different story: `hive.cards` already has `parent_card_id`, a node-key-gated `spawn_child_card` RPC (`20260907180447_hive_agent_tool_rpcs.sql`), a `waiting_on_child` status, and a trigger that resumes the parent once every child reaches `review`/`done` or cascades a `blocked` failure up immediately (`20260907204830_hive_sub_delegation_pause_resume.sql`). That's the entire "durable" half of durable — survives a lease release, survives the submitting node going offline, requires no new schema. What was actually missing was narrower: none of this was reachable from `coder.rs`'s multi-turn brain loop (ADR-024) — it only existed for the older, single-shot ADR-006 tool step machine (`worker.rs`'s pre-`Draft` pass), which is why the audit's own correction #1 called the "coordinator" name misleading without code to back it up.

Three questions were put to Jack before building anything, mirroring how ADR-030 got scoped:

1. **Where does the coordinator run?** Answer: **cheap path** — just another `code`-modality card, running through the existing `coder.rs` brain loop on one of Jack's own nodes, given new tools that operate on the parent/child machinery that already exists. Not a new always-on hosted service.
2. **What does "agent registry" mean?** Answer: **named agents with a shaped, job-specific toolset** — "so they only load tools needed for a specific job, coding, data curation, finances, etc." Not full separate credentials/memory per agent (that's a much bigger, separate question) — the concrete ask is *tool-set scoping by role*.
3. **How much autonomy, and what scope?** Answer: **fully automatic** (no per-spawn/per-correction approval, matching the ADR-027 skills-creation precedent) and **whole-fleet, cross-project** (not confined to one project) visibility.
4. **Sequencing:** build it next.

## Decision

### 1. Phase 1 (built tonight): a `coordinator` flag on `code` cards, two new brain tools

`CodeSessionSpec` (the `required_capabilities` shape a `code`-modality card already parses, `coder.rs`) gains one new field:

```json
{ "coordinator": true }
```

`false` by default — every card written before this field existed, and every card that doesn't ask for it, is completely unaffected: same four tools, same vault tools, nothing new advertised. `coordinator: true` adds two more tools to that session's `tool_specs()`, and *only* that session's:

- **`spawn_card`** — wraps `Hub::spawn_child_card` (already implemented, already RPC'd, already used by the older ADR-006 loop via `crate::tools::run_spawn_child_card`). Creates one child card in the same project; any of the member's own nodes may claim it independently.
- **`wait_for_child`** — wraps `Hub::wait_on_child`. Releases this node's lease on the *current* card and marks it `waiting_on_child` in the DB. Unlike every other tool in this module, calling it successfully **ends the session right there** — `coder::run_session`'s loop returns immediately (a `CodeSessionOutcome { waiting_on_child: Some(child_id), .. }`), no further brain turns, no further tool calls in the same batch. A later node pick-up resumes the parent automatically once every card it spawned reaches `review`/`done` (the DB trigger already does this — nothing new), or the parent gets marked `blocked` immediately if a child fails.

`worker.rs`'s `run_code_card` checks the new `waiting_on_child` field (threaded through `tools.rs`'s `ToolOutcome::data`) before its existing `lease_expired` check, and if set, emits the *already-existing* `WorkerEvent::Blocked { card, waiting_on }` (ADR-006 D44's event, reused as-is — this is the same real-world situation the old step machine already had a name for) and returns without completing/releasing/failing the card, since `wait_on_child` already did the DB-side state transition itself.

This is deliberately the whole of Phase 1: a coordinator card, running on one of Jack's own always-on nodes, that can spawn work for its other machines and pause itself durably while waiting — no new server process, no new schema, no new migration beyond one additional optional parameter (below).

### 2. CLI and RPC surface

`hive card submit` gains `--coordinator` (a plain flag, default off). Threaded through `HubClient::code_session_submit`'s existing `request_id`-style optional-parameter shape, into `docs/proposed-migrations/20260915_hive_card_submit_node_rpcs.sql`'s node-facing `hive_code_session_create_node` (new optional `p_coordinator boolean default false`, folded into the `required_capabilities` jsonb as `coordinator`). The **web-facing `hive_code_session_create`'s public signature is unchanged** — same discipline as the rest of that migration: it explicitly passes `false` into the shared core rather than exposing the new parameter, since there's no Kanban UI for this yet. Still just a draft, not applied to the live Cmd Work Supabase project.

### 3. What "agent registry" (decision 2) still needs — not built tonight

Tonight's `coordinator: bool` is the narrowest possible slice of "shaped toolset per job": binary, not named, not role-general. What Jack actually described — named agents ("the coder," "the curator," "the bookkeeper"), each with a scoped subset of the tool catalog appropriate to its job — needs a real design pass:

- **Storage.** Two candidates: (a) workspace/host-local files, mirroring `skills.rs`'s proven `SkillStore` pattern (`.hive/agents/<name>/PROFILE.md` or similar, YAML frontmatter + an allowed-tool list) — cheap, ships without a migration, but is per-machine, not fleet-wide; (b) a small Supabase table (`hive.agent_profiles` or similar) scoped to the owning member — fleet-wide by construction, but is schema, needs a real migration, and needs Jack's review before it's live, same as every other schema change this session.
- **What "shaped toolset" actually filters.** At minimum, which of `read_file`/`write_file`/`list_dir`/`run_command`/`vault_search`/`vault_read`/`spawn_card`/`wait_for_child` a session's `tool_specs()` includes. Possibly also which `.hive/skills/` entries `skills_prompt_block` surfaces (a "data curation" agent has no obvious use for a coding build recipe) — an extension of `skills_prompt_block`'s existing filtering, not a new mechanism.
- **How a card picks its profile.** Almost certainly a new optional field alongside `coordinator` on `CodeSessionSpec` (e.g. `agent_profile: Option<String>`), resolved against whichever storage Jack picks above.

This deserves its own sitting, the same reasoning applied to deferring workspace-isolation/recovery (roadmap item 4) earlier tonight — it's a real product-shape decision (file-based vs. schema-based has real trade-offs) layered on top of a feature that itself only just got wired in.

### 4. What "whole fleet, cross-project" (decision 3) still needs — not built tonight

`spawn_child_card`'s hard constraint is same-project: a spawned child always lands in `parent.project_id`. Multi-*node* dispatch already works today for claiming — `execution_mode = 'local'` cards are already claimable by any of the member's own nodes regardless of which one created the project (ADR-015/ADR-022) — so tonight's Phase 1 already satisfies the audit's literal acceptance demo ("dispatches to two owned machines"). What it does *not* yet do is let a coordinator spawn or manage work in a *different* project than its own. That needs a new authorization shape (a node-key call proving fleet ownership, not lease-holding, since `spawn_child_card`'s current check is "does this node hold the parent's lease" — meaningless across a project boundary) and is real new RPC surface, not a two-tool addition. Deferred, same reasoning as above.

## Verification

Self-verified only tonight (brace/paren/bracket balance across every touched file, no merge markers, exactly-once function definitions, careful manual trait-signature matching against `Hub`'s 13 methods) — **not compiler-verified**, no Rust toolchain in this sandbox. New/changed tests, none yet run:

- `tool_specs_advertises_coordinator_tools_only_when_flagged` — `spawn_card`/`wait_for_child` present only when `coordinator: true`.
- `execute_tool_spawn_card_reaches_the_hub` — the new tool arm actually calls `Hub::spawn_child_card` and returns its result.
- `run_session_wait_for_child_pauses_without_another_turn` — the control-flow addition this ADR is riskiest on: a fixture brain that spawns a child then asks to wait on it, asserting the session stops at exactly 2 turns with `waiting_on_child` set and neither `hit_turn_limit` nor `lease_expired`.

Verification command (unchanged): `cd "/Volumes/10TB JBOD/Agents/Claude/Projects/Apps/OH Cloud-src" && cargo test -p hive-core --features "local-hub,sandbox,llama-cpp,desktop-provider,skills" --lib && cargo check -p hive`.

## 2026-09-15 follow-on design

[ADR-035](ADR-035-bots-chat-and-agent-collaboration.md) proposes the named-agent registry and conversation/memory scopes needed for Jack's explicit bots-chat/DM request. It extends the earlier role-only scope without claiming those features implemented. [ADR-034](ADR-034-three-subscription-coordinators.md) keeps desktop subscription coordinators separate from this leased-card execution path.

## Related

ADR-006 (D44 sub-delegation, `spawn_child_card`/`WorkerEvent::Blocked`, the machinery this reuses rather than reinvents). ADR-022 (Personal Hive, the multi-node-ownership model this leans on for "any of your own nodes may claim it"). ADR-024 (the `coder.rs` brain loop this extends). ADR-025 (worker.rs/coder.rs ownership — this ADR's Rust changes are all mine). ADR-027 (skills — the "shaped toolset" filtering pattern this ADR's Phase 3 would extend). ADR-030 (external card submission — `hive card submit --coordinator` builds directly on its CLI and draft migration).
