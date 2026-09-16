# Audit triage, and Sif's next queue

Loki, 2026-09-16. Triage of `SUPER-LOKI-FABLE-AGENT-DEN-CODE-AUDIT-2026-09-15.md` (16 prioritized
items, 200 lines). Split by the ownership rules in `LOKI-SIF-SPLIT-AND-QUEUE-2026-09-16.md`.

## Already closed — don't re-do these

- **§3.3 [P0, the demo blocker]** — `agent_profiles.runtime_kind` CHECK excluded `anthropic_byok`
  and `nous_byok`, so BYOK agents could not be created *at all* and opening Bots with an Anthropic
  key failed every refresh. **Sif's schema v12** (`bots_provider_schema.sql`) fixes it. This was
  the single most demo-relevant finding and it is done.
- **§3.9 "System messages still create deliveries"** — landed in the Track A merge.
- **§3.9 "no causation/depth/Held columns"** — landed as schema v11.

## Done today by Loki

- **§3.4 [P1]** — `bots_messages_list` returned the *oldest* 32 messages for a `before` page, so
  every agent in a conversation longer than the window saw the opening of the conversation and
  never the thread it was replying to. Fixed, with a 40-message test. This was the worst remaining
  defect for reply quality and it affected app scroll-back paging too.
- **§3.9 agent broadcast** — `resolve_mentions` granted `@everyone` to agent authors. Only the
  executor's width cap was enforcing the design's asymmetry, and it truncates to an arbitrary two
  by roster order, which reads as the agent having chosen them. Now refused outright.

## Loki's remaining queue, in order

1. **Wire Sif's cloud runner (her S-A handoff).** `CloudTurnRunner` and the `bots-turn` function
   are built; the executor integration is mine. Makes Claude and Nous actually able to reply, which
   is the whole point of the room. Includes updating the route notices so a *supported* cloud agent
   is no longer labelled unsupported, and respecting primary routing so two hosts cannot both run
   one turn.
2. **§3.5 [P1] delivery finish is generation-fenced but not status-fenced**, and cancel does not
   bump the generation — so cancel → NoCapacity → fail writes `status='pending'` and a cancelled
   delivery gets re-claimed. The audit names this as the mechanism the `Held` human gate relies on,
   so slice 4's gate is currently sitting on a fence with a hole in it. Highest of the three
   preconditions.
3. **§3.6 [P1] RPC actor is caller-chosen** — any same-owner paired device can post *as any of the
   owner's agents*, creating agent-authored messages with recipients **outside the executor**, i.e.
   bypassing where every budget check lives. Must close before fan-out is enabled.
4. **§3.8 [P1] `client_request_id` is not scoped by author** — a member who posts any text using the
   executor's reserved `delivery:<msg>:<agent>` key makes the agent's later reply "succeed" by
   returning the human's message. Delivery marked done, agent never replies, no error anywhere.
5. **§3.7 [P1] core storage has no input bounds** — the 64 KiB / 16-recipient limits live only in
   FFI, while transport accepts 8 MiB. Move them into `LocalHubStore`.
6. **§3.9 remainder** — `history_boundary` never enforced (a late-joining agent sees pre-join
   history), missing hot-path indexes, `bots_message_send`'s 5+N transactions.

## S-0 — the app crashes on launch whenever the Rust side is rebuilt without it

Jack hit this at 20:19 on 2026-09-15: `Hive.app` died at launch with `EXC_BREAKPOINT` in
`makeRustCall` -> `_assertionFailure`, from `HiveNode()` in `HiveStore.init()` (HiveStore.swift:43).

Not a code defect. `HiveNode::new()` is infallible -- it returns `Self`, not `Result`, and only
builds mutexes -- so a trap there is UniFFI's contract check failing, i.e. the Swift bindings the
app was compiled against no longer match the dylib it loads. The mtimes say exactly that:

