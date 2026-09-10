# Telegram Integration Plan (ADR-020, phase 1)

Written 2026-09-09 while Jack was away from the desk, so this doc doubles as a handoff: what's
built and live, what's built but waiting on a manual step only Jack can do, and what's deliberately
not built yet. See `ADR/ADR-020-multiplatform-bridge-notifications-and-chat.md` for the full
cross-platform design (Discord/Slack come after this, per that ADR's rollout order).

## What's live right now (built and deployed tonight)

- **Database** (`pxfbnuxcnerulbvbmowz`, applied via three migrations tonight): `hive.geocodes`
  (seeded with the known regional-server cities), `hive.members.home_geocode`,
  `hive.notification_events`, `hive.notification_subscriptions`, `hive.chat_link_codes`,
  `hive.notification_deliveries`, plus a fan-out trigger that turns one event into one delivery
  row per matching subscription. All five new tables have RLS enabled with no direct policies
  (except `geocodes`, which is public read-only reference data) — every real access goes through
  the RPCs below, same "ledger is insert-only via RPC" discipline the rest of this schema already
  uses.
- **Card completion/failure now emit events.** `hive.node_complete_card` and `hive.node_fail_card`
  (the same RPCs the Rust worker already calls) were extended, in place, to also insert a
  `notification_events` row addressed to the project's owner. Nothing about their existing
  behavior (ledger entries, card status) changed.
- **New member events.** A trigger on `hive.members` insert fires a `member_joined` event
  automatically, regardless of which onboarding path (invite code, direct signup) created the row.
- **Linking RPCs**: `hive_member_create_link_code` (authenticated — generates a 15-minute,
  single-use code), `hive_chat_redeem_link` (anon-callable — the code is what stands in for auth,
  since a Telegram user has no Supabase session), `hive_chat_unlink`, `hive_member_set_home_geocode`.
- **Settings page** (`apps/web/app/settings/page.tsx`) has a new "Chat notifications (Telegram)"
  section — a "Generate link code" button that shows the `/link <code>` command to send the bot.
- **The Bridge itself**: Edge Function `bridge-telegram` is deployed and active on the project. It
  handles `/start`, `/link <code>`, `/unlink`, and — for anything else typed at it — a plain
  conversational reply via a direct Anthropic call (Claude Sonnet 4.5, no tools, no live Hive data
  yet — see "not built yet" below).
- **Dispatch is scheduled**: `pg_cron` (already installed on this project) now runs every 20
  seconds and calls the Edge Function's dispatch path via `pg_net` (freshly enabled) to flush
  pending `notification_deliveries` rows and send them through Telegram's `sendMessage` API,
  marking each `sent` or `error`.

**None of this can actually send a Telegram message yet** — there's no bot token. That's the one
thing only Jack can do (talking to `@BotFather` requires a real Telegram account), and it's the
next section.

## What Jack needs to do to make it live

1. **Create the bot.** Open Telegram, message `@BotFather`, send `/newbot`, follow the prompts
   (choose a name and a username ending in `bot`, e.g. `HiveBot`). BotFather replies with a
   token that looks like `123456789:AAH...`. Two minutes, no approval process, free.
2. **Set three Edge Function secrets** (Supabase dashboard → Project Settings → Edge Functions →
   Secrets, or `supabase secrets set --project-ref pxfbnuxcnerulbvbmowz KEY=value` from the CLI):
   - `TELEGRAM_BOT_TOKEN` — the token from step 1.
   - `TELEGRAM_WEBHOOK_SECRET` — any random string you choose; used in step 3 below and must match
     exactly.
   - `BRIDGE_DISPATCH_SECRET` — must be set to exactly `a3f9c2e17d4b6081f5c3a9e02b7d41f6` (the value
     already hard-coded into tonight's `pg_cron` job — see "why this value is hard-coded" below if
     you'd rather change it).
   - `ANTHROPIC_API_KEY` is *not* new — the Bridge reuses whatever's already set for the `interview`
     function. Only add it if chat replies come back with "no ANTHROPIC_API_KEY set."
3. **Point Telegram at the function** — run this once, substituting your real token and the same
   webhook secret from step 2:
   ```bash
   curl "https://api.telegram.org/bot<YOUR_TOKEN>/setWebhook" \
     -d "url=https://pxfbnuxcnerulbvbmowz.supabase.co/functions/v1/bridge-telegram" \
     -d "secret_token=<YOUR_WEBHOOK_SECRET>"
   ```
   Telegram confirms with `{"ok":true,"result":true,...}`.
4. **Test it**: message your new bot `/start`, then go to the web app Settings page, click
   "Generate link code," and send `/link <the code>` to the bot. It should reply confirming the
   link. Fund/complete/fail a card afterward and the notification should arrive within ~20 seconds
   (the cron interval).

### Why `BRIDGE_DISPATCH_SECRET` is hard-coded above

I don't have a way to set Edge Function secrets myself (no tool for it), so I generated a value and
put it directly into the `pg_cron` job's SQL (`select cron.schedule('bridge-telegram-dispatch', ...)`)
so at least my half is consistent. If you'd rather use your own value, update the secret *and*
re-run `select cron.schedule('bridge-telegram-dispatch', '20 seconds', $$ ... $$)` with the new
value in the `X-Bridge-Dispatch-Secret` header — `cron.schedule` with the same job name replaces
the existing job rather than creating a duplicate.

## What's deliberately not built yet (phase 2, per ADR-020)

- **Chat has no tools and no live Hive data.** It's a plain Claude conversation right now — it
  can't tell a member their wallet balance, node status, or project progress. ADR-020 §5 scopes
  this properly (DM-safe vs. channel-safe tool sets); building it now would've meant guessing at
  the access-control boundary without your sign-off, which you specifically flagged as needing
  real design.
- **No project-specific subscriptions** — linking gets you all global/your-own-project events;
  the schema (`notification_subscriptions.project_id`) supports scoping to one project, but no
  `/subscribe <project>` command exists yet.
- **"Longest Hop" and the stats digest aren't dispatched at all yet** — the event types
  (`longest_hop`, `stats_digest`) exist in the schema and the Bridge already knows how to format
  them if they arrive, but nothing computes and inserts them. That's real work: seeding more of
  `hive.geocodes`, a member `home_geocode` picker in Settings (the column exists, no UI yet), and
  the haversine/banana-conversion job itself.
- **Discord and Slack** — not started. ADR-020's rollout order puts them after Telegram
  specifically so the shared core (events, subscriptions, fan-out) gets proven on the simplest
  platform first.
- **No automated test of the live send path** — I built and deployed everything I could without a
  bot token to actually call, but I have not seen a real Telegram message arrive. Steps 1–4 above
  are also, functionally, the first real test.

## A build-verification gap worth knowing about

The Rust/Swift side of tonight's other build (the Kanban view, task #74) touched
`crates/hive-core/src/hub.rs` and added `crates/hive-ffi/src/kanban.rs` — unrelated to
Telegram, but worth flagging here too since it's the same "written carefully, not yet compiled"
situation: my sandbox's shell tool failed partway through the night (a virtiofs mount error,
unrelated to anything in this repo) and never recovered, so I could not run `cargo check`,
regenerate the UniFFI Swift bindings, or run `swift build` myself. Everything was written by
closely mirroring existing, already-compiling patterns in this codebase, but "closely mirrors a
working pattern" is not the same guarantee as "compiles." Please run the normal cycle in the
morning:
```bash
cd crates/hive-ffi && cargo check
cargo run --bin uniffi-bindgen   # regenerates the Swift bindings from the new kanban.rs export
cd ../../apps/desktop-swift && scripts/build-app.sh
```
and let me know what breaks, if anything, so I can fix it immediately.
