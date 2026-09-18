CREATE TABLE private_preparation_recoveries (
 request_id TEXT PRIMARY KEY,
 operation_id TEXT NOT NULL REFERENCES private_preparations(id),
 retired_session TEXT NOT NULL,
 requested_by TEXT NOT NULL REFERENCES nodes(id),
 created INTEGER NOT NULL,
 UNIQUE(operation_id, retired_session)
);
PRAGMA user_version=20;
