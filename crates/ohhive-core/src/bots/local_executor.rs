//! ADR-035 C1 local executor -- the piece that actually runs a model turn for a Bots DM reply,
//! split explicitly in two along a compiler-risk line (2026-09-15, Jack's call after asking
//! "if you can't compile, should you hand it to Sif who can?"):
//!
//! - **Loki (Bots-side plumbing, this crate's `bots` module):** drains `agent_deliveries`,
//!   loads the message/thread context a turn needs, calls a [`LocalBotsTurnRunner`], and on
//!   success writes the reply back via `LocalHubStore::bots_message_send` and marks the
//!   delivery done; on failure marks it failed with a reason. Continuous with the storage layer
//!   already built and verified (`local_hub/bots.rs`, `a983fea`/`670f790`).
//! - **Sif ([`LocalBotsTurnRunner`]'s real implementation, not yet written):** the only piece
//!   that has to touch `crate::worker`/`crate::hub::HubClient`'s lease/card machinery --
//!   claiming real node capacity for a turn the same way a job card does
//!   (`HubClient::claim_card`/`checkpoint`/`release_card` in `hub.rs`, the same lease semantics
//!   `worker.rs`'s `Worker` already uses), running one bounded turn through the `Backend` trait
//!   (`LlamaCppBackend::chat_with_tools` is the closest existing call shape -- see `coder.rs`),
//!   and releasing whatever it claimed whether the turn succeeds, fails, or errors. This is
//!   deliberately the highest-risk, most concurrency-sensitive part (shared scheduling state,
//!   a system that's already had a documented lease race-condition fix) and the part most worth
//!   a real compiler checking it as it's written, not after -- see the C1 storage bug
//!   (`local_hub/bots.rs:729`, fixed in `670f790`) for what "write blind, check once at the
//!   end" cost the last time.
//!
//! This file defines only the boundary: the trait, its request/outcome/error shapes. No
//! implementation lives here. Sif should feel free to reshape [`LocalTurnRequest`]/
//! [`LocalTurnOutcome`]/[`LocalTurnError`] as the real lease/`Backend` integration turns out to
//! need -- these are a starting proposal from someone who hasn't touched `worker.rs` or
//! `hub.rs` before today, not a fixed contract. The one thing worth keeping stable is the
//! trait's existence as *something injectable* (`Arc<dyn LocalBotsTurnRunner>`), so Loki's
//! queue-draining loop can be written and tested against it independently of the real
//! implementation landing.

use async_trait::async_trait;

use super::{AgentProfile, ConversationId, Message};

/// What one pending delivery needs in order to attempt a reply: the message that triggered it,
/// plus whatever bounded thread/history window the caller has already assembled. This module
/// does not decide how large that window is or how it's assembled -- that's Loki-side context
/// budgeting (design doc section 7: "Do not inject the entire team feed or every DM"), done
/// before calling `run_turn`, not inside it.
#[derive(Debug, Clone)]
pub struct LocalTurnRequest {
    pub conversation_id: ConversationId,
    /// Bounded, already-trimmed thread window, oldest first, NOT including `incoming`.
    pub history: Vec<Message>,
    /// The message that triggered this delivery -- what the agent is actually replying to.
    pub incoming: Message,
}

/// Rough token accounting for whatever `Backend`/provider actually reports -- deliberately not
/// tied to `backend::Chunk`'s exact shape so this boundary doesn't have to change if that does.
#[derive(Debug, Clone, Copy, Default)]
pub struct TurnUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct LocalTurnOutcome {
    pub reply_body: String,
    pub usage: Option<TurnUsage>,
}

/// Every way a turn can fail to produce a reply, kept distinct because the Bots-side caller
/// handles them differently: `NoCapacity` should leave the delivery `pending` for a later
/// retry (per the schema's own `retry_deadline` column), not mark it `failed` -- a busy node is
/// not a broken turn, and design doc section 4 explicitly wants a busy agent to offer a queued
/// reply rather than fail outright.
#[derive(Debug, thiserror::Error)]
pub enum LocalTurnError {
    /// No eligible local node/worker slot was available right now -- every one this agent could
    /// run on is occupied by a real job card or another turn. Not a failure of the turn itself.
    #[error("no local capacity available for this agent right now")]
    NoCapacity,
    /// The model/runtime ran but didn't produce a usable reply (backend error, malformed
    /// output, timeout past whatever bound the runner enforces).
    #[error("local turn failed: {0}")]
    RuntimeFailed(String),
    /// The delivery was cancelled (via `bots_delivery_cancel`) while the turn was in flight.
    #[error("turn cancelled")]
    Cancelled,
}

/// Implemented by the real local-capacity-aware turn runner (Sif's side, not yet written).
/// Loki's Bots-side executor loop depends on this only as `Arc<dyn LocalBotsTurnRunner>`, never
/// on a concrete type, so the two sides can be built and reviewed independently.
#[async_trait]
pub trait LocalBotsTurnRunner: Send + Sync {
    /// `agent.runtime_kind` is always `AgentRuntimeKind::Local` and `agent.preferred_host` is
    /// always `Some(..)` by the time this is called -- the Bots-side loop only routes local-
    /// runtime deliveries here; a cloud-runtime agent (ChatGPT/Copilot/Grok) is a separate,
    /// later dispatch path (C2+, Sif's subscription/account work), not this trait.
    async fn run_turn(
        &self,
        agent: &AgentProfile,
        request: LocalTurnRequest,
    ) -> Result<LocalTurnOutcome, LocalTurnError>;
}
