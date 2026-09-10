# ADR-006: Agent Runtime, Sandbox and the Interviewer Contract

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q11–Q13; D37–D48

## Context

A Hive project starts as a conversation. A member types a prompt in the web app, an **interviewer agent** interviews them (metered in $honey, D38), and the result is a structured plan materialized as a project with a kanban of typed cards (D37, D40). That planner must write `hive.projects` and `hive.cards` with authority, so it runs hub-side (D39) even though its inference may execute on a Hive node or a provider API per the local-first rule (D24). The plan's JSON contract is the first type shared by the web app, the hub and the node core.

Once a card exists, execution is deliberately pushed to the edge. One card is claimed by one node, which runs the whole agent loop locally: plan, generate, call tools, self-review, finish (D41). The hub only plans, schedules and stores (D43). This keeps the hub cheap and puts the tokens where they are generated and earned. Because nodes check out at will (D4), the loop must checkpoint at step boundaries so another capable node can resume from the last checkpoint (D42), and it may spawn child jobs through the scheduler for capabilities it lacks (D44, ADR-005).

Contributors are running other members' agent tasks on their own machines. That makes tool access a trust problem, not a feature. Jack's answer (Q13) is: inference plus **sandboxed tools by default**, with two per-node, registration-time toggles the contributor controls: `allow_internet` (whole node, off by default, D46) and `tools_level` (`inference_only` | `sandboxed_tools`, D48). Projects and cards declare `requires_internet` (D47), and the scheduler only ever matches declared need to granted capability. A card never gets network it did not declare, even on a node that opted in.

## Decision

1. **Agent loop lives in the Rust node core (D41, D43).** The `hive` core crate ships an `agent` module: a deterministic step machine `Plan → Act(tool|generate) → Observe → Review → Done`, driven by the node's `Backend` trait (ADR-003). The hub has no agent loop for cards; hub-side code is limited to the interviewer/planner (decision 8) and scheduling.
2. **Checkpoint at every step boundary (D42).** After each step the core serialises `CheckpointState { card_id, lease_id, step_index, messages[], tool_state, pending_child_ids[], artifact_refs[] }` (MessagePack, versioned), stores the payload on a regional server (ADR-007) and posts only the pointer to the coordinator, which records `hive.checkpoints(id, card_id, lease_id, step_index, blob_hash, created_at)`. Resume = fetch latest checkpoint by `card_id`, rehydrate, continue at `step_index + 1`. For ComfyUI-backed modalities the checkpoint is the workflow graph plus completed node outputs (Q17).
3. **Tools run in a WASM sandbox (D45).** All agent tools (file I/O, code execution, text transforms, web fetch) are compiled to WASI components and executed under **wasmtime** embedded in the node core, on every OS. Each card gets an isolated scratch directory preopened as the only writable path, a CPU-fuel budget and a memory cap. No host process spawning from inside the sandbox. Containers are a Linux/server-only fallback, not the primary mechanism.
4. **Network shim gated by `allow_internet` (D46, D47).** The WASI socket/HTTP host functions are provided by a single `NetShim`. It is a no-op that returns `EACCES` unless the node's `allow_internet = true` **and** the running card's `requires_internet = true`. Both must hold; the node flag alone never opens the network. The shim logs every outbound host per lease for audit.
5. **`tools_level` is enforced twice (D48).** At schedule time the coordinator refuses to place a tools-requiring card on an `inference_only` node (ADR-005). At runtime the core refuses to instantiate any tool component when `tools_level = inference_only`, so a mis-scheduled card fails closed instead of running tools.
6. **Node registration sets both flags, whole-node (D46, D48).** Registration in the node app (ADR-010) asks two explicit questions with defaults `allow_internet = false`, `tools_level = sandboxed_tools`, writes them to `hive.nodes`, and they remain editable in Preferences. Changing a flag takes effect for new leases only; live leases finish under the policy they started with.
7. **Projects and cards declare `requires_internet` (D47).** `hive.projects.requires_internet` is the project default; each `hive.cards.requires_internet` may be true only if the project's flag is true. Child jobs (D44) inherit the parent card's value and cannot widen it.
8. **Interviewer/planner is hub-side and metered (D37–D39).** Interview turns are Supabase Edge Function requests that enqueue an `interview` job; the coordinator executes it local-first on a Hive node or via a provider adapter (D24), and the Edge Function writes the resulting plan to `hive.projects`/`hive.cards` using the service role. Every turn is charged from the member's wallet at the standard token rate (D38, ADR-002). The interviewer **must** ask whether the project needs the internet and set `requires_internet`; it must also capture ownership/licence (ADR-011). A plan that omits either field is rejected by the schema validator.
9. **Structured-output contract (D40).** The interviewer's final turn must validate against the following JSON Schema (versioned `hive.plan.v1`), which is published as a shared package consumed by web app, Edge Functions and the Rust core (`serde` types generated from it):

