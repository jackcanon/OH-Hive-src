-- Hive — RTT metrics (Jack, 2026-09-12: "I feel like we should show our metrics, ms between
-- whichever server they are connected to, all of that kind of stuff").
--
-- The Hive's control plane is fully hub-centric today -- every node and every regional server
-- talks straight to this Supabase project for everything (check-in, heartbeat, card claims,
-- artifact announce). There's no direct node-to-regional-server connection for control traffic --
-- regional servers are separate heartbeating entities used for artifact storage/relay, not a
-- per-node "which server am I on" assignment. So "ms to whichever server" is, concretely, the
-- round-trip time of a node's or server's own heartbeat call to this hub -- the one connection
-- every participant actually has today.
--
-- Measurement happens client-side (crates/ohhive-core/src/hub.rs times the heartbeat RPC call
-- itself) and is reported on the NEXT heartbeat tick as p_rtt_ms -- one interval stale, which is
-- fine for a slowly-changing network metric, and avoids a second round trip just to report it.
--
-- hive.node_heartbeat and hive.server_heartbeat both change arity (new trailing param), so their
-- old signatures are explicitly dropped first -- Postgres resolves overloads by argument list, and
-- `create or replace` with a different parameter count creates a second overload instead of
-- replacing the original (bit us earlier this same day, see 20260912300000).

alter table hive.nodes add column if not exists rtt_ms integer;
alter table hive.regional_servers add column if not exists rtt_ms integer;

create table if not exists hive.rtt_samples (
  id bigserial primary key,
  subject_type text not null check (subject_type in ('node', 'regional_server')),
  subject_id uuid not null,
  region text,
  rtt_ms integer not null check (rtt_ms >= 0),
  recorded_at timestamptz not null default now()
);
create index if not exists rtt_samples_subject_idx on hive.rtt_samples (subject_type, subject_id, recorded_at desc);
create index if not exists rtt_samples_recorded_idx on hive.rtt_samples (recorded_at);
alter table hive.rtt_samples enable row level security;
create policy rtt_samples_member_read on hive.rtt_samples for select using (hive.is_member());

-- Records one RTT sample and mirrors it onto the subject's own row (a fast "latest" read without
-- hitting the history table). History retention lives in hive.housekeeping() below, not here --
-- doing a delete on every heartbeat would be wasted work on a table with an indexed timestamp.
create or replace function hive.record_rtt(p_subject_type text, p_subject_id uuid, p_region text, p_rtt_ms int) returns void
language plpgsql security definer set search_path = hive, public as $$
begin
  if p_rtt_ms is null then return; end if;
  insert into hive.rtt_samples (subject_type, subject_id, region, rtt_ms) values (p_subject_type, p_subject_id, p_region, p_rtt_ms);
  if p_subject_type = 'node' then
    update hive.nodes set rtt_ms = p_rtt_ms where id = p_subject_id;
  else
    update hive.regional_servers set rtt_ms = p_rtt_ms where node_id = p_subject_id;
  end if;
end $$;

drop function if exists hive.node_heartbeat(text);
drop function if exists public.hive_node_heartbeat(text);

create or replace function hive.node_heartbeat(raw_key text, p_rtt_ms int default null) returns timestamptz
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; reg text; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.nodes set last_heartbeat = now() where id = nid and presence = 'checked_in' returning region into reg;
  if found then perform hive.record_rtt('node', nid, reg, p_rtt_ms); end if;
  return now();
end $$;
grant execute on function hive.node_heartbeat(text, int) to anon, authenticated, service_role;

create or replace function public.hive_node_heartbeat(raw_key text, p_rtt_ms int default null) returns timestamptz
language sql security definer set search_path = hive, public as $$ select hive.node_heartbeat(raw_key, p_rtt_ms); $$;
grant execute on function public.hive_node_heartbeat(text, int) to anon, authenticated, service_role;

drop function if exists hive.server_heartbeat(text, bigint, int);
drop function if exists public.hive_server_heartbeat(text, bigint, int);

