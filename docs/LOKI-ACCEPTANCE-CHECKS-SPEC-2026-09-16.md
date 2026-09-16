# Spec: executable acceptance checks for code cards

**Loki, 2026-09-16** · **Status:** spec, ready to implement · **Implementer:** Sif
**Implements:** ADR-019's rules layer (the first tier of *rules → judge → human*)
**Why:** `docs/LOKI-DEN-BUILDER-GAP-2026-09-16.md` — the Den can run tests; nothing requires it
to, and nothing checks.

Everything below is grounded in the current code. Line references are from `35f321a`.

---

## 1. The one-line summary of the change

Today, in `crates/ohhive-core/src/tools.rs:354`, this is the entire definition of a successful
coding session:

```rust
ok: !outcome.hit_turn_limit && !outcome.lease_expired,
```

A session is "ok" if it didn't run out of turns and didn't run out of lease. **Whether the code
works is not part of it.** This spec adds one more conjunct, and the machinery to compute it
honestly.

---

## 2. Data model

### `CodeSessionSpec` — new field

```rust
/// ADR-019 rules layer: commands that must pass before this card may complete.
/// Empty (the default -- every card written before this field existed omits it) means the
/// session is UNVERIFIED, not "passed". See `AcceptanceOutcome::Unverified`.
#[serde(default)]
pub acceptance: Vec<AcceptanceCheck>,
```

`#[serde(default)]` is required, not optional politeness: `from_required_capabilities`
deserializes straight from a card's `required_capabilities` JSON, and every card already in the
queue omits this field. Without the default, every existing card fails to parse. (Same lesson as
`Chunk.truncated` and the `ModelRef` size field in the model-fit spec — mixed-version data is a
live condition in this system.)

### `AcceptanceCheck`

```rust
pub struct AcceptanceCheck {
    /// Human-readable, shown in the receipt and in the failure message. e.g. "unit tests".
    pub name: String,
    /// Program to run. Spawned directly, never through a shell -- same contract as `run_command`.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Relative to the workspace root; workspace root when omitted.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Exit status that counts as a pass. Defaults to 0.
    #[serde(default)]
    pub expect_exit: i32,
    /// A failing check fails the card. `false` records the result without gating -- for a check
    /// worth reporting but not worth refusing work over (a linter being adopted, say).
    #[serde(default = "default_required")]
    pub required: bool,
}
```

**Do not add a `shell: bool` or accept a single command string.** `coder.rs`'s `run_command`
deliberately never goes through a shell, and its module doc says so. An acceptance check that
took shell syntax would be a second, weaker path into process execution on a member's machine.
Program plus args, same as the existing tool.

### `AcceptanceResult` and `AcceptanceOutcome`

```rust
pub struct AcceptanceResult {
    pub name: String,
    pub command_line: String,   // for the receipt: "cargo test --workspace"
    pub exit_status: Option<i32>, // None == killed by timeout
    pub passed: bool,
    pub required: bool,
    pub timed_out: bool,
    pub stdout_tail: String,     // capped, see §4
    pub stderr_tail: String,
}

pub enum AcceptanceOutcome {
    /// No checks were declared. NOT a pass.
    Unverified,
    Passed(Vec<AcceptanceResult>),
    Failed(Vec<AcceptanceResult>),
    /// A check could not be run at all (binary missing, cwd doesn't exist). Distinct from
    /// Failed: "cargo not installed" is an operator problem, not the agent's bad work.
    Errored(Vec<AcceptanceResult>, String),
}
```

`Unverified` existing as its own variant is the point of step 3 in the gap doc. A card with no
checks must never be recorded as though it passed.

### `CodeSessionOutcome` — new field

```rust
pub acceptance: AcceptanceOutcome,
```

---

## 3. Where it runs

In `coder.rs::run_session`, **after the turn loop breaks and before the function returns**.

Run the checks when, and only when, the loop ended because **the brain declared itself done** —
i.e. it replied with plain text. Specifically:

