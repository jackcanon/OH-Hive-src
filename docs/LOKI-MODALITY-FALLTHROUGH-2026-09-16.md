# Modality fallthrough in `run_card` — design / fix doc

**Date:** 2026-09-16
**Author:** Loki (investigation agent)
**Repo state:** `e901ea9`, working tree dirty (uncommitted changes across `apps/`, `crates/`)
**Status:** investigation complete, **no source changed** — this doc is the only file written.

## TL;DR

The lead is **correct about the control flow** and **wrong about the payment**, on the
current build.

`crates/ohhive-core/src/worker.rs::run_card` special-cases exactly two modalities —
`speech` and `code`. `image`, `video` and `music` fall straight through into the generic
Draft → Critique → Revise text loop and would be "completed" with prose about the
artifact instead of the artifact. That part is real and quoted below.

But on the schema in this repo, such a card earns **zero honey**, and in `hive`-mode
projects it cannot even be claimed. The "wrote no file and was paid" observation is
consistent with a schema that predates `20260916050400_funded_compute_and_interviews.sql`
(the only migration that zeroes local-mode payouts). The live harm today is a **silently
wrong artifact** and, in `hive` mode, a **wedged claim loop** — not theft.

It is also **not reachable on Jack's fleet as built**: the macOS app never advertises
`image` at all. It is reachable on a `hive` CLI node with ComfyUI configured.

---

## 1. Verified: what `run_card` actually does

`crates/ohhive-core/src/worker.rs:886` — signature. The dispatch block that follows is
the whole modality story:

```rust
// worker.rs:904-916
#[cfg(feature = "whisper")]
if card.modality == "speech" {
    return self.run_speech_card(card, project, lease_expires_at).await;
}
#[cfg(not(feature = "whisper"))]
if card.modality == "speech" {
    self.hub.fail_card(card.id, "This worker was built without speech support").await?;
    self.emit(WorkerEvent::Failed { card: card.title, error: "Speech support is unavailable".into() });
    return Ok(());
}
if card.modality == "code" {
    return self.run_code_card(card, project, lease_expires_at).await;
}
```

There is no `match`, no default arm, no `else`. `card.modality` is never read again in
the function. Execution continues at `worker.rs:917`:

```rust
// worker.rs:917-924
let model = card
    .required_capabilities
    .get("model_id")
    .and_then(|v| v.as_str())
    .map(String::from)
    .or_else(|| self.default_model.clone())
    .or_else(|| self.caps.models.first().map(|m| m.id.clone()));
let single = single_step(&card);
```

…and from there into the bounded state machine (`while st.phase != Phase::Done`,
`worker.rs:1002`), which calls `self.infer(...)` (`worker.rs:1024`) against
`self.backend` — a single `&dyn Backend` field (`worker.rs:104`).

### Concretely: card `modality = 'image'`, node advertises `image`

1. `hive.node_claim_card` matches the card (§2) and leases it.
2. `Worker::tick` (`worker.rs:1155`) dispatches `run_card`.
3. Neither the `speech` nor the `code` branch matches.
4. The text loop runs: `Draft` → `Critique` → up to `MAX_REVISIONS` (2) `Revise` passes,
   every step an `infer` call on `self.backend`.
5. `self.backend` is **always `LlamaCppBackend`** — `crates/hive/src/main.rs:574` and
   `crates/ohhive-ffi/src/lib.rs:627` are the only two places a `Worker` is constructed
   in the tree, and both hardcode it. `ComfyUiBackend` is never handed to a `Worker`
   anywhere.
6. Terminal path, `worker.rs:1119-1124`:

```rust
let content = st.draft.clone().unwrap_or_default();
let done = self
    .hub
    .complete_card(card.id, &content, st.model.as_deref(), st.usage)
    .await?;
```

`content` is the LLM's prose. The card flips to `review` with that prose stored as its
output. No image is generated, no artifact hash is produced, nothing anywhere compares
the output to the card's modality.

**Verdict on the lead's claim 1: CONFIRMED, and slightly understated.** The problem is
not only that `run_card` lacks an `image` branch — the `Worker` struct has no way to
*hold* an image backend. `crates/ohhive-ffi/src/media.rs:1-6` says so in as many words:

> `worker_start` in `lib.rs` only ever runs one backend, `LlamaCppBackend`, against the
> network's job queue -- extending the worker to dispatch across modalities is separate,
> bigger work, tracked alongside the rest of M8's scheduler-side gaps.

