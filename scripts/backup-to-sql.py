#!/usr/bin/env python3
"""Turn an ohhive-backup/1 JSON document into idempotent SQL (see restore-backup.sh).

Each table becomes one INSERT … SELECT * FROM jsonb_populate_recordset(NULL::hive.<t>, $json$…$json$)
ON CONFLICT DO NOTHING, in the FK-safe order the export recorded. Columns are matched by name, so a
missing columns become NULL, so schema compatibility must be tested before recovery.
Run inside a single transaction as the table owner; user triggers are suspended during replay.
"""
import json
import re
import sys

doc = json.load(open(sys.argv[1]))
assert doc.get("format") == "ohhive-backup/1", f"unexpected format {doc.get('format')!r}"
order = doc["order"]
assert all(re.fullmatch(r"[a-z_][a-z0-9_]*", t) for t in order), "invalid table name"
tables = doc["tables"]

print(f"-- ohhive backup exported {doc['exported_at']} by node {doc['exported_by']}")
print("-- idempotent: ON CONFLICT DO NOTHING; run inside one transaction (psql -1)")
print("set search_path = hive, public;")
# Replaying stored rows must not regenerate audit/presence/ledger side effects.
# Internal constraint triggers remain active, so foreign keys are still enforced.
print("""create temporary table restore_trigger_states on commit drop as
select c.relname as tbl,t.tgname as name,t.tgenabled as enabled
from pg_trigger t join pg_class c on c.oid=t.tgrelid
join pg_namespace n on n.oid=c.relnamespace
where n.nspname='hive' and not t.tgisinternal;""")
for t in order:
    print(f'alter table hive."{t}" disable trigger user;')
for t in order:
    rows = tables.get(t) or []
    if not rows:
        print(f"-- {t}: 0 rows")
        continue
    body = json.dumps(rows, ensure_ascii=False)
    tag = "$ohhive$"
    assert tag not in body, f"{t}: payload contains the dollar-quote tag"
    print(f"-- {t}: {len(rows)} rows")
    print(
        f"insert into hive.\"{t}\" overriding system value select * from jsonb_populate_recordset(null::hive.\"{t}\", {tag}{body}{tag}::jsonb) "
        "on conflict do nothing;"
    )

# Restore identity/serial sequences after explicit IDs. Preserve a sequence already
# advanced beyond table max (important when filling gaps in an existing database).
print("""do $restore_sequences$
declare r record; seq text; max_id bigint; current_id bigint;
begin
 for r in select table_name,column_name from information_schema.columns
          where table_schema='hive' and (is_identity='YES' or column_default like 'nextval(%') loop
  seq := pg_get_serial_sequence(format('hive.%I',r.table_name),r.column_name);
  if seq is not null then
   execute format('select max(%I) from hive.%I',r.column_name,r.table_name) into max_id;
   if max_id is not null then
    execute format('select last_value from %s',seq) into current_id;
    perform setval(seq::regclass,greatest(max_id,current_id),true);
   end if;
  end if;
 end loop;
end $restore_sequences$;""")
print("""do $restore_triggers$
declare r record; action text;
begin
 for r in select * from restore_trigger_states loop
  action := case r.enabled when 'D' then 'disable' when 'A' then 'enable always'
            when 'R' then 'enable replica' else 'enable' end;
  execute format('alter table hive.%I %s trigger %I',r.tbl,action,r.name);
 end loop;
end $restore_triggers$;""")