- `hit_turn_limit` → **skip the checks.** The session is already failing; running `cargo test`
  on a half-finished workspace burns minutes to tell us something we know.
- `lease_expired` → **skip.** This node no longer owns the card; it must not spend more of the
  member's CPU on it.
- `waiting_on_child.is_some()` (ADR-032) → **skip.** The session is paused, not finished. The
  card's state transition already happened inside `wait_on_child`; the parent will run its checks
  when it resumes and genuinely finishes.

Post a progress event per check via the existing `post_event` (`"acceptance_check"`), the same way
every tool call already posts one. A member watching a card build should see `cargo test` start,
not a silent two-minute gap.

---

## 4. Execution details, and the trap in them

Reuse `run_command_tool`'s machinery — do not write a second process runner. In particular reuse:
separate stdout/stderr capture, `read_capped`, `Stdio::null()` on stdin, `kill_on_drop`, and the
independent reader tasks that survive a timeout (Sif's own finding 4 fix, `coder.rs:876`).

Two deliberate differences from `run_command`:

1. **Its own timeout.** `RUN_COMMAND_TIMEOUT` is tuned for a model poking at things. A test suite
   is legitimately slower. Add `ACCEPTANCE_TIMEOUT`, default it higher, and make it per-check
   overridable later if needed. A timed-out required check is a **fail**, with `timed_out: true`
   so the receipt distinguishes "tests failed" from "tests never finished."
2. **Keep only tails.** Full output of a failing test suite is enormous and this is going into a
   card receipt that gets replicated. Cap each stream to a few KB from the **end** — the failure
   summary lives at the bottom of almost every test runner's output.

**The trap, already documented in the source and still unfixed:** `kill_on_drop` owns the direct
child only, not grandchildren (`coder.rs:899`). `cargo test` spawns `rustc` and test binaries. A
timed-out acceptance check will leave those running, and unlike a one-off `run_command` this now
happens on a schedule, on every card, on a node taking work all day. Filed separately; **do not
let it block this**, but do not let it be discovered later either.

---

## 5. Wiring the result through

`tools.rs:354` becomes:

```rust
ok: !outcome.hit_turn_limit
    && !outcome.lease_expired
    && !matches!(outcome.acceptance, AcceptanceOutcome::Failed(_) | AcceptanceOutcome::Errored(..)),
```

and the `data` payload gains the results so they reach the card receipt.

`Unverified` deliberately does **not** make `ok` false. Refusing every card that omits checks
would fail every card in the queue today. It must be *visible*, not *fatal* — that is what the
receipt field is for.

In `worker.rs::run_code_card`, a failed acceptance takes the **existing `fail_card` path**
(`worker.rs:652`/`684`), with a message naming which checks failed and their exit statuses. No new
failure mechanism; this is the same shape as the truncated-Draft failure that landed today.

---

## 6. Telling the agent, which is where the real win is

Once the checks exist, add them to the system prompt in `code_system_prompt`
(`coder.rs:1780`-ish), after the Task block:

```
This task is accepted only if all of these pass, run from the workspace root when
you are done:
  - unit tests: cargo test --workspace
  - lints: cargo clippy --workspace -- -D warnings
You can run them yourself with run_command at any point, and you should before
you finish. If you cannot make them pass, say so plainly in your final reply
rather than reporting success.
```

An agent told it will be judged by `cargo test` behaves differently from one told to stop when it
feels finished. This is nearly free once §2–§5 exist.

### Measured, 2026-09-16 — and it went against me

I originally wrote here that I expected **more** improvement from this than from the gate itself.
I ran the experiment instead of leaving that as an assertion, and it did not hold.

Two cloud cards, identical workspace and identical bug (`percent_change` dividing by `new`
instead of `old`, with a three-case unittest suite already present and failing 2/3). The only
difference was the task text. The suite was instrumented to append to `RAN.log` on execution, so
whether it ran is a fact on disk rather than a claim in the report.

| | told to run the tests? | ran them? | fix correct? |
|---|---|---|---|
| A | no — only "fix the bug" | **yes**, 1 execution | yes |
| B | yes — "accepted only if `python3 -m unittest test_calc` passes" | yes, 1 execution | yes |

**The agent verified its own work without being asked.** So the prompt line is worth having, but
it is not the bigger lever, and §2–§5 should not be justified by it.

Three caveats, because this is n=1 per arm and I would rather not over-correct in the other
direction: the test file was named `test_calc.py`, sat in the workspace root, and was named in
the task, so verification was about as discoverable as it gets; the bug was a one-token fix; and
neither run *iterated* — one execution each, after the edit, not a fix-run-fix loop.

**What the experiment does not change is the case for the gate.** In both arms the card completed
identically, and the Hive had no way to tell the tests had run. The model happened to be honest
and happened to be right; nothing checked either. An agent that verifies unprompted is a better
starting position than I assumed, and it is still not a verdict the system can act on.

Worth testing before relying on the good behaviour: a task with no visible test file, a bug whose
fix is not obvious, and a case where the first fix attempt fails — that last one is where
"did it iterate?" actually gets answered.

**A note on measurement, since it nearly caught me.** My first attempt used the presence of
`__pycache__` as the fingerprint for "the tests ran". I ran a control before trusting it: running
the suite on these nodes does *not* create `__pycache__`, so that signal was worthless and would
have produced a confident, wrong "it never ran the tests". The instrumented `RAN.log` replaced it.
Check the check.

---

## 7. Tests worth having

1. Check passes → `ok: true`, `Passed`, card completes.
2. Required check fails (exit 1) → `ok: false`, `Failed`, `fail_card` called, message names the
   check.
3. `required: false` check fails → `ok: true`, result still recorded in the receipt.
4. No checks declared → `Unverified`, `ok: true`, and the receipt says unverified. **Assert the
   receipt field**, not just `ok` — the whole point is that this is distinguishable.
5. Missing binary (`command: "definitely-not-a-real-binary"`) → `Errored`, not `Failed`.
6. Check exceeds `ACCEPTANCE_TIMEOUT` → fail with `timed_out: true`, and the partial output that
   had already been read is still present (this is exactly the regression finding 4 fixed; it
   would be a shame to reintroduce it here).
7. `hit_turn_limit` → checks **not** run. Assert the process was never spawned, not merely that
   the result is absent.
8. A card whose `required_capabilities` omits `acceptance` entirely still deserializes.
9. `expect_exit: 1` passes when the command exits 1 (some tools invert).

---

## 8. What is deliberately not here

- **No judge model.** ADR-019's middle tier. Rules cover code; code is what this week is for.
- **No per-check `shell`.** §2.
- **No auto-derived checks.** Do not infer `cargo test` from the presence of a `Cargo.toml`.
  Guessing what "done" means is how a card fails for a reason the member never chose. Explicit
  only.
- **No retry-on-failure loop.** Giving the agent its failing test output and letting it try again
  is a real and probably good idea, but it is a change to the *turn loop*, not to acceptance, and
  it deserves its own decision about turn budgets and cost.

---

## 9. A first card to test with, once this lands

Small enough to run in a minute, and it fails honestly if the gate is wrong:

```json
{
  "task": "Add a function `add(a: i64, b: i64) -> i64` to src/lib.rs and a unit test for it.",
  "workspace_path": "/path/to/a/scratch/cargo/project",
  "max_turns": 12,
  "acceptance": [
    { "name": "unit tests", "command": "cargo", "args": ["test"] }
  ]
}
```

Then the one that actually proves the gate works: **run it again with the task changed to
something the agent cannot satisfy** (ask it to make a deliberately failing test pass without
touching the test). It should fail the card, not complete it. A gate that has only ever been
observed passing has not been tested.

---

Loki
