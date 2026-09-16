//! Domain types for ADR-035 "Bots chat and agent collaboration", C0 slice.
//!
//! These are the nine entities from `docs/SIF-BOTS-CHAT-DESIGN-2026-09-15.md` section 5's
//! table, plus the small pure helpers (dedup keys, terminal-state checks, the loop-prevention
//! defaults from section 6) that don't need storage to be correct. No persistence, no FFI, no
//! network calls live here -- see `bots/mod.rs` for what's deliberately not built yet.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::job::{JobId, ProjectId};
use crate::node::NodeId;

pub type AgentId = Uuid;
pub type ConversationId = Uuid;
pub type MessageId = Uuid;
pub type MessageRevisionId = Uuid;
pub type RuntimeBindingId = Uuid;
pub type HandoffId = Uuid;
pub type ProviderAccountId = Uuid;
pub type RuntimeSessionId = Uuid;
pub type UserId = Uuid;

/// Which of the three ADR-034 subscription runtimes (or plain local inference) an agent
/// prefers to run on. Kept separate from `node_id`/`provider_account_id`: this says what kind
/// of seat the agent wants, not which machine or credential currently holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeKind {
    /// Local inference on a member's own hardware -- no subscription coordinator involved.
    Local,
    ChatgptSubscription,
    CopilotSubscription,
    GrokSubscription,
    /// Direct Anthropic API call using the member's own BYOK key (Settings), resolved and
    /// spent hub-side only -- the key never reaches the member's device (ADR-008). Distinct
    /// from a future subscription-seat variant: this is "bring your own API key", not a seat
    /// on a consumer subscription.
    AnthropicByok,
    /// Direct Nous Portal call (OpenAI-compatible), same BYOK/hub-only shape as
    /// `AnthropicByok`.
    NousByok,
}

/// A `principal` is whoever is acting: a human member or an agent teammate. Conversation
/// membership, message authorship and search scope are all expressed in terms of it so the
/// same contract covers a user's DM to an agent and an agent's handoff to another agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum Principal {
    User(UserId),
    Agent(AgentId),
}

/// `agent_profiles`: owner, id, name, role/instruction revision, runtime kind, preferred host,
/// capability policy, memory namespace, archived flag; credential references only.
///
/// `capability_policy_ref` and `provider_account_ref` are opaque references into the vault /
/// capability store (ADR-027/028) -- this type never carries a credential or token itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: AgentId,
    pub owner: UserId,
    pub name: String,
    /// Bumped every time the agent's role/instructions change; conversations pin the revision
    /// they were bound under (see `RuntimeBinding::policy_revision`) so an instruction edit
    /// mid-conversation doesn't silently reinterpret history.
    pub role_revision: u32,
    pub runtime_kind: AgentRuntimeKind,
    pub preferred_host: Option<NodeId>,
    /// Reference into the capability policy store, not an inline policy body.
    pub capability_policy_ref: String,
    /// Reference into the vault/credential store when `runtime_kind` needs one. `None` for
    /// `AgentRuntimeKind::Local`.
    pub provider_account_ref: Option<ProviderAccountId>,
    /// Scopes what this agent's memory read/write is confined to (ADR-027 precedent).
    pub memory_namespace: String,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// `conversations`: id, owner, kind (team/project/agent_dm), project, coordinator, storage
/// scope, policy revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    Team,
    Project,
    AgentDm,
}

/// Local-first authority per section 5.1: private conversations are owned by the selected
/// LocalHub; project rooms are hub-backed under the same contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageScope {
    LocalOnly,
    HubBacked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    #[serde(default)]
    pub title: Option<String>,
    pub id: ConversationId,
    pub owner: UserId,
    pub kind: ConversationKind,
    pub project_id: Option<ProjectId>,
    /// The single agent (if any) that owns this room's work plan (section 6: "one coordinator
    /// per room").
    pub coordinator: Option<AgentId>,
    pub storage_scope: StorageScope,
    pub policy_revision: u32,
    pub created_at: DateTime<Utc>,
}

