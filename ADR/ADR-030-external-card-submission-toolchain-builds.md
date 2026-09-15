# ADR-030: External Card Submission — Real Toolchain Builds (Xcode Included) via a Member's Private Fleet

**Status:** Proposed · **Date:** 2026-09-15 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** Jack: "one of the issues we run into with Claude Cowork is that its not able to compile things in Xcode, are we able to tie Xcode to hive somehow so that Hive is able to compile swift programs?"

## Context

Cowork (this working session) reaches Jack's Mac through a device bridge (`device_bash`), but that
bridge runs inside an isolated Linux VM on the Mac with no Rust or Swift toolchain installed — it
can read/write files in a mounted folder and run arbitrary shell commands, but it cannot invoke
`swift build` or `xcodebuild` for real. Every Swift/Rust change this session has made all day has
had to be handed to Jack as a self-contained terminal command for him to paste into his own real
Terminal and report results back. That's the concrete problem: Cowork has no path to a real
compiler for this codebase's native side.

Checked whether Hive already solves a version of this problem before proposing anything new — it
does, almost entirely. ADR-024 built a real coding agent, scoped to a member's own Private Fleet,
whose `run_command` tool spawns a genuine, unsandboxed child process on the member's own machine
(`crates/ohhive-core/src/coder.rs`) — no shell, real stdout/stderr/exit code, confined only to a
declared workspace directory (`resolve_in_workspace`), explicitly documented as *not* a sandbox
around what the command itself can do once it runs. That is already sufficient to run `swift
build`/`xcodebuild` for real, on a Mac that has Xcode installed — nothing Swift- or Xcode-specific
is missing from the execution side. Confirmed this isn't theoretical by finding a completed live
smoke test of exactly this mechanism already sitting in Hive's own card history — card
`code-smoke-adr024` (project "Local Fleet Test", `execution_mode='local'`), whose task was
literally "run `uname -a` for real and write the output to a file," status `review`. The mechanism
works today.

