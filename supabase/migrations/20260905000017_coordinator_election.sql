-- OH Hive — coordinator election over the hub (ADR-005 §1). Safe to re-run.
--
-- Exactly one regional server holds hive.coordinator_lease at a time. Servers call
-- coordinator_try(raw_key, ttl) on every heartbeat: the holder renews; anyone else acquires
-- only if the lease is empty or expired. Failover latency = ttl (default 90 s) + heartbeat (30 s).
-- The lease row is a singleton; the RPC does the compare-and-set under row lock, so two servers
-- racing can't both win. ADR-013 §F.18: standby servers only win if no healthy primary is a candidate,
-- so we prefer primaries by making standbys wait one extra cycle after expiry.

alter table hive.coordinator_lease add column if not exists acquired_at timestamptz;
alter table hive.coordinator_lease add column if not exists generation bigint not null default 0;

create or replace function hive.coordinator_try(raw_key text, p_ttl_seconds int default 90) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; l hive.coordinator_lease; tier text; won boolean := false;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select s.tier into tier from hive.regional_servers s where s.node_id = nid;
  if tier is null then raise exception 'server_not_registered'; end if;
  select * into l from hive.coordinator_lease where singleton for update;
  if l.node_id = nid then
    update hive.coordinator_lease set expires_at = now() + make_interval(secs => p_ttl_seconds), updated_at = now() where singleton;
    won := true;
  elsif l.node_id is null or l.expires_at is null or l.expires_at < now() - (case when tier = 'standby' then interval '30 seconds' else interval '0' end) then
    update hive.coordinator_lease set node_id = nid, expires_at = now() + make_interval(secs => p_ttl_seconds), acquired_at = now(),
           generation = generation + 1, updated_at = now() where singleton;
    won := true;
  end if;
  select * into l from hive.coordinator_lease where singleton;
  return jsonb_build_object('coordinator', won, 'holder', l.node_id, 'holder_name', (select display_name from hive.nodes where id = l.node_id),
                            'expires_at', l.expires_at, 'generation', l.generation);
end $$;

-- a coordinator stepping down (graceful shutdown) frees the lease immediately
create or replace function hive.coordinator_release(raw_key text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.coordinator_lease set expires_at = now(), updated_at = now() where singleton and node_id = nid;
  return jsonb_build_object('released', found);
end $$;

create or replace function public.hive_coordinator_try(raw_key text, p_ttl_seconds int default 90) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.coordinator_try(raw_key, p_ttl_seconds); $$;
create or replace function public.hive_coordinator_release(raw_key text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.coordinator_release(raw_key); $$;
grant execute on function public.hive_coordinator_try(text, int) to anon, authenticated;
grant execute on function public.hive_coordinator_release(text) to anon, authenticated;

-- surface it on the pulse
create or replace function hive.status() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'members', (select count(*) from hive.members where status = 'active'),
    'nodes_online', (select count(*) from hive.nodes where presence = 'checked_in'),
    'nodes_total', (select count(*) from hive.nodes),
    'models_online', (select count(distinct m->>'id') from hive.nodes, jsonb_array_elements(coalesce(capabilities->'models','[]'::jsonb)) m where presence = 'checked_in'),
    'servers_online', (select count(*) from hive.regional_servers where status = 'online'),
    'coordinator', (select jsonb_build_object('name', n.display_name, 'since', c.acquired_at, 'expires_at', c.expires_at, 'generation', c.generation)
                    from hive.coordinator_lease c left join hive.nodes n on n.id = c.node_id where c.expires_at > now()),
    'projects', (select count(*) from hive.projects where deleted_at is null),
    'cards', (select coalesce(jsonb_object_agg(s, n), '{}'::jsonb) from (select status::text s, count(*) n from hive.cards group by status) x),
    'honey_paid_24h', (select coalesce(sum(amount_honey), 0) from hive.ledger_entries where entry_type = 'earn_compute' and direction = 'credit' and created_at > now() - interval '24 hours'),
    'tokens_24h', (select coalesce(sum(tokens_out), 0) from hive.ledger_entries where entry_type = 'earn_compute' and direction = 'credit' and created_at > now() - interval '24 hours'),
    'recent', coalesce((select jsonb_agg(jsonb_build_object('at', e.created_at, 'card', c.title, 'project', p.title, 'node', n.display_name, 'tokens', e.tokens_out, 'honey', e.amount_honey) order by e.created_at desc)
                from (select * from hive.ledger_entries where entry_type = 'earn_compute' and direction = 'credit' order by created_at desc limit 8) e
                join hive.cards c on c.id = e.card_id join hive.projects p on p.id = c.project_id left join hive.nodes n on n.id = e.node_id), '[]'::jsonb)
  ) where hive.is_member();
$$;