| artifact | built |
|---|---|
| `Hive.app/Contents/MacOS/Hive` | **16:57** |
| `target/aarch64-apple-darwin/release/libohhive_ffi.dylib` | **19:41** |
| `Sources/OHHiveFFI/ohhive_ffi.swift` | **19:43** |

The Rust side was rebuilt and the bindings regenerated 2.5 hours after the app binary, and the app
was never rebuilt. Rebuilding it (`apps/desktop-swift/scripts/build-app.sh`) fixes it immediately;
the database was never the problem, and never got touched -- the real `vault-host.sqlite3` is still
at v9, and opening a copy of it with current source migrates 9 -> 13 cleanly.

**Why it deserves a queue item rather than a shrug:** the app bundle carries no dylib of its own and
loads the repo's build product by path (your own note: "still uses the repository's FFI library
path... not a portable distribution package"). So *any* `cargo build` of the FFI between app builds
turns a working install into a launch crash, with a stack trace that points at `HiveStore.init()`
and looks like a real bug. We will both keep doing that, and so will every future contributor.

Two fixes, either acceptable:
1. **Copy the dylib into `Hive.app/Contents/Frameworks/` at assembly time** and link it with
   `@rpath`, so the bundle is self-contained and cannot drift. This is the real fix.
2. At minimum, have `build-app.sh` rebuild the Rust target and regenerate bindings itself, so the
   three artifacts can never be produced out of order by hand.

Worth doing alongside: a readable failure. A version skew currently presents as SIGTRAP with no log
line -- the app dies before logging initializes, so `desktop.log` had nothing after 13:04 while the
crash was at 20:19. Even a `fatalError` with "FFI bindings are older than libohhive_ffi.dylib --
rebuild the app" would have turned a twenty-minute investigation into a five-second one.

## Sif's queue

Ordered so two very small items land first and the account-takeover exposure closes today.

**S-1 — §4.3 `node_fail_card` lets any node key block any card. XS, one line.**
`20260905000004:139-148` deletes a lease that may match nothing, then unconditionally sets
`status='blocked'` and writes a `FAILED:` output; the cascade trigger then blocks parents. The
wrapper is granted to `anon, authenticated`. Add the `if not found then raise` that
`node_release_card` already has.

**S-2 — §6.1 the shipped `hive-server` is not the configuration CI checks. XS.**
`release.yml:37,39` builds `-p hive -p hive-server` in one cargo invocation, so resolver-2 feature
unification links the released server against a `hive-core` built with wasmtime, sysinfo and the
inference backends — while `hive-server/Cargo.toml` says "No inference backends, ever." and the
README claims CI enforces it. Split into two invocations and add a `cargo tree | grep -E
'wasmtime|sysinfo|rusqlite|openssl' && exit 1` gate to the footprint job.

**S-3 — §3.1 [P0] members' Supabase JWTs are handed to volunteer regional servers.**
Interim first, and today: `apps/web/lib/live.ts` `pickServer` and `projects/page.tsx`
`loadOverview` filter to `operator === 'hjm'` — the field is already returned, so it is a couple of
lines. That closes the exposure while the real fix is built.
Then the real fix: `hive.live_token_mint(p_project_id)` returning a short-lived (≤5 min)
project-scoped HMAC, plus a node-key-gated `hive.project_board_for(...)` that applies
`project_visible`, so a server never holds a member bearer token at all.
Why it matters: `hive.servers()` orders by `region, display_name`, any member can self-register the
`regional_server` role at `/pair`, and the server uses the token as a full bearer — so a volunteer
named "AAA" can call `hive_fund_project`, `hive_member_key_set/remove` and `hive_invite_create` as
any member who opens a board. **This is the most severe finding in the report.** It is only latent
because every server today is HJM's, which is exactly why the interim filter is worth two lines.

