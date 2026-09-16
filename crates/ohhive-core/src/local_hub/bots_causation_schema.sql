-- Schema v11: ADR-035 C2 Track A slice 2 -- causation and the human gate on the delivery path.
--
-- Three columns give agent_deliveries the causation it needs to bound an agent-to-agent chain:
-- cause_message_id (the message whose reply produced this delivery), root_message_id (the
-- human-authored message that began the chain) and turn_depth. Without them HandoffBudgets is
-- unenforceable -- the budgets live on `handoffs`, which carries a depth, while the delivery
-- path the executor actually runs carried none. See docs/LOKI-TRACK-A-ROOMS-AND-MENTIONS.
--
-- Also adds 'held' to the status CHECK. SQLite cannot ALTER a CHECK constraint, so this is a
-- full table rebuild rather than three ADD COLUMNs plus a rebuild -- one copy does everything.
-- agent_deliveries is a leaf (nothing references it), so DROP + RENAME is safe with foreign
-- keys on: the rebuilt table's own outbound references to messages/agent_profiles still hold.
--
-- Existing rows migrate as depth-0 roots with their own message as the root, which is exactly
-- what they are: every delivery created before this migration was caused by a human send.
--
-- Numbered 11, not 10: this was built on a branch as v10 at the same time Sif took v10 on main
-- for `conversations.title`, and she flagged the collision in the continuity log before either
-- landed. Renumbered here rather than renumbering hers, since hers shipped first.

CREATE TABLE agent_deliveries_v11(
 message_id TEXT NOT NULL REFERENCES messages(id),
 recipient TEXT NOT NULL REFERENCES agent_profiles(id),
 status TEXT NOT NULL CHECK(status IN
  ('pending','running','done','failed','cancelled','unknown','held')) DEFAULT 'pending',
 lease_generation INTEGER NOT NULL DEFAULT 0,
 retry_deadline INTEGER,
 bound_runtime_session TEXT,
 bound_turn_ref TEXT,
 updated_at INTEGER NOT NULL,
 -- The message whose reply produced this delivery. NULL for a human-originated delivery.
 cause_message_id TEXT REFERENCES messages(id),
 -- The message that began this chain. Defaults to message_id for pre-v10 rows.
 root_message_id TEXT REFERENCES messages(id),
 -- 0 for a delivery caused by a human message; incremented once per agent hop.
 turn_depth INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(message_id, recipient)
);

INSERT INTO agent_deliveries_v11
 (message_id,recipient,status,lease_generation,retry_deadline,bound_runtime_session,
  bound_turn_ref,updated_at,cause_message_id,root_message_id,turn_depth)
SELECT message_id,recipient,status,lease_generation,retry_deadline,bound_runtime_session,
  bound_turn_ref,updated_at,NULL,message_id,0 FROM agent_deliveries;

DROP TABLE agent_deliveries;
ALTER TABLE agent_deliveries_v11 RENAME TO agent_deliveries;

-- "Every delivery caused by that one thing Jack said" in one indexed query -- this is what
-- makes the per-root turn budget a single COUNT rather than a recursive walk.
CREATE INDEX IF NOT EXISTS agent_deliveries_root ON agent_deliveries(root_message_id);
-- The active-turns-per-agent budget reads this one.
CREATE INDEX IF NOT EXISTS agent_deliveries_recipient_status
  ON agent_deliveries(recipient,status);

PRAGMA user_version=11;
