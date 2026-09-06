-- OH Hive — replication factor 2 (ADR-007 D?/§2), pull-based. Safe to re-run.
--
-- Every minute each server asks "what should I fetch?": artifacts that are pinned, have fewer than
-- `replication` replicas, that this server doesn't hold, and that some *online* server does hold.
-- The server pulls from a holder's /a/<hash>, stores, announces. Prefer filling from a different
-- region than the existing holder (ADR-007: replicas in ≥ 2 places), but any online holder works.
-- Coordinator-driven placement can replace this later; the RPC is the placement policy in one spot.

alter table hive.artifacts add column if not exists replication int not null default 2;

create or replace function hive.replication_plan(raw_key text, p_limit int default 20) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; my_region text;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and status = 'online') then raise exception 'server_not_registered'; end if;
  select region into my_region from hive.nodes where id = nid;
  return coalesce((
    select jsonb_agg(jsonb_build_object('hash', t.hash, 'bytes', t.bytes, 'mime', t.mime, 'kind', t.kind,
                                        'project_id', t.project_id, 'card_id', t.card_id, 'from', t.url, 'from_name', t.name) order by t.bytes)
    from (
      select a.hash, a.bytes, a.mime, a.kind, a.project_id, a.card_id, src.url, src.name
      from (
        select a.*, (select count(*) from hive.artifact_replicas r where r.hash = a.hash) as have
        from hive.artifacts a
        where a.pinned and a.returned_at is null
      ) a
      cross join lateral (
        select rtrim(s.public_url, '/') || '/a/' || a.hash as url, n.display_name as name
        from hive.artifact_replicas r
        join hive.regional_servers s on s.node_id = r.node_id and s.status = 'online' and s.public_url is not null
        join hive.nodes n on n.id = r.node_id
        where r.hash = a.hash and r.node_id <> nid
        order by (n.region is distinct from my_region) desc, s.last_heartbeat desc
        limit 1
      ) src
      where a.have < a.replication
        and not exists (select 1 from hive.artifact_replicas r where r.hash = a.hash and r.node_id = nid)
      order by a.bytes asc
      limit p_limit
    ) t
  ), '[]'::jsonb);
end $$;

create or replace function public.hive_replication_plan(raw_key text, p_limit int default 20) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.replication_plan(raw_key, p_limit); $$;
grant execute on function public.hive_replication_plan(text, int) to anon, authenticated;

-- housekeeping: a replica on a server that has been offline > 24 h no longer counts
create or replace function hive.reap_dead_replicas() returns int language sql as $$
  with d as (
    delete from hive.artifact_replicas r using hive.regional_servers s
    where s.node_id = r.node_id and s.status = 'offline' and coalesce(s.last_heartbeat, s.updated_at) < now() - interval '24 hours'
    returning r.hash, r.node_id
  ), u as (
    update hive.artifacts a set replicas = array_remove(a.replicas, d.node_id) from d where a.hash = d.hash returning 1
  )
  select count(*)::int from d;
$$;
