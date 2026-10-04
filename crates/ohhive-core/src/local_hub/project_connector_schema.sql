-- Independent project-scoped credentials. Never reuse a fleet node key.
CREATE TABLE project_connector_grants (
 id TEXT PRIMARY KEY,
 key_hash TEXT NOT NULL UNIQUE,
 owner TEXT NOT NULL,
 agent TEXT NOT NULL REFERENCES agent_profiles(id),
 rooms TEXT NOT NULL,
 can_post INTEGER NOT NULL CHECK(can_post IN (0,1)),
 expires_at INTEGER NOT NULL,
 revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1))
);
-- Fingerprint and result are committed with the real Bots message, not after it.
CREATE TABLE project_connector_receipts (
 grant_id TEXT NOT NULL REFERENCES project_connector_grants(id),
 request_id TEXT NOT NULL,
 payload TEXT NOT NULL,
 response TEXT NOT NULL,
 PRIMARY KEY(grant_id,request_id)
);
