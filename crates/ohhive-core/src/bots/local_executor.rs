//! ADR-035 local turn boundary. Loki owns delivery/context/reply persistence; Sif's
//! `LocalModelTurnRunner` implements bounded tool-free inference with capacity shared by Worker.
//! See docs/SIF-LOCAL-BOTS-RUNNER-2026-09-15.md for construction and rollout limits.
//! Card claim RPCs reserve real jobs, so they must not be used as dummy chat reservations.

use async_trait::async_trait;

use super::{AgentProfile, ConversationId, Message, Principal};

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
    /// Display name per participant, for rendering history the model can actually follow.
    ///
    /// Without this the prompt carries `Principal` verbatim, which serializes as
    /// `{"kind":"agent","id":"<uuid>"}`. In a two-party DM that is survivable -- there is only
    /// "you" and "them". In a room it is not: the model cannot tell two teammates apart, cannot
    /// address anyone by name, and cannot tell which line came from the person. Assembled by the
    /// executor, which already reads the room roster.
    ///
    /// A `Principal` missing from this list renders as an anonymous participant rather than
    /// leaking a UUID into the prompt.
    pub speakers: Vec<(Principal, String)>,
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

/// Implemented by the real local-capacity-aware turn runner (`runner::LocalModelTurnRunner`).
/// Loki's Bots-side executor loop depends on this only as `Arc<dyn LocalBotsTurnRunner>`, never
/// on a concrete type, so the two sides can be built and reviewed independently.
#[async_trait]
pub trait LocalBotsTurnRunner: Send + Sync {
    /// Sender closure also cancels. Dropping this future is supported by the real runner.
    async fn run_turn_cancellable(
        &self,
        agent: &AgentProfile,
        request: LocalTurnRequest,
        mut cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<LocalTurnOutcome, LocalTurnError> {
        tokio::select! {
            biased;
            _ = async { loop { if *cancel.borrow() { break; } if cancel.changed().await.is_err() { break; } } } => Err(LocalTurnError::Cancelled),
            result = self.run_turn(agent, request) => result,
        }
    }

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
