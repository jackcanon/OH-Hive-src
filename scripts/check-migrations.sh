#!/usr/bin/env bash
# Static guards over supabase/migrations (no DB needed). Run in CI and before committing a migration.
#  1. ADR-013 D70: no migration may add a hive.* table to a Realtime publication.
#  2. ADR-001: every `create table hive.X` has a matching `alter table hive.X enable row level security`.
# The live counterparts run daily in the DB (hive.schema_guards, migration 0012).
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0

# 1. Realtime publication
if grep -nEi 'publication\s+supabase_realtime\s+add\s+table[^;]*hive\.' supabase/migrations/*.sql; then
  echo "FAIL: a migration adds a hive.* table to supabase_realtime (ADR-013 D70)"; fail=1
fi
if grep -nEi 'for\s+all\s+tables' supabase/migrations/*.sql | grep -i publication; then
  echo "FAIL: a migration creates a FOR ALL TABLES publication"; fail=1
fi

# 2. RLS on every hive table created in migrations
created=$(grep -hoEi 'create\s+table\s+(if\s+not\s+exists\s+)?hive\.[a-z_]+' supabase/migrations/*.sql | awk '{print tolower($NF)}' | sort -u)
for t in $created; do
  if ! grep -qEi "alter\s+table\s+${t//./\\.}\s+enable\s+row\s+level\s+security" supabase/migrations/*.sql; then
    echo "FAIL: $t is created without enabling RLS"; fail=1
  fi
done

[ "$fail" = 0 ] && echo "migrations ok: $(echo "$created" | wc -l | tr -d ' ') hive tables, all RLS, none in Realtime"
exit $fail
