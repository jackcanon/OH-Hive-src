-- One saved copy per source reply and collection; deletion permits a deliberate new save.
CREATE TABLE bots_library_saves (
 message TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
 vault TEXT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
 document TEXT NOT NULL UNIQUE REFERENCES vault_documents(id) ON DELETE CASCADE,
 owner TEXT NOT NULL,
 node TEXT NOT NULL REFERENCES nodes(id),
 created_at INTEGER NOT NULL,
 PRIMARY KEY(message,vault)
);
