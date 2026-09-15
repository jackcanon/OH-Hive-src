//! Shared service interface for ADR-035 "Bots chat and agent collaboration", C0 slice.
//!
//! Method names and argument shapes mirror section 5 of
//! `docs/SIF-BOTS-CHAT-DESIGN-2026-09-15.md` ("Proposed shared service methods") as closely as
//! a typed Rust trait allows. This is the contract every storage backend (LocalHub first, a
//! hub-backed implementation for project rooms later) will implement identically -- nothing in
//! this file talks to a database, a runtime, or the network. See `bots/mod.rs` for what's
//! deliberately not built yet.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::job::ProjectId;
use super::types::{
    AgentDelivery, AgentId, AgentProfile, AgentRuntimeKind, Conversation, ConversationId,
    ConversationKind, ConversationMember, ConversationReadPosition, DeliveryKey, Handoff,
    HandoffBudgets, HandoffId, Message, MessageId, MessageKind, Principal, StorageScope, UserId,
};

/// Section 5: "authenticate before reading; derive author server-side. Neither a node nor
/// model can submit arbitrary `author_id=another_agent`." Every fallible `BotsService` call
/// returns one of these, never a bare storage error -- an implementation maps its own storage
/// failures into `Storage` rather than leaking them.
#[derive(Debug, thiserror::Error)]
pub enum BotsError {
    #[error("not found: {0}")]
    NotFound(String),
    /// The actor is authenticated but not allowed to do this -- e.g. posting with
    /// `author_id` set to a principal other than the caller, or a member without
    /// `MemberAction::Post`.
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// An optimistic-concurrency mismatch: `expected_policy_revision` didn't match the
    /// conversation's current revision, or a client_request_id was reused with a different
    /// payload.
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// A handoff was rejected for exceeding `HandoffBudgets` (active turns, active specialist
    /// handoffs, correction rounds, or depth) -- section 6: "exhaustion produces a concise
    /// blocker for the user."
    #[error("handoff budget exhausted: {0}")]
    BudgetExhausted(String),
    #[error("storage error: {0}")]
    Storage(String),
}

pub type BotsResult<T> = Result<T, BotsError>;

/// Draft for `agents_create`. `id`/`created_at`/`updated_at`/`archived` are assigned by the
/// implementation, not the caller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewAgentProfile {
    pub owner: UserId,
    pub name: String,
    pub runtime_kind: AgentRuntimeKind,
    pub preferred_host: Option<crate::node::NodeId>,
    pub capability_policy_ref: String,
    pub provider_account_ref: Option<super::types::ProviderAccountId>,
    pub memory_namespace: String,
}

/// Partial update for `agents_update`. `None` fields are left unchanged; this is a patch, not
/// a replacement -- callers never have to round-trip fields they're not changing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentProfilePatch {
    pub name: Option<String>,
    pub preferred_host: Option<Option<crate::node::NodeId>>,
    pub capability_policy_ref: Option<String>,
    pub memory_namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewConversation {
    pub owner: UserId,
    pub kind: ConversationKind,
    pub project_id: Option<ProjectId>,
    pub coordinator: Option<AgentId>,
    pub storage_scope: StorageScope,
}

/// Draft for `message_send`. `id`/`server_sequence`/`created_at` are assigned by the
/// implementation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewMessage {
    pub thread_root: Option<MessageId>,
    pub kind: MessageKind,
    pub body: Option<String>,
    pub attachment_refs: Vec<String>,
    pub task_ref: Option<crate::job::JobId>,
    pub turn_ref: Option<String>,
    pub source_event_ref: Option<String>,
}

/// `messages_list(before/after, limit)` per section 5, as a struct so call sites read as named
/// fields rather than three positional numbers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct MessagePage {
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub limit: u32,
}

/// Draft for `handoff_create`, matching the `handoff()` call shape in section 6. `id`,
/// `depth`, `state` and `receipt` are assigned/derived by the implementation -- `depth` from
/// the parent handoff (if `reply_to_thread`/`parent_run` chains to one), `state` always
/// starting at `HandoffState::Requested`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewHandoff {
    pub source_agent: AgentId,
    pub target_agent: AgentId,
    pub project_id: Option<ProjectId>,
    pub task_or_question: String,
    pub acceptance_criteria: String,
    pub artifact_refs: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub parent_run: Option<crate::job::JobId>,
    pub reply_to_thread: Option<MessageId>,
    /// `None` uses `HandoffBudgets::default()` (the section 6 defaults); a caller may tighten
    /// or loosen them per conversation policy, never bypass them.
    pub budgets: Option<HandoffBudgets>,
    pub deadline: DateTime<Utc>,
}

/// `conversation_search(scope, query, cursor)` per section 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum SearchScope {
    /// Search everything the actor can read.
    Everything,
    Conversation(ConversationId),
    Project(ProjectId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub message: Message,
    /// Implementation-defined relevance score; not comparable across implementations.
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    pub next_cursor: Option<String>,
}

/// The C0 contract. Section 5's method list, typed. No default methods, no storage: every
/// implementation (LocalHub first) provides every method itself, so nothing here can silently
/// paper over a backend that hasn't actually implemented an invariant (e.g. the "unique
/// principal per room" and "one fenced writer" rules called out in `bots::types`).
#[async_trait]
pub trait BotsService: Send + Sync {
    async fn agents_list(&self, owner: UserId) -> BotsResult<Vec<AgentProfile>>;
    async fn agents_create(&self, draft: NewAgentProfile) -> BotsResult<AgentProfile>;
    async fn agents_update(
        &self,
        actor: UserId,
        agent_id: AgentId,
        patch: AgentProfilePatch,
    ) -> BotsResult<AgentProfile>;
    async fn agents_archive(&self, actor: UserId, agent_id: AgentId) -> BotsResult<()>;

    async fn conversations_list(&self, actor: Principal) -> BotsResult<Vec<Conversation>>;
    async fn conversations_create(&self, draft: NewConversation) -> BotsResult<Conversation>;
    async fn conversations_join(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
    ) -> BotsResult<ConversationMember>;

    async fn messages_list(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        page: MessagePage,
    ) -> BotsResult<Vec<Message>>;

    /// `client_request_id` makes a retry idempotent; `expected_policy_revision` is optimistic
    /// concurrency against `Conversation::policy_revision` (section 5: "atomic message+outbox
    /// transaction so restart cannot lose dispatch").
    async fn message_send(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
    ) -> BotsResult<Message>;

    async fn conversation_mark_read(
        &self,
        actor: UserId,
        conversation_id: ConversationId,
        up_to_sequence: u64,
    ) -> BotsResult<ConversationReadPosition>;

    async fn handoff_create(&self, request: NewHandoff) -> BotsResult<Handoff>;
    async fn handoff_status(&self, actor: Principal, handoff_id: HandoffId) -> BotsResult<Handoff>;

    async fn delivery_cancel(
        &self,
        actor: Principal,
        delivery_key: DeliveryKey,
    ) -> BotsResult<AgentDelivery>;

    async fn conversation_search(
        &self,
        actor: Principal,
        scope: SearchScope,
        query: String,
        cursor: Option<String>,
    ) -> BotsResult<SearchPage>;
}
