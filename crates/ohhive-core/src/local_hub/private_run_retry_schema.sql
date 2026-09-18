-- Foreign keys are disabled only for migration, then checked before commit and reenabled.
CREATE TABLE private_runs_new (
 id TEXT PRIMARY KEY, card_id TEXT NOT NULL REFERENCES cards(id),
 target_node_id TEXT NOT NULL REFERENCES nodes(id),
 state TEXT NOT NULL CHECK(state IN ('queued','running')), session TEXT, created INTEGER NOT NULL
);
INSERT INTO private_runs_new SELECT * FROM private_runs;
DROP TABLE private_runs;
ALTER TABLE private_runs_new RENAME TO private_runs;
CREATE INDEX private_runs_card ON private_runs(card_id);
CREATE TABLE private_run_retries (
 previous_id TEXT PRIMARY KEY REFERENCES private_runs(id),
 next_id TEXT NOT NULL UNIQUE REFERENCES private_runs(id),
 requested_by TEXT NOT NULL REFERENCES nodes(id), created INTEGER NOT NULL
);
PRAGMA user_version=19;
