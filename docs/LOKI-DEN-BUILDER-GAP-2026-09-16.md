# What Loki's Den needs to be a strong builder

**Loki, 2026-09-16.** Jack: *"Without Loki's Den configured to be a useful builder, the Hive is
pointless... we've built cool infrastructure, but now we need to build strong workers."*

Read the code rather than theorised. One finding dominates.

---

## The harness is better than I expected. I was wrong twice getting here.

My first read said a card could only `read_file` and `write_file` — I had grepped the head of the
tool list and stopped. Wrong. The real surface, from `coder.rs`:

```
read_file  write_file  list_dir  run_command
vault_search  vault_read          (ADR-028, feature-gated)
spawn_card  wait_for_child        (ADR-032, coordinator cards only)
+ MCP tools via mcp.rs            (ADR-023, stdio)
```

`run_command` spawns a real subprocess — no shell, args passed separately, timeout, stdout and
stderr captured separately, output capped, `kill_on_drop`. A code card runs `coder.rs`'s **real
multi-turn tool loop**, not the bounded Draft/Critique/Revise machine the text modalities get.

**So the Den can already build and test.** It can run `cargo test`, read the failure, edit, and
run it again. That is a genuine agentic harness and it is better than I assumed.

---

## The gap: it can run tests. Nothing requires it to, and nothing checks.

Here is the whole definition of done, from the system prompt the agent receives:

> *"Work by calling tools until the task is complete. When you are done, reply with plain text
> (no further tool calls) summarizing what you did — that reply becomes this session's final
> report."*

And here is everything a finished session reports back:

```rust
pub struct CodeSessionOutcome {
    pub final_text: String,          // what the model says it did
    pub turns: u32,
    pub hit_turn_limit: bool,
    pub lease_expired: bool,
    pub waiting_on_child: Option<Uuid>,
}
```

**There is no pass/fail field. There is no verification result.** And `CodeSessionSpec` — task,
`workspace_path`, `repo_url`, `repo_ref`, `brain`, `model_id`, `max_turns`, `vault_name`,
`coordinator` — carries **no acceptance criteria, no build command, no test command.** The
`acceptance` string that exists is on `spawn_card`, for a coordinator describing a *child* card;
it is free text, and in the trait it is `_acceptance`.

So the sequence today is:

1. Agent is given a task string and a turn budget.
2. Agent writes files. It may or may not build them. Nothing obliges it to.
3. Agent says "done."
4. The card completes. $honey is paid.

**The model is the sole judge of whether the model succeeded.** A session that writes plausible
code that has never once been compiled is indistinguishable, to the Hive, from one that built and
tested green. Both return `final_text` and complete.

That is the difference between a worker and a text generator with filesystem access, and it is
the single highest-value thing to fix for Den-as-builder.

---

## Why this is the right thing to fix first

Everything else shipped this month has been closing *silent wrong output* one channel at a time:

- **Truncation guard** (today) — a Draft cut off mid-token can no longer be reported as complete.
- **Modality refusal** (Sif, `448216d`) — a node without a speech executor refuses instead of
  falling through to the text loop.
- **Model-fit gate** (specced today) — a node stops advertising models it cannot actually run.

Each one stops the system confidently returning something wrong. **Acceptance is the same class of
fix at the top of the stack**, and it is the one that faces the member: everything below it can be
perfect and a card can still complete having produced code that does not compile.

It is also what makes the *rest* of the infrastructure mean something. Replication, storage
settlement and $honey all assume the artifact being replicated and paid for is worth keeping.
Nothing currently establishes that.

---

## The build, cheapest useful version first

ADR-019 already exists for this — *Triangulated Card Verification (rules → judge → human)* — and
is unstarted. The rules layer alone is most of the value and needs no judge model.

**Step 1 — executable acceptance on the spec.** Add to `CodeSessionSpec` something like
`acceptance: Vec<AcceptanceCheck>`, where a check is a command and an expected exit status. The
session runs them after the agent declares done, on the host, through the same `run_command`
plumbing that already exists. `CodeSessionOutcome` gains the results. `worker.rs`'s
`run_code_card` fails the card when a required check fails, exactly as it already fails a
truncated Draft.

This is small: the execution machinery is built, the failure path is built, and the shape mirrors
a guard that landed today.

**Step 2 — tell the agent the criteria up front.** Put the acceptance commands in the system
prompt. An agent that knows it will be judged by `cargo test` behaves differently from one told
to stop when it feels finished. This is nearly free once step 1 exists and probably produces more
improvement than step 1 does alone.

**Step 3 — make "no checks" a visible choice, not the default.** A card with no acceptance
criteria should be marked as unverified in its receipt rather than silently indistinguishable
from a verified one. Members deciding whether to trust Hive output need to see which it was.

**Step 4 — the judge layer.** ADR-019's middle tier, for work where no command can express "is
this right" (prose, design, research). Deliberately last: the rules layer covers code, code is
what the Den is for this week, and a judge model is a much larger commitment.

**Not in this ladder, deliberately:** ADR-036's git workspaces (repo picker, persistent clone,
worktrees, PRs). It matters — today `prepare_workspace` still wipes and re-clones per run, and a
builder whose output never becomes a reviewable PR is only half useful. But acceptance is worth
more first: a PR full of unverified code is a worse artifact than no PR.

---

## One smaller thing found on the way

`run_command`'s `kill_on_drop` owns the direct child only, not grandchildren — already flagged in
the source as a known gap from Sif's efficiency audit. A timed-out `cargo build` leaves `rustc`
running. Harmless once; on a node taking cards all day it accumulates. Worth a small item, not a
priority.

---

Loki
