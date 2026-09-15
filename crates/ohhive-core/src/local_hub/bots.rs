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
use super::*;
use async_trait::async_trait;
use chrono::DateTime;
use crate::bots::{
    AgentDelivery, AgentId, AgentProfile, AgentProfilePatch, AgentRuntimeKind, BotsError,
    BotsResult, BotsService, Conversation, ConversationId, ConversationKind, ConversationMember,
    ConversationReadPosition, DeliveryKey, Handoff, HandoffId,
    HandoffState, MemberAction, Message, MessageKind, MessagePage,
    NewAgentProfile, NewConversation, NewHandoff, NewMessage, Principal, RevisionKind,
    SearchHit, SearchPage, SearchScope, StorageScope, UserId,
};

const SEARCH_PAGE_SIZE: i64 = 20;

// --- enum <-> TEXT column conversions -------------------------------------------------------

fn runtime_kind_to_str(k: AgentRuntimeKind) -> &'static str {
    match k {
        AgentRuntimeKind::Local => "local",
        AgentRuntimeKind::ChatgptSubscription => "chatgpt_subscription",
        AgentRuntimeKind::CopilotSubscription => "copilot_subscription",
        AgentRuntimeKind::GrokSubscription => "grok_subscription",
    }
}
fn runtime_kind_from_str(s: &str) -> Result<AgentRuntimeKind> {
    match s {
        "local" => Ok(AgentRuntimeKind::Local),
        "chatgpt_subscription" => Ok(AgentRuntimeKind::ChatgptSubscription),
        "copilot_subscription" => Ok(AgentRuntimeKind::CopilotSubscription),
        "grok_subscription" => Ok(AgentRuntimeKind::GrokSubscription),
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
fn revision_kind_to_columns(k: &RevisionKind) -> (&'static str, Option<String>) {
    match k {
        RevisionKind::Replacement { new_body } => ("replacement", Some(new_body.clone())),
        RevisionKind::Tombstone => ("tombstone", None),
    }
}
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
);
fn conversation_from_row(row: ConversationRow) -> Result<Conversation> {
    let (id, owner, kind, project_id, coordinator, storage_scope, policy_revision, created_at) =
        row;
    Ok(Conversation {
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
                "INSERT INTO conversations VALUES(?1,?2,?3,?4,?5,?6,1,?7)",
                params![
                    id.to_string(),
                    draft.owner.to_string(),
                    conversation_kind_to_str(draft.kind),
                    draft.project_id.map(|p| p.to_string()),
                    draft.coordinator.map(|c| c.to_string()),
                    storage_scope_to_str(draft.storage_scope),
                    ts,
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
                     created_at FROM conversations WHERE id=?1",
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
                     c.policy_revision,c.created_at FROM conversations c \
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
                    ))
                })
                .map_err(db_error)?;
            rows.map(|r| conversation_from_row(r.map_err(db_error)?))
                .collect()
        })
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
                Err(rejected("forbidden: missing the required conversation permission"))
            }
        })
    }

    pub fn bots_conversations_join(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
    ) -> Result<ConversationMember> {
        self.bots_conversation_get(conversation_id)?; // 404s if the room doesn't exist
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
            return Err(rejected("invalid request: page limit must be greater than zero"));
        }
        let limit = page.limit.min(500);
        let after = page.after.map(as_i64).transpose()?;
        let before = page.before.map(as_i64).transpose()?;
        self.transaction(|tx| {
            let sql = "SELECT id,conversation_id,thread_root,author_kind,author_id,\
                       server_sequence,client_request_id,kind,body,attachment_refs,task_ref,\
                       turn_ref,source_event_ref,created_at FROM messages \
                       WHERE conversation_id=?1 \
                       AND (?2 IS NULL OR server_sequence > ?2) \
                       AND (?3 IS NULL OR server_sequence < ?3) \
                       ORDER BY server_sequence ASC LIMIT ?4";
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
            rows.map(|r| message_from_row(r.map_err(db_error)?)).collect()
        })
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
        check_text(&client_request_id, 200)?;
        self.bots_require_member(conversation_id, actor, MemberAction::Post)?;
        let conversation = self.bots_conversation_get(conversation_id)?;
        if conversation.policy_revision != expected_policy_revision {
            return Err(rejected("conflict: conversation policy revision changed"));
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
            for recipient in &recipient_ids {
                tx.execute(
                    "INSERT INTO agent_deliveries VALUES(?1,?2,'pending',0,NULL,NULL,NULL,?3)",
                    params![id.to_string(), recipient.to_string(), ts],
                )
                .map_err(db_error)?;
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
        self.bots_require_member(
            conversation_id,
            Principal::User(actor),
            MemberAction::Read,
        )?;
        let up_to_sequence = i64::try_from(up_to_sequence)
            .map_err(|_| rejected("invalid request: sequence out of range"))?;
        let ts = now();
        self.transaction(|tx| {
            tx.execute(
                "INSERT INTO conversation_read_positions VALUES(?1,?2,?3,?4) \
                 ON CONFLICT(user_id,conversation_id) DO UPDATE SET \
                 last_seen_sequence=max(last_seen_sequence,excluded.last_seen_sequence), \
                 updated_at=?4",
                params![actor.to_string(), conversation_id.to_string(), up_to_sequence, ts],
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
                    params![delivery_key.message_id.to_string(), delivery_key.recipient.to_string()],
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
                return Err(rejected("forbidden: not the message author or the recipient"));
            }
            if matches!(current_status.as_str(), "done" | "failed" | "cancelled") {
                return Err(rejected("conflict: delivery already terminal"));
            }

            let ts = now();
            tx.execute(
                "UPDATE agent_deliveries SET status='cancelled',updated_at=?3 \
                 WHERE message_id=?1 AND recipient=?2",
                params![delivery_key.message_id.to_string(), delivery_key.recipient.to_string(), ts],
            )
            .map_err(db_error)?;

            let row: (i64, Option<i64>, Option<String>, Option<String>) = tx
                .query_row(
                    "SELECT lease_generation,retry_deadline,bound_runtime_session,bound_turn_ref \
                     FROM agent_deliveries WHERE message_id=?1 AND recipient=?2",
                    params![delivery_key.message_id.to_string(), delivery_key.recipient.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(db_error)?;
            let (lease_generation, retry_deadline, bound_runtime_session, bound_turn_ref) = row;
            Ok(AgentDelivery {
                key: delivery_key,
                status: delivery_status_from_row("cancelled")?,
                lease_generation: as_u64(lease_generation)?,
                retry_deadline: retry_deadline.map(from_unix).transpose()?,
                bound_runtime_session: parse_opt_uuid(
                    bound_runtime_session,
                    "invalid stored runtime session",
                )?,
                bound_turn_ref,
                updated_at: from_unix(ts)?,
            })
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
                     bound_runtime_session,bound_turn_ref,updated_at FROM agent_deliveries \
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
                })
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
                    "UPDATE agent_deliveries SET status=?4,retry_deadline=?5,updated_at=?3 \
                     WHERE message_id=?1 AND recipient=?2 AND lease_generation=?6",
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
            mapped.collect::<rusqlite::Result<Vec<_>>>().map_err(db_error)
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
    let row: (i64, Option<i64>, Option<String>, Option<String>, i64) = tx
        .query_row(
            "SELECT lease_generation,retry_deadline,bound_runtime_session,bound_turn_ref,\
             updated_at FROM agent_deliveries WHERE message_id=?1 AND recipient=?2",
            params![delivery_key.message_id.to_string(), delivery_key.recipient.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map_err(db_error)?;
    let (lease_generation, retry_deadline, bound_runtime_session, bound_turn_ref, updated_at) = row;
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
        self.bots_agents_update(actor, agent_id, patch).map_err(Into::into)
    }
    async fn agents_archive(&self, actor: UserId, agent_id: AgentId) -> BotsResult<()> {
        self.bots_agents_archive(actor, agent_id).map_err(Into::into)
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
        self.bots_conversations_join(actor, conversation_id).map_err(Into::into)
    }
    async fn messages_list(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        page: MessagePage,
    ) -> BotsResult<Vec<Message>> {
        self.bots_messages_list(actor, conversation_id, page).map_err(Into::into)
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
        self.bots_handoff_status(actor, handoff_id).map_err(Into::into)
    }
    async fn delivery_cancel(
        &self,
        actor: Principal,
        delivery_key: DeliveryKey,
    ) -> BotsResult<AgentDelivery> {
        self.bots_delivery_cancel(actor, delivery_key).map_err(Into::into)
    }
    async fn conversation_search(
        &self,
        actor: Principal,
        scope: SearchScope,
        query: String,
        cursor: Option<String>,
    ) -> BotsResult<SearchPage> {
        self.bots_conversation_search(actor, scope, query, cursor).map_err(Into::into)
    }
}
