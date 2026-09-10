-- Hive — region-aware replication eligibility (ADR-007, fixes a real gap found
-- 2026-09-07 during cross-region comms testing).
--
-- Bug: hive.replication_plan's "should I fetch this?" gate was purely count-based
-- (have < replication). Every online server evaluates the same query on its own
-- tick, so whichever servers race fastest — in practice, LAN boxes with lower
-- latency and more of them — grab all the replication slots before a cross-region
-- server gets a turn. Confirmed live: both real pinned backups (replication=3) had
-- all 3 replicas on LAN-region servers, zero geographic diversity, despite
-- Amsterdam/Chicago/Sydney existing specifically for that (ADR-013 D73/D76).
--
-- Fix: a server only becomes eligible to add a same-region replica once every
-- currently-online region is already represented in that artifact's replica set.
-- Nodes with region null/'unknown' can't be meaningfully deduplicated by region,
-- so they fall back to the old count-only behavior (unchanged for them).
-- Safe to re-run; touches only function bodies, no data.

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
        and (
          my_region is null or my_region = 'unknown'
          or not exists (
            select 1 from hive.artifact_replicas r2
            join hive.nodes n2 on n2.id = r2.node_id
            where r2.hash = a.hash and n2.region = my_region
          )
          or (
            select count(distinct n3.region) from hive.artifact_replicas r3
            join hive.nodes n3 on n3.id = r3.node_id
            where r3.hash = a.hash and n3.region is not null and n3.region <> 'unknown'
          ) >= (
            select count(distinct n4.region) from hive.regional_servers rs
            join hive.nodes n4 on n4.id = rs.node_id
            where rs.status = 'online' and n4.region is not null and n4.region <> 'unknown'
          )
        )
      order by a.bytes asc
      limit p_limit
    ) t
  ), '[]'::jsonb);
end $$;
