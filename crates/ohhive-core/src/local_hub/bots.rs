//! ADR-035 C1: LocalHub storage for Bots chat and agent collaboration.
//!
//! Implements the nine `agent_profiles`/`conversations`/... tables from `bots_schema.sql`
//! (migration version 7) as plain inherent methods on `LocalHubStore` (`bots_agent_*`,
//! `bots_conversation_*`, `bots_message_*`, `bots_handoff_*`, `bots_delivery_*`), matching
//! `vault.rs`'s established style exactly: `self.transaction(|tx| { ... })`, `params![]`,
//! `rejected("...")`/`db_error` for errors, `HubError` (this module's shared `Result<T>`), not a
//! bespoke error type. At the bottom, a thin `#[async_trait] impl BotsService for LocalHubStore`
//! delegates to those sync methods and converts `HubError` into `BotsError` -- see
//! `impl From<HubError> for BotsError` for how (a `rejected("not found: ...")`-style prefix on
//! the message picks the specific `BotsError` variant; anything else falls back to `Storage`).
//!
//! **What's real here:** full CRUD for all nine tables, the invariants the schema itself
//! enforces (unique principal per room, idempotent `message_send` via a `client_request_id`
//! unique constraint, the `(message, recipient)` delivery key, a `messages_fts` full-text index
//! for `conversation_search`), and everything the `BotsService` C0 contract's fourteen methods
//! need. Every non-trivial SQL pattern here (idempotent send-and-return, the `max()`-guarded
//! mark-read upsert, scope-filtered search) was verified against a real SQLite engine
//! (`python3`'s `sqlite3` module) before being ported to `rusqlite`, not just brace-balanced --
//! see the commit message for the verification transcript.
//!
//! **What's deliberately NOT here yet:** any FFI (`crates/ohhive-ffi`) or UI wiring, and no
//! runtime executor -- nothing here actually runs a model turn for a DM reply, drains the
//! `agent_deliveries` queue, or enforces `HandoffBudgets` beyond storing them. Two specific,
//! honestly-scoped gaps: (1) `handoff_create` always stores `depth: 0` -- C0's `NewHandoff`
//! carries no parent-handoff reference to derive a real depth from, so chained-handoff depth
//! tracking needs a C0 contract change, not a C1 storage bug fix; (2) `conversation_search`
//! paginates by a plain integer offset (encoded as the cursor string), not a real keyset cursor
//! -- fine for C1's data-layer purposes, worth revisiting if result sets get large.
//!
//! Same self-verification caveat as `subscription/` and `bots/` (ADR-033 Stage 1, ADR-035 C0):
//! written and reviewed without a Rust toolchain in this sandbox, not yet compiler- or
//! `cargo test`-verified.
mod rooms;
#[cfg(feature = "subscription-coordinator")]
mod subscription;

use super::*;
use crate::bots::{
    AgentDelivery, AgentId, AgentProfile, AgentProfilePatch, AgentRuntimeKind, BotsError,
    BotsResult, BotsService, Conversation, ConversationId, ConversationKind, ConversationMember,
    ConversationReadPosition, DeliveryCause, DeliveryKey, Handoff, HandoffId, HandoffState,
    MemberAction, Message, MessageId, MessageKind, MessagePage, NewAgentProfile, NewConversation,
    NewHandoff, NewMessage, Principal, RevisionKind, SearchHit, SearchPage, SearchScope,
    StorageScope, UserId,
};
use async_trait::async_trait;
use chrono::DateTime;

const SEARCH_PAGE_SIZE: i64 = 20;

// --- enum <-> TEXT column conversions -------------------------------------------------------

fn runtime_kind_to_str(k: AgentRuntimeKind) -> &'static str {
    match k {
        AgentRuntimeKind::Local => "local",
        AgentRuntimeKind::ChatgptSubscription => "chatgpt_subscription",
        AgentRuntimeKind::CopilotSubscription => "copilot_subscription",
        AgentRuntimeKind::GrokSubscription => "grok_subscription",
        AgentRuntimeKind::AnthropicByok => "anthropic_byok",
        AgentRuntimeKind::NousByok => "nous_byok",
    }
}
fn runtime_kind_from_str(s: &str) -> Result<AgentRuntimeKind> {
    match s {
        "local" => Ok(AgentRuntimeKind::Local),
        "chatgpt_subscription" => Ok(AgentRuntimeKind::ChatgptSubscription),
        "copilot_subscription" => Ok(AgentRuntimeKind::CopilotSubscription),
        "grok_subscription" => Ok(AgentRuntimeKind::GrokSubscription),
        "anthropic_byok" => Ok(AgentRuntimeKind::AnthropicByok),
        "nous_byok" => Ok(AgentRuntimeKind::NousByok),
        _ => Err(rejected("invalid stored agent runtime kind")),
    }
}
fn conversation_kind_to_str(k: ConversationKind) -> &'static str {
    match k {
        ConversationKind::Team => "team",
        ConversationKind::Project => "project",
        ConversationKind::AgentDm => "agent_dm",
    }
}
fn conversation_kind_from_str(s: &str) -> Result<ConversationKind> {
    match s {
        "team" => Ok(ConversationKind::Team),
        "project" => Ok(ConversationKind::Project),
        "agent_dm" => Ok(ConversationKind::AgentDm),
        _ => Err(rejected("invalid stored conversation kind")),
    }
}
fn storage_scope_to_str(s: StorageScope) -> &'static str {
    match s {
        StorageScope::LocalOnly => "local_only",
        StorageScope::HubBacked => "hub_backed",
    }
}
fn storage_scope_from_str(s: &str) -> Result<StorageScope> {
    match s {
        "local_only" => Ok(StorageScope::LocalOnly),
        "hub_backed" => Ok(StorageScope::HubBacked),
        _ => Err(rejected("invalid stored storage scope")),
    }
}
fn message_kind_to_str(k: MessageKind) -> &'static str {
    match k {
        MessageKind::Text => "text",
        MessageKind::System => "system",
        MessageKind::TaskReceipt => "task_receipt",
    }
}
fn message_kind_from_str(s: &str) -> Result<MessageKind> {
    match s {
        "text" => Ok(MessageKind::Text),
        "system" => Ok(MessageKind::System),
        "task_receipt" => Ok(MessageKind::TaskReceipt),
        _ => Err(rejected("invalid stored message kind")),
    }
}
// The write halves of two round-trip pairs whose write paths are ADR-035 C3 (handoffs) and message
// revisions -- both schema'd and neither wired yet, so only the read halves have callers today.
// Kept rather than deleted, and kept next to their partners: these three functions *are* the
// mapping between the enum and the column values the schema's CHECK constraints allow, and
// re-deriving that from the SQL later is how a storage layer ends up with two disagreeing
// spellings of "awaiting_correction". Delete them if C3 is ever abandoned, not before.
#[allow(dead_code)]
fn handoff_state_to_str(s: HandoffState) -> &'static str {
    match s {
        HandoffState::Requested => "requested",
        HandoffState::Accepted => "accepted",
        HandoffState::Rejected => "rejected",
        HandoffState::InProgress => "in_progress",
        HandoffState::AwaitingCorrection => "awaiting_correction",
        HandoffState::Completed => "completed",
        HandoffState::Failed => "failed",
        HandoffState::Expired => "expired",
    }
}
fn handoff_state_from_str(s: &str) -> Result<HandoffState> {
    match s {
        "requested" => Ok(HandoffState::Requested),
        "accepted" => Ok(HandoffState::Accepted),
        "rejected" => Ok(HandoffState::Rejected),
        "in_progress" => Ok(HandoffState::InProgress),
        "awaiting_correction" => Ok(HandoffState::AwaitingCorrection),
        "completed" => Ok(HandoffState::Completed),
        "failed" => Ok(HandoffState::Failed),
        "expired" => Ok(HandoffState::Expired),
        _ => Err(rejected("invalid stored handoff state")),
    }
}
#[allow(dead_code)] // See the note above `handoff_state_to_str`.
fn revision_kind_to_columns(k: &RevisionKind) -> (&'static str, Option<String>) {
    match k {
        RevisionKind::Replacement { new_body } => ("replacement", Some(new_body.clone())),
        RevisionKind::Tombstone => ("tombstone", None),
    }
}
#[allow(dead_code)] // See the note above `handoff_state_to_str`.
fn revision_kind_from_columns(kind: &str, new_body: Option<String>) -> Result<RevisionKind> {
    match kind {
        "replacement" => Ok(RevisionKind::Replacement {
            new_body: new_body.ok_or_else(|| rejected("replacement revision missing new_body"))?,
        }),
        "tombstone" => Ok(RevisionKind::Tombstone),
        _ => Err(rejected("invalid stored revision kind")),
    }
}
fn principal_to_columns(p: Principal) -> (&'static str, String) {
    match p {
        Principal::User(id) => ("user", id.to_string()),
        Principal::Agent(id) => ("agent", id.to_string()),
    }
}
fn principal_from_columns(kind: &str, id: &str) -> Result<Principal> {
    let parsed = parse_uuid(id, "invalid stored principal identity")?;
    match kind {
        "user" => Ok(Principal::User(parsed)),
        "agent" => Ok(Principal::Agent(parsed)),
        _ => Err(rejected("invalid stored principal kind")),
    }
}

// --- small shared conversions ---------------------------------------------------------------

fn parse_uuid(s: &str, what: &'static str) -> Result<Uuid> {
    s.parse().map_err(|_| rejected(what))
}
fn parse_opt_uuid(s: Option<String>, what: &'static str) -> Result<Option<Uuid>> {
    s.map(|s| parse_uuid(&s, what)).transpose()
}
fn from_unix(secs: i64) -> Result<DateTime<Utc>> {
    DateTime::from_timestamp(secs, 0).ok_or_else(|| rejected("invalid stored timestamp"))
}
fn as_u32(n: i64) -> Result<u32> {
    u32::try_from(n).map_err(|_| rejected("invalid stored counter"))
}
fn as_u64(n: i64) -> Result<u64> {
    u64::try_from(n).map_err(|_| rejected("invalid stored counter"))
}
/// The other direction: a caller-supplied `u64` (a `MessagePage` cursor bound) being bound
/// into a query parameter. rusqlite has no `ToSql` impl for `u64` (it can exceed `i64`'s
/// range, so an unchecked cast could silently wrap), so every such value needs this checked
/// conversion first -- same idea as `bots_conversation_mark_read`'s `up_to_sequence` check.
fn as_i64(n: u64) -> Result<i64> {
    i64::try_from(n).map_err(|_| rejected("invalid request: sequence out of range"))
}

