-- Expand the runtime CHECK without rewriting child foreign-key references.
-- Caller disables FK enforcement before its atomic migration transaction, checks
-- integrity before commit, then re-enables enforcement.
CREATE TABLE agent_profiles_v12(
 id TEXT PRIMARY KEY,
 owner TEXT NOT NULL,
 name TEXT NOT NULL,
 role_revision INTEGER NOT NULL DEFAULT 1,
 runtime_kind TEXT NOT NULL CHECK(runtime_kind IN
  ('local','chatgpt_subscription','copilot_subscription','grok_subscription','anthropic_byok','nous_byok')),
 preferred_host TEXT,
 capability_policy_ref TEXT NOT NULL,
 provider_account_ref TEXT,
 memory_namespace TEXT NOT NULL,
 archived INTEGER NOT NULL DEFAULT 0,
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL
);
INSERT INTO agent_profiles_v12 SELECT * FROM agent_profiles;
DROP TABLE agent_profiles;
ALTER TABLE agent_profiles_v12 RENAME TO agent_profiles;
PRAGMA user_version=12;
