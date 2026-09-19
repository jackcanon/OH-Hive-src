# What our agents will actually need: an inventory drawn from the work

**2026-09-19, Loki.** Jack asked what tools the Den's agents should be granted, and said to start by
finding out what Loki and Sif have actually used building it. This is that inventory, and then the
translation into role grants.

The evidence is `Halo-src/docs/CONTINUITY.md` — 442 dated entries, 2026-09-10 to 2026-09-19, read in
full. Line references below point into it. This document is the evidence base for
`docs/agent-templates/README.md`'s seven roles; where the two disagree, this one has citations and
that one has intent, so reconcile rather than assume.

---

## The three findings that should shape the grants

**1. The most-used tools are verification, not action.**

Counting backticked invocations across the log: `cargo test` 36, `cargo check` 22, `cargo fmt` 10,
`cargo clippy` 7, `git diff --check` 49, "whitespace check" 26, `codesign --deep --strict` on every
shipped bundle, SHA-256 on every transfer. Against that, the *doing* tools are comparatively few.
Two agents spent two weeks mostly proving things were true.

So a template shaped as "can run commands" grants the wrong thing. What the work needed was a small
set of **verbs that produce receipts** — build, test, lint, sign, compare, submit — not a shell.

**2. The self-written helpers are the product's missing-tool list.**

Thirteen categories of script exist purely because a built-in did not (§4). The loudest is
installing a signed bundle on a target machine with backup and rollback: Sif rewrote that as a
throwaway Python script **at least seven times in nine days** — `install-agent-templates.py`,
`install-combined-showcase.py`, `install-directory-update.py`, `install-library-id-fix.py`,
`install-library-update.py`, `install-spark-connector.py`, `install-spark-email.py`. When a
capability gets rewritten weekly, that is not a script, that is a tool the product owes its users.

**3. Two individually-correct permission systems can silently remove a capability.**

`backup_export` requires `operator='hjm'`. The nightly run happens only on the coordinator-lease
holder. Both rules are right. When a volunteer box took the lease, **backups stopped for three days
and nothing noticed** (L509-515). No grant matrix should be designed without assuming this shape of
failure: the dangerous state is not "permission denied", it is "permission quietly never applies".
Whatever we build needs to answer *"what can this agent actually do right now"* by evaluation, not
by reading two tables and hoping.

A live instance of the same shape, found the same day: a library grant is **node-scoped, not
agent-scoped**. An agent policy naming a library its realm cannot read refuses silently — configured
in the UI, dead in practice.

---

## 1. What was used, by category

Risk key: **R** read-only · **WL** writes locally · **WR** writes a shared/remote system ·
**$** spends money · **IRR** irreversible.

### Build and test — the irreducible core
`cargo build/check/test/clippy/fmt` (both agents, WL) · `swift build` / `swift test` (Sif, WL) ·
UniFFI binding generation with byte-identity checks (Sif, WL) · `deno test` for Edge Functions
(both, WL) · `npm`/`npx tsc`/`pnpm build` for web and Tauri (both, WL) · `python3` stdlib as
universal glue (both, WL).

### Verification specifically
`git diff --check` before nearly every commit · `codesign -vvv --deep --strict` after every signed
build · SHA-256 for transfer integrity and binary identity across machines · isolated bundle
engine-load probe · migration replay from an empty database (91, then 104 migrations) ·
`compare-baseline.py` schema diff that hard-fails on a missing object · **sabotage testing** —
break the check, confirm it fires, restore byte-identically · `ollama show`/`ps` to confirm a
model's real quant and that nothing was loaded · `pgrep` to confirm an app was really closed before
overwriting it · `lsof` for lock diagnosis.

### Version control
`git` commit/diff/log/worktree/tag (both, WL) · **`git worktree` as a house rule** after an
incident: build and commit from an isolated worktree at `origin/main`, never the shared checkout ·
`git merge` with hand-resolved conflicts (Loki, WL) · `git push` to main (**always gated on
explicit approval**, WR/IRR) · GitHub Actions run and log reads (both, R) · GitHub API for repo
metadata and blob SHAs (both, R).