What's actually missing is narrower than "compile Swift": **there is no way for anything outside
Hive's own client apps to create a card at all.** The `hive` CLI has `probe`/`run`/`check-in`/
`work`/`pair` — nothing that submits work. Cards get created by whichever client writes a card the
same way the **Kanban** (Hive's own web-app board, ADR-009) does, under the member's own
authenticated Hive session. Cowork isn't such a client. Closing that gap is what this ADR is
actually about; the Xcode use case is simply the first real reason to close it.

**Jack, explicitly, on how this must be scoped:** don't tie this to Cmd Work — Hive's `hive.cards`
happens to be hosted in the same Postgres instance as Happy Jack Media's internal Cmd Work tool
(ADR-001 decision 2, a hosting/billing choice, not a product relationship), but the overwhelming
majority of Hive members will never have a Cmd Work account and never should need one. Any
community member can already reach the Kanban with nothing but their own Hive membership
(ADR-008's invite-only onramp + Supabase Auth) — that is the one surface every member, not just
Jack, already has. This ADR is revised below to keep the design anchored to the Kanban and to a
plain Hive account, and to stop referencing the underlying Supabase project by name as if it were
part of the design.

## Decision

### 1. No new execution mechanism — Xcode is just another `run_command`

Reuse ADR-024's private-fleet coding agent exactly as built. A build task is an ordinary `code`
card: `required_capabilities` carries `workspace_path` (e.g. the Swift app's directory),
`task` (plain-language instructions — "run `swift build -c release`, report the exact compiler
output"), `brain` (`local`/cloud, member's choice, unchanged from ADR-024 decision 3), `max_turns`,
`tools_level` — the exact shape the live smoke-test card above already used. No Xcode-specific code
path, no new card modality, no new trust boundary beyond the one ADR-024 decision 2 already drew
("unsandboxed shell... on the member's own machine, as their own OS user").

### 2. External submission goes through a new `hive` CLI surface that mirrors the Kanban — not a raw database write, and not anything Cmd-Work-specific

Considered and rejected: having Cowork write directly into `hive.cards` via a database connection
with broad access (which is how this ADR's own research was done, read-only, to confirm the schema
and find the smoke-test card). Two independent reasons that's wrong, not one: it bypasses
`hive.project_roles`/RLS ownership checks entirely, consistent with no other card-creation path in
this system; and it only works for whoever holds that particular database credential — which is not
something a random Hive member's own Cowork session would ever have, and must never be assumed to.
**The only credential this design may require is a plain Hive account** — the same one every member
already gets through ADR-008's invite-only onramp, nothing about Happy Jack Media's internal tools.

The `hive` CLI's whole existing premise is "subcommands map to what the desktop app does in its
GUI." A card submitted from a terminal should be created exactly the way the **Kanban** creates one
today — same authenticated-member write, same `hive.project_roles` check, just from a different
client. **Decision: add `hive card submit` (creates one card in a project the caller already has a
role on, via the caller's own Hive session — the existing `hive pair` flow is the natural way to
get one) and `hive card status <id>` / `hive card await <id>` (polls `hive.cards.status` +
`hive.card_outputs`, the same tables the Kanban already reads) to the CLI**, rather than teaching
Cowork to talk to the database directly for writes. Exact flag/output shape is an open
implementation question (§ below), not decided here. This also means the feature ships for every
Hive member who runs Cowork, not only Jack — it was never Jack-specific to begin with, and this
revision makes sure the design doesn't accidentally become so.

### 3. A real worker has to run natively on the target Mac, outside any sandbox

The node that claims and runs the build card must be a `hive work` process running in Jack's real
macOS environment — not inside `device_bash`'s VM, which is exactly what lacks the toolchain in the
first place. This is closer to already-done than not: Midgaard is already a registered node
(`hive.nodes`, id `f1cc4f2b-…`) whose last-advertised capabilities already list `"code"` alongside
`"text"` in `modalities` — from earlier work this session, not something this ADR invents — but its
`presence` is currently `checked_out` (last heartbeat 2026-09-12). Closing this gap is operational,
not architectural: run `hive work --stay` (or install it as a persistent background service, the
same shape as the volunteer-kit worker scripts Halo already ships) whenever Cowork should be able to
dispatch a build.

### 4. Trust framing: unchanged from ADR-024, deliberately not widened

A submitted build card is only ever valid with `execution_mode='local'` and `node.member_id =
project.owner_id` — the exact same gate every other private-fleet card already has. External
submission does not let Cowork (or anything else) reach the shared `'hive'` pool, another member's
node, or a project Jack doesn't already own. The new surface (§2) only ever exercises a capability
that already exists for the desktop app; it does not grant a new one.

## Consequences

**Positive:** closes a real, named, daily-felt gap (every Swift/Rust change this session has needed
Jack's own hands on a keyboard) using almost entirely existing machinery — the same posture as
ADR-024/028/029 all took relative to what came before them. Xcode specifically needed nothing new;
any other toolchain Cowork can't run locally (a Windows-only build, a GPU-specific compiler) gets
the identical fix for free, for the same reason.

**Negative / risks:** this is the first time something other than Hive's own shipped clients
(kanban UI, `hive` CLI as a worker) creates a card. The CLI-submission design in decision 2 is
deliberately chosen to keep that inside the existing authenticated/RLS path rather than opening a
service-role shortcut — but it does mean Cowork becomes a de facto Hive client with its own local
credential, which needs its own care (a short-lived or per-session token Jack provides, not a
standing secret baked into every Cowork session). `RUN_COMMAND_TIMEOUT` is 600s (10 minutes) per
`run_command` call — fine for an incremental `swift build`, possibly tight for a from-scratch build
with fresh dependency resolution; worth measuring before relying on it, not assumed safe here.

**Deferred, explicitly, not solved here:** how Cowork itself authenticates a submit call in
practice (this machine's own `hive pair` node key works today — a short-lived or per-session
credential Jack issues is a nicer shape but not required to close the gap); whether to raise
`RUN_COMMAND_TIMEOUT` or give build-shaped cards their own limit; a dedicated system-prompt framing
for "this card is a build, not general coding" (nice-to-have — the existing generic coding-agent
framing already works, per the live smoke test); Windows/Linux toolchains reachable the same way
(same mechanism, not attempted here since the immediate need is Xcode on Midgaard).

## Implementation status (2026-09-15)

Decision 2's CLI is built: `hive card submit --project <title> --task "..." (--workspace <path> |
--repo <url>)` resolves the project by title against the caller's own local-execution projects,
submits via a new `HubClient::code_session_submit`, and prints the new card's id; `hive card
status <id>` and `hive card await <id>` poll it. All node-authenticated, off the same `hive pair`
key `hive work`/`hive check-in` already use — nothing Cmd-Work-specific, per the correction above.

The RPC side decision 2 calls for is **drafted, not applied**:
`docs/proposed-migrations/20260915_hive_card_submit_node_rpcs.sql` extracts
`hive_code_session_create`'s (ADR-024/Sif, `20260913180000_code_session_create.sql`) validation and
insert into a shared `hive.code_session_create_for(p_member, …)` core, then adds
`hive_code_session_create_node`/`_projects_node`/`_status_node` as node-key front doors onto that
same core and onto a matching status/output read — the exact web-RPC/node-RPC split
`20260913223915_node_key_byok_management.sql` already used to reach the Swift app for BYOK key
management, applied here for the identical reason. The existing web-facing
`hive_code_session_create` keeps its exact public signature and behavior (a `create or replace`
delegating to the new core, not a new function) — nothing already calling it needs to change.

Deliberately not run against the live Cmd Work Supabase project from this session: it's a real
schema change to production, and this ADR is still Proposed. Once Jack reviews the migration and
applies it (`supabase migration ...` or the dashboard — whichever this repo's own convention for
promoting `docs/proposed-migrations/` is), `hive card submit`/`status`/`await` work end-to-end with
no further code changes on either side. Self-verified (brace/paren/bracket balance, no duplicate
definitions), not compiler-verified — no Rust toolchain in this sandbox; see CONTINUITY.md's
2026-09-15 entry for the exact `cargo build` Jack should run.

## Related

ADR-024 (coding agent — the exact execution mechanism this reuses unchanged), ADR-025 (`worker.rs`/
`coder.rs` ownership — any CLI/worker-side change from decision 2/3 falls under it), ADR-009 (web
app / Kanban — the parity target for the new `hive card submit`/`status`/`await` surface), ADR-008
(invite-only membership/onramp — the one credential this design may ever assume a caller has),
ADR-001 (hub and source of record — purely a hosting/billing note: `hive.cards` happens to live in
the same Postgres instance as Happy Jack Media's internal Cmd Work tool, which is incidental and
must not become a dependency of this design, per Jack's explicit correction above).
