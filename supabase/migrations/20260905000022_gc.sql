-- Hive — server-side garbage collection of unpinned blobs (ADR-007 grace, D73 retention). Safe to re-run.
--
-- Servers never deleted anything: an unpinned output or a retired backup stayed on disk forever.
-- Now a server periodically asks `gc_plan(hashes it holds)` and drops what the hub says is droppable:
-- no artifact row at all, or unpinned with grace expired, or returned. It then calls `replica_drop`
-- so the registry stops counting it. Pinned artifacts are never in the plan, so the last copy of
-- anything that matters cannot be removed this way.

create or replace function hive.gc_plan(raw_key text, p_hashes text[]) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid) then raise exception 'server_not_registered'; end if;
  return coalesce((
    select jsonb_agg(h)
    from unnest(p_hashes) h
    left join hive.artifacts a on a.hash = h
    where a.hash is null
       or a.returned_at is not null
       or (not a.pinned and coalesce(a.grace_until, '-infinity'::timestamptz) <= now())
  ), '[]'::jsonb);
end $$;

create or replace function hive.replica_drop(raw_key text, p_hash text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; n int;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.artifact_replicas where hash = p_hash and node_id = nid;
  get diagnostics n = row_count;
  update hive.artifacts set replicas = array_remove(replicas, nid) where hash = p_hash;
  return jsonb_build_object('hash', p_hash, 'dropped', n > 0,
    'replicas_left', (select count(*) from hive.artifact_replicas where hash = p_hash));
end $$;

-- retired backups get a day of grace before servers may drop them
create or replace function hive.retire_old_backups(p_keep int default 14) returns int
language sql security definer set search_path = hive, public as $$
  with keep as (select hash from hive.artifacts where kind = 'backup' order by created_at desc limit p_keep),
  u as (update hive.artifacts set pinned = false, grace_until = now() + interval '1 day'
        where kind = 'backup' and pinned and hash not in (select hash from keep) returning 1)
  select count(*)::int from u;
$$;

create or replace function public.hive_gc_plan(raw_key text, p_hashes text[]) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.gc_plan(raw_key, p_hashes); $$;
create or replace function public.hive_replica_drop(raw_key text, p_hash text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.replica_drop(raw_key, p_hash); $$;
grant execute on function public.hive_gc_plan(text, text[]) to anon, authenticated;
grant execute on function public.hive_replica_drop(text, text) to anon, authenticated;
