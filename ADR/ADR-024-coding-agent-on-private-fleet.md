# ADR-024: A real coding agent, scoped to a member's own Private Fleet first

Status: Proposed · Date: 2026-09-13 · Deciders: Jack Blair (owner), Loki (architect) · Source: this conversation, following directly from ADR-022 (Personal Hive) and ADR-023 (MCP tool surface)

## Context

Earlier today, discussing whether Hive's fleet could "build these features" (Hermes/Buzz-inspired work) itself, the honest answer was no: a card can generate text/image/audio/video, run a sandboxed WASI component, or (as of ADR-023) call one tool on a member-configured MCP server — but none of that is "read this repo, edit files, run the build, iterate." Jack's response, verbatim:

> "I mean the entire process, I want Hive to be able to use hermes, anthropic, and openai to be able to do programming work in local private fleets and be able to contribute to the hive as well. The hive integration can come after working in the private fleet if we need to since there are possible trust concerns, but no trust issues exist on a hive members own private fleet."

And, mid-turn, correcting an assumption that the model doing the reasoning would always be a BYOK cloud provider:

> "and I should have also included local agents as well as the clouds. The goal is to be able to do both."

So: a real coding agent — read/write files, run real shell commands, iterate across many turns until the task is done — driven by either a cloud BYOK model (Anthropic/OpenAI/Nous=Hermes) or a local model already running on the member's own hardware (Ollama via the existing `llama_cpp` backend; Hermes models specifically are trained for strong tool-use, which is why Jack named it alongside the two cloud providers rather than only as "the free option"). Scoped, in v1, to a member's own Private Fleet only — the multi-tenant "any Hive node can pick this up" case raises real trust questions (arbitrary code execution on a stranger's machine, or a stranger's code running as a card whose output feeds back into someone else's project) that don't exist when the node and the project are owned by the same person. That case is explicitly deferred, not solved, here.

A useful fact discovered while scoping this: `hive.modality` already has a `'code'` enum value (`text, code, image, video, speech, music`) — added at some earlier point in this project's history but never wired to anything. This ADR is what finally builds it.

## Decision

### 1. Hard trust gate: private-fleet-only, no exceptions, in this phase

Unlike ADR-023's MCP gate (which allows a `'hive'`-mode project too, as long as the claiming node is still owned by that project's own member), a `code`-modality card in this phase **only** claims under `execution_mode = 'local'` **and** `node.member_id = project.owner_id`. There is no `'hive'` branch for this modality at all — not "gated more strictly," genuinely absent. Contributing a coding card to the wider Hive marketplace is a real, separate decision (review/provenance/sandboxing questions Jack explicitly flagged) for a future ADR once the private-fleet version has been lived with.

### 2. The agent loop is real: unsandboxed shell + file access, on the member's own machine

Same trust framing as ADR-023's MCP client: this is the member's own subprocess, on their own hardware, as their own OS user. No curated, watered-down tool set — `read_file`, `write_file`, `list_dir`, and `run_command` (a real shell command, real stdout/stderr/exit code), because a coding agent that can't run the actual build/test/lint commands for a project isn't a coding agent. Hive's job is deciding *whether* a session runs at all (the gate above) and *what workspace* it can touch (decision 4) — never sandboxing what happens once it's running, which would defeat the point of a member choosing to trust their own machine with this.

### 3. Two interchangeable "brains" — local and cloud, member's choice per session

Both drive the same tool-calling loop against the same tool set; only how a "what should I do next" turn gets answered differs:

- **Local**: the node's own `LlamaCppBackend` (Ollama), extended to speak an OpenAI-compatible tool-calling chat completion (`/v1/chat/completions` with a `tools` array — Ollama already supports this for models trained for it, Hermes chief among them). No network call leaves the node; no BYOK key involved. Fully free, fully private, exactly the "our local models and their powers" Jack asked for two turns ago.
- **Cloud (BYOK)**: same shape as `interview`'s existing per-provider routing (Anthropic/OpenAI/Nous), extended to accept a running tool-calling conversation and a `tools` schema, and to return the next assistant turn (text and/or tool calls) instead of a single plain reply. The member's key never leaves the server (same as every existing BYOK path in this codebase) — the node sends the conversation-so-far and any new tool results, gets back the model's next move, executes any tool calls locally, and sends the results back on the next turn. One Edge Function round-trip per turn, looped by the node instead of by a human clicking send.