So even a correct dispatch table would have nothing to dispatch *to* today.

---

## 2. Reachability: can an image/video/music card be claimed today?

### 2a. Which backends advertise which modality

| Backend | File:line | Modalities advertised | Cargo feature |
|---|---|---|---|
| `llama_cpp` | `backend/llama_cpp.rs:155` | `Text`, `Code` | `llama-cpp` |
| `whisper` | `backend/whisper.rs:84` | `Speech` | `whisper` |
| `comfyui` | `backend/comfyui.rs:240` | `Image` | `comfyui` |
| `mock` | `backend/mock.rs:38` | `Text` | always |

**Nothing in the tree advertises `Video` or `Music`.** `Modality::Video` / `Modality::Music`
exist in the enum (`capability.rs:12,14`) and are referenced only in tests
(`capability.rs:208`) and in the lease-TTL `case` in SQL. No backend, no code path,
produces them.

`comfyui` and `whisper` are both in the `hive` CLI's **default** feature set —
`crates/hive/Cargo.toml:29`:

```toml
default = ["llama-cpp", "whisper", "comfyui"]
```

and in `hive-ffi`'s required feature list (`crates/ohhive-ffi/Cargo.toml:18`).

### 2b. How advertised modalities are assembled — two different builders

**`hive` CLI (`crates/hive/src/main.rs:234-288`)** takes the *union* of every configured
backend's modalities:

```rust
// main.rs:268-278
#[cfg(feature = "comfyui")]
if let (Some(url), Some(checkpoint)) = (&cfg.comfyui_url, &cfg.comfyui_checkpoint) {
    let be = hive_core::backend::comfyui::ComfyUiBackend::new(url, checkpoint);
    match be.capabilities().await {
        Ok(c) => {
            modalities.extend(c.modalities);   // <-- Image lands in check_in's payload
            models.extend(c.models);
        }
        Err(e) => tracing::warn!("comfyui backend at {url} unavailable: {e}"),
    }
}
```

That same `Capabilities` value is what `hive work` sends to `check_in` and hands to the
`Worker` as `caps` (`main.rs:558,578`), while the `Worker`'s executor is
`LlamaCppBackend` (`main.rs:574`). **The advertisement and the executor are built from
different sources.** That divergence is the root cause.

So a node running `hive work` with both `HIVE_COMFYUI_URL` and `HIVE_COMFYUI_CHECKPOINT`
set (`nodeconfig.rs:100-101`) and a reachable ComfyUI advertises
`["text","code","image"]` — and can only execute two of the three.

**macOS desktop app (`crates/ohhive-ffi/src/lib.rs:196-222`)** probes **only** llama.cpp:

```rust
async fn capabilities(cfg: &NodeConfig) -> (Capabilities, bool) {
    let hardware = hive_core::probe::probe_hardware();
    let be = LlamaCppBackend::new(&cfg.llama_url);
    let (mut modalities, mut models, ok) = match be.capabilities().await { ... };
    if modalities.is_empty() { modalities.push(Modality::Text); }
```

No whisper probe, no comfyui probe — despite the crate being *compiled* with both
features. The desktop node can therefore only ever advertise `["text","code"]`, which is
**exactly what Jack's fleet reports.**

### 2c. The claim gate

`supabase/migrations/20260913100000_code_modality_gate.sql` is the newest definition of
`hive.node_claim_card` (checked: no later migration redefines it). Its modality predicate
is one line:

```sql
and c.modality::text = any (select jsonb_array_elements_text(coalesce(n.capabilities->'modalities', '[]'::jsonb)))
```

The only other modality-aware clause is the `code` trust gate:

```sql
-- ADR-024 decision 1
and (c.modality <> 'code' or (p.execution_mode = 'local' and n.tools_level = 'sandboxed_tools'))
```

There is **no** predicate restricting `image`/`video`/`music`. Advertisement alone is
sufficient to be handed the card. The identical predicate exists in the control-plane
delegated claim, `hive.ctl_d_node_claim_card`
(`supabase/migrations/20260914030000_control_pilot_delegation.sql:307,331`) — any fix
must touch both.

Note also that `node_claim_card` already knows about these modalities for lease TTLs:

