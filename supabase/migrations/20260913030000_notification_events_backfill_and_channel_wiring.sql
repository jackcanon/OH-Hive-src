-- Hive — backfill hive.notification_events into version control, then wire it into the Personal
-- Hive channel (ADR-022 S2/#182).
--
-- Drift found while starting #182 (wiring real events into the new personal_channel_posts table,
-- 20260913020000): `hive.notification_events` -- the durable event table ADR-020 (multi-platform
-- bridge) proposed -- already exists live on pxfbnuxcnerulbvbmowz, and `hive.node_complete_card`,
-- `hive.node_fail_card`, and `hive.notify_member_joined` already insert into it (`card_completed`/
-- `card_failed`/`member_joined`), but NONE of that has a migration file anywhere in this repo --
-- confirmed by grepping every migration for "notification_events" (zero hits) and then reading
-- the live function definitions directly via `pg_get_functiondef`. Someone applied this by hand
-- (or a prior session's work never got committed) sometime before this one. This migration is
-- `create table/function if not exists`/`create or replace`, safe to run against the project that
-- already has these objects (true today) and correct if ever replayed against a fresh one (not
-- true today, which is the actual risk this migration closes) -- table/constraint/index shapes
-- below were read directly off the live objects via information_schema/pg_constraint/pg_indexes/
-- pg_trigger, not reconstructed from ADR-020's proposal text, so they match exactly what's
-- actually running.
--
-- The new work: `hive.personal_channel_post_from_event`, an AFTER INSERT trigger on
-- `notification_events` that forwards any event with a real `member_id` (today: card_completed,
-- card_failed -- member_joined is community-wide and posts with `member_id = null`, so it's
-- naturally excluded, not specially filtered) into that member's Personal Hive channel
-- (20260913020000_personal_channel.sql). This satisfies ADR-022 S2 decision 1 ("post volume:
-- everything") for these two event types with zero changes to node_complete_card/node_fail_card
-- themselves -- much lower-risk than editing those functions again, and it means any future event
-- type added to notification_events (stats_digest/longest_hop are already reserved in the check
-- constraint; more may come from ADR-020's bridge work) flows into the channel automatically as
-- long as it carries a member_id, with no further trigger changes needed.
--
-- node_id resolution: notification_events has no node_id column (it's a member-facing
-- notification record, not a fleet-activity record). `node_complete_card`/`node_fail_card` both
-- insert into `hive.card_outputs (card_id, node_id, ...)` *before* inserting the notification_event
-- in the same transaction and never delete that row, so the trigger looks up the most recent
-- card_outputs row for the same card_id to label which machine the receipt is about. A card with
-- no matching card_outputs row (shouldn't happen for these two event types, but defensive) just
-- posts with `node_id = null` rather than failing the write.

-- 1. Backfill: the table, exactly as it exists live (read via information_schema/pg_constraint/
--    pg_indexes) -- a no-op today, a correctness fix for any future fresh environment.
create table if not exists hive.notification_events (
  id          uuid primary key default gen_random_uuid(),
  event_type  text not null check (event_type in ('card_completed', 'card_failed', 'member_joined', 'stats_digest', 'longest_hop')),
  project_id  uuid references hive.projects(id) on delete set null,
  card_id     uuid references hive.cards(id) on delete set null,
  member_id   uuid references hive.members(id) on delete set null,
  payload     jsonb not null default '{}'::jsonb,
  created_at  timestamptz not null default now()
);
create index if not exists notification_events_created_idx on hive.notification_events (created_at desc);
alter table hive.notification_events enable row level security;
-- Live today with zero policies (equivalent to deny-all under RLS) -- made explicit here to match
-- this codebase's house style of a visible deny-all policy rather than an empty, easy-to-miss set.
drop policy if exists notification_events_no_direct_access on hive.notification_events;
create policy notification_events_no_direct_access on hive.notification_events for all to authenticated using (false);

-- 2. Backfill: the member-joined notifier, exactly as it exists live.
create or replace function hive.notify_member_joined() returns trigger
language plpgsql security definer set search_path = hive, public as $$
declare v_name text; begin
  select display_name into v_name from public.profiles where id = new.id;
  insert into hive.notification_events (event_type, member_id, payload)
  values ('member_joined', null, jsonb_build_object('display_name', coalesce(v_name, 'a new member')));
  return new;
end $$;
drop trigger if exists members_notify_joined on hive.members;
create trigger members_notify_joined after insert on hive.members for each row execute function hive.notify_member_joined();

-- 3. New: forward member-owned notification_events into that member's Personal Hive channel.
create or replace function hive.personal_channel_post_from_event() returns trigger
language plpgsql security definer set search_path = hive, public as $$
declare v_node_id uuid; v_body text; begin
  if new.member_id is null then
    return new; -- community-wide events (member_joined, stats_digest) have no personal channel to post to
  end if;
  select node_id into v_node_id from hive.card_outputs where card_id = new.card_id order by created_at desc limit 1;
  v_body := case new.event_type
    when 'card_completed' then coalesce(new.payload->>'card_title', 'A card') || ' completed'
      || case when new.payload ? 'earned_honey' then ' (earned ' || (new.payload->>'earned_honey') || ' Honey)' else '' end
    when 'card_failed' then coalesce(new.payload->>'card_title', 'A card') || ' failed: ' || coalesce(new.payload->>'reason', 'no reason given')
    else initcap(replace(new.event_type, '_', ' '))
  end;
  perform hive.personal_channel_post_core(new.member_id, v_node_id, 'node', new.event_type, v_body, new.payload);
  return new;
end $$;
drop trigger if exists notification_events_to_personal_channel on hive.notification_events;
create trigger notification_events_to_personal_channel after insert on hive.notification_events
  for each row execute function hive.personal_channel_post_from_event();
