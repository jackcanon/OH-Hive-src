CREATE TABLE private_coding_readiness (
 node_id TEXT PRIMARY KEY REFERENCES nodes(id), report TEXT NOT NULL, observed_at INTEGER NOT NULL
);
PRAGMA user_version=21;
