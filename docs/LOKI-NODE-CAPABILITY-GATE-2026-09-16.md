# Node capability gate — advertise what the *software* can execute (2026-09-16)

Design only. No source changes accompany this doc.

## 1. The bug, as observed in production today

A node running `hive 0.3.0` (tag `v0.3.0` = `8e22201`, 2026-09-06) claimed a
`modality = 'code'` card and ran it through the **old text loop** (Draft → Critique →
Revise) instead of the coder/tool-calling loop.

Evidence:

- No file was written anywhere in the workspace.
- The "deliverable" was the source pasted into the card output as a fenced markdown block.
- The card reported status `review` — i.e. **success** — and the node **earned honey**.
- Node log: `step=1 next=Critique tokens_out=1024`. `1024` is the pre-fix
  `max_tokens` cap; the current tree's cap is `MAX_TOKENS_ARTIFACT = 4096`
  (`crates/ohhive-core/src/worker.rs:232`). The log line is itself a version fingerprint.

Nothing failed. The hub cannot tell this card apart from a good one.

## 2. Root cause, confirmed in code

### 2.1 The claim gate never asks whether the node's *build* can execute the modality

`hive.node_claim_card`, as replaced by
`supabase/migrations/20260913100000_code_modality_gate.sql:27` (function body), filters a
candidate card on exactly these node-related predicates:

| Migration line | Predicate | What it actually describes |
| --- | --- | --- |
| `:41` | `c.modality::text = any(... n.capabilities->'modalities' ...)` | **node-self-reported configuration** |
| `:42` | internet: `n.allow_internet` | node trust flag |
| `:43` | `n.tools_level = 'sandboxed_tools'` | node trust flag |
| `:44-45` | `required_capabilities.model_id` ∈ `n.capabilities->'models'` | installed models |
| `:47-50` | `p.execution_mode` / `n.member_id = p.owner_id` | project ownership |
| `:53-65` | `mcp_server_id` ownership + enabled | member config |
| `:70` | ADR-024: `c.modality <> 'code' or (p.execution_mode='local' and n.tools_level='sandboxed_tools')` | project mode + trust flag |

