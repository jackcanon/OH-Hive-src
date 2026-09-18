CREATE TABLE IF NOT EXISTS bots_agent_tool_policies (
 agent TEXT PRIMARY KEY REFERENCES agent_profiles(id),
 policy TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS bots_agent_tool_receipts (
 id TEXT PRIMARY KEY,
 agent TEXT NOT NULL REFERENCES agent_profiles(id),
 node TEXT NOT NULL REFERENCES nodes(id),
 tool TEXT NOT NULL,
 vault TEXT NOT NULL,
 policy_revision INTEGER NOT NULL,
 created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS bots_agent_tool_receipts_agent ON bots_agent_tool_receipts(agent,created_at);
