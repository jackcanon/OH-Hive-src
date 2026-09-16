//! ADR-035 "Bots chat and agent collaboration", **C0 only**: contracts (schemas, service
//! interfaces, permission tests, LocalHub-first) -- the first of five delivery phases in the
//! ADR (`ADR/ADR-035-bots-chat-and-agent-collaboration.md`), formalizing
//! `docs/SIF-BOTS-CHAT-DESIGN-2026-09-15.md`.
//!
//! **What's real here:** the nine domain types from the design doc's section 5 table
//! (`types.rs`: `AgentProfile`, `Conversation`, `ConversationMember`, `Message`,
//! `MessageRevision`, `AgentDelivery`, `RuntimeBinding`, `ConversationReadPosition`,
//! `Handoff`), the shared service trait section 5 proposes (`service.rs`: `BotsService`, with
//! `agents_*`/`conversations_*`/`messages_*`/`message_send`/`conversation_mark_read`/
//! `handoff_*`/`delivery_cancel`/`conversation_search`), and the small pure logic that doesn't
//! need storage to be correct or tested -- terminal-state checks, the section 6
//! loop-prevention defaults (`HandoffBudgets::default()`), and a dedup-key helper.
//!
//! **What's deliberately NOT here yet (C1+):** any FFI (`crates/ohhive-ffi`) or Swift/Tauri
//! UI, the "Agents" workspace information architecture, coordinator selection, mentions, or
//! anything from `handoff()`'s actual dispatch behavior beyond the type shape. LocalHub
//! storage exists (`local_hub/bots.rs`), and as of 2026-09-15 both halves of the C1 turn
//! path are in: `runner.rs`'s `LocalModelTurnRunner` (Sif, `b2bc3be`) implements
//! `LocalBotsTurnRunner` against real node capacity and the `Backend` trait, and
//! `executor.rs`'s `DeliveryExecutor` (Loki) drains `agent_deliveries`, builds bounded turn
//! context, calls a `dyn LocalBotsTurnRunner`, and persists a reply or a terminal/retry
//! delivery status -- see each file's own doc for exactly what's covered and what's still a
//! known gap (no ohhive-ffi wiring, no UI to see/DM a registered agent, no cancellation
//! wiring into `run_turn_cancellable` yet). This module makes zero storage calls itself,
//! spawns no process, and holds no credentials -- every credential reference
//! (`AgentProfile::provider_account_ref`) is an opaque ID into the vault, never a token.
//!
//! Same self-verification caveat as `subscription/` (ADR-033 Stage 1): written and reviewed
//! without a Rust toolchain in this sandbox, checked for brace/paren/bracket balance and
//! careful manual review, not yet compiler- or `cargo test`-verified.

pub mod executor;
pub mod mentions;
pub mod local_executor;
pub mod service;
pub mod types;

#[cfg(test)]
mod tests;

pub use local_executor::{
    LocalBotsTurnRunner, LocalTurnError, LocalTurnOutcome, LocalTurnRequest, TurnUsage,
};
pub use executor::{DeliveryExecutor, DrainSummary};
pub use mentions::{resolve_mentions, MentionSet};
pub use service::{
    AgentProfilePatch, BotsError, BotsResult, BotsService, MessagePage, NewAgentProfile,
    NewConversation, NewHandoff, NewMessage, SearchHit, SearchPage, SearchScope,
};
pub use types::{
    AgentDelivery, AgentId, AgentProfile, AgentRuntimeKind, Conversation, ConversationId,
    ConversationKind, ConversationMember, ConversationReadPosition, DeliveryCause, DeliveryKey,
    DeliveryStatus,
    Handoff, HandoffBudgets, HandoffId, HandoffReceipt, HandoffState, MemberAction, Message,
    MessageId, MessageKind, MessageRevision, MessageRevisionId, Principal, ProviderAccountId,
    RevisionKind, RuntimeBinding, RuntimeBindingId, RuntimeSessionId, StorageScope, UserId,
};

pub mod runner;
pub use runner::LocalModelTurnRunner;

