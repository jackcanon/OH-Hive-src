-- ADR-035 C1: LocalHub storage for Bots chat and agent collaboration.
-- Table names and columns follow docs/SIF-BOTS-CHAT-DESIGN-2026-09-15.md section 5's table
-- literally (agent_profiles, conversations, conversation_members, messages, message_revisions,
-- agent_deliveries, runtime_bindings, conversation_read_positions, handoffs). Timestamps are
-- unix seconds (this module's own now(), matching schema.sql/vault_schema.sql -- not the
-- vault_*_ms convention some later vault modules use).

CREATE TABLE agent_profiles(
 id TEXT PRIMARY KEY,
 owner TEXT NOT NULL,
 name TEXT NOT NULL,
 role_revision INTEGER NOT NULL DEFAULT 1,
 runtime_kind TEXT NOT NULL CHECK(runtime_kind IN
  ('local','chatgpt_subscription','copilot_subscription','grok_subscription')),
 preferred_host TEXT,
 capability_policy_ref TEXT NOT NULL,
 provider_account_ref TEXT,
 memory_namespace TEXT NOT NULL,
 archived INTEGER NOT NULL DEFAULT 0,
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL
);

CREATE TABLE conversations(
 id TEXT PRIMARY KEY,
 owner TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('team','project','agent_dm')),
 project_id TEXT,
 coordinator TEXT REFERENCES agent_profiles(id),
 storage_scope TEXT NOT NULL CHECK(storage_scope IN ('local_only','hub_backed')),
 policy_revision INTEGER NOT NULL DEFAULT 1,
 created_at INTEGER NOT NULL
);

-- "Unique principal per room" (section 5) is the composite primary key itself, not an
-- application-level check.
CREATE TABLE conversation_members(
 conversation_id TEXT NOT NULL REFERENCES conversations(id),
 principal_kind TEXT NOT NULL CHECK(principal_kind IN ('user','agent')),
 principal_id TEXT NOT NULL,
 allowed_actions TEXT NOT NULL,
 history_boundary INTEGER NOT NULL,
 joined_at INTEGER NOT NULL,
 PRIMARY KEY(conversation_id, principal_kind, principal_id)
);

-- UNIQUE(conversation_id, client_request_id) is what makes message_send idempotent: a retry
-- with the same key hits the constraint instead of inserting a second row, and the caller
-- returns the existing row (application logic, not the schema).
CREATE TABLE messages(
 id TEXT PRIMARY KEY,
 conversation_id TEXT NOT NULL REFERENCES conversations(id),
 thread_root TEXT REFERENCES messages(id),
 author_kind TEXT NOT NULL CHECK(author_kind IN ('user','agent')),
 author_id TEXT NOT NULL,
 server_sequence INTEGER NOT NULL,
 client_request_id TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('text','system','task_receipt')),
 body TEXT,
 attachment_refs TEXT NOT NULL DEFAULT '[]',
 task_ref TEXT,
 turn_ref TEXT,
 source_event_ref TEXT,
 created_at INTEGER NOT NULL,
 UNIQUE(conversation_id, server_sequence),
 UNIQUE(conversation_id, client_request_id)
);
CREATE INDEX messages_conversation ON messages(conversation_id, server_sequence);

CREATE TABLE message_revisions(
 id TEXT PRIMARY KEY,
 original_id TEXT NOT NULL REFERENCES messages(id),
 revision_kind TEXT NOT NULL CHECK(revision_kind IN ('replacement','tombstone')),
 new_body TEXT,
 author_kind TEXT NOT NULL CHECK(author_kind IN ('user','agent')),
 author_id TEXT NOT NULL,
 created_at INTEGER NOT NULL
);

-- (message_id, recipient) unique key per section 5 -- the primary key itself.
CREATE TABLE agent_deliveries(
 message_id TEXT NOT NULL REFERENCES messages(id),
 recipient TEXT NOT NULL REFERENCES agent_profiles(id),
 status TEXT NOT NULL CHECK(status IN
  ('pending','running','done','failed','cancelled','unknown')) DEFAULT 'pending',
 lease_generation INTEGER NOT NULL DEFAULT 0,
 retry_deadline INTEGER,
 bound_runtime_session TEXT,
 bound_turn_ref TEXT,
 updated_at INTEGER NOT NULL,
 PRIMARY KEY(message_id, recipient)
);

-- writer_generation is what a caller compares against before writing (the "one fenced writer"
-- rule) -- the UNIQUE constraint below is a soft backstop, not the enforcement itself: SQLite
-- treats NULL thread_root values as distinct, so two NULL-thread bindings for the same
-- (conversation, agent) would NOT collide on this constraint alone.
CREATE TABLE runtime_bindings(
 id TEXT PRIMARY KEY,
 conversation_id TEXT NOT NULL REFERENCES conversations(id),
 thread_root TEXT REFERENCES messages(id),
 agent_id TEXT NOT NULL REFERENCES agent_profiles(id),
 provider_account_id TEXT NOT NULL,
 policy_revision INTEGER NOT NULL,
 runtime_session_id TEXT NOT NULL,
 writer_generation INTEGER NOT NULL DEFAULT 0,
 created_at INTEGER NOT NULL,
 UNIQUE(conversation_id, thread_root, agent_id)
);

CREATE TABLE conversation_read_positions(
 user_id TEXT NOT NULL,
 conversation_id TEXT NOT NULL REFERENCES conversations(id),
 last_seen_sequence INTEGER NOT NULL DEFAULT 0,
 updated_at INTEGER NOT NULL,
 PRIMARY KEY(user_id, conversation_id)
);

CREATE TABLE handoffs(
 id TEXT PRIMARY KEY,
 source_agent TEXT NOT NULL REFERENCES agent_profiles(id),
 target_agent TEXT NOT NULL REFERENCES agent_profiles(id),
 project_id TEXT,
 task_or_question TEXT NOT NULL,
 acceptance_criteria TEXT NOT NULL,
 artifact_refs TEXT NOT NULL DEFAULT '[]',
 allowed_tools TEXT NOT NULL DEFAULT '[]',
 parent_run TEXT,
 reply_to_thread TEXT REFERENCES messages(id),
 budgets TEXT NOT NULL,
 deadline INTEGER NOT NULL,
 depth INTEGER NOT NULL DEFAULT 0,
 state TEXT NOT NULL CHECK(state IN
  ('requested','accepted','rejected','in_progress','awaiting_correction',
   'completed','failed','expired')) DEFAULT 'requested',
 receipt TEXT,
 created_at INTEGER NOT NULL
);
CREATE INDEX handoffs_target ON handoffs(target_agent, state);

-- conversation_search: full-text over message bodies, same external-content FTS5 pattern as
-- vault_fts in vault_schema.sql.
CREATE VIRTUAL TABLE messages_fts USING fts5(body,content='messages',content_rowid='rowid');
CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
 INSERT INTO messages_fts(rowid,body) VALUES(new.rowid,coalesce(new.body,''));
END;
CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
 INSERT INTO messages_fts(messages_fts,rowid,body) VALUES('delete',old.rowid,coalesce(old.body,''));
END;
CREATE TRIGGER messages_fts_update AFTER UPDATE ON messages BEGIN
 INSERT INTO messages_fts(messages_fts,rowid,body) VALUES('delete',old.rowid,coalesce(old.body,''));
 INSERT INTO messages_fts(rowid,body) VALUES(new.rowid,coalesce(new.body,''));
END;

PRAGMA user_version=7;