```sql
ttl := case card.modality
         when 'video' then interval '90 minutes'
         when 'image' then interval '20 minutes'
         when 'music' then interval '30 minutes'
         when 'code'  then interval '4 hours'
         else interval '15 minutes' end;
```

i.e. the scheduler is provisioned for modalities the worker cannot run.

### 2d. Where do image cards come from?

Not from a human picking "image" in a UI — `apps/web` never offers a modality selector
(the only `modality` references are display-only, `apps/web/app/projects/[id]/page.tsx:285,296`).
Two real sources:

1. **Interview planner.** `hive.create_project_from_plan`
   (`supabase/migrations/20260905000006_interview.sql:49`) casts an LLM-authored plan
   straight into the enum:
   `(c->>'modality')::hive.modality` — any of the six values, no capability check.
2. **`spawn_child_card`** (`supabase/migrations/20260907180447_hive_agent_tool_rpcs.sql:55-77`,
   and the delegated twin at `20260914030000:359-380`) lets any *running* card create a
   child of any modality: `values (..., p_modality::hive.modality, ...)`. A text card can
   mint an image card mid-loop. No check that any node can run it.

### Verdict on reachability

| Scenario | Reachable? |
|---|---|
| Jack's macOS fleet (`["text","code"]`) | **No.** `ohhive-ffi` never advertises `image`/`speech`, so the gate never matches an image card. |
| `hive` CLI node, default features, ComfyUI configured + reachable | **Yes — live.** Advertises `image`, claims image cards, runs the text loop. |
| `hive` CLI node, whisper configured | Advertises `speech`, but `speech` **is** handled (`run_speech_card`). Not affected. |
| `video` / `music` cards | **Theoretical only.** No backend in the tree advertises them; only a hand-rolled client posting fabricated `capabilities` to `check_in` could. |

So: **live, but narrow.** It needs one CLI node with ComfyUI wired up. It is one
`hive set HIVE_COMFYUI_URL …` away from being live on Jack's fleet, and the desktop app
gaining a whisper/comfyui probe (an obvious, small, likely-next change) would make it live
without anyone touching `worker.rs`.

---

## 3. Blast radius: does a mis-executed card get paid?

Newest definition of `hive.node_complete_card` is
`supabase/migrations/20260916050400_funded_compute_and_interviews.sql`. The relevant
sequence:

```sql
insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
values (p_card_id, nid, p_content, p_model_id,
        jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));
...
amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);
if c.modality='speech' then ... end if;
if mode='local' then amt:=0; end if;
```

### 3a. No output validation, anywhere

- `node_complete_card`'s content parameter is `p_content **text**`. There is no artifact
  hash, no MIME type, no size, no modality argument.
- `hive.card_outputs` stores `content` and a `usage` jsonb. Nothing is compared against
  `c.modality`.
- The only content-shape guard in the whole path is client-side and text-specific:
  `worker.rs:1042-1069` fails a card whose Draft/Revise hit the token cap.

So an image card completes with prose and reaches `status = 'review'` indistinguishably
from a legitimate text card. A reviewing member sees a completed card. **This is the real
live harm.**

### 3b. Payment — `hive`-mode projects: the card cannot be claimed at all

`20260916050400` adds a `BEFORE INSERT` trigger on `hive.leases`:

```sql
create or replace function hive.reserve_compute_on_lease() returns trigger ... as $$
begin
 if exists(select from hive.cards c join hive.projects p on p.id=c.project_id
           where c.id=new.card_id and p.execution_mode='hive' and c.modality<>'speech') then
   q:=hive.validate_compute_budget(new.card_id);
   ...
create trigger reserve_compute_on_lease before insert on hive.leases for each row execute function hive.reserve_compute_on_lease();
```

and `hive.validate_compute_budget` refuses non-text/code outright:

```sql
if q.card_id is null then raise exception 'compute_budget_required'; end if;
...
if p.deleted_at is not null or p.execution_mode<>'hive'
   or c.modality not in ('text','code')
   or q.input_hash is distinct from md5(c.inputs||c.required_capabilities::text)
   then raise exception 'compute_input_changed'; end if;
```

A budget row cannot exist for an image card in the first place — the web UI only renders
the `ComputeBudget` control for `c.modality === "text"`
(`apps/web/app/projects/[id]/page.tsx:296`). So the lease insert inside
`node_claim_card` raises, the whole claim transaction aborts, and the card stays `ready`.

**A `hive`-mode image/video/music card is unclaimable today. It earns nothing because it
never runs.**

