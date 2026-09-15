CREATE TABLE private_fleet_authority (
  id INTEGER PRIMARY KEY CHECK(id=1), authority_id TEXT NOT NULL UNIQUE,
  fleet_id TEXT, owner_id TEXT, trust TEXT
);
CREATE TABLE private_fleet_challenges (
  node_id TEXT PRIMARY KEY REFERENCES nodes(id), key_hash TEXT NOT NULL,
  nonce TEXT NOT NULL UNIQUE, expires_at INTEGER NOT NULL
);
CREATE TABLE private_fleet_enrollments (
  assertion_id TEXT PRIMARY KEY, node_id TEXT NOT NULL REFERENCES nodes(id), accepted_at INTEGER NOT NULL
);
PRAGMA user_version=9;
