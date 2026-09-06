#!/usr/bin/env sh
# Fetch + decrypt a Hive backup (ADR-013 D73) and turn it into SQL you can review, then apply with psql.
#
#   scripts/restore-backup.sh <sha256> [server-url]        # → backup.json + restore.sql in ./restore-<hash12>/
#   HIVE_BACKUP_KEY=~/.config/ohhive/backup-key.txt         # age identity (default shown)
#
# Then, deliberately:  psql "$DATABASE_URL" -1 -f restore-<hash12>/restore.sql
# The SQL is INSERT … ON CONFLICT DO NOTHING per table in FK-safe order — it fills gaps, never overwrites.
# For a from-scratch rebuild, apply supabase/migrations first, then this.
set -eu
HASH="${1:?usage: restore-backup.sh <sha256> [server-url]}"
SERVER="${2:-https://heimdall.ohghive.com}"
KEY="${HIVE_BACKUP_KEY:-$HOME/.config/ohhive/backup-key.txt}"
command -v age >/dev/null || { echo "need age (brew install age / apt install age)"; exit 1; }
[ -f "$KEY" ] || { echo "age identity not found at $KEY (set HIVE_BACKUP_KEY)"; exit 1; }

out="restore-$(printf %s "$HASH" | cut -c1-12)"; mkdir -p "$out"
echo "→ fetching $SERVER/a/$HASH"
curl -fsSL "$SERVER/a/$HASH" -o "$out/backup.age"
got=$( (sha256sum "$out/backup.age" 2>/dev/null || shasum -a 256 "$out/backup.age") | cut -d' ' -f1)
[ "$got" = "$HASH" ] || { echo "hash mismatch: got $got"; exit 1; }
age -d -i "$KEY" "$out/backup.age" | gunzip > "$out/backup.json"
python3 "$(dirname "$0")/backup-to-sql.py" "$out/backup.json" > "$out/restore.sql"
echo "→ $out/backup.json ($(wc -c < "$out/backup.json" | tr -d ' ') bytes), $out/restore.sql"
echo "   review, then:  psql \"\$DATABASE_URL\" -1 -f $out/restore.sql"
