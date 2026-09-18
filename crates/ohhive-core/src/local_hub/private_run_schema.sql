CREATE TABLE private_runs (
    id TEXT PRIMARY KEY,
    card_id TEXT NOT NULL UNIQUE REFERENCES cards(id),
    target_node_id TEXT NOT NULL REFERENCES nodes(id),
    state TEXT NOT NULL CHECK(state IN ('queued','running')),
    session TEXT,
    created INTEGER NOT NULL
);
PRAGMA user_version=17;
