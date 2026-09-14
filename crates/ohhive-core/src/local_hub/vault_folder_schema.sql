CREATE TABLE vault_sources(
 vault_id TEXT PRIMARY KEY REFERENCES vaults(id),
 root TEXT NOT NULL,
 root_identity TEXT NOT NULL,
 generation INTEGER NOT NULL DEFAULT 0,
 indexed_at INTEGER,
 last_error TEXT
);
PRAGMA user_version=3;
