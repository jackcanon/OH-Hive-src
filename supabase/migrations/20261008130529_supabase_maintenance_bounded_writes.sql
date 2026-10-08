-- Preserve per-request key validity/revocation checks; bound usage telemetry writes.
CREATE OR REPLACE FUNCTION hive.verify_node_key(raw_key text)
RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER
SET search_path TO hive, public, extensions
AS $function$
DECLARE h text; nid uuid; used timestamptz; touched uuid;
BEGIN
 h := encode(extensions.digest(raw_key::bytea, 'sha256'), 'hex');
 SELECT node_id,last_used_at INTO nid,used FROM hive.node_keys WHERE key_hash=h AND revoked_at IS NULL;
 IF nid IS NULL THEN RETURN NULL; END IF;
 IF used IS NULL OR used < now()-interval '60 seconds' THEN
  UPDATE hive.node_keys SET last_used_at=now() WHERE key_hash=h AND revoked_at IS NULL
   AND (last_used_at IS NULL OR last_used_at < now()-interval '60 seconds') RETURNING node_id INTO touched;
  IF touched IS NULL THEN
   -- Distinguish another validator's telemetry update from a concurrent revocation.
   SELECT node_id INTO nid FROM hive.node_keys WHERE key_hash=h AND revoked_at IS NULL;
  END IF;
 END IF;
 RETURN nid;
END $function$;

-- Preserve format, authorization, dependency order and all business tables.
-- Omit only expiring network samples and housekeeping diagnostics; report exclusions.
CREATE OR REPLACE FUNCTION hive.backup_export(raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; tbl text; tbl_rows jsonb; tables jsonb := '{}'::jsonb; ord text[];
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and status = 'online' and operator = 'hjm') then
    raise exception 'backup_requires_hjm_server';
  end if;
  with recursive deps as (
    select ch.relname::text as child, pa.relname::text as parent
    from pg_constraint c
    join pg_class ch on ch.oid = c.conrelid join pg_namespace n on n.oid = ch.relnamespace
    join pg_class pa on pa.oid = c.confrelid join pg_namespace pn on pn.oid = pa.relnamespace
    where c.contype = 'f' and n.nspname = 'hive' and pn.nspname = 'hive' and c.conrelid <> c.confrelid
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
    -- Expiring diagnostics are deliberately excluded from recovery exports.
    if tbl in ('rtt_samples', 'housekeeping_log') then
      tables := tables || jsonb_build_object(tbl, '[]'::jsonb);
      continue;
    end if;
    execute format('select coalesce(jsonb_agg(to_jsonb(x)), ''[]''::jsonb) from hive.%I x', tbl) into tbl_rows;
    tables := tables || jsonb_build_object(tbl, tbl_rows);
  end loop;
  return jsonb_build_object('format', 'ohhive-backup/1', 'exported_at', now(), 'exported_by', nid, 'schema', 'hive', 'order', to_jsonb(ord), 'tables', tables, 'excluded_tables', jsonb_build_array('rtt_samples', 'housekeeping_log'));
end $function$
;