**S-4 — §3.2 [P0] `/live/<pid>` authenticates nobody.**
`live.rs:147` checks only `token.len() < 20`. Joining replays the last board frame *and* overwrites
the room's polling JWT, so a 20-character junk token reads any project that has a legitimate
viewer and then poisons the room until everyone reconnects. Validate on join before incrementing
the subscriber count, replacing the JWT, or replaying `last`; per-subscriber visibility check; cap
rooms per server; stop a poller after K consecutive errors.

**S-5 — §4.5 table grants and `SECURITY DEFINER` functions were never revoked from PUBLIC.**
One migration. 100+ `hive.*` functions are PUBLIC-executable including
`member_key_set_for/remove_for` (overwrite or delete another member's BYOK key), `settle_storage`,
`retire_old_backups(0)`. Blanket `insert, update` on `members` with no column restriction lets
anyone set `is_admin = true`. `hive.balances` has no RLS. ADR-001 says the schema will be exposed
by default — the day that flips, all of it is live. Test by replaying into pglite, `set role
authenticated`, and asserting `update hive.members set is_admin = true` fails.

**S-6 — §4.1 no row locks around any balance check.** `fund_project` and `node_complete_card` read
a balance then post without `FOR UPDATE`, so two concurrent calls both pass and the nightly
integrity job only reports it afterwards. `perform 1 from hive.accounts where id = wallet for
update;` at the top of every debiting function, and lock the lease row in complete/checkpoint/
release.

**S-7 — §4.4 private projects leak, and two unanchored URL regexes.** `hive.snapshot_source`
returns every non-deleted project, and the web board prefers it over `hive_projects_overview`, so
`execution_mode='local'` projects are visible to all members — this silently undoes the
2026-09-13 visibility fix. Add `and p.execution_mode = 'hive'`. Separately,
`bug_report_add_attachment` and `custom_avatar_url` check ownership with an **unanchored** regex, so
a `javascript:` URL passes and is rendered into `<a href>` and `<img src>` (the latter polled for
every member every 20 s). Anchor both.

**S-8 — §4.2 honey can be minted from the treasury, and per-card payout is uncapped.** Storage
settlement pays on self-reported bytes for project-null replicas and the treasury is excluded from
the negative-balance check; the interview fund tops itself from the treasury and the member
reimbursement can simply never be called; `node_complete_card` caps payout only by fund balance, so
one completion can drain a whole fund. Fine while every node is trusted, not fine after.

**S-9 — §6.4 archive the Tauri app.** **RETRACTED 2026-09-16 — see the retraction note in
`LOKI-SIF-QUEUE-2-2026-09-16.md`. ADR-018 decision 1 keeps the Tauri app as the Windows/Linux
shell; only its *macOS* build is retired. Do not archive it, do not drop it from the workspace or
the release matrix.** The per-tick cost below is still a real bug and still worth fixing in place.
~~CI excludes it, the Swift shell covers its features, and its
uncommitted `bots.rs` would run a hub write plus a `LocalHubStore::open` (which flips every vault
`unavailable`) on every 2-second poll. Move to `archive/`, drop from workspace members and the
release matrix, correct the README layout section.~~

**Later, still hers:** §5.1 (node/vault keys exported into the process env and inherited by
children, including agent-run commands — the FFI half is hers, `core/tunnel.rs` is contended),
§5.6/§5.7 (both chat engines recreate `LanguageModelSession` every send so the model never sees
prior turns; chat history is wiped on the first decode failure), §4.6 (migrations do not replay;
baseline the schema and add a CI replay job), §4.7 (two contracts silently lost to
`create or replace`: the local-mode no-ledger rule and the 50-honey welcome grant).

## Not in either queue yet

§5.2 (a coordinator brain can mint child cards bypassing every `code_session_create` check),
§5.3/§5.4 (no-deadline paths, and `write_file` silently truncating to empty above ~1 K tokens).
Both are real P1s in the card/coder path, which is neither of our current threads. They want a
deliberate session, not a squeeze into Track A.
