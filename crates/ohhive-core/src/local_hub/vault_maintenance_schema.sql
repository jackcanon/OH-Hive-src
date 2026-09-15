ALTER TABLE vault_archives RENAME TO vault_archives_v5;
CREATE TABLE vault_archives(document_id TEXT PRIMARY KEY,vault_id TEXT NOT NULL,revision TEXT NOT NULL,snapshot TEXT,archived_ms INTEGER NOT NULL);
INSERT INTO vault_archives SELECT * FROM vault_archives_v5;
DROP TABLE vault_archives_v5;
CREATE TABLE vault_maintenance(
 vault_id TEXT PRIMARY KEY REFERENCES vaults(id), policy TEXT NOT NULL,
 next_due_ms INTEGER NOT NULL, token TEXT, lease_until_ms INTEGER NOT NULL DEFAULT 0,
 last_finished_ms INTEGER, last_result TEXT
);
PRAGMA user_version=6;
