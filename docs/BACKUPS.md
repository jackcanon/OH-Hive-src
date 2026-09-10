# Hub backups on the overlay (ADR-013 D73)

The Hive backs itself up; there is no Supabase PITR. Every night an **HJM-operated `hive-server` that
holds the coordinator lease** exports the whole `hive` schema and stores it on the overlay:

```
hive.backup_export()  →  JSON (every table, FK-safe order)
   → gzip → age-encrypt to HIVE_BACKUP_RECIPIENT (age1… public key)
   → local artifact store (/a/<sha256>)
   → hive.backup_record()  (kind='backup', pinned, replication 3)
```

Ordinary pull replication then copies it to other servers within a minute or two. Retention keeps the
newest 14 pinned (`hive.retire_old_backups`, pg_cron 03:30 UTC). Members can check health with
`select hive.backup_status()` — newest hash, age in hours, replica count.

## Where the key lives

- **Public half** (`age1…`) is on the servers as `HIVE_BACKUP_RECIPIENT` (`hive set HIVE_BACKUP_RECIPIENT age1…`).
  It can only *encrypt*. Safe to commit, paste, lose.
- **Private half** is an age identity file on Jack's Cowork Mac: `~/.config/hive/backup-key.txt` (mode 600).
  Nothing on a server, in the repo, or in the hub can read a backup without it. Keep a second copy
  somewhere offline (password manager, printed) — losing it makes every backup unreadable.

## Turning it on for a server

Only servers registered with `operator=hjm` can export (the hub enforces that; `hjm` itself needs the
founder account). One-time on each HJM box:

```sh
hive set HIVE_OPERATOR hjm
hive set HIVE_BACKUP_RECIPIENT age1…        # the public key above
# optional: hive set HIVE_BACKUP_HOUR_UTC 9   (default 09:00 UTC = 02:00 Phoenix)
systemctl --user restart hive-server         # or launchctl kickstart on macOS
```

Whichever HJM server is coordinator when the hour passes does the export; the others skip. Force one now:

```sh
hive-server backup            # prints {hash, bytes, plaintext_bytes}
```

## Restoring

```sh
scripts/restore-backup.sh <sha256> [https://heimdall.ohghive.com]
#  → restore-<hash12>/backup.json (decrypted) and restore.sql
psql "$DATABASE_URL" -1 -f restore-<hash12>/restore.sql
```

`restore.sql` is one `INSERT … ON CONFLICT DO NOTHING` per table in FK-safe order — it fills in what is
missing and never overwrites what exists, so it is safe to run against a live database. For a
from-scratch rebuild apply `supabase/migrations/` first, then the restore. The ledger's append-only
trigger allows inserts, so ledger history restores intact.

Find the newest hash: `select hive.backup_status();` — or, if the hub is gone, any server's blob
directory (`~/.local/share/hive/blobs`) holds them as `application/age` files.
