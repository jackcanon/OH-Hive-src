-- Host-managed intake receipts; contents continue to use vault_documents and its existing FTS.
-- Excluded sources retain only identity/generation to prevent stale deliveries restoring them.
CREATE TABLE vault_intake (
 vault_id TEXT NOT NULL REFERENCES vaults(id),
 source_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation > 0),
 fingerprint TEXT NOT NULL,
 document_id TEXT NOT NULL UNIQUE,
 revision TEXT,
 PRIMARY KEY(vault_id, source_id)
);
PRAGMA user_version=4;
