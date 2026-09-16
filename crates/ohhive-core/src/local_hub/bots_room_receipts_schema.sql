-- S-B: transactionally record room+roster creation for reconnect-safe retries.
CREATE TABLE bots_room_create_receipts (
    owner TEXT NOT NULL,
    request_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    payload TEXT NOT NULL,
    response TEXT NOT NULL,
    PRIMARY KEY(owner, request_id)
);
PRAGMA user_version=13;
