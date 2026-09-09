# ADR-020: Multi-Platform Bridge — Notifications, Subscriptions, and Conversational Chat (Telegram/Discord/Slack)

**Status:** Proposed · **Date:** 2026-09-09 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** `docs/Good Idea Fairy.md` ("we need Telegram and Discord connectors and Slack as well... asap"), followed by Jack's chat scoping: job completion/failure, longest hops, statistics, new-member announcements, per-project subscriptions, and free-form chat "like Hermes" (an external analogy — confirmed via repo search that "Hermes" names no real OH Hive system; safe to build against without conflict). "Longest hops" refined once more by Jack: this is a brand/community moment for Office Hours Global — the actual **geographic** distance between two real points on Earth involved in a day's work (his example: Jack in his own city connecting to a project a node in Sydney is executing), reported for scale in **bananas**, in the spirit of "banana for scale."

## Context

Nothing here exists today. Repo research turned up no bot, webhook, or subscription infrastructure to reconcile with:

- Card lifecycle events (`WorkerEvent::Completed`/`Failed`/etc., `crates/ohhive-core/src/worker.rs`) are broadcast **in-process only**, over a `tokio::sync::broadcast::Sender`, with no persistence and no external delivery path.
- Supabase Realtime was explicitly dropped from this project (`supabase/migrations/20260905000011_realtime_drop.sql`) — whatever delivers events to the outside world has to be built without it.
- No "hop" or routing-latency metric exists anywhere (no `coordinator.rs`, nothing in ADR-013's cost/capacity model). "Longest hops" is a new metric this ADR has to define, not one it can just wire up.
- No per-project subscription/notification-preference concept exists in the web app or schema.
- The closest existing pattern for "surface a lifecycle event to a person" is the Swift app's local-only `HiveStore.activity` feed (`apps/desktop-swift/Sources/OHHive/HiveStore.swift`) — useful as a UX reference, not reusable infrastructure, since it only runs inside one member's own app.
- The closest existing pattern for "call an LLM provider from server-side code with fallback" is the interview Edge Function (`supabase/functions/interview/index.ts`): Deno/TypeScript, Anthropic primary via `ANTHROPIC_API_KEY`, falls back to the member's own OpenAI key, then to the Nous Portal's OpenAI-compatible endpoint. This is the template this ADR's chat backend follows.

Two genuinely different capabilities are being asked for together, and this ADR treats them as one system with two faces rather than two separate builds, because they share almost everything below the platform adapters: **(1)** outbound notifications a member didn't ask for in the moment (job done, job failed, new member, periodic stats) and **(2)** inbound conversational chat a member starts on demand. Building one delivery/adapter layer for both, per the "one core, not duplicated" discipline this codebase already applies elsewhere (ADR-018 decision 2).

## Decision

### 1. One Bridge, three thin adapters

A single Supabase Edge Function project, `bridge`, handles all three platforms. Telegram, Discord, and Slack all support webhook-based delivery (Telegram's `setWebhook`, Discord's Interactions endpoint, Slack's Events API) — so the whole thing can run as stateless HTTP handlers, no always-on process, no dependency on any one member's machine or Cloudflare Tunnel (that automation is for a *member's own* regional server, per ADR-013 D74 — this bridge belongs to the Hive's own Supabase project, not a member's node). Each platform gets a thin adapter (`bridge/telegram.ts`, `bridge/discord.ts`, `bridge/slack.ts`) that only handles that platform's payload shape and formatting quirks (Telegram Markdown, Discord embeds, Slack Block Kit); everything else — event fan-out, subscription lookups, the chat/tool-calling core — is shared code underneath, written once.

**Rollout order: Telegram first, then Discord, then Slack**, matching the earlier easiest-to-ship analysis (Telegram needs zero approval and zero public-endpoint ceremony beyond the webhook itself; Discord is nearly as simple below the 100-server verification threshold; Slack's Events API is the most involved to wire up correctly, though not gated by any approval for installing into your own workspace).

### 2. Making card events durable and externally visible

`WorkerEvent` stays as the in-process signal it already is — this ADR doesn't touch `worker.rs`'s broadcast channel. Instead, the RPCs that already exist for lifecycle transitions (`hub.rs`'s `complete_card`, `fail_card`) are extended to also insert a row into a new `hive.notification_events` table (`id`, `event_type` — `card_completed`/`card_failed`/`member_joined`/`stats_digest`, `project_id`, `card_id` nullable, `payload` jsonb, `created_at`). This is the same discipline ADR-019 used for `card_verifications`: a durable, queryable table, not another broadcast that only lives as long as a process does.

Delivery from that table to the Bridge uses **Supabase Database Webhooks** (Postgres triggers that call an HTTP endpoint on insert) rather than Realtime, since Realtime is off. This needs a one-time verification step against the actual `pxfbnuxcnerulbvbmowz` project settings before build starts — flagged as an open question below rather than assumed, since I haven't confirmed Database Webhooks are enabled there.

### 3. Subscriptions: linked accounts, not OAuth-per-platform

New table `hive.notification_subscriptions` (`member_id`, `project_id` nullable — null means "everything I'm entitled to see", `channel` — `telegram`/`discord`/`slack`, `external_chat_id`, `event_types` — array of the `notification_events.event_type` values they want). A member links a chat account to their Hive membership with a short-lived linking code generated from their web app Settings page (`apps/web/app/settings`) and redeemed by DMing the bot `/link <code>` — this avoids building OAuth against three different platforms just to answer "which Hive member is this chat account." Project-scoped subscriptions only fire for cards belonging to that project; global ones (new member, periodic stats digest) go to whoever's subscribed with `project_id = null`.

### 4. Defining "longest hops": real-world geographic distance, reported in bananas

Confirmed with Jack: this is not a technical routing metric — it's a brand/community moment celebrating how genuinely global Office Hours is. A "hop" is the great-circle distance between two real, physical points on Earth involved in the same piece of work on a given day (his example: Jack, wherever he physically is, opening/connecting to a project whose card is executing on a node in Sydney). Nothing in the repo geocodes anything today — `region` (`nodeconfig.rs`, `hive.nodes.region`) is a free-text operator-typed label used only for equality checks in the replication-diversity logic (`20260907000030_region_aware_replication.sql`), never for geometry — so this whole layer is new:

- **New static table `hive.region_geocodes`** (`region` text primary key, `city`, `country`, `lat`, `lon` numeric). Seeded by hand for every region string actually in use (the known regional servers — Amsterdam, Chicago, Sydney, per ADR-013/ADR-017 — plus whatever hosting-provider cities show up as new regions get added). Not automatic geocoding, since `region` strings are free-typed by whoever configures a node; a small admin-maintained lookup is simpler and more reliable than trying to parse arbitrary strings into coordinates.
- **New optional member field**, `hive.members.home_city` (or similar) plus a `home_region_geocode` reference, self-reported once in `apps/web/app/settings` ("where are you joining from?") — coarse, city-level, and opt-in, deliberately **not** IP-based geolocation. No location concept exists for members today (confirmed — only nodes/servers have a `region` string), and IP geolocation would be a real privacy step this ADR doesn't take without it being an explicit, visible choice the member made. A member who skips this just doesn't participate in hop-of-the-day candidacy.
- **Hop candidate events**: once both ends of an interaction have a geocode — a member's `home_city` and the region of the node executing a card they're connected to/watching, or two regions in the same delegation tree (a parent card's node and a child card's node from `spawn_child_card`) — compute the great-circle (haversine) distance between them. Every such pair recorded that day is a candidate; the day's maximum is "Longest Hop of the Day," posted in the stats digest (§8-ish, the periodic notification event).
- **Bananas for scale**: distance is reported in a whimsical banana-length unit alongside real units (km/mi), using a fixed constant — proposed default **1 banana = 7.5 inches** (a reasonable average; happy to use a different "official OH banana" length if one already exists somewhere in the brand — flagged as an open question rather than assumed). A hop like Jack's-city-to-Sydney comes out in the millions of bananas, which is exactly the fun, shareable number this feature is for.

### 5. Chat: same tool-calling shape as the on-device engine, different scope and backend

The conversational side mirrors `ChatEngine.swift`'s `Tool`-based pattern (ADR-015/018) structurally — a small set of named tools the model can call, kept to short, plain answers rather than open-ended chat — but runs server-side against the interview function's provider-fallback chain (Anthropic primary → member's own key → Nous) instead of on-device Foundation Models, because a Telegram/Discord/Slack bot has no "this Mac" to be local to; it needs live Hive-wide data. Initial tool set: `projectStatus(project_id)`, `memberSummary()` (a member's own wallet/nodes/subscriptions), `fleetHealth()` (aggregate, no per-member sensitive detail to strangers in a shared server channel), and `recentActivity(project_id)`. Chat is scoped per-platform-context: in a DM, the bot knows who's talking (via the same linked-account table as §3) and can answer member-specific questions; in a shared server/channel, it only answers with data safe for a shared audience (project status a member has already made visible on the community board, not another member's wallet balance).

## Consequences

### Positive
- One shared core (event fan-out, subscription matching, tool-calling chat) serving three platforms — no tripled logic, same discipline as ADR-018's core-in-`ohhive-core` pattern applied to Edge Functions instead of Rust crates.
- Durable `notification_events`/`notification_subscriptions` tables mean the bot's behavior is auditable and replayable, not just "whatever the bot happened to send" — if delivery to Slack fails, the event still exists to retry or backfill.
- Telegram-first rollout gets something real in front of Jack fastest, with the shared core already built for the two harder platforms to slot into.
- Chat reuses proven shapes from two other places in this codebase (`ChatEngine.swift`'s tool pattern, the interview function's provider fallback) instead of inventing a third pattern.

### Negative
- A new always-growing `notification_events` table with no retention policy defined here (same open pattern as ADR-019's `card_verifications` — likely the same eventual answer, ADR-013's archival approach, not decided now).
- Chat backend cost: every chat message becomes an LLM call against the provider-budget path, on top of interviewer spend and ADR-019's coordinator-agent spend — a third consumer of the same budget system with no combined view of total spend yet (open question below).
- The account-linking flow (`/link <code>`) is new UX surface across three platforms and the web app Settings page that doesn't exist yet and needs real design, not just backend plumbing.
- Shared-channel chat answering safely (not leaking one member's private data into a public Discord/Slack channel) is a real access-control surface this ADR only sketches (§5) — needs careful scoping before build, not left implicit.

### Risks & mitigations
- **Spam/abuse of the chat feature** (someone hammering the bot with messages to run up provider spend). Mitigation: a simple per-linked-member rate limit on chat messages, mirroring how card leasing already rate-limits via lease timeouts — exact numbers TBD at build time, not decided here.
- **Database Webhooks turning out to be unavailable or disabled** on this Supabase project. Mitigation: fallback is a lightweight polling Edge Function on a cron schedule reading `notification_events` for unsent rows — slightly higher latency, same durability guarantee, no architecture change needed if webhooks aren't viable.
- **A member's `home_city` revealing more than they intended in a shared channel.** Mitigation: hop-of-the-day posts show city-level distance and the fun banana number, never a member's precise identity tied to sensitive detail beyond what they already made visible by setting the field — and it's opt-in exactly so a member who doesn't want their city known publicly simply never sets it.
- **Chat leaking member-specific data into a public Discord/Slack channel** (the guardrail Jack explicitly flagged as needed). Mitigation: the tool set in §5 is split into "DM-safe" (wallet, own nodes, own subscriptions) and "channel-safe" (a project's already-public board status, aggregate fleet health, hop-of-the-day/stats digest content) at the tool-definition level, not as an afterthought filter on the response text — a channel-context chat session simply never has the member-specific tools available to call in the first place.

## Open questions
- Confirm whether Supabase Database Webhooks are enabled/available on project `pxfbnuxcnerulbvbmowz`, or whether the polling fallback (§/Risks) is actually the v1 path.
- Is there already an "official OH banana" length used elsewhere in the brand, or is the proposed 7.5-inch default fine to standardize on?
- Chat provider spend: its own budget line, shared with interviewer spend, shared with ADR-019's coordinator-agent spend, or one combined "AI features" line covering all three? No combined spend view exists today across interview/coordinator/chat — worth deciding once, not three separate times across three ADRs.
- Retention/archival for `notification_events` — deferred to whenever real volume exists, same discipline as ADR-019.
- Should new-member announcements and stats digests be opt-out (everyone subscribed by default to global events) or opt-in (silent until a member links and subscribes)? Default proposed: opt-in, consistent with not messaging someone who never asked to be on a chat platform's radar at all.
- What exactly counts as a "hop candidate" beyond Jack's member-connects-to-a-Sydney-node example — is a parent/child card pair spanning two regional servers (no member involved at all) also a valid Longest Hop moment, or does it always need a member's own `home_city` on one end? Default proposed: both count, since either makes for a good global-reach story, but worth Jack's explicit yes.

## Related
- ADR-013-cost-capacity-and-hosting (D74 Cloudflare Tunnel — a member's own regional server, explicitly not what this bridge runs on; also the likely template for `notification_events` retention)
- ADR-015 / ADR-018-native-macos-swift-shell (the `Tool`-based chat pattern this ADR's chat backend mirrors structurally, on a different backend)
- ADR-019-triangulated-card-verification (same "durable queryable table, not a transient broadcast" discipline applied here to `notification_events`; a second concurrent consumer of provider budget alongside this ADR's chat spend)
- ADR-006-agent-runtime-and-sandbox (the Interviewer Contract terminology and the interview Edge Function's provider-fallback shape this ADR's chat backend follows)