That is a *different*, un-filed bug, and arguably a worse one operationally:

- `node_claim_card` selects exactly one card (`order by c.priority desc, c.order_index,
  c.created_at limit 1`), then raises on the lease insert. It does not try the next card.
- `Worker::tick` surfaces that as an error; `run_forever` logs and backs off
  (`worker.rs:1247-1250`: `Err(e) => { tracing::warn!("tick failed: {e}"); break; }`).
- Next poll, the same card sorts first again. **A single `hive`-mode image card at the top
  of the queue permanently starves every node that advertises `image`** — they never claim
  anything again while it sits there. Silent: it only appears as a repeating
  `tick failed` warning in the node log.

### 3c. Payment — `local`-mode projects: zero honey

No trigger fires (the guard requires `execution_mode='hive'`). The claim succeeds, the
text loop runs, and `node_complete_card` reaches `if mode='local' then amt:=0; end if;`.
`budget.card_id` is null on this path, so nothing overwrites the zero. **`earned_honey`
is 0.** The card still flips to `review` with prose in it.

### 3d. So was the "earn honey" claim wrong?

On the schema in this repo, **yes** — for the reasons above. But it was true recently:
`20260916050400` (dated today) is the **only** migration in the tree containing
`mode='local' then amt:=0`. Before it, a local-mode completion paid
`tokens_out * compute_output_rate` like any other. That is entirely consistent with the
observed `code`-card incident on an old binary — `code` cards are local-only by the
ADR-024 gate, so under the older schema they were paid for producing nothing. The
payment hole is closed for the moment by a *pricing* change, not by any correctness fix.
Nothing stops it reopening: the class of bug (execute the wrong thing, report `review`,
bill for tokens) is untouched.

### 3e. Summary of blast radius

| Consequence | `hive` mode | `local` mode |
|---|---|---|
| Card claimed | **No** (lease trigger raises) | Yes |
| Wrong artifact reported as `review` | n/a | **Yes** |
| Honey paid | 0 (never runs) | **0** on current schema; `tokens_out × rate` on any schema before `20260916050400` |
| Tokens/compute burned | none | Yes — up to 1 Draft + 3 Critique + 2 Revise inferences |
| Claim loop starved | **Yes — node stops claiming anything** | no |
| Owner-visible signal | repeated `tick failed` in node log only | card looks successfully completed |

---

## 4. Recommended fix

Four changes, in priority order. Fix 1 and Fix 2 are the minimum; Fix 3 is the actual
root cause; Fix 4 is the belt-and-braces that also resolves §3b.

### Is the text loop ever a legitimate handler for a non-text modality?

**No.** The loop's product is `st.draft: Option<String>`, handed verbatim to
`complete_card` as the card's artifact (`worker.rs:1119-1124`). For a modality to be
legitimately served by it, the modality's artifact would have to *be* text. `image`,
`video` and `music` are by definition not. There is no configuration, prompt or
`required_capabilities` value that makes prose a valid image. The fallthrough has no
defensible case; it should be a compile-visible exhaustive match, not an implicit else.

### Fix 1 — make `run_card`'s dispatch total (`crates/ohhive-core/src/worker.rs:904`)

Replace the sequence of `if`s with an exhaustive match whose default arm refuses:

```rust
match card.modality.as_str() {
    // The Draft/Critique/Revise loop below is a text producer. Only 'text' may use it.
    "text" => {}
    #[cfg(feature = "whisper")]
    "speech" => return self.run_speech_card(card, project, lease_expires_at).await,
    #[cfg(not(feature = "whisper"))]
    "speech" => return self.refuse(card, "this worker was built without speech support").await,
    "code" => return self.run_code_card(card, project, lease_expires_at).await,
    other => {
        return self
            .refuse(card, &format!(
                "no executor for modality '{other}' on this node: the card worker \
                 implements text, code and speech only. This node should not have \
                 advertised '{other}'."
            ))
            .await
    }
}
```

`refuse` is a small helper wrapping `hub.fail_card` + `WorkerEvent::Failed` — the same
shape the `not(feature = "whisper")` arm already uses at `worker.rs:909-913`, so there is
no new hub surface.

Adding an `image` arm later means adding a *variant*, not un-breaking a default — which
is the property worth buying here.

### Fix 2 — refuse means **fail**, not **release** (the release-loop hazard)

