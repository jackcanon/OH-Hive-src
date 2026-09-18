CREATE TABLE private_run_stops (
    operation_id TEXT PRIMARY KEY REFERENCES private_runs(id),
    requested_by TEXT NOT NULL REFERENCES nodes(id),
    created INTEGER NOT NULL
);
PRAGMA user_version=18;
