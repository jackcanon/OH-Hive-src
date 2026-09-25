-- Per-(agent,host) write secret for the web_post_json tool. Never surfaced through
-- bots_agent_tool_policy_get/settings -- the model gets a "{{SECRET}}" placeholder to put in
-- its request body, and the value here is substituted only on the agent host, right before the
-- request goes out. Owner-set only (see owner_check in agent_tools.rs).
CREATE TABLE IF NOT EXISTS agent_tool_secrets (
 agent TEXT NOT NULL REFERENCES agent_profiles(id),
 host TEXT NOT NULL,
 secret TEXT NOT NULL,
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL,
 PRIMARY KEY(agent, host)
);
