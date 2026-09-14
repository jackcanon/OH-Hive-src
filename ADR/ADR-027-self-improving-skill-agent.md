# ADR-027: A Self-Improving Agent That Writes Its Own Skills

**Status:** Proposed · **Date:** 2026-09-14 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** Jack: "I do want us to follow Hermes Agent's lead with having a self improving agent where it creates skills from repetitive activities."

## Context

Grounded this in what Hermes Agent (Nous Research, self-hosted, released Feb 2026, since become one of the most-used open agent frameworks) actually does before designing anything, rather than guessing from the name alone: it accumulates memory across sessions, and **when it completes a task it automatically writes a reusable skill file** — a `SKILL.md` with a name, description, and step-by-step procedure. Next time a similar task comes up, it finds and reuses that file instead of re-deriving the approach from scratch. Skills also self-improve during use (the agent edits its own skill file when a run surfaces a wrinkle the original procedure missed), and everything is recalled across sessions via full-text search.

That `SKILL.md` convention isn't a Hermes invention to imitate from a distance — it's the exact same shape as the skills this Cowork session itself loads (a name, a one-line description shown up front, a full procedure loaded on demand). Hive doesn't need to invent a new format; it needs to give its own agents the same write-a-skill-when-you're-done habit, using a format that already has a working implementation to copy.

This lands directly on top of ADR-024's coding agent: `read_file`/`write_file`/`list_dir`/`run_command`, one continuous session per card, private-fleet-only trust gate (`execution_mode='local'` and `node.member_id = project.owner_id`, no `'hive'` branch). A self-improving skill system doesn't need new tools or a new trust gate — it needs a convention for *when* the agent writes to a new kind of file, and a way to make existing skills visible to the next session before it starts working.

## Decision

### 1. Format: reuse `SKILL.md` as-is, not a new schema

Name, one-line description, step-by-step procedure, plain markdown. A future session's system prompt lists every skill's name + description up front (cheap, small); the full procedure loads only when the agent actually decides to use one (mirrors this session's own `Skill` tool: description always visible, body loaded on demand so unused skills cost nothing).

### 2. Storage: per-machine, inside the member's own workspace, local-first

Skills live in a `.hive/skills/` directory the coding-agent session already has write access to (ADR-024 decision 4's workspace), not synced through the Supabase hub. This matches ADR-025's fully-local-hub direction and keeps a self-editing-its-own-instructions agent inside the same "member's own machine, member's own trust call" boundary ADR-024 already drew — a skill silently propagating to *other* members' machines is a materially different (and harder) trust question than a member's own agent getting faster at that member's own repeated work, and this ADR doesn't attempt to solve that one. Cross-machine sync of a member's own skill library (their other paired Macs) is a real, likely-wanted follow-on — explicitly deferred, same shape of decision as ADR-026's still-open per-machine-vs-synced question for connectors.

### 3. Trigger for creation: automatic, like Hermes — decided

**Decided (2026-09-14, Jack): fully automatic**, matching Hermes's actual behavior exactly — no approval step. After a card finishes, the agent itself judges whether the work was repetitive/generalizable enough to warrant a skill, and writes it without asking. This was a real, named tradeoff, not a default: the alternative (agent proposes, member approves before saving) was on the table specifically because an automatically-self-editing agent is a new risk surface — a subtly wrong procedure can get saved and then *trusted more*, not less, on reuse. Jack's call was to accept that risk in exchange for the actual "gets faster over time without babysitting it" behavior that's the point of following Hermes's lead. Decision 5's Settings visibility list is therefore not a nice-to-have but the load-bearing mitigation for this choice — it needs to actually ship alongside this, not slip to a later pass, since it's the only place a member can catch a bad skill before it's reused again.

### 4. Self-improvement of existing skills needs no new tool

The agent already has `write_file`. The only new thing is permission/convention: when reusing a skill and hitting a case the written procedure didn't cover, the agent may edit that skill's `.md` file directly, the same way Hermes's skills "self-improve during use." Same automatic-vs-approved question from Decision 3 applies here too — whatever Jack picks there should govern both creation and editing.

### 5. Visibility: a Settings surface, not silent background state

Given skills accumulate indefinitely and shape what the agent does without being asked each time, a member should be able to see what's in their own library — likely a simple list (name, description, last-used) in the same Node/Server-tab family already in Settings, with delete. Not building the UI in this ADR, just establishing that "the agent has learned things you can't see" is not the intended end state.

## Consequences

**Positive:** directly reuses ADR-024's existing tool set and trust gate — no new architecture, just a convention layered on top. Repeated coding-agent work (the same kind of fix, the same project's build/test dance) gets measurably faster over sessions instead of re-deriving the same steps every time. The skill format has a working reference implementation (this very session) to validate against.

**Negative / risks:** an automatically-self-editing agent is a new kind of risk surface even scoped to private-fleet-only — a skill that encodes a subtly wrong assumption gets *more* trusted with reuse, not less, unless a human occasionally looks at what's accumulating (Decision 5 exists because of this). Skill files can go stale as a project's actual codebase changes out from under them.

**Deferred, explicitly, not solved here:** cross-machine skill sync; any `'hive'`-mode (multi-tenant) version of this, which inherits every open question ADR-024 already deferred for that mode plus a new one (a skill written on one member's machine should almost certainly never silently execute on another's); semantic/fuzzy skill matching beyond whatever the agent's own judgment does when picking a skill off a short list.

## Related

ADR-024 (coding agent core engine — the tool set and trust gate this rides on), ADR-025 (fully local hub — the local-first storage precedent), ADR-023 (MCP tool surface — the earlier "member's own subprocess, Hive only gates whether it runs" precedent).
