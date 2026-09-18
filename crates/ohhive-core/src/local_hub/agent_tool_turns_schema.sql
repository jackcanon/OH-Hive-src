CREATE TABLE bots_agent_tool_turns (
 receipt TEXT PRIMARY KEY REFERENCES bots_agent_tool_receipts(id),
 message TEXT NOT NULL REFERENCES messages(id),
 conversation TEXT NOT NULL REFERENCES conversations(id),
 agent TEXT NOT NULL REFERENCES agent_profiles(id),
 generation INTEGER NOT NULL
);
CREATE INDEX bots_agent_tool_turn_budget ON bots_agent_tool_turns(message,agent,generation);