This is the part the lead flagged and it needs deciding explicitly. Three candidates:

**(a) `release_card` — REJECTED.** The card returns to `ready`. This node still advertises
the modality, so `node_claim_card`'s predicate matches it again on the very next poll. The
card sorts to the same position, gets leased, gets refused, gets released. **Infinite
claim/release loop** at the node's poll interval (5s by default), burning claim RPCs,
lease inserts, and — in `hive` mode — `reserve_compute_on_lease` work per cycle, plus a
`card_claimed` post to the owner's personal channel *every single cycle*
(`node_claim_card` ends with `hive.personal_channel_post_core(... 'card_claimed' ...)`).
That is a notification-spam DoS on the project owner, on top of the spin. Release is only
correct when the *next* attempt can plausibly differ; here it provably cannot.

**(b) `fail_card` — RECOMMENDED.** The card terminates with a reason the owner sees.
Rationale:
- The failure is a property of the *build*, not of transient conditions. Retrying cannot
  change the outcome — exactly the case where release is wrong and fail is right.
- There is no better-equipped node to preserve the card for: **no** node anywhere in the
  tree can execute an image card in the worker (§1, step 5). Holding the card `ready` in
  the hope that a capable node appears is holding it forever.
- `fail_card` already requires an owned lease
  (`supabase/migrations/20260915150000_fail_card_requires_owned_lease.sql`), so this is the
  smallest-surface terminal path.
- The reason string is the diagnostic that makes the *advertisement* bug visible, which is
  what actually needs fixing.

**(c) Release with a per-node modality blocklist.** On refusal, the node drops the modality
from its advertised `Capabilities` and re-`check_in`s, then releases the card so a
(hypothetical) capable node can take it. This is strictly better than (a) — it breaks the
loop by construction rather than by luck. It is worth building **once a real image
executor exists** and heterogeneous fleets are a thing. Today it adds machinery to
preserve a card nobody can run, so it is deferred. Record the intent; ship (b).

### Fix 3 — stop advertising what the worker cannot execute (root cause)

`crates/hive/src/main.rs:234-288` unions probed-backend modalities into the payload sent to
`check_in`, while the `Worker` gets a single `LlamaCppBackend`. Two levels of fix:

**3a (immediate, small).** Advertise only what `run_card` can dispatch. Keep probing
ComfyUI so `hive run --backend comfyui` and the desktop Generate Image tab keep working —
those are direct, non-scheduled calls (`crates/ohhive-ffi/src/media.rs:1-15`) and are not
affected by any of this — but filter `Image` out of the `Capabilities` handed to
`check_in`. A single `WORKER_MODALITIES: &[Modality] = &[Text, Code, Speech]` constant in
`ohhive-core`, applied in both builders (`main.rs:279`, `ohhive-ffi/src/lib.rs:206`) and
asserted against `run_card`'s match arms by a unit test, is enough.

**3b (the durable version).** Give `Worker` a `HashMap<Modality, &dyn Backend>` instead of
the single `backend: &dyn Backend` field (`worker.rs:104`) and derive the advertised
modality list from that map's keys. Then the dispatch table and the advertisement are the
same object and this bug class becomes unrepresentable. This is the "extending the worker
to dispatch across modalities is separate, bigger work" already noted in
`media.rs:5-6`; Fix 3a is the honest stopgap until it lands.

### Fix 4 — server-side gate (also fixes the §3b starvation)

Add to `hive.node_claim_card`
(`supabase/migrations/20260913100000_code_modality_gate.sql`) **and** its delegated twin
`hive.ctl_d_node_claim_card`
(`supabase/migrations/20260914030000_control_pilot_delegation.sql:307`) a predicate in the
same shape as the existing ADR-024 `code` clause:

```sql
-- No node in any shipped build can execute image/video/music; never hand one out,
-- whatever a (possibly stale, possibly hand-rolled) client advertises.
and c.modality in ('text','code','speech')
```

Better still, drive it from a one-column `hive.executable_modalities` table so enabling
`image` later is a data change, not a function redefinition.

This matters independently of Fix 1 because it is the only defence against a **stale
binary** or a client that posts fabricated `capabilities` to `check_in` — and `check_in`
takes the node's self-report at face value. It also fixes the starvation in §3b for free:
an unexecutable card is never *selected*, so it never reaches the lease trigger that
aborts the claim.

### Fix 5 — what happens to a card nobody can execute?

