-- Authorization and observed results are distinct. NULL observation means not reported.
CREATE TABLE bots_source_evidence (
 receipt TEXT PRIMARY KEY REFERENCES bots_agent_tool_receipts(id),
 source TEXT NOT NULL,
 observation TEXT,
 observed_at INTEGER,
 content_sha256 TEXT
);
