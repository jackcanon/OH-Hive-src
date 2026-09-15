//! ADR-035 C1 Bots-side executor loop -- Loki's half of the `local_executor.rs` split
//! (2026-09-15, after Sif's `LocalModelTurnRunner` landed, `b2bc3be`). Drains `agent_deliveries`
//! for the agents this host runs locally, builds bounded turn context, calls a
//! `LocalBotsTurnRunner`, and persists the result: a reply via `message_send` plus a terminal
//! delivery status, or a retry requeue for `LocalTurnError::NoCapacity`.
//!
//! Deliberately touches only the `BotsService`/`LocalHubStore` storage surface and a
//! `dyn LocalBotsTurnRunner` -- no `worker.rs`/`hub.rs` lease code lives here, consistent with
//! the split `local_executor.rs`'s own doc describes. Capacity arbitration on the machine
//! (whether *any* local turn or card claim can run right now) is entirely the runner's problem
//! (`execution_capacity.rs`); this loop just reacts to whatever the runner reports.
//!
//! **Known gaps, stated rather than hidden:** assumes the delivered-to agent is already a
//! conversation member with `Post` permission (true for anything `message_send` could have
//! created a delivery for in the first place, but not re-verified here); does not distinguish
//! "reply sent but `bots_delivery_complete` itself failed" from "reply never sent" -- both fall
//! through to a `failed` delivery, which could show a spurious failure next to a reply that
//! actually landed (narrow: both operations hit the same local SQLite file). No cancellation
//! wiring yet (`run_turn_cancellable`'s `cancel` receiver isn't threaded from
//! `bots_delivery_cancel` here -- this loop calls plain `run_turn`). Not compiler-verified --
//! no Rust toolchain in this sandbox; the storage methods it calls were dry-run verified
//! against real sqlite3 (see `local_hub/bots.rs`'s new delivery methods), this orchestration
//! layer was not.

use std::sync::Arc;

use crate::{
    bots::{
        AgentId, AgentProfile, AgentRuntimeKind, BotsService, ConversationId, DeliveryKey,
        LocalBotsTurnRunner, LocalTurnError, LocalTurnRequest, Message, MessageKind,
        MessagePage, NewMessage, Principal,
    },
    local_hub::LocalHubStore,
    node::NodeId,
};

/// History window handed to the runner, oldest first, not including the triggering message.
/// Design doc section 7: "Do not inject the entire team feed or every DM" -- an
/// arbitrary-but-bounded value, not a tuned one.
const HISTORY_WINDOW: u32 = 32;

/// Pending deliveries pulled per agent per drain pass.
const DRAIN_BATCH: u32 = 10;

/// How long a `NoCapacity` delivery waits before it's eligible to be claimed again. A fixed
/// backoff, not exponential -- C1 has no retry-count tracking to base one on.
const NO_CAPACITY_RETRY_SECONDS: i64 = 20;

pub struct DeliveryExecutor {
    store: Arc<LocalHubStore>,
    runner: Arc<dyn LocalBotsTurnRunner>,
    host: NodeId,
    owner: uuid::Uuid,
}

/// What happened during one `drain_once` pass -- a plain summary for the caller (the `hive
/// bots work` loop) to log, not part of the storage contract itself.
#[derive(Debug, Default, Clone, Copy)]
pub struct DrainSummary {
    pub agents_checked: usize,
    pub delivered: usize,
    pub failed: usize,
    pub requeued: usize,
}

enum AttemptOutcome {
    Delivered,
    NoCapacity,
    Failed,
}

impl DeliveryExecutor {
    pub fn new(
        store: Arc<LocalHubStore>,
        runner: Arc<dyn LocalBotsTurnRunner>,
        host: NodeId,
        owner: uuid::Uuid,
    ) -> Self {
        Self {
            store,
            runner,
            host,
            owner,
        }
    }

    /// One drain pass over every locally-hosted agent this account owns: claims and attempts
    /// whatever is currently pending and eligible (not in retry backoff), then returns. Not a
    /// long-running loop itself -- the caller decides poll cadence (`hive bots work`'s `--poll`,
    /// mirroring `hive work`'s own shape).
    pub async fn drain_once(&self) -> DrainSummary {
        let mut summary = DrainSummary::default();
        let agents = match self.store.agents_list(self.owner).await {
            Ok(a) => a,
            Err(_) => return summary,
        };
        let local_agents: Vec<AgentProfile> = agents
            .into_iter()
            .filter(|a| {
                !a.archived
                    && a.runtime_kind == AgentRuntimeKind::Local
                    && a.preferred_host == Some(self.host)
            })
            .collect();
        summary.agents_checked = local_agents.len();
        for agent in &local_agents {
            self.drain_agent(agent, &mut summary).await;
        }
        summary
    }

