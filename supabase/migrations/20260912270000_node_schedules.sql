-- Scheduled check-in/out for workers and servers (feature request 1ec6538d-114b-438b-bd34-1280bc13203b,
-- Jack, 2026-09-12):
-- "We need to be able to have workers have scheduled shifts that they run. So people can
-- unattended check in and out their machines without having to do it manually. This should work
-- for both Servers and Workers." v1 scope: one recurring weekly window per node (a set of weekday
-- checkboxes + one start/end time), evaluated in the node's own local clock -- no timezone storage
-- needed since the CLI that enforces it runs on that same machine. A node with no schedule set
-- (null, the default) behaves exactly as today: always eligible, no auto check-out.
--
-- schedule shape: a jsonb array of {"day": 0-6 (0=Sunday), "start": "HH:MM", "end": "HH:MM"}.
-- Validated server-side so the CLI never has to defend against malformed data.

do $$ begin
  alter table hive.nodes add column if not exists schedule jsonb;
exception when duplicate_column then null; end $$;

create or replace function hive.node_validate_schedule(p_schedule jsonb) returns void
language plpgsql as $$
declare w jsonb; begin
  if p_schedule is null then return; end if;
  if jsonb_typeof(p_schedule) != 'array' then raise exception 'invalid_schedule'; end if;
  for w in select * from jsonb_array_elements(p_schedule) loop
    if not (w ? 'day' and w ? 'start' and w ? 'end') then raise exception 'invalid_schedule'; end if;
    if (w->>'day')::int not between 0 and 6 then raise exception 'invalid_schedule'; end if;
    if w->>'start' !~ '^([01][0-9]|2[0-3]):[0-5][0-9]$' or w->>'end' !~ '^([01][0-9]|2[0-3]):[0-5][0-9]$' then
      raise exception 'invalid_schedule';
    end if;
    if w->>'start' >= w->>'end' then raise exception 'invalid_schedule'; end if;
  end loop;
end $$;

-- Web: member sets their own node's schedule. Same ownership shape as hive.node_set_avatar.
create or replace function hive.node_set_schedule(p_node_id uuid, p_schedule jsonb) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  perform hive.node_validate_schedule(p_schedule);
  if not exists (select 1 from hive.nodes where id = p_node_id and member_id = auth.uid()) then
    raise exception 'node_not_found';
  end if;
  update hive.nodes set schedule = p_schedule where id = p_node_id;
  return jsonb_build_object('id', p_node_id, 'schedule', p_schedule);
end $$;

create or replace function public.hive_node_set_schedule(p_node_id uuid, p_schedule jsonb) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.node_set_schedule(p_node_id, p_schedule);
$$;
grant execute on function public.hive_node_set_schedule(uuid, jsonb) to authenticated;

-- CLI: the running `hive check-in --stay` loop has only a node key, no member session. Cheap,
-- read-only, refetched every tick so an edit made on the web takes effect within one interval.
create or replace function public.hive_node_schedule_get(p_raw_key text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return (select schedule from hive.nodes where id = nid);
end $$;
revoke all on function public.hive_node_schedule_get(text) from public;
grant execute on function public.hive_node_schedule_get(text) to anon, authenticated;

-- hive.my_wallet gains `schedule` per node so /wallet can prefill the picker without a second call.
-- Same shape as before otherwise (20260912250000_node_avatars.sql).
create or replace function hive.my_wallet(p_limit int default 50) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'balance', hive.account_balance((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid())),
    'sources', (select jsonb_object_agg(source, balance) from hive.account_sources((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid()))),
    'provider', hive.provider_available(auth.uid()),
    'rate', (select jsonb_build_object('honey_per_output_token', honey_per_unit, 'model_ref', model_ref, 'since', effective_from) from hive.rate_table where kind = 'compute_output' and effective_to is null order by effective_from desc limit 1),
    'entries', coalesce((select jsonb_agg(jsonb_build_object(
        'at', e.created_at, 'type', e.entry_type, 'direction', e.direction, 'amount', e.amount_honey, 'source', e.source,
        'tokens_out', e.tokens_out, 'memo', e.memo,
        'card', (select key from hive.cards where id = e.card_id), 'node', (select display_name from hive.nodes where id = e.node_id)
      ) order by e.created_at desc)
      from (select * from hive.ledger_entries where account_id = (select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid()) order by created_at desc limit p_limit) e), '[]'::jsonb),
    'nodes', coalesce((select jsonb_agg(jsonb_build_object('id', n.id, 'display_name', n.display_name, 'presence', n.presence, 'region', n.region,
        'role', n.role, 'avatar_choice', n.avatar_choice, 'schedule', n.schedule,
        'gpu', n.capabilities->'hardware'->>'gpu_model', 'models', jsonb_array_length(coalesce(n.capabilities->'models','[]'::jsonb)),
        'last_heartbeat', n.last_heartbeat, 'allow_internet', n.allow_internet, 'tools_level', n.tools_level))
      from hive.nodes n where n.member_id = auth.uid()), '[]'::jsonb)
  ) where hive.is_member();
$$;