// --- row -> domain-type conversions -----------------------------------------------------------

type AgentRow = (
    String,
    String,
    String,
    i64,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    i64,
    i64,
    i64,
);
fn agent_profile_from_row(row: AgentRow) -> Result<AgentProfile> {
    let (
        id,
        owner,
        name,
        role_revision,
        runtime_kind,
        preferred_host,
        capability_policy_ref,
        provider_account_ref,
        memory_namespace,
        archived,
        created_at,
        updated_at,
    ) = row;
    Ok(AgentProfile {
        id: parse_uuid(&id, "invalid stored agent identity")?,
        owner: parse_uuid(&owner, "invalid stored owner identity")?,
        name,
        role_revision: as_u32(role_revision)?,
        runtime_kind: runtime_kind_from_str(&runtime_kind)?,
        preferred_host: parse_opt_uuid(preferred_host, "invalid stored host identity")?,
        capability_policy_ref,
        provider_account_ref: parse_opt_uuid(
            provider_account_ref,
            "invalid stored provider account identity",
        )?,
        memory_namespace,
        archived: archived != 0,
        created_at: from_unix(created_at)?,
        updated_at: from_unix(updated_at)?,
    })
}

type ConversationRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    i64,
    Option<String>,
);
fn conversation_from_row(row: ConversationRow) -> Result<Conversation> {
    let (
        id,
        owner,
        kind,
        project_id,
        coordinator,
        storage_scope,
        policy_revision,
        created_at,
        title,
    ) = row;
    Ok(Conversation {
        title,
        id: parse_uuid(&id, "invalid stored conversation identity")?,
        owner: parse_uuid(&owner, "invalid stored owner identity")?,
        kind: conversation_kind_from_str(&kind)?,
        project_id: parse_opt_uuid(project_id, "invalid stored project identity")?,
        coordinator: parse_opt_uuid(coordinator, "invalid stored coordinator identity")?,
        storage_scope: storage_scope_from_str(&storage_scope)?,
        policy_revision: as_u32(policy_revision)?,
        created_at: from_unix(created_at)?,
    })
}

type MessageRow = (
    String,
    String,
    Option<String>,
    String,
    String,
    i64,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
);
fn message_from_row(row: MessageRow) -> Result<Message> {
    let (
        id,
        conversation_id,
        thread_root,
        author_kind,
        author_id,
        server_sequence,
        client_request_id,
        kind,
        body,
        attachment_refs,
        task_ref,
        turn_ref,
        source_event_ref,
        created_at,
    ) = row;
    Ok(Message {
        id: parse_uuid(&id, "invalid stored message identity")?,
        conversation_id: parse_uuid(&conversation_id, "invalid stored conversation identity")?,
        thread_root: parse_opt_uuid(thread_root, "invalid stored thread root")?,
        author: principal_from_columns(&author_kind, &author_id)?,
        server_sequence: as_u64(server_sequence)?,
        client_request_id,
        kind: message_kind_from_str(&kind)?,
        body,
        attachment_refs: decode(&attachment_refs)?,
        task_ref: parse_opt_uuid(task_ref, "invalid stored task reference")?,
        turn_ref,
        source_event_ref,
        created_at: from_unix(created_at)?,
    })
}

type HandoffRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    i64,
    String,
    Option<String>,
    i64,
);
fn handoff_from_row(row: HandoffRow) -> Result<Handoff> {
    let (
        id,
        source_agent,
        target_agent,
        project_id,
        task_or_question,
        acceptance_criteria,
        artifact_refs,
        allowed_tools,
        parent_run,
        reply_to_thread,
        budgets,
        deadline,
        depth,
        state,
        receipt,
        created_at,
    ) = row;
    Ok(Handoff {
        id: parse_uuid(&id, "invalid stored handoff identity")?,
        source_agent: parse_uuid(&source_agent, "invalid stored source agent identity")?,
        target_agent: parse_uuid(&target_agent, "invalid stored target agent identity")?,
        project_id: parse_opt_uuid(project_id, "invalid stored project identity")?,
        task_or_question,
        acceptance_criteria,
        artifact_refs: decode(&artifact_refs)?,
        allowed_tools: decode(&allowed_tools)?,
        parent_run: parse_opt_uuid(parent_run, "invalid stored parent run")?,
        reply_to_thread: parse_opt_uuid(reply_to_thread, "invalid stored reply thread")?,
        budgets: decode(&budgets)?,
        deadline: from_unix(deadline)?,
        depth: as_u32(depth)?,
        state: handoff_state_from_str(&state)?,
        receipt: match receipt {
            Some(r) => Some(decode(&r)?),
            None => None,
        },
        created_at: from_unix(created_at)?,
    })
}

impl LocalHubStore {
    // --- agents -------------------------------------------------------------------------------