Failing the card (Fix 2b) tells the owner *something* broke, but it blames the node for a
planning mistake. Two upstream changes make the system honest about capability gaps:

1. **Don't plan what the fleet can't run.** `hive.capacity_summary()` already exists
   (`supabase/migrations/20260905000006_interview.sql`) precisely to let the interviewer
   say "the Hive currently has N text nodes, 0 video nodes". The interview prompt should
   consult it and refuse to emit cards in a modality with zero capable nodes, rather than
   minting cards that can never run. Same check belongs on `hive.spawn_child_card` /
   `hive.ctl_d_spawn_child_card`, which today accept any of the six enum values from a
   running card with no validation at all.
2. **Make an unschedulable card visible, not silent.** A `ready` card whose modality no
   checked-in node advertises is currently indistinguishable from one that is merely
   queued. A periodic sweep that marks such a card (a distinct status, or a
   `notification_events` row for the owner, after N minutes) turns "my project is stuck"
   into "nothing in the Hive can make images yet". That is strictly better than either
   failing it on a node that happened to over-advertise, or leaving it queued forever.

---

## 5. What I could not determine

1. **Whether the migrations in this repo are the live schema.** Every payment conclusion in
   §3 assumes `supabase/migrations/` == deployed. `20260916050400_funded_compute_and_interviews.sql`
   is dated today and is the *only* thing zeroing local-mode payouts. **If it is not yet
   applied to the production project, the lead's payment claim holds verbatim**: a
   local-mode image card would pay `tokens_out × compute_output rate` for prose. This
   should be checked against the live database before the "no money at risk" line is
   relied on — it is the single assumption the whole blast-radius assessment rests on.
2. **Whether any node in the fleet actually has ComfyUI configured.** `HIVE_COMFYUI_URL` /
   `HIVE_COMFYUI_CHECKPOINT` live in per-machine `node.env` (`nodeconfig.rs:100-101`),
   which is not in the repo. "Jack's nodes advertise `[\"text\",\"code\"]`" is consistent
   with the desktop app's hardcoded llama-only probe (§2b), so it tells us nothing about
   whether a CLI node exists with ComfyUI wired up. Check `hive.nodes.capabilities` in the
   live DB for any row whose `modalities` contains `image`.
3. **The provenance of the observed paid `code` card.** The "old binary wrote no file and
   was paid" story is *consistent* with a pre-`20260916050400` schema, but I did not read
   the ledger rows to confirm the payment predates that migration. If a `code` card was
   paid *after* it, something else is wrong and §3c is incomplete.
4. **Whether `hive.cards.status` has a value suitable for "unschedulable"** — I did not
   enumerate the status enum, so Fix 5.2's implementation shape is a sketch.
5. **`crates/hive-coordinator`** has its own scheduler
   (`hive-coordinator/src/lib.rs:34,52` — `ProviderOverflow` for text/code, `Starved`
   otherwise) and `crates/hive-server/src/control.rs:360,590-598` has a modality-aware
   worker-matching test. Neither constructs a `Worker` or executes a card, so neither
   changes the findings above, but they are a second place where modality policy lives and
   would need to agree with any fix.

---

## 6. Filing summary

| # | Issue | Severity | Status |
|---|---|---|---|
| 1 | `run_card` runs the text loop for `image`/`video`/`music`; produces prose, reports `review` | High (correctness) | **Confirmed live** on a ComfyUI-configured `hive` CLI node; unreachable on the macOS app |
| 2 | Node advertises modalities its `Worker` cannot execute (advertisement built separately from executor) | High (root cause) | Confirmed, `main.rs:234-288` vs `main.rs:574` |
| 3 | `node_claim_card` has no predicate for unexecutable modalities | Medium | Confirmed, both claim functions |
| 4 | A `hive`-mode image card aborts the claim in the lease trigger and **permanently starves** every image-advertising node | Medium–High (availability) | Confirmed, found during this investigation, not previously filed |
| 5 | Nothing validates that a completed card's output matches its modality | Medium | Confirmed, `node_complete_card(p_content text)` |
| 6 | `spawn_child_card` / interview planner can mint any modality with no capability check | Medium | Confirmed |
| 7 | Payment for a mis-executed card | **Not currently live** — 0 honey in both modes on this schema; was live before `20260916050400` | Verify against the deployed schema (§5.1) |

*No source files were modified. This document is the only change.*
