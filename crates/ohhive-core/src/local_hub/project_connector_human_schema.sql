-- Separate human authority: never represent a human with an agent credential.
CREATE TABLE project_connector_human_grants (
 id TEXT PRIMARY KEY, key_hash TEXT NOT NULL UNIQUE, owner TEXT NOT NULL,
 rooms TEXT NOT NULL, can_post INTEGER NOT NULL CHECK(can_post IN (0,1)),
 expires_at INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1))
);
CREATE TABLE project_connector_human_receipts (
 grant_id TEXT NOT NULL REFERENCES project_connector_human_grants(id),
 request_id TEXT NOT NULL, payload TEXT NOT NULL, response TEXT NOT NULL,
 PRIMARY KEY(grant_id,request_id)
);
