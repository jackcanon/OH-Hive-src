# Loki → Sif: priority reset, 2026-09-16 night

**This supersedes the ordering in both earlier queue docs today.** Jack has reset priorities and
it changes what matters.

> *"Loki's Den is the priority. The Hive doesn't really work until its end users have a system
> that can actually build projects... we've built cool infrastructure, but now we need to build
> strong workers. Without Loki's Den configured to be a useful builder, the Hive is pointless."*

He wants to **start testing code building through the Den.** That is the target.

File ownership rule is unchanged: you own `apps/desktop-swift/`, `crates/`,
`apps/desktop/src-tauri/`. I own `.github/workflows/` and `docs/`.

---

## 1. TOP PRIORITY — executable acceptance checks (ADR-019 rules layer)

Cmd Work `33cd68e7`. **Spec is written and ready:**
`docs/LOKI-ACCEPTANCE-CHECKS-SPEC-2026-09-16.md` (commit `9e400b7`), with line references into
the current code. Background and the argument for doing this first:
`docs/LOKI-DEN-BUILDER-GAP-2026-09-16.md`.

**The finding, in one line.** `tools.rs:354` defines a successful coding session as:

```rust
ok: !outcome.hit_turn_limit && !outcome.lease_expired,
```

Whether the code *works* is not part of it. `CodeSessionOutcome` has no pass/fail field;
`CodeSessionSpec` has no acceptance criteria, build command or test command. The agent writes
files, may never compile them, says "done", the card completes, and $honey is paid. **The model
is the sole judge of whether the model succeeded.**

Worth saying clearly: **the harness is not the problem.** I assumed at first that a card only had
`read_file`/`write_file` and I was wrong — it has `list_dir`, `run_command`, the vault tools,
`spawn_card`/`wait_for_child` and MCP, and code cards run `coder.rs`'s real multi-turn tool loop.
It can already build and test. Nothing *requires* it to. That is the whole gap.

The spec has the design decisions with their reasons. Three I would not want lost:

- **`#[serde(default)]` on the new field is load-bearing**, not politeness. Every card in the
  queue omits it and would otherwise fail to parse. Same lesson as `Chunk.truncated`.
- **Program + args, never shell.** `run_command` deliberately never goes through a shell. A
  second, weaker path into process execution on a member's machine is not worth the convenience.
- **`Unverified` is its own outcome and does not fail a card.** Refusing every card without
  checks would fail the entire queue. It has to be *visible*, not *fatal*.

**§6 is where the real win is and it is nearly free:** put the acceptance criteria into the
system prompt. An agent that knows it will be judged by `cargo test` behaves differently from one
told to stop when it feels finished. The gate catches failures; the prompt prevents them.

**§9 has a first card to run, and the one that actually proves it** — a task the agent *cannot*
satisfy, which must fail the card rather than complete it. A gate only ever observed passing has
not been tested.

## 2. HIGH — the two real CI failures

Both still open, both yours, both surfaced by the Swift job landing (which is now green).

- `9440cd80` — `managed_login_correlation_visibility_cancel_and_logout` panics on CI:
  `called Result::unwrap() on an Err value: "Codex version check timed out"`. It shells out to a
  real `codex` binary. **Check the production path before fixing the test** — if that `unwrap` is
  reachable outside tests, a wedged codex binary panics a node over an *optional* adapter.
- `6e7862ba` — Windows-only clippy, one unnecessary `mut`. Small, but it may be that leg's first
  honest verdict after weeks of being cancelled mid-build, and the MSI test needs it green.

## 3. HIGH — `cargo fmt --all`, as its own commit

Still red on ubuntu. Your work is committed now, so the sequencing constraint is satisfied.

## 4. MEDIUM — model-fit gate

`7be35ffa`, spec at `docs/LOKI-MODEL-FIT-GATE-SPEC-2026-09-16.md`. Heimdall joined the fleet
today and is a second, better test case than Jotunheim: 67 GB RAM but a **12.9 GB** RTX 4070,
advertising `qwen3.8:27b`. It *completes* on CPU instead of dying, so the failure is silent.
Gate on `vram_bytes` where present.

## 5. MEDIUM — process-tree cleanup on timeout

`3e80441c`. Already a known gap in your own finding 4 note. Acceptance checks turn it from
occasional into scheduled: a timed-out `cargo test` leaves `rustc` running, on every card, all
day, on someone's laptop.

---

## Deprioritised tonight, so you do not pick them up

Hive infrastructure is explicitly **not** this week. I over-weighted the Linode retirement as a
data-loss risk without asking whether failover had been tested — it has been, deliberately, and
it worked. Two items I raised to high today are back to medium. See
`docs/LOKI-HIVE-SANITY-MINIMUM-2026-09-16.md`.

ADR-036 git workspaces also waits, deliberately: a PR full of unverified code is a worse artifact
than no PR. Acceptance first, then workspaces.

Loki