/// What a member is allowed to do in a conversation. Deliberately coarse for C0 -- fine-grained
/// per-action ACLs are a later phase, not part of this contracts slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberAction {
    Read,
    Post,
    /// Invite/remove members, change the coordinator, edit policy.
    Manage,
}

/// `conversation_members`: conversation, principal kind/id, allowed actions, join/history
/// boundary; unique principal per room. Uniqueness is a storage-layer invariant (there is no
/// storage here yet); `Conversation::validate_membership` below is the pure part of that check
/// a caller can run before it ever reaches a store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationMember {
    pub conversation_id: ConversationId,
    pub principal: Principal,
    pub allowed_actions: Vec<MemberAction>,
    /// A member never sees messages before this point -- set at join time, not retroactively
    /// widened.
    pub history_boundary: DateTime<Utc>,
    pub joined_at: DateTime<Utc>,
}

/// `messages`: id, conversation, thread/root, authenticated author, server sequence, client
/// request ID, kind, body/attachment refs, timestamp, task/turn/source event refs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Text,
    /// A lifecycle/status post (e.g. "still working") -- per section 6, these must NOT wake
    /// every participant. `BotsService` implementations enforce that; the type just says what
    /// a message is.
    System,
    /// A durable record that a task/turn completed, linked to real artifacts -- not a claim a
    /// reviewer can accept on its own (section 6: "not independent evidence if it simply
    /// repeats the builder's claim").
    TaskReceipt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    /// `None` for a root message; `Some(root)` places this message in a thread.
    pub thread_root: Option<MessageId>,
    pub author: Principal,
    /// Server-assigned, strictly increasing per conversation. Client timestamps are display
    /// metadata only -- this field is the real ordering authority (section 5).
    pub server_sequence: u64,
    /// Caller-supplied idempotency key; a retry with the same key and conversation must not
    /// create a second message.
    pub client_request_id: String,
    pub kind: MessageKind,
    pub body: Option<String>,
    pub attachment_refs: Vec<String>,
    pub task_ref: Option<JobId>,
    pub turn_ref: Option<String>,
    pub source_event_ref: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// `message_revisions`: original ID, replacement/tombstone, author and time; source receipts
/// remain immutable. `MessageKind::TaskReceipt` messages are never revised -- a `BotsService`
/// impl rejects a revision targeting one; nothing in this type stops it (that's an enforcement
/// concern), but keeping the two facts next to each other here is deliberate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RevisionKind {
    Replacement { new_body: String },
    Tombstone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageRevision {
    pub id: MessageRevisionId,
    pub original_id: MessageId,
    pub revision: RevisionKind,
    pub author: Principal,
    pub created_at: DateTime<Utc>,
}

/// `agent_deliveries`: message+recipient unique key, pending/running/done/failed/cancelled/
/// unknown, lease generation, retry deadline, bound runtime/turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Running,
    Done,
    Failed,
    Cancelled,
    /// Lost track of it (e.g. after a crash/restart past the lease generation it started
    /// under) -- distinct from `Failed`, which means the runtime reported failure.
    Unknown,
    /// Created but deliberately not runnable: the chain this delivery belongs to reached
    /// `HandoffBudgets::max_turns_per_root`, so it waits for a person to release it rather than
    /// running or dying (Track A section 3.3.1, Jack 2026-09-15). Not terminal -- a release
    /// moves it back to `Pending`. A held chain that is never released stays held on purpose: a
    /// silent expiry into `Cancelled` is the exact failure mode the gate exists to prevent.
    Held,
}

impl DeliveryStatus {
    /// True once nothing further will change this delivery's status on its own -- a caller
    /// still has to decide whether a terminal `Unknown`/`Failed` warrants a retry delivery.
    /// `Held` is deliberately NOT terminal: it is waiting on a person, and a release returns it
    /// to `Pending`.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            DeliveryStatus::Done | DeliveryStatus::Failed | DeliveryStatus::Cancelled
        )
    }
}

