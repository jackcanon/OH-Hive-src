# Sif's next queue

Loki, 2026-09-16. She cleared the entire first audit queue — S-1 through S-6 plus S-A and S-B —
and self-assigned S-7 correctly from the triage doc. All of it is now committed and pushed
(`06fef35`..`d24232b`), and the working tree is clean for the first time today.

Quality note worth recording: every one of her handoffs states its own limits rather than papering
them. S-6 says outright that PGlite serializes requests so the lock tests are **not** genuine
contention tests, and lists the six disposable-PostgreSQL scenarios needed before rollout. She also
corrected the audit where it was wrong (`hive.balances` is a VIEW, so access is revoked rather than
given table RLS). That is the behavior that makes the rest of her verification trustworthy.

## The thing that now gates everything: none of it is deployed

Six migrations are written and unapplied — `private_fleets`, `node_account_summary`,
`fail_card_requires_owned_lease`, `project_scoped_live_tokens`, `hive_permissions_boundary`,
`debit_and_lease_locks` — plus the `bots-turn` Edge Function. Until they deploy:

- the account-takeover fix (3.1/3.2) is **not live**
- the permissions boundary is **not live**
- the ledger locks are **not live**
- Claude and Nous agents still cannot reply
- private-fleet enrollment still does not work

So the security work exists and protects nobody yet. **This needs Jack** — production credentials,
a window, and a decision about order — and it should be planned rather than squeezed between
features.

**And it needs S-1 below done first.** Audit 4.6 found the migration set is not the source of
truth and does not replay: a migration grants on a table no migration creates, three RPCs are
called and defined nowhere, one file depends on a function defined in a later-sorting file, two
files marked "PROPOSED — do not apply" sit in the applied directory, and several later files are
non-idempotent. Deploying six migrations — one of which rewrites ACLs across 132 wrappers — onto a
schema that cannot be replayed from scratch is how you find out at 2am. Baseline first.

## Queue

**S-1 — Audit 4.6: baseline the schema and make migrations replay.** Promoted above her own pick,
because it gates the deployment that gates everything else. `supabase db dump --schema hive` into a
baseline migration; move or delete the two do-not-apply files; renumber `20260913180000` after
`223915`; move `public.private_fleets` into `hive` (it violates the never-touch-public rule and
`check-migrations.sh` only inspects `create table … hive.`); add a CI step replaying every
migration into pglite or postgres from scratch. `supabase/tests/*.sql` currently hard-code a
production project UUID and are not in CI at all.

**S-2 — Audit 4.4 (her pick, S-7 in the old numbering).** `hive.snapshot_source` returns every
non-deleted project and the web board prefers it over `hive_projects_overview`, so
`execution_mode='local'` projects are visible to all members — silently undoing the 2026-09-13
visibility fix. Add `and p.execution_mode = 'hive'`. Separately `bug_report_add_attachment` and
`custom_avatar_url` check ownership with **unanchored** regexes, so a `javascript:` URL passes and
is rendered into `<a href>` and `<img src>` — the latter polled for every member every 20 s. Anchor
both, or store the object path only.

**S-3 — The app launch crash (S-0 from the previous queue, still open).** It bit Jack tonight:
`Hive.app` dies at launch with `EXC_BREAKPOINT` whenever the Rust side is rebuilt without the app,
because the bundle carries no dylib and loads the repo's build product by path. Bundle the dylib
into `Contents/Frameworks` with `@rpath`, or have `build-app.sh` rebuild the Rust target and
regenerate bindings itself. Plus a readable failure — it currently dies before logging initializes,
so there is no log line at all.

**S-4 — Audit 4.2: honey minting and uncapped payout.** Storage settlement pays on self-reported
bytes for project-null replicas with the treasury excluded from the negative-balance check; the
interview fund tops itself from the treasury and the member reimbursement can simply never be
called; `node_complete_card` caps payout only by fund balance, so one completion can drain a fund.
Fine while every node is trusted, not fine after — same shape as 3.1.

**S-5 — Holds and releases surface (S-C, now actually blocking).** `bots_deliveries_held` and
`bots_deliveries_release_root` still have no caller outside tests, so a held chain is invisible and
unreleasable in every app — the 30-turn gate currently *stops* a chain permanently instead of
pausing it. Per `LOKI-AGENT-PROFILE-BUILDOUT-2026-09-16.md`, build this **inside the agent activity
log** rather than as its own screen: a "waiting for you" row with a Release action is the release
surface and the debugging surface at once. Must land before S-6.

**S-6 — The "let agents talk to each other" switch.** Fan-out is off unless a caller passes
`with_budgets(HandoffBudgets::default())` and nothing in any app can. Off by default; the copy
should say plainly that it lets agents start their own turns and what the 30-turn pause means.

**S-7 — Per-agent model pinning, UI half.** Jack 2026-09-16, product-defining: `@Loki` means a
specific Anthropic model, chosen at creation. Needs a real picker fed by Ollama's `/api/tags` for
local and a per-provider list for cloud — free-typing a model name produces an agent that can never
run. Core half and the allowlist are mine. See the correction in the profile buildout doc.

**S-8 — Agent profile buildout, UI half.** Instructions and description editing, avatar, Channels
tab (a list view over `conversations_list(Principal::Agent)`, which already works), Managed by,
Archive. **Not tool toggles** — `capability_policy_ref` is unenforced and the panel should stay
honest about that.

**S-9 — Audit 6.4: archive the Tauri app.** ~~CI excludes it, the Swift shell covers its features,
and its `bots.rs` would run a hub write plus a `LocalHubStore::open` on every 2-second poll.~~

> **RETRACTED 2026-09-16 (Loki). Do not archive the Tauri app. Do not drop it from the workspace
> or the release matrix.** I queued this off the audit's §6.4 without checking it against ADR-018,
> which decides the opposite in its decision 1: *"Windows and Linux keep the existing Tauri
> (Rust + React) app unchanged."* The Tauri app is not a superseded macOS shell — it is the only
> Windows and Linux GUI this repo has, and Jack has asked (2026-09-16, late) to test Loki's Den on
> Windows and Linux before Friday 2026-09-18 13:00. Archiving it would have removed the thing he
> wants to test, days before he tried to test it. My error, not the audit's: the audit finding is
> true about the *macOS* Tauri build, which ADR-018 decision 6 does retire, and I generalized it.
>
> What survives of the finding, and is still yours if you want it: the `bots.rs` 2-second poll
> really does do a hub write plus a `LocalHubStore::open` per tick, and that is worth fixing on its
> own merits — the Tauri app is about to get *more* use, not less. Take it as a bug fix in place,
> not as a prelude to removal. See `docs/LOKI-DEN-WINDOWS-LINUX-PLAN-2026-09-16.md` for the wider
> plan this now sits inside.

## Mine, for cross-reference

3.8 (reserved `client_request_id` can swallow an agent's reply) — in progress, last of the three
fan-out preconditions. Then 3.7 core input bounds; the CLI's `room-create` moved to her atomic
`bots_rooms_create` (her handoff assigns me this); per-agent model core half with server-side
allowlist; 3.9 remainder.
