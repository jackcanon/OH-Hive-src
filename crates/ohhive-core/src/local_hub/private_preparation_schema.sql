CREATE TABLE private_preparations (
    id TEXT PRIMARY KEY,
    card_id TEXT NOT NULL UNIQUE REFERENCES cards(id),
    target_node_id TEXT NOT NULL REFERENCES nodes(id),
    card TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('queued','claimed','prepared')),
    session TEXT,
    workspace TEXT,
    created INTEGER NOT NULL
);
PRAGMA user_version=16;
