# The third worker already exists. It needs a card, not a build.

**Loki, 2026-09-16** · Question from Jack: can we put the Claude API to work as a third worker,
given we keep hitting usage limits, and can it get us through Gate 1 by Friday 11:00?

**Short answer: yes, and almost none of it needs building.** The path is complete, deployed and
provisioned. I verified every link rather than reading the design doc and assuming.

---

## What I checked, and what it says

| link in the chain | state | evidence |
|---|---|---|
| `CodeBrain` seam — the "what do I do next" decision point | built | `brain.rs:223`, one method, stateless |
| `CloudBrain` — routes turns to the hub instead of a local model | **built** | `coder.rs:388-436`, full `CodeBrain` impl |
| Worker picks it for `brain: "anthropic"` | **built** | `worker.rs:700` — `provider @ ("anthropic" \| "openai" \| "nous")` |
| `code-brain-turn` Edge Function | **ACTIVE, v5** | live on `pxfbnuxcnerulbvbmowz` |
| Anthropic support in that function | built | `api.anthropic.com/v1/messages`, `x-api-key`, tool_use parsed, default `claude-sonnet-4-5` |
| Anthropic key in the vault | **present**, stored 2026-09-15 | `hive.member_keys` — one anthropic row (value never read) |
| A way to create the card without the web app | built | `hive.code_session_create_for(...)`, migration applied 09-13 |
| Code-capable nodes online right now | **3 usable** | see below |

Nodes checked in and heartbeating within the last minute:

```
Jotunheim   built 2026-09-16 08:53   cloud brain: YES
Odin        built 2026-09-16 09:23   cloud brain: YES
Overgaard   built 2026-09-16 08:17   cloud brain: YES
Heimdall    built 2026-09-12 20:18   cloud brain: NO  <- v0.4.0 release, predates this work
```

**Heimdall cannot run a cloud card** — the v0.4.0 release was cut 09-12 and the cloud-coding work
landed 09-13. Everything else on the fleet is a local build from `main` and has the path compiled
in. That is a second, quieter cost of the release-versioning slip: the one node running a proper
*release* is the one that can't do the newest thing.

So the sequence to a running Claude-API worker is: create a card with `p_brain := 'anthropic'`
and `p_cloud_consent := true`, and one of the three polling nodes claims it within about five
seconds. **The key never touches the node** — it stays in Supabase Vault and the Edge Function
makes the call. Tool execution (`read_file`/`write_file`/`list_dir`/`run_command`) still happens
on the member's own machine, which is the whole point of ADR-024.

---

## Why this genuinely solves the usage-limit problem

The limits we keep hitting are on **subscriptions** — Jack's Claude plan driving me, and Codex
driving Sif. A `brain: "anthropic"` card bills the **API account** instead, metered per token and
independent of either subscription. Adding cloud workers does not eat into the quota that runs
this conversation.

`hive.provider_budget` already exists, so there is somewhere for a spend cap to live. **Worth
setting one before the first real run** — an agent in a tool loop with `max_turns: 40` can spend
in a way a chat session does not, and discovering that from a bill is a bad way to learn it.

---

## Two different "third workers" — worth not conflating them

**A. A cloud-brained Hive node** (everything above). This is the *product* working. It proves the
Den can build using a frontier model, on a member's own machine, with the member's own key. It is
ready tonight.

**B. A Claude Agent SDK harness working the repo like Sif does.** A separate process with an API
key, reading the queue docs and implementing against them. This is *capacity* rather than
product, and it is not built — it would need its own harness, its own file-ownership lane, and a
collision story with Sif.

Jack's phrasing ("a 3rd worker... so we cross that first gate") points at **B**, but **A** is what
exists and A is the more valuable thing to prove this week, because it is the actual product
thesis: the Hive is pointless unless the Den builds. B is a staffing decision we can make
separately, and honestly it is a bigger lift than it sounds.

---

## Friday 11:00 — what has to be true

Gate 1's condition, as written in the roadmap: **"a card produces verified work or fails
honestly."** Reading that literally matters here, because it decides what is actually required.

**Required for the gate:** acceptance checks §2–§5 (the mechanism) and §6 (telling the agent its
criteria). Nothing else.

**Not required, despite being in the Phase 1 list:** the model-fit gate is about matching models
to hardware, not about verifying work — on the gate's own wording it belongs to node scheduling,
not to trust in output. CI green (fmt, Windows clippy, the codex-timeout test) is hygiene we want
badly, but a card can produce verified work with CI red.

So the honest Friday scope is: **acceptance checks, implemented and demonstrated both ways** —
passing on a task the agent can do, *failing* on one it cannot. That, plus a cloud-brained card
doing it, would cross Gate 1 and demonstrate the third worker in the same run.

**What I would not promise:** Phase 1 in full by Friday. The estimate was 3–5 days and there are
about 1.5 working days. The constraint is not worker count — it is that acceptance checks touch
`coder.rs`, `tools.rs` and `worker.rs`, which are Sif's lane, and parallelising inside one lane
mostly produces merge conflicts. A third worker adds throughput on *independent* work (Windows
clippy, fmt, model-fit gate), not on the critical path item.

If Friday 11:00 is firm, the lever is scope, not staffing: ship the gate, defer the rest of
Phase 1, and say so plainly rather than declaring a gate crossed that wasn't.

---

## What I would do next, in order

1. **Set a spend cap** in `hive.provider_budget` before any real cloud run. Jack's call on the
   number.
2. **One throwaway cloud card tonight** on a scratch repo — `p_brain := 'anthropic'`, a trivial
   task, `max_turns` low. This costs cents and answers the only question left: does the whole
   chain actually run end to end? Nobody has run one. Sif's 09-13 test exercised the endpoint and
   the conversation, explicitly *not* a live worker or real filesystem changes.
3. Only then plan Friday around it.

Step 2 is the one that matters. Everything above is verified-by-inspection, and this session has
already produced three confident readings that inspection got wrong.

Loki