    async fn drain_agent(&self, agent: &AgentProfile, summary: &mut DrainSummary) {
        let pending = self
            .store
            .bots_deliveries_pending_for_agent(agent.id, DRAIN_BATCH)
            .unwrap_or_default();
        for delivery in pending {
            self.drain_one(agent, delivery.key, summary).await;
        }
    }

    async fn drain_one(&self, agent: &AgentProfile, key: DeliveryKey, summary: &mut DrainSummary) {
        // A concurrent poll (or a second `hive bots work` process for the same agent, which
        // shouldn't normally run but isn't prevented here) may have already claimed this --
        // that's not an error, just nothing left for this pass to do.
        let claimed = match self.store.bots_delivery_claim(key) {
            Ok(c) => c,
            Err(_) => return,
        };
        let lease = claimed.lease_generation;
        match self.attempt_reply(agent, key).await {
            AttemptOutcome::Delivered => {
                if self.store.bots_delivery_complete(key, lease).is_ok() {
                    summary.delivered += 1;
                } else {
                    // Reply already landed (see this module's doc: a known, narrow gap) --
                    // mark the delivery failed rather than leave it stuck `running` with a
                    // lease nothing will ever match again.
                    let _ = self.store.bots_delivery_fail(key, lease, None);
                    summary.failed += 1;
                }
            }
            AttemptOutcome::NoCapacity => {
                let retry_at =
                    chrono::Utc::now() + chrono::Duration::seconds(NO_CAPACITY_RETRY_SECONDS);
                let _ = self.store.bots_delivery_fail(key, lease, Some(retry_at));
                summary.requeued += 1;
            }
            AttemptOutcome::Failed => {
                let _ = self.store.bots_delivery_fail(key, lease, None);
                summary.failed += 1;
            }
        }
    }

    /// Resolve context, run the turn, and send the reply message. Does not claim the delivery
    /// (`drain_one` already did that before calling this) or mark it complete/failed on the
    /// way out -- `drain_one` does all delivery-status transitions itself, from the returned
    /// outcome, using the `lease_generation` its own `bots_delivery_claim` call returned, so
    /// `bots_delivery_complete`/`bots_delivery_fail`'s generation check always matches.
    async fn attempt_reply(&self, agent: &AgentProfile, key: DeliveryKey) -> AttemptOutcome {
        let incoming: Message = match self.store.bots_message_get(key.message_id) {
            Ok(m) => m,
            Err(_) => return AttemptOutcome::Failed,
        };

        let history = self
            .store
            .messages_list(
                Principal::Agent(agent.id),
                incoming.conversation_id,
                MessagePage {
                    before: Some(incoming.server_sequence),
                    after: None,
                    limit: HISTORY_WINDOW,
                },
            )
            .await
            .unwrap_or_default();

        let policy_revision = match self.conversation_policy_revision(agent.id, incoming.conversation_id).await {
            Some(r) => r,
            None => return AttemptOutcome::Failed,
        };

        let request = LocalTurnRequest {
            conversation_id: incoming.conversation_id,
            history,
            incoming: incoming.clone(),
        };
        let outcome = match self.runner.run_turn(agent, request).await {
            Ok(o) => o,
            Err(LocalTurnError::NoCapacity) => return AttemptOutcome::NoCapacity,
            Err(_) => return AttemptOutcome::Failed,
        };

        let sent = self
            .store
            .message_send(
                Principal::Agent(agent.id),
                incoming.conversation_id,
                // Deterministic per (message, recipient): a retried drain pass over the same
                // still-pending delivery can never double-post a reply, same idempotency
                // mechanism `message_send` already gives every other caller.
                format!("delivery:{}:{}", key.message_id, key.recipient),
                policy_revision,
                Vec::new(),
                NewMessage {
                    thread_root: incoming.thread_root.or(Some(incoming.id)),
                    kind: MessageKind::Text,
                    body: Some(outcome.reply_body),
                    attachment_refs: Vec::new(),
                    task_ref: None,
                    turn_ref: None,
                    source_event_ref: None,
                },
            )
            .await;
        match sent {
            Ok(_) => AttemptOutcome::Delivered,
            Err(_) => AttemptOutcome::Failed,
        }
    }

    /// The conversation's current `policy_revision`, as the agent itself can see it --
    /// `message_send`'s optimistic-concurrency check needs the real current value, and nothing
    /// on `AgentDelivery`/`Message` carries it. Requires the agent to already be a conversation
    /// member (true for anything that produced a delivery in the first place); `None` if not,
    /// which the caller treats as a hard failure rather than guessing a value.
    async fn conversation_policy_revision(
        &self,
        agent_id: AgentId,
        conversation_id: ConversationId,
    ) -> Option<u32> {
        let conversations = self
            .store
            .conversations_list(Principal::Agent(agent_id))
            .await
            .ok()?;
        conversations
            .into_iter()
            .find(|c| c.id == conversation_id)
            .map(|c| c.policy_revision)
    }
}
