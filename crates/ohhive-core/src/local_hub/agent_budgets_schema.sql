-- Per-agent monthly token usage and optional cap. Token-based (not USD) in v1: usage is exactly
-- what the turn runner reports (TurnUsage), so there is no per-provider price table to keep in
-- sync or trust -- see den-paperclip-integration-decision-2026-09-24.md ("do it natively rather
-- than trusting Paperclip's cost reporting"), applied here to locally-run turns too.
--
-- Recording happens only on the machine that actually ran the turn (LocalHubStore's
-- DeliveryStore::record_agent_usage override; RemoteLocalHub keeps the trait's no-op default) --
-- a fleet-hosted agent's usage is not tracked here in v1, the same narrowing the native
-- scheduler already has for cross-node worker claims.
CREATE TABLE IF NOT EXISTS agent_usage_periods (
 agent TEXT NOT NULL REFERENCES agent_profiles(id),
 period TEXT NOT NULL, -- "YYYY-MM", UTC
 prompt_tokens INTEGER NOT NULL DEFAULT 0,
 completion_tokens INTEGER NOT NULL DEFAULT 0,
 updated_at INTEGER NOT NULL,
 PRIMARY KEY(agent, period)
);

-- Owner-set monthly token cap. Absent = unlimited. v1 is record+report only: exceeding this does
-- not block a turn -- see the module doc on local_hub::agent_budgets for why enforcement is
-- deliberately deferred.
CREATE TABLE IF NOT EXISTS agent_budgets (
 agent TEXT PRIMARY KEY REFERENCES agent_profiles(id),
 monthly_token_limit INTEGER NOT NULL,
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL
);