/// The natural unique key for an `agent_deliveries` row (section 5: "message+recipient unique
/// key") -- a delivery is per (message, recipient), never duplicated for the same pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeliveryKey {
    pub message_id: MessageId,
    pub recipient: AgentId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentDelivery {
    pub key: DeliveryKey,
    pub status: DeliveryStatus,
    /// Bumped on every reconnect/restart of the runtime driving this delivery, so a stale
    /// in-flight result can never resolve a delivery that has since moved on (same generation
    /// pattern as `subscription::Reducer`, ADR-033 Stage 1).
    pub lease_generation: u64,
    pub retry_deadline: Option<DateTime<Utc>>,
    pub bound_runtime_session: Option<RuntimeSessionId>,
    pub bound_turn_ref: Option<String>,
    pub updated_at: DateTime<Utc>,
    /// The message whose reply produced this delivery; `None` for a human-originated one.
    pub cause_message_id: Option<MessageId>,
    /// The message that began this chain. Pre-v10 rows migrate with their own `message_id`.
    pub root_message_id: Option<MessageId>,
    /// 0 for a delivery caused by a human message, incremented once per agent hop. This is what
    /// `HandoffBudgets::max_depth` is compared against -- before schema v10 the delivery path
    /// carried no depth at all, which is why the budgets were unenforceable.
    pub turn_depth: u32,
}

/// Why a delivery exists, threaded from the delivery an executor is currently draining into the
/// reply it produces. `None` at a call site means a human-originated send: depth 0, and the new
/// message is its own chain root.
///
/// Carried as a parameter rather than derived later on purpose: the message and its deliveries
/// are created in one transaction, so if depth were written after the rows existed, a concurrent
/// drain could claim a delivery still showing the default 0 and walk straight past the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryCause {
    pub cause_message_id: MessageId,
    pub root_message_id: MessageId,
    /// The depth of the *new* delivery, i.e. the draining delivery's `turn_depth + 1`.
    pub depth: u32,
}

/// `runtime_bindings`: conversation/thread+agent+account+policy revision to runtime session;
/// one fenced writer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeBinding {
    pub id: RuntimeBindingId,
    pub conversation_id: ConversationId,
    pub thread_root: Option<MessageId>,
    pub agent_id: AgentId,
    pub provider_account_id: ProviderAccountId,
    pub policy_revision: u32,
    pub runtime_session_id: RuntimeSessionId,
    /// Bumped whenever this binding is re-fenced (e.g. reconnect); only the current generation
    /// may write. Enforcing "one fenced writer" is a storage/service concern -- this field is
    /// what that enforcement compares against.
    pub writer_generation: u64,
    pub created_at: DateTime<Utc>,
}

/// `conversation_read_positions`: user+conversation, last seen sequence; notifications derived
/// separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationReadPosition {
    pub user_id: UserId,
    pub conversation_id: ConversationId,
    pub last_seen_sequence: u64,
    pub updated_at: DateTime<Utc>,
}

/// `handoffs`: request ID, requester/assignee, project/task, accepted state, artifact refs,
/// budgets and terminal receipt. Field names below mirror the `handoff()` call shape in section
/// 6 (`request_id, source_agent, target_agent, project, task_or_question, acceptance_criteria,
/// artifact_refs, allowed_tools, parent_run, reply_to_thread, max_followups, deadline`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffState {
    /// Sent, not yet accepted. Section 6: "no acceptance means 'sent/pending,' not 'started.'"
    Requested,
    Accepted,
    Rejected,
    InProgress,
    /// A reviewer rejected the deliverable and requested a correction (section 6); still
    /// within `HandoffBudgets::max_correction_rounds`.
    AwaitingCorrection,
    Completed,
    Failed,
    /// Budget or deadline exhausted before a terminal outcome (section 6: "exhaustion produces
    /// a concise blocker for the user").
    Expired,
}

impl HandoffState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            HandoffState::Rejected
                | HandoffState::Completed
                | HandoffState::Failed
                | HandoffState::Expired
        )
    }
}

