-- Hive — feature request board (Jack, 2026-09-12: members are asking for a place to suggest
-- features, on both the website and the desktop app). Safe to re-run.
--
-- Two submission paths, one core insert: the web app has a real member Supabase session (a JWT,
-- auth.uid() works directly); the Swift/CLI desktop node does not -- it only ever holds a node key
-- (see 20260912190000_hosted_media_generation.sql's header for the full explanation of why). So
-- writes go through hive.feature_request_create_core(p_member, ...), and each surface gets its own
-- thin, correctly-authenticated wrapper: public.hive_feature_request_create (auth.uid(), for the
-- web app) and public.hive_feature_request_create_node (raw node key, for the Swift app), reusing
-- hive.verify_node_key + nodes.member_id exactly like the hosted-media wrappers do.
--
-- Voting is a separate table (one row per member per request) rather than a counter column, so a
-- member can only upvote once and can un-vote, without a trigger to keep a counter in sync.

create table if not exists hive.feature_requests (
  id          uuid primary key default gen_random_uuid(),
  member_id   uuid not null references hive.members(id) on delete cascade,
  title       text not null check (char_length(title) between 3 and 120),
  description text not null default '' check (char_length(description) <= 4000),
  status      text not null default 'open' check (status in ('open','planned','in_progress','shipped','declined')),
  created_at  timestamptz not null default now()
);
create index if not exists feature_requests_created_idx on hive.feature_requests(created_at desc);
alter table hive.feature_requests enable row level security;
drop policy if exists feature_requests_member_read on hive.feature_requests;
create policy feature_requests_member_read on hive.feature_requests for select to authenticated using (hive.is_member());
grant select on hive.feature_requests to authenticated;
-- No insert/update/delete policies -- all writes go through the security-definer functions below.

create table if not exists hive.feature_request_votes (
  request_id uuid not null references hive.feature_requests(id) on delete cascade,
  member_id  uuid not null references hive.members(id) on delete cascade,
  created_at timestamptz not null default now(),
  primary key (request_id, member_id)
);
alter table hive.feature_request_votes enable row level security;
drop policy if exists feature_request_votes_member_read on hive.feature_request_votes;
create policy feature_request_votes_member_read on hive.feature_request_votes for select to authenticated using (hive.is_member());
grant select on hive.feature_request_votes to authenticated;

-- Core insert, shared by both submission paths.
create or replace function hive.feature_request_create_core(p_member uuid, p_title text, p_description text)
returns hive.feature_requests language plpgsql security definer set search_path = hive, public as $$
declare row hive.feature_requests; begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then
    raise exception 'not_a_hive_member';
  end if;
  insert into hive.feature_requests (member_id, title, description)
  values (p_member, trim(p_title), trim(coalesce(p_description, '')))
  returning * into row;
  return row;
end $$;
revoke all on function hive.feature_request_create_core(uuid, text, text) from public;

-- Web app: member has a real session, auth.uid() is trustworthy on its own.
create or replace function hive.feature_request_create(p_title text, p_description text default '')
returns hive.feature_requests language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  return hive.feature_request_create_core(auth.uid(), p_title, p_description);
end $$;
grant execute on function hive.feature_request_create(text, text) to authenticated;
create or replace function public.hive_feature_request_create(p_title text, p_description text default '')
returns hive.feature_requests language sql security definer set search_path = hive, public as $$
  select hive.feature_request_create(p_title, p_description); $$;
grant execute on function public.hive_feature_request_create(text, text) to authenticated;

-- Desktop/CLI node: only a raw node key, no member session -- same shape as node_checkin.
create or replace function public.hive_feature_request_create_node(p_raw_key text, p_title text, p_description text default '')
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; mid uuid; row hive.feature_requests; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  row := hive.feature_request_create_core(mid, p_title, p_description);
  return to_jsonb(row);
end $$;
revoke all on function public.hive_feature_request_create_node(text, text, text) from public;
grant execute on function public.hive_feature_request_create_node(text, text, text) to anon, authenticated;

-- List with vote counts + whether the caller has voted, newest-first within each vote tier.
create or replace function hive.feature_request_list() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object(
      'id', r.id, 'title', r.title, 'description', r.description, 'status', r.status, 'created_at', r.created_at,
      'submitted_by', p.display_name, 'votes', coalesce(v.n, 0),
      'voted_by_me', exists (select 1 from hive.feature_request_votes mv where mv.request_id = r.id and mv.member_id = auth.uid())
    ) order by coalesce(v.n, 0) desc, r.created_at desc), '[]'::jsonb)
  from hive.feature_requests r
  join public.profiles p on p.id = r.member_id
  left join (select request_id, count(*) n from hive.feature_request_votes group by request_id) v on v.request_id = r.id
  where hive.is_member();
$$;
grant execute on function hive.feature_request_list() to authenticated;
create or replace function public.hive_feature_request_list() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.feature_request_list(); $$;
grant execute on function public.hive_feature_request_list() to authenticated;

-- Toggle a vote (idempotent either direction).
create or replace function hive.feature_request_vote(p_request_id uuid, p_on boolean default true) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare n int; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if not exists (select 1 from hive.feature_requests where id = p_request_id) then raise exception 'request_not_found'; end if;
  if p_on then
    insert into hive.feature_request_votes (request_id, member_id) values (p_request_id, auth.uid()) on conflict do nothing;
  else
    delete from hive.feature_request_votes where request_id = p_request_id and member_id = auth.uid();
  end if;
  select count(*) into n from hive.feature_request_votes where request_id = p_request_id;
  return jsonb_build_object('votes', n, 'voted_by_me', p_on);
end $$;
grant execute on function hive.feature_request_vote(uuid, boolean) to authenticated;
create or replace function public.hive_feature_request_vote(p_request_id uuid, p_on boolean default true) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.feature_request_vote(p_request_id, p_on); $$;
grant execute on function public.hive_feature_request_vote(uuid, boolean) to authenticated;
