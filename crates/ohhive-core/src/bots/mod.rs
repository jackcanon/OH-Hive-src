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
//! **What's deliberately NOT here yet (C1+):** any LocalHub storage implementation of
//! `BotsService` (the nine tables above are described, not created -- no migration exists
//! yet), any FFI (`crates/ohhive-ffi`) or Swift/Tauri UI, the "Agents" workspace information
//! architecture, offline outbox/delivery execution, coordinator selection, mentions, or
//! anything from `handoff()`'s actual dispatch behavior beyond the type shape. This module
//! makes zero storage calls, spawns no process, and holds no credentials -- every credential
//! reference (`AgentProfile::provider_account_ref`) is an opaque ID into the vault, never a
//! token.
//!
//! Same self-verification caveat as `subscription/` (ADR-033 Stage 1): written and reviewed
//! without a Rust toolchain in this sandbox, checked for brace/paren/bracket balance and
//! careful manual review, not yet compiler- or `cargo test`-verified.

pub mod service;
pub mod types;

#[cfg(test)]
mod tests;

pub use service::{
    AgentProfilePatch, BotsError, BotsResult, BotsService, MessagePage, NewAgentProfile,
    NewConversation, NewHandoff, NewMessage, SearchHit, SearchPage, SearchScope,
};
pub use types::{
    AgentDelivery, AgentId, AgentProfile, AgentRuntimeKind, Conversation, ConversationId,
    ConversationKind, ConversationMember, ConversationReadPosition, DeliveryKey, DeliveryStatus,
    Handoff, HandoffBudgets, HandoffId, HandoffReceipt, HandoffState, MemberAction, Message,
    MessageId, MessageKind, MessageRevision, MessageRevisionId, Principal, ProviderAccountId,
    RevisionKind, RuntimeBinding, RuntimeBindingId, RuntimeSessionId, StorageScope, UserId,
};
