-- Hive — Personal Hive fleet channel (ADR-022 S2). Jack, 2026-09-13, deciding between a single
-- chat thread and Buzz's (Block) channel model for a member's own fleet: "I want to be able to
-- see the receipts, so I think we take Buzz's channel model and run with it." A chat thread's
-- "the assistant tells you what happened, from memory" is a summary that can be wrong or
-- incomplete; a channel where every event is its own durable, timestamped row is an actual audit
-- trail, independent of any model's summarization.
--
-- Four follow-on decisions (also Jack, same conversation) shape this schema directly:
--   1. Post volume: everything. No significance filtering in v1 -- every card/job lifecycle
--      event, worker/server start-stop, and pairing event becomes a post once callers are wired
--      up (see the follow-on migration this one enables -- schema only here, no lifecycle RPCs
--      touched in this migration).
--   2. Channel scope: one fleet-wide channel per member, not one per machine -- "can we get both,
--      then you pick which view you prefer" is satisfied by tagging each post with which node (if
--      any) produced it and filtering client-side/query-side, not by a second table or a second
--      channel concept.
--   3. Additive, not a replacement: the existing local-only Swift `HiveStore.activity` feed keeps
--      working exactly as it does today (cheap, on-device, no hub round-trip) -- this is the new
--      shared, cross-machine, cross-session record sitting alongside it, not instead of it.
--   4. Web app first: this migration's RPCs are what apps/web's new fleet-channel page (adapting
--      the existing project-forum comment-thread pattern, hive.project_comments) will call.
--
-- Distinct from hive.project_comments (2026-09-08) in the one way that matters here: a project's
-- forum is member-authored only (`author_id references hive.members`) since it's a community
-- discussion space. A Personal Hive channel needs a **non-human author** too -- a paired node
-- posting its own activity -- so posts are tagged by `node_id` (null = a member-authored message
-- or, later, an assistant reply) and `author_kind`, not by a single member-only author column.
--
-- Scope of this migration: the table, RLS, and the member-facing list/post RPCs only. The
-- node-authored side is deliberately just the internal `hive.personal_channel_post_core` helper
-- -- every node-originated event this channel will eventually carry (card claimed/completed/
-- failed, node checked in, worker/server started/stopped) already arrives at the hub through an
-- existing node-key-authenticated SECURITY DEFINER function; wiring each of those to also call
-- this core helper is its own follow-on pass (many existing functions to touch carefully, not a
-- bulk find-replace), tracked separately rather than rushed into this schema migration.

create table if not exists hive.personal_channel_posts (
  id           uuid primary key default gen_random_uuid(),
  member_id    uuid not null references hive.members(id) on delete cascade,
  -- Which paired machine produced this post; null for a member-typed message or (later) an
  -- assistant reply not tied to a specific node.
  node_id      uuid references hive.nodes(id) on delete set null,
  author_kind  text not null check (author_kind in ('member', 'node', 'assistant')),
  -- 'message' for anything a member or assistant typed; a lifecycle event name
  -- ('card_claimed'/'card_completed'/'card_failed'/'worker_started'/'worker_stopped'/
  -- 'server_started'/'server_stopped'/'node_paired'/...) for automatic node posts. Free text
  -- rather than an enum -- the follow-on wiring pass will add event types incrementally and
  -- shouldn't need a migration each time.
  event_type   text not null default 'message',
  body         text not null check (char_length(body) <= 4000),
  -- Structured detail for future UI drill-down (project_id, card_id, duration, etc.) -- optional,
  -- never required to render a post's body text.
  payload      jsonb not null default '{}'::jsonb,
  created_at   timestamptz not null default now()
);
create index if not exists personal_channel_posts_member_idx on hive.personal_channel_posts (member_id, created_at desc);
create index if not exists personal_channel_posts_node_idx on hive.personal_channel_posts (member_id, node_id, created_at desc);
alter table hive.personal_channel_posts enable row level security;
-- Same "no direct client access, everything through SECURITY DEFINER RPCs" discipline as
-- hive.project_comments -- RLS here is defense-in-depth, not the primary access control.
drop policy if exists personal_channel_posts_no_direct_access on hive.personal_channel_posts;
create policy personal_channel_posts_no_direct_access on hive.personal_channel_posts for all to authenticated using (false);

-- Internal helper -- no auth check of its own, callable from any other trusted SECURITY DEFINER
-- hive.* function in the same transaction (the follow-on lifecycle-wiring pass calls this
-- directly from inside node_checkin/complete_card/server_start/etc., since those already resolve
-- the owning member via the node key before this would ever run). Not exposed as a public/
-- service-role RPC in this migration -- add one only if a genuinely external caller (an Edge
-- Function, a direct node-key RPC) turns out to need it, which the lifecycle-wiring pass will
-- determine.
create or replace function hive.personal_channel_post_core(
  p_member uuid, p_node_id uuid, p_author_kind text, p_event_type text, p_body text, p_payload jsonb default '{}'::jsonb
) returns hive.personal_channel_posts
language plpgsql security definer set search_path = hive, public as $$
declare row hive.personal_channel_posts; begin
  if p_author_kind not in ('member', 'node', 'assistant') then raise exception 'invalid_author_kind'; end if;
  insert into hive.personal_channel_posts (member_id, node_id, author_kind, event_type, body, payload)
  values (p_member, p_node_id, p_author_kind, coalesce(nullif(trim(p_event_type), ''), 'message'), trim(p_body), coalesce(p_payload, '{}'::jsonb))
  returning * into row;
  return row;
end $$;
revoke all on function hive.personal_channel_post_core(uuid, uuid, text, text, text, jsonb) from public;

-- Member: post a plain message into your own channel (author_kind='message', no node).
create or replace function hive.personal_channel_post(p_body text) returns hive.personal_channel_posts
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if trim(coalesce(p_body, '')) = '' then raise exception 'empty_post'; end if;
  return hive.personal_channel_post_core(auth.uid(), null, 'member', 'message', p_body);
end $$;
grant execute on function hive.personal_channel_post(text) to authenticated;
create or replace function public.hive_personal_channel_post(p_body text) returns hive.personal_channel_posts
language sql security definer set search_path = hive, public as $$ select hive.personal_channel_post(p_body); $$;
grant execute on function public.hive_personal_channel_post(text) to authenticated;

-- Member: read your own channel, optionally filtered to one node -- "one fleet-wide channel,
-- filterable per node" (decision 2 above). `node_display` is joined in so the web UI never has
-- to make a second round-trip just to label which machine posted what.
create or replace function hive.personal_channel_list(p_node_id uuid default null, p_limit integer default 200) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', c.id,
    'node_id', c.node_id,
    'node_display', n.display_name,
    'author_kind', c.author_kind,
    'event_type', c.event_type,
    'body', c.body,
    'payload', c.payload,
    'created_at', c.created_at
  ) order by c.created_at desc), '[]'::jsonb)
  from (
    select * from hive.personal_channel_posts
    where member_id = auth.uid() and (p_node_id is null or node_id = p_node_id)
    order by created_at desc
    limit greatest(1, least(coalesce(p_limit, 200), 500))
  ) c
  left join hive.nodes n on n.id = c.node_id
  where hive.is_member();
$$;
grant execute on function hive.personal_channel_list(uuid, integer) to authenticated;
create or replace function public.hive_personal_channel_list(p_node_id uuid default null, p_limit integer default 200) returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.personal_channel_list(p_node_id, p_limit); $$;
grant execute on function public.hive_personal_channel_list(uuid, integer) to authenticated;
