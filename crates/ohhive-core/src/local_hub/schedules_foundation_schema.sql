CREATE TABLE IF NOT EXISTS schedules(
  id TEXT PRIMARY KEY,
  owner TEXT NOT NULL,
  name TEXT NOT NULL,
  current_revision TEXT,
  recurrence_json TEXT NOT NULL,
  missed_run_policy_json TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  paused INTEGER NOT NULL DEFAULT 0,
  last_materialized_through INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS schedule_authorizations(
  id TEXT PRIMARY KEY,
  schedule_id TEXT NOT NULL REFERENCES schedules(id),
  owner TEXT NOT NULL,
  scope_json TEXT NOT NULL,
  revision_digest TEXT NOT NULL,
  approved_at INTEGER NOT NULL,
  expires_at INTEGER,
  revoked_at INTEGER
);
CREATE TABLE IF NOT EXISTS schedule_revisions(
  id TEXT PRIMARY KEY,
  schedule_id TEXT NOT NULL REFERENCES schedules(id),
  revision_number INTEGER NOT NULL,
  task_spec_json TEXT NOT NULL,
  agent_id TEXT REFERENCES agent_profiles(id),
  host_policy_json TEXT NOT NULL,
  model_policy_json TEXT,
  resource_scope_json TEXT NOT NULL,
  budgets_json TEXT NOT NULL,
  output_destination_json TEXT,
  authorization_id TEXT NOT NULL REFERENCES schedule_authorizations(id),
  created_at INTEGER NOT NULL,
  UNIQUE(schedule_id, revision_number)
);
CREATE TABLE IF NOT EXISTS schedule_occurrences(
  id TEXT PRIMARY KEY,
  schedule_id TEXT NOT NULL REFERENCES schedules(id),
  revision_id TEXT NOT NULL REFERENCES schedule_revisions(id),
  due_at INTEGER NOT NULL,
  state TEXT NOT NULL CHECK(state IN (
    'queued','waiting_for_computer','waiting_for_resources','awaiting_approval',
    'running','succeeded','failed','interrupted','outcome_unknown','skipped','cancelled'
  )),
  reason TEXT,
  request_id TEXT NOT NULL UNIQUE,
  linked_delivery_ref TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(schedule_id, revision_id, due_at)
);
CREATE INDEX IF NOT EXISTS schedule_occurrences_schedule ON schedule_occurrences(schedule_id, due_at);
CREATE TABLE IF NOT EXISTS schedule_attempts(
  id TEXT PRIMARY KEY,
  occurrence_id TEXT NOT NULL REFERENCES schedule_occurrences(id),
  attempt_number INTEGER NOT NULL,
  worker_node_id TEXT REFERENCES nodes(id),
  lease_generation INTEGER NOT NULL DEFAULT 0,
  started_at INTEGER,
  ended_at INTEGER,
  failure_class TEXT,
  result_ref TEXT,
  usage_json TEXT,
  created_at INTEGER NOT NULL,
  UNIQUE(occurrence_id, attempt_number)
);
CREATE TABLE IF NOT EXISTS schedule_outbox(
  id TEXT PRIMARY KEY,
  occurrence_id TEXT NOT NULL REFERENCES schedule_occurrences(id),
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  dedupe_key TEXT NOT NULL UNIQUE,
  dispatched_at INTEGER,
  acknowledged_at INTEGER,
  created_at INTEGER NOT NULL
);
