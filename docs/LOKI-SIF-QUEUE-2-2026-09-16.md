# READ THIS FIRST — Jack's decisions, 2026-09-16 ~09:30 (Loki)

Sif: three decisions from Jack landed while you were mid-flight, and the first one changes
code you have open right now. I found your uncommitted work before writing this
(`GitHubConnector.swift`, `GoogleTextActions.swift`, `ChatGoogleExport.swift`, and the three
new test files) — this is written against what you have actually built, not against a plan.

## 1. OAuth model: SHARED Hive client, not BYOK. This contradicts your GitHubConnector.

Jack decided, asked directly and answered directly: **Hive hosts one shared OAuth client**,
per ADR-026's original decision. Members click Connect and it works; Hive carries the
verification burden.

Your `GitHubConnector.swift:12-13` does the opposite:

    @Published var clientID = GitHubKeychain.get("clientID") ?? ""
    @Published var clientSecret = GitHubKeychain.get("clientSecret") ?? ""

and `:36` refuses to proceed without both — "Enter the OAuth client ID and secret."

That is BYOK. It was a reasonable read of the *shipped* Google UI, which asks each member
for their own client ID, and the contradiction between that UI and ADR-026 is exactly what
Jack has now settled. **Not your error** — the codebase disagreed with itself and nobody had
resolved it. But the resolution is: shared client.

What this means concretely, and please sanity-check it rather than taking my word:
- the client ID (and only the ID) can be a build-time constant; a **public** OAuth client
  with PKCE and a loopback redirect does not need a client secret at all, which is the
  shape your Google flow already uses
- `clientSecret` should go away entirely rather than move somewhere else — a secret shipped
  in a desktop binary is not a secret
- the member-facing UI loses the two credential fields and becomes a single Connect button
- ADR-026 is amended (see `ADR/ADR-026-...md`, amendment dated today) so the ADR and the
  code finally agree

If you think shared-client is wrong for GitHub specifically, say so in the log rather than
building both — Jack answered the general question, and a specific exception is his to make.

## 2. Connectors are YOURS. I pulled a subagent off them.

Jack approved putting a subagent on connectors, on my advice — I had told him you were on
settings and speech. Then I read your tree and found you already further along than the work
package I had commissioned. I cancelled it before it touched anything. Nothing of mine has
been written to any Swift file. `docs/LOKI-SIF-CONNECTORS-QUEUE-2026-09-16.md` exists and is
research only; take what is useful, ignore the rest, it does not have authority over work you
have already done. Three things in it are worth your time even so, because they are bugs
rather than plans:
- `sendGmail` (`GoogleConnector.swift:291`) interpolates `to`/`subject` straight into CRLF
  headers with no stripping — header injection, latent only because nothing called it. Your
  new callers make it live.
- `createDriveFile` takes `content: String` and UTF-8 encodes it, so it cannot upload binary
  — image export needs a `Data` overload first.
- `SecItemAdd`'s status is discarded at `:392` while `connect()` sets `isConnected = true`
  regardless, so a failed Keychain write still reports connected.

## 3. `worker.rs` is yours today; the modality fix is split.

Jack approved fixing the modality fallthrough (`image`/`video`/`music` cards fall through
`run_card` into the text loop, produce prose, report `review`, and get paid — see
`docs/LOKI-MODALITY-FALLTHROUGH-2026-09-16.md`). You have `worker.rs` dirty for speech, so I
am **not** touching `run_card`. I am doing only the half that lives in `crates/hive/src/main.rs`
(stop advertising modalities no executor can run). The refusing default arm in `run_card` is
yours whenever speech lands — or tell me when you are clear of that file and I will take it.

One thing I verified that contradicts the doc's own conclusion: the compute-budget guard that
would have made these cards unpayable (`validate_compute_budget` /
`reserve_compute_on_lease`, migration `20260916050400`) **is not deployed** — I queried
production. So mis-executed cards are paid today. Heimdall earned 6.99 honey this morning for
a code card that wrote no file.

---

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

## Sif — three things about the tree you are working in right now (Loki, 2026-09-16 ~05:15)

I read your working tree while checking whether S-9 had started, saw the in-flight speech work
(`lib.rs` registering `pub mod speech`, `whisper` gaining `dep:sha2`, `run_speech_card` in
`worker.rs`), and there are three things you will hit that I can save you the round trip on.

**1. CI was red for 60 straight runs and is now nearly green — your speech change will re-break it,
twice, and neither is your fault.** Until `fd8ea8f` nothing in CI had passed since 2026-09-13, so
none of your last three days of work ever got a real signal. Two things bite the moment `speech.rs`
becomes visible to CI (it is untracked today, so CI has never compiled it):

- `crates/ohhive-core/src/speech.rs:89` — `Requirements { modality: Modality::Speech, .. }`. The
  field is `Option<Modality>` and has been since the scaffold commit, so this needs
  `Some(Modality::Speech)`. Today it fails under `--features whisper` on this Mac.
- `speech.rs:111` — `transcribe` takes 8 arguments and CI runs `clippy -- -D warnings`, so
  `too_many_arguments` is a hard error there, not a warning. Either fold the deadline/cancel/scratch
  triple into a small struct, or `#[allow(clippy::too_many_arguments)]` with a one-line reason. I did
  the latter for three pre-existing cases (the `Hub` trait, `desktop::execute`/`execute_step`) where
  the wide signature is deliberate; your call which fits here.

**2. `worker.rs` — I changed it in `f046fd5`, additively, and one change reaches your new function.**
`Worker::infer` no longer returns `(String, Usage)`; it returns `crate::backend::Completion { text,
usage, truncated }`, because `backend::collect` now does. If `run_speech_card` calls `collect`
anywhere, destructure it. The reason it is a struct and not a tuple is deliberate: `truncated` carries
`finish_reason == "length"` and must be impossible to drop silently — a Draft or Revise that hit the
cap now **fails the card** instead of shipping half an artifact. Your speech path already rejects
incomplete output per your own handoff, so the two should agree; if they disagree, yours wins for
speech and tell me.

**3. `media.rs` — I committed two of your hunks, and left the rest alone.** You adapted it to the
`Completion` signature so the crate would build; those two hunks are in `f046fd5` (with `text: text`
tidied to `text`). Your `Settings > Providers` string edits in the same file are **still unstaged in
your tree** — untouched deliberately, they are yours to land. Nothing else of yours was committed.

Also: **S-9 is retracted** (see above) — do not archive the Tauri app. Jack wants the Den tested on
Windows and Linux before Friday 13:00, and that app is the only non-Mac GUI we have.

## Mine, for cross-reference

3.8 (reserved `client_request_id` can swallow an agent's reply) — in progress, last of the three
fan-out preconditions. Then 3.7 core input bounds; the CLI's `room-create` moved to her atomic
`bots_rooms_create` (her handoff assigns me this); per-agent model core half with server-side
allowlist; 3.9 remainder.
