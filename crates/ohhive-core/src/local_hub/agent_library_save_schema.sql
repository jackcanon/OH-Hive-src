CREATE TABLE bots_agent_library_saves (
 agent TEXT NOT NULL REFERENCES agent_profiles(id),
 message TEXT NOT NULL REFERENCES messages(id),
 generation INTEGER NOT NULL,
 vault TEXT NOT NULL REFERENCES vaults(id),
 document TEXT NOT NULL REFERENCES vault_documents(id) ON DELETE CASCADE,
 payload TEXT NOT NULL,
 created_at INTEGER NOT NULL,
 PRIMARY KEY(agent,message,generation,vault)
);