### Packaging and signing
`build-app.sh` — the compound ritual: rebuild Rust, snapshot engine, regenerate bindings, package
the dylib, **sign inside-out**, verify, then replace (Sif owns, WL) · `codesign` with the Developer
ID (~20 bundles, credential) · `zip` + SHA-256 manifest for cross-machine transfer · `sips` to
verify icon dimensions · **backup-before-replace on every install**, app *and* database, into
`backups/pre-<change>-<timestamp>` at 0700. Notarization was never done — worth recording as an
absence, not an omission.

### Databases
Local SQLite (`vault-host.sqlite3`): read diagnosis, and **hand-written rows for things with no UI**
— realm library grants, a bad `preferred_host`, a room roster, six agent bios. Always backed up
first. Hosted Postgres: read-only Management API SELECTs for live verification; `apply_migration`
for production schema changes (**WR, IRR, always approval-gated**); Edge Function deploys. `psql`
was never used.

### Machine-to-machine — the largest concentration of risk
SSH with one key as `jack@` on five Macs and **`root@` on three Linodes** · SSH aliases, two of
which were stale and wrong · **SSH must be wrapped in `script -q /dev/null`** or `llama-server`
hangs silently on `/health` — two days were lost to that · remote binary replacement preserving
`.bak-<reason>-<date>` · remote `.app` replacement on a second Mac after confirming closure ·
`systemctl`/`launchd` stop, edit, reload, restart — including hand-editing a **system** unit ·
`journalctl` reads · multi-GB model downloads onto remote disks · **blanket NOPASSWD sudo on
Heimdall** for network impairment · Tailscale and `cloudflared` tunnels.

### Credentials touched
Developer ID signing identity · `~/.ssh/id_ed25519` · `~/.halo/anthropic_api_key` · Keychain OAuth
tokens · `gh` token piped over **stdin, explicitly without logging** · `HIVE_GOOGLE_OAUTH_CREDENTIAL_JSON`
embedded at build time · `LOKISLAB_PUBLISH_TOKEN` (sensitive Vercel var — **blocked** the intended
publish path) · Supabase service-role auth · Private Fleet signing-key provisioning, which required
a **second explicit approval** beyond the deployment approval · the `age` backup key, whose offline
copy is still unconfirmed.

Flagged as a security finding in the 2026-09-15 audit and still worth carrying: `HIVE_NODE_KEY` and
`HIVE_VAULT_SELF_KEY` are exported into process env and **inherited by agent-run commands**.

### Model runtimes and the product's own CLI
`ollama` (pull/show/ps/run, HTTP :11434) · `llama.cpp` family for Halo pooling · HaloBench, their own
signed SwiftUI app driving fleet benchmarks over live SSH ptys · **the `hive` CLI itself** —
`card submit/status/await`, `bots agent-register/list`, `work --poll`, `check-in`, `pair`. The
product dogfooding itself, and `--expect-acceptance` became the receipt-reading verification
primitive.