A session picks one brain at start (member's choice, surfaced wherever the session is kicked off) and keeps using it for the whole run — no switching mid-session in v1.

### 4. Workspace: an existing path, or a fresh git clone

A `code` card's `required_capabilities` carries either `workspace_path` (a directory already on the node — the common case: a repo the member already has checked out) or `repo_url` (+ optional `repo_ref`) for the node to clone fresh into scratch space before starting. Both are supported from the start since they're roughly equal implementation cost and cover meaningfully different real uses (iterate on a live checkout vs. hand it a URL and a task). No credential handling for private repos in v1 — public URLs or an already-authenticated local checkout only; a member's git credentials are exactly the kind of secret this phase doesn't need to touch yet.

### 5. One card, one continuous session — no new claim/lease machinery needed

A coding session runs entirely inside one claimed card's lease, the same way `exec_wasm` already does — the whole multi-turn loop (however many tool calls it takes) happens locally on the node between claim and `node_complete_card`, never re-surfacing as separate hub-visible steps. The only schema change `node_claim_card` needs beyond decision 1's gate is a longer lease TTL for this modality (a coding session can run far longer than a 15-minute default) — a generous fixed budget in v1 (proposed: 4 hours), not a renewal RPC; if that's not enough in practice, a renewal mechanism is a cheap follow-on once real sessions show the need.

### 6. Progress lands in the Private Fleet channel

Each meaningful step (session started, each tool call, session finished with a summary) posts to `hive.personal_channel_posts` via the same `personal_channel_post_core` helper every other node-authored event already uses (ADR-022 S2) — a coding session is exactly the kind of receipt that channel exists for. This also gives Jack a live way to watch a session work without needing a dedicated UI surface in the very first cut.

### 7. Web first, one small surface to start a session and watch it

Matching this project's established sequencing (ADR-022's "web app first, Swift catches up"): a small new web surface to pick a paired local-fleet node, a workspace (path or clone URL), a task description, and a brain (local model on that node, or one of the member's BYOK keys), then create the `code` card and let the existing project/card machinery + Private Fleet channel carry the rest. No new bespoke "sessions" UI beyond that in v1 — the project page and the channel already show everything that matters.

## Consequences

**Positive**: closes the actual gap identified this session — Hive can now do real programming work, not just generation, entirely within a trust boundary Jack has already accepted (his own machines). Reuses almost every piece already built today (ADR-022's channel, ADR-023's ownership-gate pattern, the existing BYOK provider routing, the existing card/lease/modality machinery) rather than inventing a parallel system. Local-model tool-calling is a real, free, private "our own AI doing our own work" capability the moment Ollama/Hermes is running on a paired machine.

**Negative / risks**: `run_command` is real, unsandboxed shell execution — the most powerful (and highest blast-radius, on the member's *own* machine) tool this codebase has ever shipped. A buggy or badly-prompted session can genuinely damage the workspace it's pointed at (or anything else that OS user can reach) — this is accepted, explicitly, per Jack's framing that this is his call to make on his own hardware, not a risk Hive should paternalistically block. A long-running session occupies a node's one lease slot for hours, so a member running one session effectively takes that machine out of the rest of the fleet's rotation for that window — worth surfacing clearly in whatever UI starts a session, not a blocker to building it.

**Deferred, explicitly, not solved here**: the `'hive'` (multi-tenant) execution mode for `code` cards; git credentials/private repos; mid-session brain switching; a lease-renewal mechanism (fixed generous TTL for now); curated/restricted tool subsets (full power only, in v1); a dedicated "sessions" UI beyond the existing project page + channel.

## Related

ADR-015 (local execution mode, the ownership rule this reuses), ADR-006 (sandbox/tool trust levels, `tools_level=sandboxed_tools` reused here), ADR-022 (Personal Hive fleet control plane, the channel this posts to), ADR-023 (MCP tool surface, the immediate precedent for "member's own unsandboxed subprocess, Hive only gates whether it runs").