```json
{
  "$id": "hive.plan.v1",
  "type": "object",
  "required": ["project", "cards"],
  "properties": {
    "project": {
      "type": "object",
      "required": ["title", "goal", "requires_internet", "license"],
      "properties": {
        "title": {"type": "string", "maxLength": 120},
        "goal": {"type": "string"},
        "requires_internet": {"type": "boolean"},
        "license": {
          "type": "object",
          "required": ["kind"],
          "properties": {
            "kind": {"enum": ["owner_only", "open_source"]},
            "spdx": {"type": "string"}
          }
        }
      }
    },
    "cards": {
      "type": "array", "minItems": 1,
      "items": {
        "type": "object",
        "required": ["key", "title", "type", "inputs", "deps", "acceptance", "requires_internet"],
        "properties": {
          "key": {"type": "string", "pattern": "^[a-z0-9-]+$"},
          "title": {"type": "string"},
          "type": {"enum": ["text", "code", "image", "video", "audio"]},
          "inputs": {"type": "object"},
          "deps": {"type": "array", "items": {"type": "string"}},
          "acceptance": {"type": "array", "items": {"type": "string"}, "minItems": 1},
          "requires_internet": {"type": "boolean"},
          "tools_level": {"enum": ["inference_only", "sandboxed_tools"]},
          "required_capabilities": {"type": "object"}
        }
      }
    }
  }
}
```

   `deps` reference other cards' `key` values and must form a DAG (validator rejects cycles). `tools_level` defaults to `sandboxed_tools`; `required_capabilities` defaults are derived from `type` by the hub if omitted.

Node policy record (subset of `hive.nodes`):

```sql
alter table hive.nodes
  add column allow_internet boolean not null default false,        -- D46
  add column tools_level text not null default 'sandboxed_tools'
    check (tools_level in ('inference_only','sandboxed_tools')),  -- D48
  add column policy_updated_at timestamptz not null default now();
```

## Consequences

### Positive
- Tokens are generated where they are earned; the hub carries no inference cost for card execution and stays cheap at 2,000 nodes.
- Step-boundary checkpoints make node churn a resume, not a restart, and give the scheduler a natural place to meter per lease.
- One WASM sandbox on all three OSes avoids three platform-specific isolation stacks and makes the network shim a single audited choke point.
- Double enforcement (schedule-time and runtime) means a contributor's opt-out survives scheduler bugs.
- A versioned plan schema is a stable contract between three codebases and makes the interviewer replaceable (any model that emits valid `hive.plan.v1`).

### Negative
- WASM tools are slower and more constrained than native; heavy code-execution tasks (compiling, running test suites) will be awkward or impossible in v1.
- Whole-node `allow_internet` is coarse: a contributor cannot allow fetch for one trusted project only.
- Checkpointing every step costs upload bandwidth and regional-server storage; large media intermediate outputs make checkpoints big.
- The interviewer running through the coordinator adds latency to a chat UI compared with a direct provider call.

### Risks & mitigations
- **Sandbox escape.** Mitigation: wasmtime with fuel and memory limits, no host FS beyond the scratch preopen, no `spawn`, and tool components signed by the hub; a compromised tool cannot reach the node's Backend or credentials.
- **Data exfiltration via an internet-enabled card.** Mitigation: `requires_internet` is visible on the card badge (Q13) and in the plan the owner approved; the shim logs outbound hosts per lease; consider an allow-list of hosts per card as a follow-up.
- **Checkpoint incompatibility across node versions.** Mitigation: `CheckpointState` is versioned; the scheduler only resumes a card on a node whose core version ≥ the checkpoint's `min_core_version`.
- **Interviewer forgets `requires_internet`, cards starve.** Mitigation: schema requires the field; the web app shows eligible-node counts so starvation is visible immediately (D63).
- **Prompt injection from project inputs steering the loop.** Mitigation: tool calls are constrained by the card's declared `tools_level`/`requires_internet`; nothing the model says can widen policy.

## Open questions
- Which tools ship in the v1 WASI tool set? Default assumption: `fs` (scratch only), `exec_wasm` (run member-supplied WASM), `http_fetch` (shim-gated), `artifact_get/put`, `spawn_child_card`.
- Does the interviewer run on Hive nodes in v1 or provider-only for latency? Default assumption: local-first per D24, with provider overflow enabled for `interview` jobs because they are interactive.
- Is a per-project internet allow-list (host list) wanted in v1.1? Open; whole-node flag is the v1 decision.
- How often are checkpoints taken for long single-step media jobs (a 40-minute video render has no step boundary)? Default assumption: adapter emits synthetic progress checkpoints (ComfyUI per-node completion).
- What is the review step's acceptance mechanism: model self-check only, or owner approval gate? Default assumption: self-check against `acceptance[]`, then `ready_for_review` for owner/admin.

## Related
- ADR-001-hub-and-source-of-record
- ADR-002-honey-economics
- ADR-003-node-core-and-backends
- ADR-005-scheduler-and-leases
- ADR-007-artifact-storage
- ADR-009-web-app
- ADR-010-node-desktop-app
- ADR-011-ownership-and-licensing