create or replace function hive.server_heartbeat(raw_key text, p_storage_used_bytes bigint default 0, p_connections int default 0, p_rtt_ms int default null) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; reg text;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.regional_servers set status = 'online', storage_used_bytes = p_storage_used_bytes, connections = p_connections, last_heartbeat = now(), updated_at = now() where node_id = nid;
  if not found then raise exception 'server_not_registered'; end if;
  update hive.nodes set presence = 'checked_in', last_heartbeat = now() where id = nid returning region into reg;
  perform hive.record_rtt('regional_server', nid, reg, p_rtt_ms);
  return jsonb_build_object('ok', true, 'coordinator', (select node_id from hive.coordinator_lease));
end $$;
grant execute on function hive.server_heartbeat(text, bigint, int, int) to anon, authenticated;

create or replace function public.hive_server_heartbeat(raw_key text, p_storage_used_bytes bigint default 0, p_connections int default 0, p_rtt_ms int default null) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.server_heartbeat(raw_key, p_storage_used_bytes, p_connections, p_rtt_ms); $$;
grant execute on function public.hive_server_heartbeat(text, bigint, int, int) to anon, authenticated;

-- members: same servers() list, now with each server's own hub RTT alongside its other stats.
-- Same signature as before -- plain create or replace, no drop needed.
create or replace function hive.servers() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object('node_id', s.node_id, 'name', n.display_name, 'region', n.region, 'operator', s.operator, 'tier', s.tier,
           'status', s.status, 'public_url', s.public_url, 'storage_gb_offered', n.storage_gb_offered, 'storage_used_bytes', s.storage_used_bytes,
           'connections', s.connections, 'last_heartbeat', s.last_heartbeat, 'version', s.version, 'rtt_ms', s.rtt_ms) order by n.region, n.display_name), '[]'::jsonb)
  from hive.regional_servers s join hive.nodes n on n.id = s.node_id where hive.is_member();
$$;

-- members: a connectivity overview -- per-region latency stats, the currently-connected node
-- list with each one's latest RTT, and the same servers() payload, in one round trip.
create or replace function hive.connectivity_summary() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'by_region', coalesce((select jsonb_agg(jsonb_build_object('region', region, 'count', cnt, 'avg_rtt_ms', avg_rtt, 'min_rtt_ms', min_rtt, 'max_rtt_ms', max_rtt) order by region)
      from (select coalesce(n.region, 'unspecified') as region, count(*) as cnt, round(avg(n.rtt_ms)) as avg_rtt, min(n.rtt_ms) as min_rtt, max(n.rtt_ms) as max_rtt
            from hive.nodes n where n.presence = 'checked_in' and n.rtt_ms is not null group by coalesce(n.region, 'unspecified')) r), '[]'::jsonb),
    'nodes', coalesce((select jsonb_agg(jsonb_build_object('node_id', n.id, 'name', n.display_name, 'region', n.region, 'role', n.role, 'presence', n.presence,
                'rtt_ms', n.rtt_ms, 'last_heartbeat', n.last_heartbeat) order by n.rtt_ms desc nulls last)
      from hive.nodes n where n.presence = 'checked_in'), '[]'::jsonb),
    'servers', hive.servers()
  ) where hive.is_member();
$$;
grant execute on function hive.connectivity_summary() to authenticated;
create or replace function public.hive_connectivity_summary() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.connectivity_summary(); $$;
grant execute on function public.hive_connectivity_summary() to authenticated;

-- Retention for the new history table, alongside the existing 7-day housekeeping_log trim.
-- Same signature as before -- plain create or replace, no drop needed.
create or replace function hive.housekeeping() returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare t0 timestamptz := clock_timestamp(); l int; n int; p int; s int;
begin
  l := hive.reap_expired_leases();
  n := hive.reap_stale_nodes('90 seconds');
  p := hive.pair_sweep();
  s := hive.reap_stale_servers();
  insert into hive.housekeeping_log (leases_reaped, nodes_reaped, pairings_swept, duration_ms)
  values (l, n, p, extract(milliseconds from clock_timestamp() - t0)::int);
  delete from hive.housekeeping_log where ran_at < now() - interval '7 days';
  delete from hive.rtt_samples where recorded_at < now() - interval '14 days';
  return jsonb_build_object('leases_reaped', l, 'nodes_reaped', n, 'pairings_swept', p, 'servers_offlined', s);
end $$;