Every one of those is hardware, installed models, member configuration, or a trust flag.
**None of them is the node's software version or its set of implemented executors.** The
ADR-024 clause at `:70` was written as a *trust* gate ("code cards stay inside a private
fleet") and it does that correctly — it was never a *capability* gate.

### 2.2 Where a `code` card is dispatched today (current tree)

`crates/ohhive-core/src/worker.rs`:

- `Worker::run_card` — declared at **`worker.rs:886`**.
- `worker.rs:905` / `:909` — `if card.modality == "speech"` (cfg'd two ways).
- **`worker.rs:914`** — `if card.modality == "code" { return self.run_code_card(card, project, lease_expires_at).await; }`
- `Worker::run_code_card` — **`worker.rs:639`** (`#[cfg(feature = "sandbox")]`, the real one) and
  **`worker.rs:813`** (`#[cfg(not(feature = "sandbox"))]`, which calls `fail_card` cleanly).
- Anything that reaches **`worker.rs:917`** and beyond enters the Draft/Critique/Revise
  state machine (`Phase` at `:139-141`, loop from `:952` onward).

So in the current tree the dispatch is a single `if` at line 914, placed *before* any
Draft-phase state is constructed. The build-without-`sandbox` case is already handled
fail-closed at `:813`. Both facts matter below.

### 2.3 What a 0.3.0 binary does with a `code` card — confirmed, not inferred

Checked directly against the tag:

- `git show v0.3.0:crates/ohhive-core/src/worker.rs | grep 'modality =='` → **zero matches.**
  v0.3.0's `run_card` contains **no modality dispatch of any kind.** Every card, whatever its
  modality, falls straight into Draft/Critique/Revise.
- `git show v0.3.0:crates/ohhive-core/src/coder.rs` → **does not exist.** No `CodeBrain`,
  no `CodeSessionSpec`, no `run_code_session`.
- `git show v0.3.0:crates/ohhive-core/Cargo.toml | grep sandbox` → **no `sandbox` feature.**
  The `#[cfg(not(feature = "sandbox"))]` fail-closed arm at `worker.rs:813` did not exist
  either, so there is not even a graceful failure path to reach.
- v0.3.0 `max_tokens_for` returns `1024` for non-Critique phases
  (`git show v0.3.0:...worker.rs`, line 140). Exactly matches the observed log.
- `git merge-base --is-ancestor 883d824 v0.3.0` → **not an ancestor.** The commit that
  introduced the `card.modality == "code"` dispatch (`883d824`, 2026-09-13) post-dates the
  0.3.0 build by a week.

**Conclusion: a 0.3.0 node executing a `code` card through the text loop is not a
misconfiguration or a race. It is the only thing that binary can do.** It has no branch
to take, no feature flag to miss, and no error to raise.

### 2.4 Why the node advertised `code` in the first place

`crates/ohhive-core/src/backend/llama_cpp.rs:155` returns
`modalities: vec![Modality::Text, Modality::Code]`. The same line exists at v0.3.0
(`llama_cpp.rs:111`) and traces back to `01317c4`, the original llama.cpp adapter commit —
long before code sessions existed.

`crates/hive/src/main.rs:233-292` (`async fn capabilities`) builds the advertisement by
*unioning the modalities every configured backend claims*: `main.rs:243` (llama_cpp),
`:262` (whisper), `:273` (comfyui). The desktop/FFI equivalents do the same
(`crates/ohhive-ffi/src/lib.rs:199-213`, `apps/desktop/src-tauri/src/lib.rs:203`).

So "I have a text LLM" has silently meant "I can do code cards" on **every node, at every
version, since the llama.cpp adapter landed.** `Modality::Code` here means "this model can
emit code tokens" — the backend author's meaning — while the claim gate reads it as "this
node can run a coding-agent session." Two different claims, one JSON key.

### 2.5 What a node advertises today — the exhaustive list

`crates/ohhive-core/src/capability.rs:83-96`, `struct Capabilities`:
`hardware` (`Hardware` at `:49-68`: cpu/ram/vram/disk/bandwidth), `modalities`, `models`
(`ModelRef` at `:70-78`: `id`, `modality`, `backend`), `allow_internet`, `tools_level`,
`storage_gb_offered`, `shard_capable`.

**There is no version field, no build field, no feature list, and no executor list.**
Confirmed on the storage side too: `grep -rn 'agent_version|node_version|software_version|build_version|app_version' supabase/migrations`
returns nothing, and the only columns ever added to `hive.nodes` are `avatar_choice`,
`schedule`, and `rtt_ms`. The hub has never known what software any node is running.

`hive.node_checkin` (`supabase/migrations/20260913040000_channel_wiring_checkin_checkout.sql:19-36`)
does `capabilities = p_capabilities` — a whole-blob overwrite. That is important: **any new
key the node puts in `Capabilities` is persisted today with no migration.** It also means a
one-off `UPDATE` stripping `code` from a stale node's row is worthless: the node's next
check-in restores it.

`HubClient::claim_card` (`crates/ohhive-core/src/hub.rs:1068-1074`) calls the
`hive_node_claim_card` RPC directly. There is **no Edge Function in the claim path**
(`supabase/functions/` holds only `bots-turn`, `code-brain-turn`, `export-project`,
`generate-image`, `interview`, `private-fleet-enroll`). Any server-side gate change is
therefore a change to the SQL function — i.e. a migration.

### 2.6 The bug class is wider than version skew — it is live in the current build

`run_card` special-cases exactly two modalities (`speech` at `:905`/`:909`, `code` at `:914`)
and then **falls through to the text loop for everything else.** `hive.modality` has six
values. A node whose ComfyUI backend advertises `image` (`backend/comfyui.rs:240`) and which
claims an `image` card on **0.4.1, today** will run Draft/Critique/Revise and hand back a
prose description of a picture, reported as `review`. `crates/ohhive-core/src/ffi/media.rs:5`
says so in as many words: "extending the worker to dispatch across modalities is separate,
bigger work."

The 0.3.0 incident is the version-skew instance of a general defect: **the text loop is the
default branch, and the default branch always "succeeds."** Any fix that only names `code`
leaves the same trap set for `image`, `video` and `music`.

### 2.7 Adjacent finding, same function (fix in the same replace)

Migration `:44-45` requires `required_capabilities->>'model_id'` to appear in
`n.capabilities->'models'`. But `CodeSessionSpec` (`crates/ohhive-core/src/coder.rs:157`)
*also* reads `model_id`, and for a cloud brain (`"brain": "anthropic" | "openai" | "nous"`,
dispatched at `worker.rs:~700`) that value names a **provider-side** model the node will
never have installed. A code card that names its cloud model is therefore **unclaimable by
every node** — silently `nothing_to_do` forever. Worth correcting while the function is open:
scope the `model_id` clause to `c.modality <> 'code'`, or have code cards carry the brain's
model under a distinct key.

---

## 3. Options

### (a) Node advertises its version + executor list; the claim RPC filters on it

Add to `Capabilities` (`capability.rs:83`):

```rust
/// This build's crate version, `env!("CARGO_PKG_VERSION")`. Absent on any node
/// built before this field existed — absence is the signal, see the gate.
pub agent_version: Option<String>,
/// The modalities this BUILD has an executor for — not what the models can emit.
/// Populated from the cfg'd dispatch arms in `Worker::run_card`, never from a backend.
pub executes: Vec<Modality>,
```

`hive.node_checkin` stores both with **no migration** (whole-blob overwrite, §2.5).
Then one `create or replace function hive.node_claim_card` adding a single predicate:

```sql
and coalesce(n.capabilities->'executes', '[]'::jsonb) ? c.modality::text
```

**The decisive property: absence is refusal.** A 0.3.0 node sends no `executes` key, so
`coalesce(...,'[]') ? 'code'` is false and it matches **nothing that needs an executor** —
without that node being upgraded, restarted, or even reachable. This is the only option in
the list whose correctness does not depend on the old binary doing something.

Note the predicate is written for *all* modalities, not just `code`, which also closes §2.6.
It must therefore ship together with the node-side change that populates `executes`
honestly, or every node stops claiming everything. Sequencing is in §5.

- **Pro:** correct for mixed-version fleets by construction; fail-closed by default;
  generalizes to every future modality for free; makes staleness *visible* (see §5.1).
- **Con:** the gate half is a migration, and migrations are blocked (§4).
- **Con:** `capabilities` is self-reported. `executes` is an honesty gate, not a security
  boundary — a node that lies still claims. Acceptable here: ADR-024 already confines `code`
  cards to `execution_mode='local'` with `n.member_id = p.owner_id`, so the liar can only lie
  to its own owner. This is a correctness bug, not an exploit.

### (b) Node-side refusal: the worker checks before claiming, releases if it can't

**This cannot fix the reported bug, and the reason is worth stating precisely.** The fix
would live in `run_card`, in a binary that was compiled on 2026-09-06. The deployed 0.3.0
`run_card` has no modality dispatch at all (§2.3) — there is no place for the check to be
and no way to put one there retroactively. A node-side refusal protects exactly the nodes
that are already new enough not to need protecting. As *the* fix it is circular.

It is still worth doing, as the third layer: a node that claims a modality it has no arm for
should `release_card` — not `fail_card` (the card is fine; this node is wrong) and above all
not fall through to Draft. That converts the *next* skew (a 0.5.0 modality reaching a 0.4.1
node, or an `image` card today) from a fabricated deliverable into a released lease that
another node picks up. Concretely: replace the fallthrough at `worker.rs:917` with an
explicit match, default arm → `release_card("this node has no executor for modality X")`.

- **Pro:** no migration, ships in the next build, closes §2.6's live hole.
- **Con:** zero effect on any binary already in the field, which is the entire bug.

### (c) Card-side: `required_capabilities` gains a `min_version`

- **Con:** the gate must *read* `min_version`, so it costs the same migration as (a) while
  fixing strictly less.
- **Con:** puts a moving target on every card author. The correct floor for a `code` card is
  "whatever build introduced `run_code_card`", which no author knows; for a card setting
  `coordinator: true` (`coder.rs:~180`, ADR-032) it is a different, later floor.
- **Con:** a version floor is the wrong shape of claim. What a card needs is a *capability*
  ("can run a coding session"), and version is a lossy proxy for it — it breaks the moment a
  build is compiled without the `sandbox` feature, which is a supported configuration
  (`worker.rs:813`) and is *not* expressible as a version number.
- **Pro:** genuinely useful later as a narrow refinement on top of (a), for the
  per-card-feature case (`coordinator`, `vault_name`) that a coarse `executes` list misses.

### (d) Combination — **recommended**

Four layers, in dependency order. Each is useful alone; only layer 3 is blocked.

**Layer 0 — stop lying at the source (no migration, no schema, one line).**
`backend/llama_cpp.rs:155` must advertise `vec![Modality::Text]`. A text-model adapter has
no business claiming `Code`. `Modality::Code` should be contributed to `modalities` by the
*capabilities builder* (`crates/hive/src/main.rs:279`, and the FFI/desktop twins) under
`#[cfg(feature = "sandbox")]`, i.e. by the presence of the executor. This is the actual
node-side root defect (§2.4) and it costs nothing. It fixes every node from the next build
forward — **and no node already deployed.**

**Layer 1 — advertise `agent_version` + `executes` (no migration).** As in (a). Ships with
the next build. Standalone value even before layer 3 exists: see §5.1.

**Layer 2 — fail-closed dispatch in `run_card` (no migration).** As in (b): default arm →
`release_card`, never Draft. Closes the live `image`/`video`/`music` hole from §2.6.

**Layer 3 — the claim-gate predicate (one migration).** As in (a). This is the only layer
that reaches an already-deployed 0.3.0 binary, and it is the one that costs a migration.
That is the trade the whole design turns on.

---

## 4. The migration cost, honestly

Migrations are currently blocked on the baseline problem — see
`docs/SIF-MIGRATION-VERSION-RECONCILIATION-2026-09-16.md`: production migration-history
repair is an unapplied rollout step, roughly thirty local files have no name match in
production, and that doc explicitly says its name-based candidates "must not be used for
automatic repair."

Three things make layer 3 the least-bad migration to need right now:

1. **It is a function replace, not a schema change.** No new table, column, type, enum value,
   index, or RLS policy. `hive.node_claim_card` is already `create or replace`d by
   `20260913100000_code_modality_gate.sql`, which *does* have a confirmed production
   counterpart (`20260913200644_code_modality_gate`). The file to write is a copy of the live
   body (re-fetched via `pg_get_functiondef`, the discipline that migration's own header
   describes) plus one `and` line.
2. **The node-side storage half needs no migration at all** (§2.5), so layers 0–2 ship on
   their own schedule and layer 3 becomes a one-line change applied whenever the history is
   safe to touch.
3. **It is reversible by replacing the function back.** Nothing it does is destructive, and
   a bad deploy fails in the safe direction (nodes claim nothing rather than claim wrongly).

**Judgment call for Jack:** because it touches no table and no type, layer 3 could be applied
as a function-only change *ahead of* the history repair. That is a deliberate deviation from
"no migrations until baseline is fixed" and should be an explicit decision, not something
smuggled in under this doc. If the answer is no, the fleet runs on §5.1's mitigation until
the baseline lands, and that is a survivable state — but only because the fleet is small
enough to enumerate by hand today.

---

## 5. Interim mitigation vs. the durable fix

### 5.1 Available today: upgrade every node — and why it is not enough

Upgrading every node to ≥ the build containing `worker.rs:914` (0.4.x) *is* a real
mitigation: a 0.4.x node dispatches `code` cards to `run_code_card`, and a 0.4.x node built
without `sandbox` fails the card cleanly at `worker.rs:813` instead of drafting prose. Do it
now; it is the only thing that helps before layer 3 lands.

Its three weaknesses are the argument for the durable fix:

- **It is a campaign, not a state.** It must be re-run every time a node joins, is restored
  from an old install, or is rolled back.
- **It fails silently.** There is no alarm, no degraded mode, no failed card — a missed node
  just keeps earning honey for markdown.
- **It cannot be verified.** The hub has no version field (§2.5), so "is the fleet upgraded?"
  is unanswerable from the hub today. You are checking machines by hand.

That last point is why **layer 1 has standalone value before layer 3 exists.** Shipping
`agent_version` into `capabilities` needs no migration and immediately turns the upgrade
campaign from a blind sweep into a checklist: stale nodes are the ones whose
`capabilities->>'agent_version'` is null or below the floor, queryable from the hub, surfaced
on the node list. Ship layer 1 even if layers 0/2/3 slip.

### 5.2 The durable fix is layer 3

Only a hub-side predicate whose default is refusal can protect against a binary that will
never be upgraded — the node that is off, forgotten, air-gapped, or owned by someone who has
stopped reading release notes. `executes`-absence-means-no is that predicate.

---

## 6. What this does **not** fix

- **The card already in `review`, and the honey already paid.** The gate is not retroactive.
  That card needs manual reconciliation — reopen it, and decide separately whether the honey
  is clawed back. Worth a query for other `code` cards completed with a suspiciously small
  `tokens_out` and a markdown-block output before the fix lands.
- **A node that lies.** `capabilities` is self-reported (§3a). Mitigated, not solved, by
  ADR-024's `execution_mode='local'` + `member_id = owner_id` confinement.
- **A node new enough to dispatch but unable to do the work.** `executes` says "this build has
  the arm," not "this model can call tools competently." A 0.4.1 node pointed at a small local
  model will enter `run_code_session` and produce a poor session. Different problem, correctly
  reported (the session runs, `outcome.ok` is honest), out of scope here.
- **Truncation detection does not catch this.** ADR-036's `finish_reason == "length"` check
  (`worker.rs:1047-1055`) post-dates 0.3.0 entirely, and in any case a short code answer that
  fits under the cap is not truncated — it is simply the wrong artifact. Loud truncation and
  capability gating are orthogonal defences.
- **Card authoring.** Nothing here stops a member from creating a `code` card with a
  `workspace_path` that does not exist on the claiming node; that fails cleanly today at
  `coder.rs:195-199`.
- **The `model_id` collision in §2.7** is a separate correction that happens to live in the
  same function.

## 7. Recommendation

Ship **(d)**. Layers 0, 1 and 2 immediately, in the next build, with no migration. Layer 3 —
the `executes` predicate in `hive.node_claim_card` — as the durable fix, applied as a
function-only replace, with Jack's explicit sign-off on doing it ahead of the baseline repair.
Run the fleet upgrade (§5.1) today regardless, and treat it as a stopgap with a known
expiry, not as the fix.