### Coordination between the two agents
`CONTINUITY.md` itself, append-only, each entry signed and ending with an **Effort:** line ·
`continuity_index.py` so a session can read bounded slices of a 1.1 MB file rather than all of it ·
Cmd Work items, decisions, project memory and channels · handoff docs named `SIF-*` / `LOKI-*` ·
`reports/*.md` with JSON receipts committed alongside · explicit file ownership ("Sif owns
coder.rs") · `reports/.live-status.json`, a poll-able status file so a long GUI run could be
monitored **without screenshots**.

### Web and media
Fetching **primary-source vendor documentation** with exact URLs — Sif's default research move ·
web search for naming-collision screening · direct `curl` against a provider API to disprove an
assumption from outside the product · `image_gen` for brand and character work · a PowerPoint deck ·
the SVG→PNG→ICO/ICNS brand asset pipeline · published artifact pages, one of which was **corrected
and republished** after being disproved.

---

## 2. What failed or was refused — as informative as the successes

- **Loki had no Rust, Swift or Xcode toolchain** for most of the period. It wrote code anyway, then
  dry-ran every non-trivial SQL pattern through Python's `sqlite3`, ran brace/paren/bracket balance
  checks as a compiler substitute, and labelled the result **"self-verified only, not
  compiler-verified"** before handing it to Sif. That honesty is the practice worth keeping; the
  substitutes are not.
- **macOS UI automation timed out on at least five separate occasions.** Each time the fallback was
  better: compile a standalone Swift harness from the production connector source, or ask Jack to
  click. Do not grant agents a UI-automation tool expecting it to work.
- **A sandbox denied loopback binds**, causing 7–14 spurious test failures more than once. Both
  agents re-ran with loopback permitted and said so, rather than reporting a green run.
- **An automatic approval review refused a combined commit-and-push-to-main.** No push happened.
  The system working.
- **Agents cannot set `done` in Cmd Work** — the database rejects it. 59+ items moved to
  `ready_for_review` with evidence instead, for a human to close.
- **`publish-article.sh` could not run** because its token is a sensitive Vercel variable. Published
  through the git door instead.
- **No video tool existed**, so the Fenrir animation was written from scratch as a procedural
  renderer.
- **A subagent redid work Sif had already done.** Ownership boundaries were made explicit after.

---

## 3. Proposed grants, by role

Tiers, derived from how the work was actually gated:

| Tier | Meaning | Gate |
|---|---|---|
| **T0** | Reads. No credential, no side effect. | Grant freely per agent. |
| **T1** | Writes locally, trivially reversible. | Grant per agent, receipt recorded. |
| **T2** | Writes a shared system, reversible with effort. | Grant + receipt + backup-before-replace. |
| **T3** | Spends money. | Owner-approved budget, frozen per task. The mechanism already exists — `validate_compute_budget`, `card_has_funded_budget`. |
| **T4** | Irreversible, or handles a credential. | **Never a standing grant.** Ask every time, name the exact action. |

| Role | T0 | T1 | T2 | T3 | T4 — ask every time |
|---|---|---|---|---|---|
| **Assistant** | library search/read, web fetch | — | — | — | — |
| **Researcher** | library search/read, web fetch+search, cite | — | — | — | — |
| **Librarian** | library read, directory listing | ingest, index, organise a collection | — | — | crawling a new location |
| **Developer** | repo read, CI read, receipts | worktree, build, test, lint, fmt | submit a card to a realm | cloud brain on a card | any migration |
| **Reviewer** | repo/diff/CI read, read receipts | run the test and lint gates | — | — | — (never writes, by design) |
| **Integrator** | repo/diff/CI read, connector read (Drive, Gmail, Spark, GitHub) | — | structured commit and **push to configured branches**, open/update a PR | — | force push, branch deletion, protected-branch merge, production deploy; every connector write (send mail, create a file) |
| **Coordinator** | fleet and work-item read | write work items, channel messages | dispatch a card to a realm | budget assignment | — |

Three notes on that table.

**Reviewer writes nothing, deliberately.** The log's best verification came from a role that could
run the gates and had no way to change the thing it was checking. Keep that.

**Developer's T2 is "submit a card", not "run a command".** The card path already carries acceptance
checks, a receipt, a lease and a budget. An agent that submits a card inherits all of that; an agent
with a shell inherits none of it.

**Integrator is the only role that pushes** (Jack, 2026-09-19), which matches
`README.md`'s original split: Developer hands off, Integrator lands. Push is a standing T2 for it,
scoped to configured branches and remotes. Its *connector* writes stay T4 — every live one in two
weeks (one Drive file, one email) was individually approved, and that was right.

---

## 4. The missing built-ins, ranked by how loudly the evidence asks for them

1. **Install a signed bundle on a target machine, with backup and rollback.** Rewritten seven-plus
   times. The single biggest gap.
2. **Grant a realm access to a library.** No UI exists; it was done by hand in `sqlite3` on
   2026-09-19. This is the cross-device consent step `local_hub.rs`'s header still calls unbuilt.
3. **Per-session effort and cost accounting.** `session_effort_report.py` (16 references) and
   `codex_effort_report.py` exist because neither agent had native access to its own cost.
4. **Bounded reading of a long document.** `continuity_index.py`, written because reading 1.1 MB
   every turn was untenable. Any agent with a growing log needs this.
5. **Submit a job from outside the product.** `cloud_card.py`, kept rule-identical to the Rust CLI by
   hand. This whole line exists because *"one of the issues we run into with Claude Cowork is that
   it's not able to compile things"*.
6. **Run a migration safely and prove it.** Six `.mjs` regression harnesses plus
   `compare-baseline.py`.
7. **Multi-machine test rigs.** Four separate scripts, including a deliberately stdlib-only node-2
   so the second machine needs no repo and no toolchain.
8. **Status without screenshots** — `reports/.live-status.json`. An agent should be able to watch a
   long job by reading state, not by looking at pictures.

---

## 5. What not to grant, and why

- **A general shell.** Nothing in the record needed one that a verb with a receipt would not have
  served better, and a shell defeats every gate above.
- **UI automation.** Attempted five times, timed out five times. The fallback was always better.
- **Standing credential access.** Every credential use in the log was either build-time embedding, a
  stdin pipe that was explicitly not logged, or a one-off with its own approval.
- **`root@` anywhere**, and blanket NOPASSWD sudo. Both exist today on the fleet; neither should be
  reachable by an agent grant.
- **Any capability whose availability depends on two separate conditions holding.** See finding 3.
  If a grant can be silently voided by an unrelated correct decision elsewhere, it will be.

---

## 6. Decisions, 2026-09-19

**1. The grant unit stays both — so both must be visible.** (Jack.) Keeping agent-scoped policy and
node-scoped grants is right; the bug is that only one of them is shown. The fix is not to collapse
them but to make effective access an *evaluated* answer. Concretely: the Tools and access panel
resolves the agent policy against the host node's `vault_readers` rows and renders three states, not
two — granted, denied, and **granted-but-inert** ("this agent is allowed to read Halo-src; Alfheim
is not, so it cannot"). Finding 3 says the silent state is the dangerous one; naming it is the whole
remedy. Same evaluation for `backup_export`: allowed, and *currently applying*, are different
questions.

**2. Push belongs to the Integrator.** (Jack.) Standing T2, scoped to configured branches and
remotes; force push, branch deletion and protected-branch merge stay T4. Developer hands off and does
not push. This restores `README.md`'s original split, which the first draft of this table got wrong.

**3. The budget unit — open, and the blocker is attribution, not policy.**

What exists is strong but is not a budget in the household sense. It is a **price approval per
card**, and it is enforced properly:

- `hive.compute_card_budgets` is keyed on `card_id` — one approval per card, by the project owner.
- The rates are **snapshotted at approval** (`input_rate`, `output_rate`), so a later rate change
  cannot silently reprice approved work.
- `input_hash = md5(inputs || required_capabilities)` binds the approval to the exact job. Edit the
  prompt and the approval is void.
- `reserve_compute_on_lease` **escrows** the remaining amount from the payer account the moment a
  node takes the lease. This is real money set aside, not a check.
- `node_complete_card` refuses on both `compute_budget_exceeded` and `compute_reservation_missing`.
- `hive_compute_budget_replace` allows re-approval only before a lease or reservation exists, and
  writes `compute_budget_history`.

What does not exist is any **aggregate** — no running total per agent, per day, per project or per
month, and no cap on how many funded cards one agent may create.

The reason that gap cannot be closed by policy alone: **nothing in the money path records which
agent spent it.** `hive.cards.suggested_by` references `hive.members(id)` — a human. The ledger
carries `account_id`, `card_id` and `node_id`, and no agent id at all. So "how much has Thor spent
today?" has no query today. Agent attribution on the card, and carried into `ledger_entries`, is the
prerequisite for any per-agent budget; the cap itself is easy afterwards.

Three shapes worth weighing once attribution exists, each with a different failure mode:

- **Per agent per day.** Matches how an owner worries. Fails open at midnight — a runaway agent
  resumes on a fresh allowance.
- **Per agent, replenishing balance.** A wallet that refills at a rate. A runaway drains it and stays
  drained until the owner intervenes, which is the right failure direction.
- **Per delegation chain.** The budget travels with the root card down through `parent_card_id`, so a
  Coordinator hands out a slice it cannot exceed. Closest to how `HandoffBudgets` already limits turn
  fan-out, and the only shape that survives an agent spawning agents — which is exactly what the
  Coordinator role is for.
