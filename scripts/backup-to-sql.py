#!/usr/bin/env python3
"""Turn an ohhive-backup/1 JSON document into idempotent SQL (see restore-backup.sh).

Each table becomes one INSERT … SELECT * FROM jsonb_populate_recordset(NULL::hive.<t>, $json$…$json$)
ON CONFLICT DO NOTHING, in the FK-safe order the export recorded. Columns are matched by name, so a
backup from a slightly older schema still loads (missing columns take their defaults).
"""
import json
import sys

doc = json.load(open(sys.argv[1]))
assert doc.get("format") == "ohhive-backup/1", f"unexpected format {doc.get('format')!r}"
order = doc["order"]
tables = doc["tables"]

print(f"-- ohhive backup exported {doc['exported_at']} by node {doc['exported_by']}")
print("-- idempotent: ON CONFLICT DO NOTHING; run inside one transaction (psql -1)")
print("set search_path = hive, public;")
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
        f"insert into hive.\"{t}\" select * from jsonb_populate_recordset(null::hive.\"{t}\", {tag}{body}{tag}::jsonb) "
        "on conflict do nothing;"
    )
