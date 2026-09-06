-- OH Hive — nightly backups on the overlay (ADR-013 D73, ADR-007 kind='backup'). Safe to re-run.
--
-- Supabase PITR costs $100/mo; the Hive backs itself up instead. An HJM-operated server that holds the
-- coordinator lease calls hive.backup_export() once a day, gzips + age-encrypts the JSON to the hub
-- recipient key (private half lives with Jack, never on a server), stores the ciphertext in its own
-- artifact store and announces it as kind='backup' — pinned, replication 3 — so the ordinary pull
-- replication spreads it across servers. Retention: 14 nightly copies. Restore: scripts/restore-backup.sh.
--
-- Only servers registered with operator='hjm' may export (that already requires the founder account).

-- 1. Export: every base table in schema hive as JSON, plus an FK-safe insertion order for restore.
create or replace function hive.backup_export(raw_key text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; tbl text; tbl_rows jsonb; tables jsonb := '{}'::jsonb; ord text[];
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and status = 'online' and operator = 'hjm') then
    raise exception 'backup_requires_hjm_server';
  end if;

  -- FK-safe order: tables with no unmet dependencies first (self-references ignored)
  with recursive deps as (
    select ch.relname::text as child, pa.relname::text as parent
    from pg_constraint c
    join pg_class ch on ch.oid = c.conrelid join pg_namespace n on n.oid = ch.relnamespace
    join pg_class pa on pa.oid = c.confrelid join pg_namespace pn on pn.oid = pa.relnamespace
    where c.contype = 'f' and n.nspname = 'hive' and pn.nspname = 'hive' and c.conrelid <> c.confrelid  -- FKs into auth.* don't gate order
  ), all_t as (
    select table_name::text as t from information_schema.tables
    where table_schema = 'hive' and table_type = 'BASE TABLE'
  ), lvl as (
    select t, 0 as l from all_t where not exists (select 1 from deps where deps.child = all_t.t)
    union
    select d.child, lvl.l + 1 from deps d join lvl on lvl.t = d.parent where lvl.l < 20
  )
  select array_agg(t order by maxl, t) into ord from (select t, max(l) maxl from lvl group by t) x;

  foreach tbl in array ord loop
    execute format('select coalesce(jsonb_agg(to_jsonb(x)), ''[]''::jsonb) from hive.%I x', tbl) into tbl_rows;
    tables := tables || jsonb_build_object(tbl, tbl_rows);
  end loop;

  return jsonb_build_object(
    'format', 'ohhive-backup/1',
    'exported_at', now(),
    'exported_by', nid,
    'schema', 'hive',
    'order', to_jsonb(ord),
    'tables', tables
  );
end $$;

-- 2. Record: announce the ciphertext as a pinned backup artifact with replication 3.
create or replace function hive.backup_record(raw_key text, p_hash text, p_bytes bigint, p_exported_at timestamptz default now()) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; r jsonb;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and operator = 'hjm') then raise exception 'backup_requires_hjm_server'; end if;
  r := hive.artifact_announce(raw_key, p_hash, p_bytes, 'application/age', 'backup', null, null, null);
  update hive.artifacts set replication = 3, pinned = true, created_at = least(created_at, p_exported_at) where hash = p_hash;
  return r || jsonb_build_object('kind', 'backup', 'replication', 3);
end $$;

-- 3. Retention: keep the newest 14 backups pinned; older ones are unpinned (servers may drop them).
create or replace function hive.retire_old_backups(p_keep int default 14) returns int
language sql security definer set search_path = hive, public as $$
  with keep as (select hash from hive.artifacts where kind = 'backup' order by created_at desc limit p_keep),
  u as (update hive.artifacts set pinned = false where kind = 'backup' and pinned and hash not in (select hash from keep) returning 1)
  select count(*)::int from u;
$$;

-- 4. Members (any role) can see backup health: newest backup, its age, and replica count.
create or replace function hive.backup_status() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce((
    select jsonb_build_object('hash', a.hash, 'bytes', a.bytes, 'created_at', a.created_at,
             'age_hours', round(extract(epoch from now() - a.created_at) / 3600.0, 1),
             'replicas', (select count(*) from hive.artifact_replicas r where r.hash = a.hash),
             'replication', a.replication,
             'total_backups', (select count(*) from hive.artifacts where kind = 'backup' and pinned))
    from hive.artifacts a where a.kind = 'backup' order by a.created_at desc limit 1
  ), jsonb_build_object('total_backups', 0))
  where hive.is_member();
$$;

-- PostgREST wrappers
create or replace function public.hive_backup_export(raw_key text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.backup_export(raw_key); $$;
create or replace function public.hive_backup_record(raw_key text, p_hash text, p_bytes bigint, p_exported_at timestamptz default now()) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.backup_record(raw_key, p_hash, p_bytes, p_exported_at); $$;
create or replace function public.hive_backup_status() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.backup_status(); $$;
grant execute on function public.hive_backup_export(text) to anon, authenticated;
grant execute on function public.hive_backup_record(text, text, bigint, timestamptz) to anon, authenticated;
grant execute on function public.hive_backup_status() to authenticated;
grant execute on function hive.backup_status() to authenticated;

-- 5. Daily retention sweep (03:30 UTC, after the 02:00-ish nightly export window).
do $$ begin
  perform cron.unschedule(jobid) from cron.job where jobname = 'hive_backup_retention';
exception when others then null; end $$;
select cron.schedule('hive_backup_retention', '30 3 * * *', $$select hive.retire_old_backups(14)$$);