    pub fn bots_agents_list(&self, owner: UserId) -> Result<Vec<AgentProfile>> {
        self.transaction(|tx| {
            let mut q = tx
                .prepare(
                    "SELECT id,owner,name,role_revision,runtime_kind,preferred_host,\
                     capability_policy_ref,provider_account_ref,memory_namespace,archived,\
                     created_at,updated_at FROM agent_profiles \
                     WHERE owner=?1 AND archived=0 ORDER BY name,id LIMIT 1000",
                )
                .map_err(db_error)?;
            let rows = q
                .query_map(params![owner.to_string()], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, String>(8)?,
                        r.get::<_, i64>(9)?,
                        r.get::<_, i64>(10)?,
                        r.get::<_, i64>(11)?,
                    ))
                })
                .map_err(db_error)?;
            rows.map(|r| agent_profile_from_row(r.map_err(db_error)?))
                .collect()
        })
    }

    pub fn bots_agents_create(&self, draft: NewAgentProfile) -> Result<AgentProfile> {
        check_text(&draft.name, 200)?;
        check_text(&draft.capability_policy_ref, 500)?;
        check_text(&draft.memory_namespace, 200)?;
        let id = Uuid::new_v4();
        let ts = now();
        self.transaction(|tx| {
            tx.execute(
                "INSERT INTO agent_profiles VALUES(?1,?2,?3,1,?4,?5,?6,?7,?8,0,?9,?9)",
                params![
                    id.to_string(),
                    draft.owner.to_string(),
                    draft.name,
                    runtime_kind_to_str(draft.runtime_kind),
                    draft.preferred_host.map(|h| h.to_string()),
                    draft.capability_policy_ref,
                    draft.provider_account_ref.map(|p| p.to_string()),
                    draft.memory_namespace,
                    ts,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })?;
        self.bots_agent_get(id)
    }

    /// Not part of `BotsService` (which has no single-agent read), but the natural building
    /// block for `bots_agents_create`'s return value and `bots_agents_update`/`_archive`'s
    /// ownership checks.
    fn bots_agent_get(&self, id: Uuid) -> Result<AgentProfile> {
        self.transaction(|tx| {
            let row: Option<AgentRow> = tx
                .query_row(
                    "SELECT id,owner,name,role_revision,runtime_kind,preferred_host,\
                     capability_policy_ref,provider_account_ref,memory_namespace,archived,\
                     created_at,updated_at FROM agent_profiles WHERE id=?1",
                    params![id.to_string()],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, Option<String>>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, Option<String>>(7)?,
                            r.get::<_, String>(8)?,
                            r.get::<_, i64>(9)?,
                            r.get::<_, i64>(10)?,
                            r.get::<_, i64>(11)?,
                        ))
                    },
                )
                .optional()
                .map_err(db_error)?;
            let row = row.ok_or_else(|| rejected("not found: agent"))?;
            agent_profile_from_row(row)
        })
    }

    pub fn bots_agents_update(
        &self,
        actor: UserId,
        agent_id: AgentId,
        patch: AgentProfilePatch,
    ) -> Result<AgentProfile> {
        let existing = self.bots_agent_get(agent_id)?;
        if existing.owner != actor {
            return Err(rejected("forbidden: not this agent's owner"));
        }
        let ts = now();
        self.transaction(|tx| {
            tx.execute(
                "UPDATE agent_profiles SET \
                 name=COALESCE(?2,name), \
                 capability_policy_ref=COALESCE(?3,capability_policy_ref), \
                 memory_namespace=COALESCE(?4,memory_namespace), \
                 updated_at=?5 WHERE id=?1",
                params![
                    agent_id.to_string(),
                    patch.name,
                    patch.capability_policy_ref,
                    patch.memory_namespace,
                    ts,
                ],
            )
            .map_err(db_error)?;
            // preferred_host is Option<Option<NodeId>>: None means "leave as is", so it needs
            // its own conditional statement rather than a COALESCE (NULL is itself a valid
            // target value -- clearing the host -- so COALESCE can't distinguish "don't touch"
            // from "clear it").
            if let Some(new_host) = patch.preferred_host {
                tx.execute(
                    "UPDATE agent_profiles SET preferred_host=?2 WHERE id=?1",
                    params![agent_id.to_string(), new_host.map(|h| h.to_string())],
                )
                .map_err(db_error)?;
            }
            Ok(())
        })?;
        self.bots_agent_get(agent_id)
    }

    pub fn bots_agents_archive(&self, actor: UserId, agent_id: AgentId) -> Result<()> {
        let existing = self.bots_agent_get(agent_id)?;
        if existing.owner != actor {
            return Err(rejected("forbidden: not this agent's owner"));
        }
        let ts = now();
        self.transaction(|tx| {
            tx.execute(
                "UPDATE agent_profiles SET archived=1,updated_at=?2 WHERE id=?1",
                params![agent_id.to_string(), ts],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    // --- conversations --------------------------------------------------------------------

    pub fn bots_conversations_create(&self, draft: NewConversation) -> Result<Conversation> {
        let id = Uuid::new_v4();
        let ts = now();
        self.transaction(|tx| {
            tx.execute(
                "INSERT INTO conversations(id,owner,kind,project_id,coordinator,storage_scope,policy_revision,created_at,title) VALUES(?1,?2,?3,?4,?5,?6,1,?7,?8)",
                params![
                    id.to_string(),
                    draft.owner.to_string(),
                    conversation_kind_to_str(draft.kind),
                    draft.project_id.map(|p| p.to_string()),
                    draft.coordinator.map(|c| c.to_string()),
                    storage_scope_to_str(draft.storage_scope),
                    ts,
                    draft.title,
                ],
            )
            .map_err(db_error)?;
            let owner_actions = encode(&vec![
                MemberAction::Read,
                MemberAction::Post,
                MemberAction::Manage,
            ])?;
            tx.execute(
                "INSERT INTO conversation_members VALUES(?1,'user',?2,?3,?4,?4)",
                params![id.to_string(), draft.owner.to_string(), owner_actions, ts],
            )
            .map_err(db_error)?;
            if let Some(coordinator) = draft.coordinator {
                let agent_actions = encode(&vec![MemberAction::Read, MemberAction::Post])?;
                tx.execute(
                    "INSERT INTO conversation_members VALUES(?1,'agent',?2,?3,?4,?4)",
                    params![id.to_string(), coordinator.to_string(), agent_actions, ts],
                )
                .map_err(db_error)?;
            }
            Ok(())
        })?;
        self.bots_conversation_get(id)
    }

    fn bots_conversation_get(&self, id: Uuid) -> Result<Conversation> {
        self.transaction(|tx| {
            let row: Option<ConversationRow> = tx
                .query_row(
                    "SELECT id,owner,kind,project_id,coordinator,storage_scope,policy_revision,\
                     created_at,title FROM conversations WHERE id=?1",
                    params![id.to_string()],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, Option<String>>(3)?,
                            r.get::<_, Option<String>>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, i64>(6)?,
                            r.get::<_, i64>(7)?,
                            r.get::<_, Option<String>>(8)?,
                        ))
                    },
                )
                .optional()
                .map_err(db_error)?;
            let row = row.ok_or_else(|| rejected("not found: conversation"))?;
            conversation_from_row(row)
        })
    }

    pub fn bots_conversations_list(&self, actor: Principal) -> Result<Vec<Conversation>> {
        let (kind, id) = principal_to_columns(actor);
        self.transaction(|tx| {
            let mut q = tx
                .prepare(
                    "SELECT c.id,c.owner,c.kind,c.project_id,c.coordinator,c.storage_scope,\
                     c.policy_revision,c.created_at,c.title FROM conversations c \
                     JOIN conversation_members m ON m.conversation_id=c.id \
                     WHERE m.principal_kind=?1 AND m.principal_id=?2 \
                     ORDER BY c.created_at DESC LIMIT 1000",
                )
                .map_err(db_error)?;
            let rows = q
                .query_map(params![kind, id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, i64>(7)?,
                        r.get::<_, Option<String>>(8)?,
                    ))
                })
                .map_err(db_error)?;
            rows.map(|r| conversation_from_row(r.map_err(db_error)?))
                .collect()
        })
    }

    pub fn bots_room_agents(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
    ) -> Result<Vec<AgentProfile>> {
        self.bots_require_member(conversation_id, actor, MemberAction::Read)?;
        let conversation = self.bots_conversation_get(conversation_id)?;
        let ids: Vec<String> = self.transaction(|tx| {
            let mut q = tx.prepare("SELECT principal_id FROM conversation_members WHERE conversation_id=?1 AND principal_kind='agent'").map_err(db_error)?;
            let rows = q.query_map([conversation_id.to_string()], |r| r.get(0)).map_err(db_error)?;
            rows.collect::<std::result::Result<Vec<String>, _>>().map_err(db_error)
        })?;
        Ok(self
            .bots_agents_list(conversation.owner)?
            .into_iter()
            .filter(|a| ids.contains(&a.id.to_string()))
            .collect())
    }

    /// True if `actor` is a member of `conversation_id` with `required` among its allowed
    /// actions. The shared authorization check every other Bots method (beyond agent
    /// ownership) goes through.
    fn bots_require_member(
        &self,
        conversation_id: ConversationId,
        actor: Principal,
        required: MemberAction,
    ) -> Result<()> {
        let (kind, id) = principal_to_columns(actor);
        self.transaction(|tx| {
            let actions: Option<String> = tx
                .query_row(
                    "SELECT allowed_actions FROM conversation_members \
                     WHERE conversation_id=?1 AND principal_kind=?2 AND principal_id=?3",
                    params![conversation_id.to_string(), kind, id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            let actions: Vec<MemberAction> = match actions {
                Some(a) => decode(&a)?,
                None => return Err(rejected("forbidden: not a member of this conversation")),
            };
            if actions.contains(&required) {
                Ok(())
            } else {
                Err(rejected(
                    "forbidden: missing the required conversation permission",
                ))
            }
        })
    }

    /// C1 has no invitation mechanism yet (room invites are C2+), so the only principal
    /// allowed to join a conversation today is one that already belongs to its owning
    /// account -- the owning user, or one of that user's own agents. Every caller (FFI, CLI,
    /// any future transport) goes through this now; it isn't a substitute for the FFI
    /// bridge's own pre-check (`ohhive-ffi/src/bots.rs`'s `conversations_join`), which stays
    /// as defense in depth, but core no longer depends on every caller reimplementing it
    /// (gap flagged in Sif's 2026-09-15 FFI handoff).
    fn bots_authorize_join(&self, actor: Principal, conversation: &Conversation) -> Result<()> {
        let allowed = match actor {
            Principal::User(id) => id == conversation.owner,
            Principal::Agent(id) => self.bots_agent_owner(id)? == conversation.owner,
        };
        if allowed {
            Ok(())
        } else {
            Err(rejected(
                "forbidden: cannot join another account's conversation",
            ))
        }
    }

    /// The owning user of one agent, straight off `agent_profiles` -- `bots_agents_list`
    /// only lists by owner, there was no single-row lookup by agent id yet.
    fn bots_agent_owner(&self, agent_id: AgentId) -> Result<UserId> {
        self.transaction(|tx| {
            let owner: Option<String> = tx
                .query_row(
                    "SELECT owner FROM agent_profiles WHERE id=?1",
                    params![agent_id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            match owner {
                Some(o) => Uuid::parse_str(&o).map_err(|_| rejected("corrupt: agent owner id")),
                None => Err(rejected("not found: agent")),
            }
        })
    }

    /// Bare membership existence, no specific `MemberAction` required -- used to validate a
    /// `message_send` recipient is actually in the conversation before an `agent_deliveries`
    /// row is created for them, distinct from `bots_require_member`'s permission check on the
    /// *sender*.
    fn bots_is_member(&self, conversation_id: ConversationId, actor: Principal) -> Result<bool> {
        let (kind, id) = principal_to_columns(actor);
        self.transaction(|tx| {
            let exists: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM conversation_members \
                     WHERE conversation_id=?1 AND principal_kind=?2 AND principal_id=?3",
                    params![conversation_id.to_string(), kind, id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            Ok(exists.is_some())
        })
    }

    pub fn bots_conversations_join(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
    ) -> Result<ConversationMember> {
        let conversation = self.bots_conversation_get(conversation_id)?; // 404s if the room doesn't exist
        self.bots_authorize_join(actor, &conversation)?;
        let (kind, id) = principal_to_columns(actor);
        let ts = now();
        self.transaction(|tx| {
            let existing: Option<(String, i64, i64)> = tx
                .query_row(
                    "SELECT allowed_actions,history_boundary,joined_at FROM conversation_members \
                     WHERE conversation_id=?1 AND principal_kind=?2 AND principal_id=?3",
                    params![conversation_id.to_string(), kind, id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(db_error)?;
            if existing.is_none() {
                let default_actions = encode(&vec![MemberAction::Read, MemberAction::Post])?;
                tx.execute(
                    "INSERT INTO conversation_members VALUES(?1,?2,?3,?4,?5,?5)",
                    params![conversation_id.to_string(), kind, id, default_actions, ts],
                )
                .map_err(db_error)?;
            }
            Ok(())
        })?;
        self.transaction(|tx| {
            let row: (String, i64, i64) = tx
                .query_row(
                    "SELECT allowed_actions,history_boundary,joined_at FROM conversation_members \
                     WHERE conversation_id=?1 AND principal_kind=?2 AND principal_id=?3",
                    params![conversation_id.to_string(), kind, id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(db_error)?;
            let (allowed_actions, history_boundary, joined_at) = row;
            Ok(ConversationMember {
                conversation_id,
                principal: actor,
                allowed_actions: decode(&allowed_actions)?,
                history_boundary: from_unix(history_boundary)?,
                joined_at: from_unix(joined_at)?,
            })
        })
    }

    // --- messages -----------------------------------------------------------------------------

    pub fn bots_messages_list(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        page: MessagePage,
    ) -> Result<Vec<Message>> {
        self.bots_require_member(conversation_id, actor, MemberAction::Read)?;
        if page.limit == 0 {
            return Err(rejected(
                "invalid request: page limit must be greater than zero",
            ));
        }
        let limit = page.limit.min(500);
        let after = page.after.map(as_i64).transpose()?;
        let before = page.before.map(as_i64).transpose()?;
        self.transaction(|tx| {
            // `before` alone means "the page immediately preceding this point" -- the NEWEST
            // messages below it, not the oldest in the conversation. A plain
            // `ORDER BY server_sequence ASC LIMIT n` returns sequences 1..n, which for a
            // conversation longer than the window is the wrong end entirely: the executor's turn
            // context (`bots/executor.rs`, HISTORY_WINDOW) then hands an agent the opening of the
            // conversation and never the thread it is replying to, and scroll-back paging in the
            // apps walks forward instead of back. Invisible in tests because none exceeded the
            // window. Reported as §3.4 of the 2026-09-15 audit.
            //
            // So when only `before` is set, take the last `limit` rows and re-order them ascending
            // for the caller, which still gets oldest-first output. `after` (forward paging) and
            // the unbounded case keep their existing ASC behavior.
            let sql = if before.is_some() && after.is_none() {
                "SELECT * FROM (\
                   SELECT id,conversation_id,thread_root,author_kind,author_id,\
                   server_sequence,client_request_id,kind,body,attachment_refs,task_ref,\
                   turn_ref,source_event_ref,created_at FROM messages \
                   WHERE conversation_id=?1 \
                   AND (?2 IS NULL OR server_sequence > ?2) \
                   AND (?3 IS NULL OR server_sequence < ?3) \
                   ORDER BY server_sequence DESC LIMIT ?4\
                 ) ORDER BY server_sequence ASC"
            } else {
                "SELECT id,conversation_id,thread_root,author_kind,author_id,\
                 server_sequence,client_request_id,kind,body,attachment_refs,task_ref,\
                 turn_ref,source_event_ref,created_at FROM messages \
                 WHERE conversation_id=?1 \
                 AND (?2 IS NULL OR server_sequence > ?2) \
                 AND (?3 IS NULL OR server_sequence < ?3) \
                 ORDER BY server_sequence ASC LIMIT ?4"
            };
            let mut q = tx.prepare(sql).map_err(db_error)?;
            let rows = q
                .query_map(
                    params![conversation_id.to_string(), after, before, limit],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, i64>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, String>(7)?,
                            r.get::<_, Option<String>>(8)?,
                            r.get::<_, String>(9)?,
                            r.get::<_, Option<String>>(10)?,
                            r.get::<_, Option<String>>(11)?,
                            r.get::<_, Option<String>>(12)?,
                            r.get::<_, i64>(13)?,
                        ))
                    },
                )
                .map_err(db_error)?;
            rows.map(|r| message_from_row(r.map_err(db_error)?))
                .collect()
        })
    }

    #[allow(clippy::too_many_arguments)]
    /// Unchanged C1 entry point: a human-originated send with no causation and no hold. Kept as
    /// a delegating wrapper so every existing caller (FFI, CLI, transport dispatch) compiles
    /// untouched -- adding the Track A parameters to this signature instead would have churned
    /// every call site, including ones being edited concurrently for the demo slices.
    pub fn bots_message_send(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
    ) -> Result<Message> {
        self.bots_message_send_with_cause(
            actor,
            conversation_id,
            client_request_id,
            expected_policy_revision,
            recipient_ids,
            draft,
            None,
            false,
        )
    }

    /// Track A slice 2. `cause` records why this message's deliveries exist (`None` = a human
    /// send: depth 0, the new message is its own chain root). `hold` creates them `Held` instead
    /// of `Pending`, for a chain that reached `max_turns_per_root` and is waiting on a person.
    ///
    /// Both are written in the *same transaction* as the message and its delivery rows. If depth
    /// were stamped afterwards, a concurrent drain could claim a delivery still showing the
    /// default 0 and walk straight past `max_depth`.
    ///
    /// A `MessageKind::System` message never creates deliveries, whatever recipients are passed
    /// (`types.rs`: "these must NOT wake every participant"). That is what makes a budget-
    /// exhaustion notice free to post, and it closes the six-agents-times-every-status-post
    /// problem before room fan-out can create it.
    #[allow(clippy::too_many_arguments)]
    pub fn bots_message_send_with_cause(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
        cause: Option<DeliveryCause>,
        hold: bool,
    ) -> Result<Message> {
        check_text(&client_request_id, 200)?;
        self.bots_require_member(conversation_id, actor, MemberAction::Post)?;
        let conversation = self.bots_conversation_get(conversation_id)?;
        if conversation.policy_revision != expected_policy_revision {
            return Err(rejected("conflict: conversation policy revision changed"));
        }
        for recipient in &recipient_ids {
            if !self.bots_is_member(conversation_id, Principal::Agent(*recipient))? {
                return Err(rejected(
                    "invalid request: recipient is not a member of this conversation",
                ));
            }
        }
        if let Some(root) = draft.thread_root {
            if self.bots_message_get(root)?.conversation_id != conversation_id {
                return Err(rejected(
                    "invalid request: thread_root belongs to another conversation",
                ));
            }
        }
        let (author_kind, author_id) = principal_to_columns(actor);
        let ts = now();
        let message_id = self.transaction(|tx| {
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM messages WHERE conversation_id=?1 AND client_request_id=?2",
                    params![conversation_id.to_string(), client_request_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            if let Some(id) = existing {
                return Ok(id);
            }
            let next_sequence: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(server_sequence),0)+1 FROM messages WHERE conversation_id=?1",
                    params![conversation_id.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            let id = Uuid::new_v4();
            tx.execute(
                "INSERT INTO messages VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                params![
                    id.to_string(),
                    conversation_id.to_string(),
                    draft.thread_root.map(|t| t.to_string()),
                    author_kind,
                    author_id,
                    next_sequence,
                    client_request_id,
                    message_kind_to_str(draft.kind),
                    draft.body,
                    encode(&draft.attachment_refs)?,
                    draft.task_ref.map(|t| t.to_string()),
                    draft.turn_ref,
                    draft.source_event_ref,
                    ts,
                ],
            )
            .map_err(db_error)?;
            // System messages are persisted and readable but wake nobody, regardless of the
            // recipient list handed in.
            if draft.kind != MessageKind::System {
                let status = if hold { "held" } else { "pending" };
                let root = cause
                    .map(|c| c.root_message_id.to_string())
                    .unwrap_or_else(|| id.to_string());
                let depth = cause.map(|c| c.depth).unwrap_or(0);
                for recipient in &recipient_ids {
                    tx.execute(
                        "INSERT INTO agent_deliveries(message_id,recipient,status,\
                         lease_generation,retry_deadline,bound_runtime_session,bound_turn_ref,\
                         updated_at,cause_message_id,root_message_id,turn_depth) \
                         VALUES(?1,?2,?3,0,NULL,NULL,NULL,?4,?5,?6,?7)",
                        params![
                            id.to_string(),
                            recipient.to_string(),
                            status,
                            ts,
                            cause.map(|c| c.cause_message_id.to_string()),
                            root,
                            as_i64(depth as u64)?,
                        ],
                    )
                    .map_err(db_error)?;
                }
            }
            Ok(id.to_string())
        })?;
        self.transaction(|tx| {
            let row: MessageRow = tx
                .query_row(
                    "SELECT id,conversation_id,thread_root,author_kind,author_id,\
                     server_sequence,client_request_id,kind,body,attachment_refs,task_ref,\
                     turn_ref,source_event_ref,created_at FROM messages WHERE id=?1",
                    params![message_id],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, i64>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, String>(7)?,
                            r.get::<_, Option<String>>(8)?,
                            r.get::<_, String>(9)?,
                            r.get::<_, Option<String>>(10)?,
                            r.get::<_, Option<String>>(11)?,
                            r.get::<_, Option<String>>(12)?,
                            r.get::<_, i64>(13)?,
                        ))
                    },
                )
                .map_err(db_error)?;
            message_from_row(row)
        })
    }

    pub fn bots_conversation_mark_read(
        &self,
        actor: UserId,
        conversation_id: ConversationId,
        up_to_sequence: u64,
    ) -> Result<ConversationReadPosition> {
        self.bots_require_member(conversation_id, Principal::User(actor), MemberAction::Read)?;
        let up_to_sequence = i64::try_from(up_to_sequence)
            .map_err(|_| rejected("invalid request: sequence out of range"))?;
        let ts = now();
        self.transaction(|tx| {
            tx.execute(
                "INSERT INTO conversation_read_positions VALUES(?1,?2,?3,?4) \
                 ON CONFLICT(user_id,conversation_id) DO UPDATE SET \
                 last_seen_sequence=max(last_seen_sequence,excluded.last_seen_sequence), \
                 updated_at=?4",
                params![
                    actor.to_string(),
                    conversation_id.to_string(),
                    up_to_sequence,
                    ts
                ],
            )
            .map_err(db_error)?;
            let row: (i64, i64) = tx
                .query_row(
                    "SELECT last_seen_sequence,updated_at FROM conversation_read_positions \
                     WHERE user_id=?1 AND conversation_id=?2",
                    params![actor.to_string(), conversation_id.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(db_error)?;
            Ok(ConversationReadPosition {
                user_id: actor,
                conversation_id,
                last_seen_sequence: as_u64(row.0)?,
                updated_at: from_unix(row.1)?,
            })
        })
    }

    // --- handoffs -------------------------------------------------------------------------

    pub fn bots_handoff_create(&self, request: NewHandoff) -> Result<Handoff> {
        check_text(&request.task_or_question, 20_000)?;
        check_text(&request.acceptance_criteria, 20_000)?;
        let id = Uuid::new_v4();
        let ts = now();
        let budgets = request.budgets.unwrap_or_default();
        // depth is always 0 here -- see the module doc's "what's deliberately not here yet".
        self.transaction(|tx| {
            tx.execute(
                "INSERT INTO handoffs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,0,'requested',NULL,?13)",
                params![
                    id.to_string(),
                    request.source_agent.to_string(),
                    request.target_agent.to_string(),
                    request.project_id.map(|p| p.to_string()),
                    request.task_or_question,
                    request.acceptance_criteria,
                    encode(&request.artifact_refs)?,
                    encode(&request.allowed_tools)?,
                    request.parent_run.map(|p| p.to_string()),
                    request.reply_to_thread.map(|t| t.to_string()),
                    encode(&budgets)?,
                    request.deadline.timestamp(),
                    ts,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })?;
        self.bots_handoff_get(id)
    }

    fn bots_handoff_get(&self, id: Uuid) -> Result<Handoff> {
        self.transaction(|tx| {
            let row: Option<HandoffRow> = tx
                .query_row(
                    "SELECT id,source_agent,target_agent,project_id,task_or_question,\
                     acceptance_criteria,artifact_refs,allowed_tools,parent_run,reply_to_thread,\
                     budgets,deadline,depth,state,receipt,created_at FROM handoffs WHERE id=?1",
                    params![id.to_string()],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, Option<String>>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, String>(7)?,
                            r.get::<_, Option<String>>(8)?,
                            r.get::<_, Option<String>>(9)?,
                            r.get::<_, String>(10)?,
                            r.get::<_, i64>(11)?,
                            r.get::<_, i64>(12)?,
                            r.get::<_, String>(13)?,
                            r.get::<_, Option<String>>(14)?,
                            r.get::<_, i64>(15)?,
                        ))
                    },
                )
                .optional()
                .map_err(db_error)?;
            let row = row.ok_or_else(|| rejected("not found: handoff"))?;
            handoff_from_row(row)
        })
    }

    /// Scoping decision (no ACL model exists for handoffs beyond source/target agents): a
    /// `Principal::Agent` may view a handoff only if it's the source or target; any
    /// `Principal::User` may view any handoff, since users aren't tracked as handoff
    /// participants in this schema at all.
    pub fn bots_handoff_status(&self, actor: Principal, handoff_id: HandoffId) -> Result<Handoff> {
        let handoff = self.bots_handoff_get(handoff_id)?;
        if let Principal::Agent(id) = actor {
            if id != handoff.source_agent && id != handoff.target_agent {
                return Err(rejected("forbidden: not a party to this handoff"));
            }
        }
        Ok(handoff)
    }

    // --- deliveries -----------------------------------------------------------------------

    /// Authorization: the message's author, or the recipient agent itself, may cancel a
    /// delivery -- nobody else. No broader ACL is modeled for deliveries beyond that.
    ///
    /// Single transaction, not four: fetch author+current-status, authorize, check the
    /// terminal-state guard, update, and re-read the changed row for the return value -- all
    /// under one lock, so nothing can race between the check and the write.
    pub fn bots_delivery_cancel(
        &self,
        actor: Principal,
        delivery_key: DeliveryKey,
    ) -> Result<AgentDelivery> {
        self.transaction(|tx| {
            let row: Option<(String, String, String)> = tx
                .query_row(
                    "SELECT m.author_kind,m.author_id,d.status \
                     FROM agent_deliveries d JOIN messages m ON m.id=d.message_id \
                     WHERE d.message_id=?1 AND d.recipient=?2",
                    params![
                        delivery_key.message_id.to_string(),
                        delivery_key.recipient.to_string()
                    ],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(db_error)?;
            let (author_kind, author_id, current_status) =
                row.ok_or_else(|| rejected("not found: delivery"))?;
            let author = principal_from_columns(&author_kind, &author_id)?;
            let is_author = actor == author;
            let is_recipient = actor == Principal::Agent(delivery_key.recipient);
            if !is_author && !is_recipient {
                return Err(rejected(
                    "forbidden: not the message author or the recipient",
                ));
            }
            if matches!(current_status.as_str(), "done" | "failed" | "cancelled") {
                return Err(rejected("conflict: delivery already terminal"));
            }

            let ts = now();
            tx.execute(
                // Bump the generation as well as the status: a turn already in flight holds
                // the previous lease, and leaving the generation untouched let its eventual
                // complete/fail land on a delivery the user had already cancelled.
                "UPDATE agent_deliveries SET status='cancelled',\
                 lease_generation=lease_generation+1,updated_at=?3 \
                 WHERE message_id=?1 AND recipient=?2",
                params![
                    delivery_key.message_id.to_string(),
                    delivery_key.recipient.to_string(),
                    ts
                ],
            )
            .map_err(db_error)?;

            // Re-read through the shared helper rather than a second hand-written SELECT, so
            // the causation columns added in v10 come back here too and there is one place to
            // update next time the row grows. Note `held` is absent from the terminal check
            // above on purpose: a person may cancel a chain that is waiting on them instead of
            // releasing it.
            bots_delivery_row(tx, delivery_key, crate::bots::DeliveryStatus::Cancelled)
        })
    }

    /// A single message by id, with no membership check -- a caller that already holds a
    /// `DeliveryKey` (the delivery row itself proves the message was addressed to them) uses
    /// this to resolve the message's conversation before building turn context. Not part of
    /// `BotsService` (no single-message read is in the C0 contract); a natural building block
    /// like `bots_agent_get`.
    pub fn bots_message_get(&self, id: Uuid) -> Result<Message> {
        self.transaction(|tx| {
            let row: Option<MessageRow> = tx
                .query_row(
                    "SELECT id,conversation_id,thread_root,author_kind,author_id,\
                     server_sequence,client_request_id,kind,body,attachment_refs,task_ref,\
                     turn_ref,source_event_ref,created_at FROM messages WHERE id=?1",
                    params![id.to_string()],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, i64>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, String>(7)?,
                            r.get::<_, Option<String>>(8)?,
                            r.get::<_, String>(9)?,
                            r.get::<_, Option<String>>(10)?,
                            r.get::<_, Option<String>>(11)?,
                            r.get::<_, Option<String>>(12)?,
                            r.get::<_, i64>(13)?,
                        ))
                    },
                )
                .optional()
                .map_err(db_error)?;
            message_from_row(row.ok_or_else(|| rejected("not found: message"))?)
        })
    }

    /// Report pending work this local-only executor cannot run. Internal worker operation,
    /// never exposed as an RPC accepting an arbitrary owner. Keep deliveries pending so a
    /// later runner/host can recover them; notices are historical, not a liveness claim.
    pub fn bots_report_unroutable(
        &self,
        owner: UserId,
        host: Uuid,
        local_ready: bool,
    ) -> Result<usize> {
        self.transaction(|tx| {
            let mut q = tx.prepare(
                "SELECT d.message_id,d.recipient,m.conversation_id,m.thread_root,a.name,a.runtime_kind,a.preferred_host,a.archived \
                 FROM agent_deliveries d JOIN messages m ON m.id=d.message_id \
                 JOIN conversations c ON c.id=m.conversation_id JOIN agent_profiles a ON a.id=d.recipient \
                 WHERE d.status='pending' AND a.owner=?1 AND c.owner=?1 \
                 AND (a.archived<>0 OR a.runtime_kind<>'local' OR a.preferred_host IS NULL OR a.preferred_host<>?2 OR ?3=0) \
                 AND NOT EXISTS(SELECT 1 FROM messages n WHERE n.conversation_id=c.id \
                 AND n.client_request_id='unroutable:'||d.message_id||':'||d.recipient) \
                 ORDER BY d.updated_at,d.message_id,d.recipient LIMIT 100"
            ).map_err(db_error)?;
            let rows = q.query_map(params![owner.to_string(), host.to_string(), local_ready], |r| Ok((
                r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, Option<String>>(6)?, r.get::<_, bool>(7)?
            ))).map_err(db_error)?.collect::<std::result::Result<Vec<_>, _>>().map_err(db_error)?;
            drop(q);
            for (message, recipient, conversation, thread, name, runtime, preferred, archived) in &rows {
                let reason = if *archived {
                    "this agent is archived"
                } else if runtime != "local" {
                    match runtime.as_str() {
                        "anthropic_byok" => "the Anthropic reply service is unavailable; check your provider key in Settings and that the cloud reply service is deployed",
                        "nous_byok" => "the Nous reply service is unavailable; check your provider key in Settings and that the cloud reply service is deployed",
                        "chatgpt_subscription" => "the ChatGPT subscription reply connection is not implemented yet",
                        "copilot_subscription" => "the Copilot subscription reply connection is not implemented yet",
                        "grok_subscription" => "the Grok subscription reply connection is not implemented yet",
                        _ => "this agent’s reply connection is not implemented yet",
                    }
                } else if preferred.is_none() {
                    "no computer is assigned to this agent"
                } else if preferred.as_deref() != Some(host.to_string().as_str()) {
                    "this agent is assigned to another computer; this computer cannot run it, and its availability has not been verified"
                } else {
                    "a local model is not configured for replies on this computer; check the model settings"
                };
                let body = format!("{name} can't reply yet: {reason}. This message remains queued.");
                let sequence: i64 = tx.query_row("SELECT COALESCE(MAX(server_sequence),0)+1 FROM messages WHERE conversation_id=?1", [conversation], |r| r.get(0)).map_err(db_error)?;
                tx.execute("INSERT INTO messages(id,conversation_id,thread_root,author_kind,author_id,server_sequence,client_request_id,kind,body,attachment_refs,created_at) VALUES(?1,?2,?3,'user',?4,?5,?6,'system',?7,'[]',?8)", params![
                    Uuid::new_v4().to_string(), conversation, thread.as_ref().unwrap_or(message), owner.to_string(), sequence,
                    format!("unroutable:{message}:{recipient}"), body, now()
                ]).map_err(db_error)?;
            }
            Ok(rows.len())
        })
    }

    /// Pending deliveries addressed to one agent, oldest-eligible-first -- what a local
    /// executor loop drains. Excludes anything still in retry backoff (`retry_deadline` in the
    /// future, set by `bots_delivery_fail`'s `NoCapacity` path) so a busy node doesn't spin
    /// re-claiming the same delivery every poll -- verified against real sqlite3 before this
    /// port, same discipline as the rest of this file (`local_hub/bots.rs`'s own doc). Not
    /// membership-checked: the caller is this agent's own host process, not a user-facing read
    /// path (that's `messages_list`/`conversation_search`).
    pub fn bots_deliveries_pending_for_agent(
        &self,
        agent_id: AgentId,
        limit: u32,
    ) -> Result<Vec<AgentDelivery>> {
        let limit = limit.clamp(1, 200);
        let ts = now();
        self.transaction(|tx| {
            let mut q = tx
                .prepare(
                    "SELECT message_id,recipient,lease_generation,retry_deadline,\
                     bound_runtime_session,bound_turn_ref,updated_at,cause_message_id,\
                     root_message_id,turn_depth FROM agent_deliveries \
                     WHERE recipient=?1 AND status='pending' \
                     AND (retry_deadline IS NULL OR retry_deadline<=?2) \
                     ORDER BY updated_at,message_id LIMIT ?3",
                )
                .map_err(db_error)?;
            let rows = q
                .query_map(params![agent_id.to_string(), ts, limit], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, i64>(9)?,
                    ))
                })
                .map_err(db_error)?;
            rows.map(|r| {
                let (
                    message_id,
                    recipient,
                    lease_generation,
                    retry_deadline,
                    bound_runtime_session,
                    bound_turn_ref,
                    updated_at,
                    cause_message_id,
                    root_message_id,
                    turn_depth,
                ) = r.map_err(db_error)?;
                Ok(AgentDelivery {
                    key: DeliveryKey {
                        message_id: parse_uuid(&message_id, "invalid stored message id")?,
                        recipient: parse_uuid(&recipient, "invalid stored recipient")?,
                    },
                    status: crate::bots::DeliveryStatus::Pending,
                    lease_generation: as_u64(lease_generation)?,
                    retry_deadline: retry_deadline.map(from_unix).transpose()?,
                    bound_runtime_session: parse_opt_uuid(
                        bound_runtime_session,
                        "invalid stored runtime session",
                    )?,
                    bound_turn_ref,
                    updated_at: from_unix(updated_at)?,
                    cause_message_id: parse_opt_uuid(
                        cause_message_id,
                        "invalid stored cause message id",
                    )?,
                    root_message_id: parse_opt_uuid(
                        root_message_id,
                        "invalid stored root message id",
                    )?,
                    turn_depth: as_u32(turn_depth)?,
                })
            })
            .collect()
        })
    }

    /// `max_active_turns_per_agent`: how many of this agent's deliveries are mid-turn right
    /// now. The cheapest and strongest of the four Track A brakes -- an agent already thinking
    /// is not handed a second thought.
    pub fn bots_active_turns_for_agent(&self, agent_id: AgentId) -> Result<u32> {
        self.transaction(|tx| {
            let n: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM agent_deliveries WHERE recipient=?1 AND status='running'",
                    params![agent_id.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            as_u32(n)
        })
    }

    /// `max_turns_per_root`: every delivery ever created against one chain root, in one indexed
    /// COUNT. Counts held and terminal rows too -- the budget is "how much has this one thing
    /// Jack said cost", not "how much is in flight", so resolved turns must still count.
    pub fn bots_turns_for_root(&self, root_message_id: MessageId) -> Result<u32> {
        self.transaction(|tx| {
            let n: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM agent_deliveries WHERE root_message_id=?1",
                    params![root_message_id.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            as_u32(n)
        })
    }

    /// Release a chain a person has decided to let continue: every `Held` delivery for this root
    /// returns to `Pending`. Returns how many were released, so a caller can post an honest
    /// notice and so "this exchange took three approvals" is observable.
    ///
    /// Deliberately does NOT reset the root's turn count -- the released chain gets another
    /// `max_turns_per_root` worth of headroom *above the turns already spent*, so each release
    /// is one decision by one person rather than an unbounded reset.
    pub fn bots_deliveries_release_root(&self, root_message_id: MessageId) -> Result<u32> {
        let ts = now();
        self.transaction(|tx| {
            let n = tx
                .execute(
                    "UPDATE agent_deliveries SET status='pending',updated_at=?2 \
                     WHERE root_message_id=?1 AND status='held'",
                    params![root_message_id.to_string(), ts],
                )
                .map_err(db_error)?;
            as_u32(n as i64)
        })
    }

    /// Every currently-held delivery, oldest first -- what a UI lists as "waiting for you".
    pub fn bots_deliveries_held(&self, limit: u32) -> Result<Vec<AgentDelivery>> {
        let limit = limit.clamp(1, 200);
        self.transaction(|tx| {
            let mut q = tx
                .prepare(
                    "SELECT message_id,recipient FROM agent_deliveries WHERE status='held' \
                     ORDER BY updated_at,message_id LIMIT ?1",
                )
                .map_err(db_error)?;
            let keys: Vec<(String, String)> = q
                .query_map(params![limit], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(db_error)?
                .collect::<std::result::Result<_, _>>()
                .map_err(db_error)?;
            keys.into_iter()
                .map(|(message_id, recipient)| {
                    let key = DeliveryKey {
                        message_id: parse_uuid(&message_id, "invalid stored message id")?,
                        recipient: parse_uuid(&recipient, "invalid stored recipient")?,
                    };
                    bots_delivery_row(tx, key, crate::bots::DeliveryStatus::Held)
                })
                .collect()
        })
    }

    /// Claim one pending delivery for execution: atomically moves it `pending -> running` and
    /// bumps `lease_generation`, so `bots_delivery_complete`/`bots_delivery_fail` (which both
    /// require the caller's `lease_generation` to still match) can never resolve a delivery
    /// some other/later claim has since moved past -- same generation-fencing idea as
    /// `subscription::Reducer` (ADR-033 Stage 1). A concurrent claim, an already-cancelled
    /// delivery, or one still in retry backoff all fail the `WHERE`, so this returns a
    /// `conflict` rather than silently doing nothing. Dry-run verified against real sqlite3
    /// (happy path, double-claim race, and retry-backoff gating) before this port.
    pub fn bots_delivery_claim(&self, delivery_key: DeliveryKey) -> Result<AgentDelivery> {
        let ts = now();
        self.transaction(|tx| {
            let updated = tx
                .execute(
                    "UPDATE agent_deliveries SET status='running',\
                     lease_generation=lease_generation+1,updated_at=?3 \
                     WHERE message_id=?1 AND recipient=?2 AND status='pending' \
                     AND (retry_deadline IS NULL OR retry_deadline<=?3)",
                    params![
                        delivery_key.message_id.to_string(),
                        delivery_key.recipient.to_string(),
                        ts
                    ],
                )
                .map_err(db_error)?;
            if updated == 0 {
                return Err(rejected("conflict: delivery is not claimable"));
            }
            bots_delivery_row(tx, delivery_key, crate::bots::DeliveryStatus::Running)
        })
    }

    /// Mark a claimed delivery done. `lease_generation` must match the value
    /// `bots_delivery_claim` returned -- a mismatch means a later claim (after a crash or
    /// timeout requeue) already owns this delivery, and this stale caller must not resolve it
    /// out from under that one.
    pub fn bots_delivery_complete(
        &self,
        delivery_key: DeliveryKey,
        lease_generation: u64,
    ) -> Result<AgentDelivery> {
        self.bots_delivery_finish(delivery_key, lease_generation, "done", None)
    }

    /// Mark a claimed delivery failed, or -- when `retry_after` is `Some` -- requeue it as
    /// `pending` with that `retry_deadline` instead (the `LocalTurnError::NoCapacity` case: a
    /// busy node is not a broken turn, per `local_executor.rs`'s own doc). Either way,
    /// `lease_generation` must still match, same fencing as `bots_delivery_complete`.
    pub fn bots_delivery_fail(
        &self,
        delivery_key: DeliveryKey,
        lease_generation: u64,
        retry_after: Option<DateTime<Utc>>,
    ) -> Result<AgentDelivery> {
        match retry_after {
            Some(_) => {
                self.bots_delivery_finish(delivery_key, lease_generation, "pending", retry_after)
            }
            None => self.bots_delivery_finish(delivery_key, lease_generation, "failed", None),
        }
    }

    /// Shared by `bots_delivery_complete`/`bots_delivery_fail`: a single lease-generation-fenced
    /// `UPDATE` plus a re-select, matching `bots_delivery_cancel`'s "one transaction, not four"
    /// reasoning above. `status` is always a literal from this file, never caller input, so it's
    /// safe to interpolate as a bind parameter without a second validity check.
    fn bots_delivery_finish(
        &self,
        delivery_key: DeliveryKey,
        lease_generation: u64,
        status: &'static str,
        retry_deadline: Option<DateTime<Utc>>,
    ) -> Result<AgentDelivery> {
        let lease_generation = as_i64(lease_generation)?;
        let ts = now();
        let retry_ts = retry_deadline.map(|d| d.timestamp());
        self.transaction(|tx| {
            let updated = tx
                .execute(
                    // `AND status='running'` matters as much as the generation check (audit
                    // 3.5). Generation alone lets a resolved delivery be rewritten: claim
                    // (gen N) -> cancel -> the executor's NoCapacity path calls
                    // bots_delivery_fail(key, N, retry), which still matches gen N and writes
                    // status='pending', resurrecting a cancelled delivery for a later claim.
                    // It also let bots_delivery_complete(key, 0) mark a never-claimed row done.
                    // The Held human gate depends on this: a stale finish that can write
                    // 'pending' can walk a held chain straight past its own gate.
                    "UPDATE agent_deliveries SET status=?4,retry_deadline=?5,updated_at=?3 \
                     WHERE message_id=?1 AND recipient=?2 AND lease_generation=?6 \
                     AND status='running'",
                    params![
                        delivery_key.message_id.to_string(),
                        delivery_key.recipient.to_string(),
                        ts,
                        status,
                        retry_ts,
                        lease_generation
                    ],
                )
                .map_err(db_error)?;
            if updated == 0 {
                return Err(rejected("conflict: delivery lease is stale"));
            }
            bots_delivery_row(tx, delivery_key, delivery_status_from_row(status)?)
        })
    }

    // --- search -------------------------------------------------------------------------------

    pub fn bots_conversation_search(
        &self,
        actor: Principal,
        scope: SearchScope,
        query: String,
        cursor: Option<String>,
    ) -> Result<SearchPage> {
        check_text(&query, 2048)?;
        let terms: Vec<_> = query
            .split_whitespace()
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect();
        if terms.is_empty() {
            return Err(rejected("invalid request: empty search query"));
        }
        let fts_query = terms.join(" AND ");
        let offset: i64 = match &cursor {
            Some(c) => c
                .parse()
                .map_err(|_| rejected("invalid request: bad search cursor"))?,
            None => 0,
        };
        let (conversation_scope, project_scope) = match scope {
            SearchScope::Everything => (None, None),
            SearchScope::Conversation(id) => (Some(id.to_string()), None),
            SearchScope::Project(id) => (None, Some(id.to_string())),
        };
        let (principal_kind, principal_id) = principal_to_columns(actor);
        let rows: Vec<(MessageRow, f64)> = self.transaction(|tx| {
            let sql = "SELECT m.id,m.conversation_id,m.thread_root,m.author_kind,m.author_id,\
                       m.server_sequence,m.client_request_id,m.kind,m.body,m.attachment_refs,\
                       m.task_ref,m.turn_ref,m.source_event_ref,m.created_at,bm25(messages_fts) \
                       FROM messages_fts \
                       JOIN messages m ON m.rowid=messages_fts.rowid \
                       JOIN conversation_members mem ON mem.conversation_id=m.conversation_id \
                       AND mem.principal_kind=?1 AND mem.principal_id=?2 \
                       LEFT JOIN conversations c ON c.id=m.conversation_id \
                       WHERE messages_fts MATCH ?3 \
                       AND (?4 IS NULL OR m.conversation_id=?4) \
                       AND (?5 IS NULL OR c.project_id=?5) \
                       ORDER BY bm25(messages_fts),m.id LIMIT ?6 OFFSET ?7";
            let mut q = tx.prepare(sql).map_err(db_error)?;
            let mapped = q
                .query_map(
                    params![
                        principal_kind,
                        principal_id,
                        fts_query,
                        conversation_scope,
                        project_scope,
                        SEARCH_PAGE_SIZE + 1,
                        offset
                    ],
                    |r| {
                        Ok((
                            (
                                r.get::<_, String>(0)?,
                                r.get::<_, String>(1)?,
                                r.get::<_, Option<String>>(2)?,
                                r.get::<_, String>(3)?,
                                r.get::<_, String>(4)?,
                                r.get::<_, i64>(5)?,
                                r.get::<_, String>(6)?,
                                r.get::<_, String>(7)?,
                                r.get::<_, Option<String>>(8)?,
                                r.get::<_, String>(9)?,
                                r.get::<_, Option<String>>(10)?,
                                r.get::<_, Option<String>>(11)?,
                                r.get::<_, Option<String>>(12)?,
                                r.get::<_, i64>(13)?,
                            ),
                            r.get::<_, f64>(14)?,
                        ))
                    },
                )
                .map_err(db_error)?;
            mapped
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_error)
        })?;
        // Fetched one extra row (SEARCH_PAGE_SIZE + 1) to know whether another page follows,
        // without a second COUNT(*) query.
        let has_more = rows.len() as i64 > SEARCH_PAGE_SIZE;
        let mut hits = Vec::new();
        for (row, score) in rows.into_iter().take(SEARCH_PAGE_SIZE as usize) {
            hits.push(SearchHit {
                message: message_from_row(row)?,
                score: score as f32,
            });
        }
        Ok(SearchPage {
            hits,
            next_cursor: if has_more {
                Some((offset + SEARCH_PAGE_SIZE).to_string())
            } else {
                None
            },
        })
    }
}

fn delivery_status_from_row(s: &str) -> Result<crate::bots::DeliveryStatus> {
    use crate::bots::DeliveryStatus;
    match s {
        "pending" => Ok(DeliveryStatus::Pending),
        "running" => Ok(DeliveryStatus::Running),
        "done" => Ok(DeliveryStatus::Done),
        "failed" => Ok(DeliveryStatus::Failed),
        "cancelled" => Ok(DeliveryStatus::Cancelled),
        "unknown" => Ok(DeliveryStatus::Unknown),
        "held" => Ok(DeliveryStatus::Held),
        _ => Err(rejected("invalid stored delivery status")),
    }
}

/// Re-read a delivery row after a status-changing `UPDATE`, for the caller's return value.
/// Free function (not a method) so it can run against the same `&Transaction` the `UPDATE` just
/// used, same "single transaction, not four" reasoning as `bots_delivery_cancel`.
fn bots_delivery_row(
    tx: &Transaction<'_>,
    delivery_key: DeliveryKey,
    status: crate::bots::DeliveryStatus,
) -> Result<AgentDelivery> {
    #[allow(clippy::type_complexity)]
    let row: (
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
        i64,
        Option<String>,
        Option<String>,
        i64,
    ) = tx
        .query_row(
            "SELECT lease_generation,retry_deadline,bound_runtime_session,bound_turn_ref,\
             updated_at,cause_message_id,root_message_id,turn_depth FROM agent_deliveries \
             WHERE message_id=?1 AND recipient=?2",
            params![
                delivery_key.message_id.to_string(),
                delivery_key.recipient.to_string()
            ],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .map_err(db_error)?;
    let (
        lease_generation,
        retry_deadline,
        bound_runtime_session,
        bound_turn_ref,
        updated_at,
        cause_message_id,
        root_message_id,
        turn_depth,
    ) = row;
    Ok(AgentDelivery {
        key: delivery_key,
        status,
        lease_generation: as_u64(lease_generation)?,
        retry_deadline: retry_deadline.map(from_unix).transpose()?,
        bound_runtime_session: parse_opt_uuid(
            bound_runtime_session,
            "invalid stored runtime session",
        )?,
        bound_turn_ref,
        updated_at: from_unix(updated_at)?,
        cause_message_id: parse_opt_uuid(cause_message_id, "invalid stored cause message id")?,
        root_message_id: parse_opt_uuid(root_message_id, "invalid stored root message id")?,
        turn_depth: as_u32(turn_depth)?,
    })
}

// --- BotsService conformance: thin async delegation to the sync methods above ----------------

impl From<HubError> for BotsError {
    fn from(e: HubError) -> Self {
        match e {
            HubError::BadKey => BotsError::Forbidden("invalid or revoked local key".into()),
            HubError::Transport(s) => BotsError::Storage(s),
            HubError::Rejected(s) => {
                if let Some(rest) = s.strip_prefix("not found: ") {
                    BotsError::NotFound(rest.to_string())
                } else if let Some(rest) = s.strip_prefix("forbidden: ") {
                    BotsError::Forbidden(rest.to_string())
                } else if let Some(rest) = s.strip_prefix("conflict: ") {
                    BotsError::Conflict(rest.to_string())
                } else if let Some(rest) = s.strip_prefix("budget exhausted: ") {
                    BotsError::BudgetExhausted(rest.to_string())
                } else if let Some(rest) = s.strip_prefix("invalid request: ") {
                    BotsError::InvalidRequest(rest.to_string())
                } else {
                    BotsError::Storage(s)
                }
            }
        }
    }
}

#[async_trait]
impl BotsService for LocalHubStore {
    async fn agents_list(&self, owner: UserId) -> BotsResult<Vec<AgentProfile>> {
        self.bots_agents_list(owner).map_err(Into::into)
    }
    async fn agents_create(&self, draft: NewAgentProfile) -> BotsResult<AgentProfile> {
        self.bots_agents_create(draft).map_err(Into::into)
    }
    async fn agents_update(
        &self,
        actor: UserId,
        agent_id: AgentId,
        patch: AgentProfilePatch,
    ) -> BotsResult<AgentProfile> {
        self.bots_agents_update(actor, agent_id, patch)
            .map_err(Into::into)
    }
    async fn agents_archive(&self, actor: UserId, agent_id: AgentId) -> BotsResult<()> {
        self.bots_agents_archive(actor, agent_id)
            .map_err(Into::into)
    }
    async fn conversations_list(&self, actor: Principal) -> BotsResult<Vec<Conversation>> {
        self.bots_conversations_list(actor).map_err(Into::into)
    }
    async fn conversations_create(&self, draft: NewConversation) -> BotsResult<Conversation> {
        self.bots_conversations_create(draft).map_err(Into::into)
    }
    async fn conversations_join(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
    ) -> BotsResult<ConversationMember> {
        self.bots_conversations_join(actor, conversation_id)
            .map_err(Into::into)
    }
    async fn messages_list(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        page: MessagePage,
    ) -> BotsResult<Vec<Message>> {
        self.bots_messages_list(actor, conversation_id, page)
            .map_err(Into::into)
    }
    async fn message_send(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
    ) -> BotsResult<Message> {
        self.bots_message_send(
            actor,
            conversation_id,
            client_request_id,
            expected_policy_revision,
            recipient_ids,
            draft,
        )
        .map_err(Into::into)
    }
    async fn conversation_mark_read(
        &self,
        actor: UserId,
        conversation_id: ConversationId,
        up_to_sequence: u64,
    ) -> BotsResult<ConversationReadPosition> {
        self.bots_conversation_mark_read(actor, conversation_id, up_to_sequence)
            .map_err(Into::into)
    }
    async fn handoff_create(&self, request: NewHandoff) -> BotsResult<Handoff> {
        self.bots_handoff_create(request).map_err(Into::into)
    }
    async fn handoff_status(&self, actor: Principal, handoff_id: HandoffId) -> BotsResult<Handoff> {
        self.bots_handoff_status(actor, handoff_id)
            .map_err(Into::into)
    }
    async fn delivery_cancel(
        &self,
        actor: Principal,
        delivery_key: DeliveryKey,
    ) -> BotsResult<AgentDelivery> {
        self.bots_delivery_cancel(actor, delivery_key)
            .map_err(Into::into)
    }
    async fn conversation_search(
        &self,
        actor: Principal,
        scope: SearchScope,
        query: String,
        cursor: Option<String>,
    ) -> BotsResult<SearchPage> {
        self.bots_conversation_search(actor, scope, query, cursor)
            .map_err(Into::into)
    }
}

// Track E: account scope is derived from verified local pairing metadata.
impl LocalHub {
    fn bots_owner(&self) -> Result<UserId> {
        self.with_node(|tx, node| {
            let owner: Option<String> = tx.query_row("SELECT owner_member_id FROM nodes WHERE id=?1", [node], |r| r.get(0)).map_err(db_error)?;
            owner.and_then(|v| Uuid::parse_str(&v).ok()).ok_or_else(|| rejected("this node has not confirmed its Hive account owner; open Bots once while online"))
        })
    }
    fn bots_actor(&self, actor: Principal) -> Result<UserId> {
        let owner = self.bots_owner()?;
        let actor_owner = match actor {
            Principal::User(id) => id,
            Principal::Agent(id) => self.store.bots_agent_get(id)?.owner,
        };
        if owner != actor_owner {
            return Err(rejected("forbidden: actor belongs to another account"));
        }
        Ok(owner)
    }
    fn bots_conversation_scope(&self, owner: UserId, conversation: ConversationId) -> Result<()> {
        if self.store.bots_conversation_get(conversation)?.owner != owner {
            return Err(rejected(
                "forbidden: conversation belongs to another account",
            ));
        }
        Ok(())
    }
    pub fn bots_message_get(&self, id: Uuid) -> Result<Message> {
        let owner = self.bots_owner()?;
        let message = self.store.bots_message_get(id)?;
        self.bots_conversation_scope(owner, message.conversation_id)?;
        Ok(message)
    }
    pub fn bots_agents_list(&self) -> Result<Vec<AgentProfile>> {
        self.store.bots_agents_list(self.bots_owner()?)
    }
    pub fn bots_agents_create(&self, mut draft: NewAgentProfile) -> Result<AgentProfile> {
        draft.owner = self.bots_owner()?;
        self.store.bots_agents_create(draft)
    }
    pub fn bots_agents_update(
        &self,
        agent_id: AgentId,
        patch: AgentProfilePatch,
    ) -> Result<AgentProfile> {
        self.store
            .bots_agents_update(self.bots_owner()?, agent_id, patch)
    }
    pub fn bots_agents_archive(&self, agent_id: AgentId) -> Result<()> {
        self.store.bots_agents_archive(self.bots_owner()?, agent_id)
    }
    pub fn bots_conversations_list(&self, actor: Principal) -> Result<Vec<Conversation>> {
        let owner = self.bots_actor(actor)?;
        Ok(self
            .store
            .bots_conversations_list(actor)?
            .into_iter()
            .filter(|c| c.owner == owner)
            .collect())
    }
    pub fn bots_room_agents(&self, conversation_id: ConversationId) -> Result<Vec<AgentProfile>> {
        let owner = self.bots_owner()?;
        self.bots_conversation_scope(owner, conversation_id)?;
        self.store
            .bots_room_agents(Principal::User(owner), conversation_id)
    }
    pub fn bots_conversations_create(&self, mut draft: NewConversation) -> Result<Conversation> {
        draft.owner = self.bots_owner()?;
        if let Some(agent) = draft.coordinator {
            if self.store.bots_agent_get(agent)?.owner != draft.owner {
                return Err(rejected(
                    "forbidden: coordinator belongs to another account",
                ));
            }
        }
        self.store.bots_conversations_create(draft)
    }
    pub fn bots_conversations_join(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
    ) -> Result<ConversationMember> {
        self.bots_conversation_scope(self.bots_actor(actor)?, conversation_id)?;
        self.store.bots_conversations_join(actor, conversation_id)
    }
    pub fn bots_messages_list(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        page: MessagePage,
    ) -> Result<Vec<Message>> {
        self.bots_conversation_scope(self.bots_actor(actor)?, conversation_id)?;
        self.store.bots_messages_list(actor, conversation_id, page)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bots_message_send(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
    ) -> Result<Message> {
        // Audit 3.6. `bots_actor` only checks that the actor belongs to this node's owner, so
        // any same-owner paired device could post *as any of the owner's agents* -- minting
        // agent-authored messages with arbitrary recipients through a path that never goes
        // through the executor, and therefore past every loop budget the executor enforces.
        // That makes it a precondition for enabling fan-out, not a tidy-up.
        //
        // Nothing legitimate needs it today: the FFI always sends as `Principal::User`, and the
        // executor writes agent replies straight to the store rather than through this wrapper.
        // Deliberately scoped to the send path -- `conversations_list`/`join` take an agent
        // actor for real reasons and cannot mint messages. When Track E item 4 lands
        // (host-authorized delivery), this becomes "unless this node hosts that agent" rather
        // than a flat refusal.
        // Track E item 4 (host-authorized delivery) has now landed below, so this is the
        // "unless this node hosts that agent" the comment above anticipated rather than a flat
        // refusal. The audit's concern is unchanged and still enforced: a same-owner device may
        // not post as an arbitrary agent. It may now post as an agent it actually runs, which is
        // what lets a second machine's agent answer into the hub machine's Den.
        if let Principal::Agent(id) = actor {
            self.bots_hosts_agent(id)?;
        }
        let owner = self.bots_actor(actor)?;
        self.bots_conversation_scope(owner, conversation_id)?;
        for agent in &recipient_ids {
            if self.store.bots_agent_get(*agent)?.owner != owner {
                return Err(rejected("forbidden: recipient belongs to another account"));
            }
        }
        self.store.bots_message_send(
            actor,
            conversation_id,
            client_request_id,
            expected_policy_revision,
            recipient_ids,
            draft,
        )
    }
    pub fn bots_conversation_mark_read(
        &self,
        conversation_id: ConversationId,
        up_to_sequence: u64,
    ) -> Result<ConversationReadPosition> {
        let owner = self.bots_owner()?;
        self.bots_conversation_scope(owner, conversation_id)?;
        self.store
            .bots_conversation_mark_read(owner, conversation_id, up_to_sequence)
    }

    // ---- Track E item 4: host-authorized delivery ----------------------------------------
    //
    // WHY THIS EXISTS. `DeliveryExecutor` is what makes an agent actually reply, and it calls
    // five `LocalHubStore::bots_*` methods that take a `DeliveryKey` (or a `Principal`) and
    // NOTHING ELSE -- no node, no owner. That was correct while the only caller was the
    // in-process executor on the machine that owns the database: there was no one else to be.
    //
    // Over the transport there is. A paired node authenticates as itself, and if these store
    // methods were exposed directly it could pass ANY agent id and claim that agent's delivery,
    // then answer in its voice. Owner scoping does not catch it, because the attacker and the
    // victim are the same owner's machines -- that is precisely the Audit 3.6 hole that the
    // `bots_message_send` refusal above was standing in for until now.
    //
    // So none of the store methods are exposed. These wrappers are, and every one of them
    // resolves the host from the session's own key via `node_id()` and compares it to the
    // agent's `preferred_host`. The request never supplies the host. A node can only ever act
    // for agents it genuinely runs.

    /// The single gate for the delivery surface: this session's node must be the agent's host,
    /// and the agent must belong to this session's owner. Returns the owner so callers that
    /// need it do not resolve it twice.
    ///
    /// `preferred_host` is `Option<NodeId>`: `None` means no machine has claimed the agent, and
    /// that must fail rather than match, or an unhosted agent would be claimable by anyone.
    fn bots_hosts_agent(&self, agent: AgentId) -> Result<UserId> {
        let owner = self.bots_owner()?;
        let node = self.node_id()?;
        let profile = self.store.bots_agent_get(agent)?;
        if profile.owner != owner {
            return Err(rejected("forbidden: agent belongs to another account"));
        }
        if profile.preferred_host != Some(node) {
            return Err(rejected(
                "forbidden: this node does not host that agent, so it cannot act for it",
            ));
        }
        Ok(owner)
    }

    pub fn bots_delivery_claim(&self, delivery_key: DeliveryKey) -> Result<AgentDelivery> {
        self.bots_hosts_agent(delivery_key.recipient)?;
        self.store.bots_delivery_claim(delivery_key)
    }

    /// The `lease_generation` fencing in the store method is what stops a stale caller resolving
    /// a delivery a later claim already owns; this wrapper adds only the host check, so both
    /// protections apply rather than one replacing the other.
    pub fn bots_delivery_complete(
        &self,
        delivery_key: DeliveryKey,
        lease_generation: u64,
    ) -> Result<AgentDelivery> {
        self.bots_hosts_agent(delivery_key.recipient)?;
        self.store
            .bots_delivery_complete(delivery_key, lease_generation)
    }

    pub fn bots_delivery_fail(
        &self,
        delivery_key: DeliveryKey,
        lease_generation: u64,
        retry_after: Option<DateTime<Utc>>,
    ) -> Result<AgentDelivery> {
        self.bots_hosts_agent(delivery_key.recipient)?;
        self.store
            .bots_delivery_fail(delivery_key, lease_generation, retry_after)
    }

    /// The executor's reply path. Unlike `bots_message_send`, this one carries a
    /// `DeliveryCause`, which is how a reply is tied to the delivery that prompted it -- that
    /// link is what the loop budgets in `bots_turns_for_root` count, so an agent reply must come
    /// through here and not through the plain send path.
    #[allow(clippy::too_many_arguments)]
    pub fn bots_message_send_with_cause(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
        cause: Option<DeliveryCause>,
        hold: bool,
    ) -> Result<Message> {
        // An agent actor must be one this node runs. A user actor still goes through the same
        // owner check every other wrapper uses.
        let owner = match actor {
            Principal::Agent(id) => self.bots_hosts_agent(id)?,
            Principal::User(_) => self.bots_actor(actor)?,
        };
        self.bots_conversation_scope(owner, conversation_id)?;
        for agent in &recipient_ids {
            if self.store.bots_agent_get(*agent)?.owner != owner {
                return Err(rejected("forbidden: recipient belongs to another account"));
            }
        }
        self.store.bots_message_send_with_cause(
            actor,
            conversation_id,
            client_request_id,
            expected_policy_revision,
            recipient_ids,
            draft,
            cause,
            hold,
        )
    }

    /// The executor's work queue: what is pending for one agent this node hosts.
    pub fn bots_deliveries_pending_for_agent(
        &self,
        agent_id: AgentId,
        limit: u32,
    ) -> Result<Vec<AgentDelivery>> {
        self.bots_hosts_agent(agent_id)?;
        self.store
            .bots_deliveries_pending_for_agent(agent_id, limit)
    }

    /// Post the "nothing here can answer this" notices for deliveries this host cannot run.
    ///
    /// The store method takes `owner` and `host` as arguments; both are derived here from the
    /// session instead. That matters more than it looks: `host` selects which deliveries get
    /// written off as unroutable, so a caller allowed to pass an arbitrary host could post
    /// unroutable notices about ANOTHER machine's agents -- effectively telling the owner that a
    /// perfectly healthy remote agent cannot be reached. `local_ready` stays a parameter because
    /// it is a fact about the calling machine's own runner that only it can know.
    pub fn bots_report_unroutable(&self, local_ready: bool) -> Result<usize> {
        let owner = self.bots_owner()?;
        let host = self.node_id()?;
        self.store.bots_report_unroutable(owner, host, local_ready)
    }

    /// Concurrency check for one of this node's own agents.
    pub fn bots_active_turns_for_agent(&self, agent_id: AgentId) -> Result<u32> {
        self.bots_hosts_agent(agent_id)?;
        self.store.bots_active_turns_for_agent(agent_id)
    }

    /// Let a held chain continue. Scoped by the root message's conversation rather than by host:
    /// releasing is an owner decision about their own thread, not something only the running
    /// machine may do, and the held deliveries may belong to agents on several machines.
    pub fn bots_deliveries_release_root(&self, root_message_id: MessageId) -> Result<u32> {
        let owner = self.bots_owner()?;
        let message = self.store.bots_message_get(root_message_id)?;
        self.bots_conversation_scope(owner, message.conversation_id)?;
        self.store.bots_deliveries_release_root(root_message_id)
    }

    /// Read-only turn count for a thread. Scoped to the owner's own conversation rather than the
    /// host, because the executor reads this for a budget decision before it knows which agent
    /// it is about to answer for, and a count leaks nothing a member cannot already see.
    pub fn bots_turns_for_root(&self, root_message_id: MessageId) -> Result<u32> {
        let owner = self.bots_owner()?;
        let message = self.store.bots_message_get(root_message_id)?;
        self.bots_conversation_scope(owner, message.conversation_id)?;
        self.store.bots_turns_for_root(root_message_id)
    }
}