/// The section 6 loop-prevention defaults, as data: "one active turn per agent, one
/// coordinator per room, two active specialist handoffs per run, two correction rounds, and
/// depth two." Configurable rather than a user decision -- a `BotsService` impl may override
/// these per conversation/policy, but this `Default` is the agreed starting point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffBudgets {
    pub max_active_turns_per_agent: u32,
    pub max_active_specialist_handoffs_per_run: u32,
    pub max_correction_rounds: u32,
    pub max_depth: u32,
    pub max_followups: u32,
    /// Total deliveries allowed against one chain root before it pauses for a person (Track A
    /// section 3.3.1). Depth bounds the chain but not its cost: `@everyone` in a six-agent room
    /// whose replies each mention two others terminates correctly at depth 2 having burned ~18
    /// turns from one sentence, and it is quadratic in room size. Depth proves termination; this
    /// proves affordability. `#[serde(default)]` so `handoffs` rows written before v10
    /// deserialize.
    #[serde(default = "default_max_turns_per_root")]
    pub max_turns_per_root: u32,
}

/// 30, per Jack 2026-09-15 -- chosen as "where a person should be looking anyway" rather than as
/// a cost ceiling, which is only a safe way to pick it because reaching it pauses the chain
/// instead of killing it.
fn default_max_turns_per_root() -> u32 {
    30
}

impl Default for HandoffBudgets {
    fn default() -> Self {
        HandoffBudgets {
            max_active_turns_per_agent: 1,
            max_active_specialist_handoffs_per_run: 2,
            max_correction_rounds: 2,
            // 2 -> 6, Jack 2026-09-15. Coupled to `max_turns_per_root`: under depth 2 with
            // replies capped at 2 recipients, a root mentioning k agents yields ~3k turns, so a
            // six-agent room tops out near 18 and hits the depth wall before a 30-turn gate is
            // close -- the gate would have been dead code. Depth 2 also isn't a team working a
            // problem (A asks B, B asks C, done; no agent can act on an answer and report
            // back). At 6, turn count is the binding tunable control and depth is the backstop.
            max_depth: 6,
            max_followups: 2,
            max_turns_per_root: default_max_turns_per_root(),
        }
    }
}

/// The receipt a handoff resolves to once `HandoffState::is_terminal()`. Kept separate from
/// `Handoff` itself so an in-progress handoff has an unambiguous `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffReceipt {
    pub state: HandoffState,
    pub artifact_refs: Vec<String>,
    pub summary: String,
    pub resolved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handoff {
    pub id: HandoffId,
    pub source_agent: AgentId,
    pub target_agent: AgentId,
    pub project_id: Option<ProjectId>,
    pub task_or_question: String,
    pub acceptance_criteria: String,
    pub artifact_refs: Vec<String>,
    pub allowed_tools: Vec<String>,
    /// The job this handoff was spawned from, if any (section 5: "reuse existing task IDs for
    /// jobs; do not create a competing scheduler in these tables").
    pub parent_run: Option<JobId>,
    pub reply_to_thread: Option<MessageId>,
    pub budgets: HandoffBudgets,
    pub deadline: DateTime<Utc>,
    /// How deep this handoff sits in a chain of handoffs (root handoffs are depth 0). Compared
    /// against `budgets.max_depth` by a `BotsService` impl before it accepts one that would
    /// exceed it.
    pub depth: u32,
    pub state: HandoffState,
    pub receipt: Option<HandoffReceipt>,
    pub created_at: DateTime<Utc>,
}

impl Handoff {
    /// Section 6: "deduplicate by source request + target + workflow step; carry causation IDs
    /// and hop counters." `workflow_step` is the caller's own step identifier (e.g. a card ID
    /// plus a step name) -- this type doesn't know what a "workflow step" is, only how to fold
    /// one into a stable dedup key alongside the fields it does own.
    pub fn dedup_key(&self, workflow_step: &str) -> String {
        format!(
            "{}:{}:{}",
            self.id, self.target_agent, workflow_step
        )
    }
}
