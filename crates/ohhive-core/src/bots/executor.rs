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
        AgentDelivery, AgentId, AgentProfile, AgentRuntimeKind, ConversationId, DeliveryCause,
        HandoffBudgets, LocalBotsTurnRunner, LocalTurnError, LocalTurnRequest, Message, MessageId,
        MessageKind, MessagePage, NewMessage, Principal,
    },
    node::NodeId,
};

/// History window handed to the runner, oldest first, not including the triggering message.
/// Design doc section 7: "Do not inject the entire team feed or every DM" -- an
/// arbitrary-but-bounded value, not a tuned one.
const HISTORY_WINDOW: u32 = 32;

/// How the room's human is named to the agents. One token on purpose -- see where it is used.
pub const HUMAN_LABEL: &str = "Owner";

/// Pending deliveries pulled per agent per drain pass.
const DRAIN_BATCH: u32 = 10;

/// What the per-thread affordability gate decided.
pub(crate) enum BudgetDecision {
    Proceed,
    Hold(String),
}

/// Whether this thread can afford `pending` more replies, given what it has already spent.
///
/// `spent` is `None` when the count could not be read. That case holds, deliberately: reading it
/// as zero -- which is what `.unwrap_or(0)` did -- disables the gate exactly when the database is
/// unhappy, and this gate is what stops one sentence in a six-agent room becoming unbounded
/// fan-out. A hold pauses the chain for a person, which is visible and releasable. A wrong zero
/// is neither.
///
/// Pulled out of the drain as a plain function purely so the policy is testable: `DeliveryStore`
/// has sixty-odd methods, and a fake that fails one of them would be more boilerplate than the
/// rule it guards.
pub(crate) fn turn_budget_decision(
    spent: Option<u32>,
    pending: usize,
    max_turns_per_root: u32,
) -> BudgetDecision {
    let plural = if pending == 1 { "y is" } else { "ies are" };
    match spent {
        // Says which of the two happened, because "limit reached" would be a lie about a number
        // nobody managed to read.
        None => BudgetDecision::Hold(format!(
            "Couldn't check this thread's turn budget, so {pending} repl{plural} held rather than sent. Release to continue."
        )),
        Some(spent) if spent.saturating_add(pending as u32) > max_turns_per_root => {
            BudgetDecision::Hold(format!(
                "{max_turns_per_root}-turn limit reached for this thread; {pending} repl{plural} held. Release to continue."
            ))
        }
        Some(_) => BudgetDecision::Proceed,
    }
}

/// How long a `NoCapacity` delivery waits before it's eligible to be claimed again. A fixed
/// backoff, not exponential -- C1 has no retry-count tracking to base one on.
const NO_CAPACITY_RETRY_SECONDS: i64 = 20;

pub struct DeliveryExecutor {
    /// Either this machine's own vault or a hub on another machine -- see
    /// `delivery_store::DeliveryStore`. The drain loop below is identical for both.
    store: Arc<dyn super::DeliveryStore>,
    runner: Option<Arc<dyn LocalBotsTurnRunner>>,
    /// Optional second runner for BYOK provider agents (`bots::CloudTurnRunner`). `None` means
    /// this host cannot answer for Claude or Nous, and their deliveries stay pending and get an
    /// honest notice rather than silence -- see `bots_report_unroutable`.
    cloud_runner: Option<Arc<dyn LocalBotsTurnRunner>>,
    host: NodeId,
    owner: uuid::Uuid,
    /// Loop-prevention budgets for agent-to-agent turns. `HandoffBudgets::fan_out_disabled()`
    /// unless a caller explicitly opts in; see `with_budgets`.
    budgets: HandoffBudgets,
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
    /// This host has no runner for the agent's runtime. Distinct from `Failed`: nothing went
    /// wrong, the delivery simply is not ours to run, so it must be left claimable.
    NoRunner,
}

impl DeliveryExecutor {
    pub fn new(
        store: Arc<dyn super::DeliveryStore>,
        runner: Arc<dyn LocalBotsTurnRunner>,
        host: NodeId,
        owner: uuid::Uuid,
    ) -> Self {
        Self {
            store,
            runner: Some(runner),
            cloud_runner: None,
            host,
            owner,
            // Fan-out OFF by default. Every production call site (the CLI, the FFI bridge and
            // the Tauri shell) constructs an executor without choosing budgets, so the default
            // is what ships -- and a cascade that multiplies model calls must not be what you
            // get by not deciding. `with_budgets(HandoffBudgets::default())` turns it on.
            budgets: HandoffBudgets::fan_out_disabled(),
        }
    }

    /// Run cloud agents without requiring a configured local model. Local deliveries stay pending.
    pub fn without_local_runner(
        store: Arc<dyn super::DeliveryStore>,
        host: NodeId,
        owner: uuid::Uuid,
    ) -> Self {
        Self {
            store,
            runner: None,
            cloud_runner: None,
            host,
            owner,
            budgets: HandoffBudgets::fan_out_disabled(),
        }
    }

    /// Attach a runner for BYOK provider agents (`AnthropicByok` / `NousByok`).
    ///
    /// Sif's `CloudTurnRunner` resolves the member's own key hub-side (ADR-008: the key never
    /// reaches the device), so constructing one needs a trusted account context -- a caller that
    /// already holds the verified owner and node key, not something derived from a conversation.
    ///
    /// Note what does *not* gate this: `StorageScope::LocalOnly`. Every Track A room is
    /// local-only, which describes where the conversation is *stored*, not whether the member
    /// may spend their own API key. Gating on it would disable cloud agents everywhere. The
    /// explicit cloud intent is the BYOK agent existing at all -- `ensure_provider_agents` only
    /// creates one when the member has that provider's key on file -- plus a human addressing it.
    pub fn with_cloud_runner(mut self, runner: Arc<dyn LocalBotsTurnRunner>) -> Self {
        self.cloud_runner = Some(runner);
        self
    }

    /// Which runner answers for this agent, or `None` if this host cannot.
    fn runner_for(&self, agent: &AgentProfile) -> Option<&Arc<dyn LocalBotsTurnRunner>> {
        match agent.runtime_kind {
            AgentRuntimeKind::Local => self.runner.as_ref(),
            AgentRuntimeKind::AnthropicByok | AgentRuntimeKind::NousByok => {
                self.cloud_runner.as_ref()
            }
            // ChatGPT/Copilot/Grok coordinators have no runner at all yet (ADR-034: only Codex
            // has any scaffold, and Claude is deliberately excluded from that path forever).
            _ => None,
        }
    }

    /// Set the budgets for this executor, including enabling agent-to-agent fan-out at all:
    /// a fresh executor has `max_depth: 0`, so passing `HandoffBudgets::default()` here is what
    /// switches the cascade on.
    pub fn with_budgets(mut self, budgets: HandoffBudgets) -> Self {
        self.budgets = budgets;
        self
    }

    /// One drain pass over every locally-hosted agent this account owns: claims and attempts
    /// whatever is currently pending and eligible (not in retry backoff), then returns. Not a
    /// long-running loop itself -- the caller decides poll cadence (`hive bots work`'s `--poll`,
    /// mirroring `hive work`'s own shape).
    pub async fn drain_once(&self) -> DrainSummary {
        let mut summary = DrainSummary::default();
        let agents = match self.store.agents_list(self.owner).await {
            Ok(a) => a,
            // Was `Err(_) => return summary`, the first silent failure in the chain and the
            // worst placed: it aborts the pass before `drain_agent`'s own logging, before the
            // claim, and before `report_unroutable`, so a refused or unreachable hub looked
            // exactly like an empty account -- a worker idling politely forever with nothing in
            // its log to say why.
            Err(error) => {
                tracing::warn!(%error, owner = %self.owner, "Cannot list Bots agents");
                return summary;
            }
        };
        let live: Vec<AgentProfile> = agents.into_iter().filter(|a| !a.archived).collect();
        let live_count = live.len();

        let mine: Vec<AgentProfile> = live
            .into_iter()
            .filter(|a| match a.runtime_kind {
                // A local agent runs only on the machine it is pinned to.
                AgentRuntimeKind::Local => {
                    self.runner.is_some() && a.preferred_host == Some(self.host)
                }
                // A BYOK agent runs wherever a cloud runner exists. `ensure_provider_agents`
                // creates these with no `preferred_host`, because the turn happens hub-side and
                // no particular machine owns it.
                //
                // Two hosts could therefore both try one delivery. That is safe rather than
                // lucky: when they share an authority they are claiming rows in the *same*
                // database and `bots_delivery_claim`'s fenced `WHERE status='pending'` UPDATE
                // makes exactly one of them win; when they do not share one, their agents and
                // deliveries are disjoint data and there is nothing to race over. A
                // `preferred_host` that IS set is still honoured, so pinning one remains possible.
                AgentRuntimeKind::AnthropicByok | AgentRuntimeKind::NousByok => {
                    self.cloud_runner.is_some()
                        && (a.preferred_host.is_none() || a.preferred_host == Some(self.host))
                }
                _ => false,
            })
            .collect();

        summary.agents_checked = mine.len();
        // The filter above is a silent one by construction: an agent this node does not host is
        // *supposed* to be skipped without comment, so a host-id mismatch and a correctly idle
        // worker produce identical output. That is fine until the two identity namespaces
        // disagree (a vault node id where a Hive node id was expected, or the reverse), and then
        // there is nothing at all to look at. Emit the comparison itself, at `debug` so a healthy
        // worker stays quiet: `RUST_LOG=debug` turns an invisible mismatch into a printed one.
        tracing::debug!(
            host = %self.host,
            candidates = live_count,
            hosted_here = mine.len(),
            local_runner = self.runner.is_some(),
            cloud_runner = self.cloud_runner.is_some(),
            "Bots drain pass: agents hosted on this node"
        );
        for agent in &mine {
            self.drain_agent(agent, &mut summary).await;
        }

        // Report *after* draining, not before. A delivery this pass just answered is no longer
        // pending, so a supported cloud agent never collects an "unsupported" notice next to its
        // own reply -- which is exactly what reporting first would have produced the moment a
        // cloud runner was attached. Only genuinely unanswerable deliveries are still pending
        // here. `local_ready` says whether a local-model turn could have run at all.
        let local_ready = self.runner.is_some();
        if let Err(error) = self
            .store
            .report_unroutable(self.owner, self.host, local_ready)
            .await
        {
            tracing::warn!(%error, "Cannot report unavailable Bots routes");
        }
        summary
    }

    async fn drain_agent(&self, agent: &AgentProfile, summary: &mut DrainSummary) {
        let pending = match self
            .store
            .deliveries_pending_for_agent(agent.id, DRAIN_BATCH)
            .await
        {
            Ok(p) => p,
            Err(error) => {
                // Was `.unwrap_or_default()`, which turned "the hub refused this" into "there is
                // nothing to do" -- indistinguishable from an idle queue, and silent for as long
                // as you care to watch it.
                tracing::warn!(%error, agent = %agent.id, "Cannot read pending Bots deliveries");
                return;
            }
        };
        for delivery in pending {
            // `max_active_turns_per_agent`, enforced here rather than at send time.
            //
            // An earlier draft of the Track A design dropped recipients who were already busy,
            // which silently loses a message. Enforcing at claim time is strictly better: the
            // delivery stays `pending` and runs on a later pass, so a busy agent is delayed, not
            // skipped. Within one drain process this loop is already sequential per agent; the
            // check is what holds when a second `hive bots work` process exists for the same
            // agent, which nothing prevents.
            // Fails closed. `.unwrap_or(0)` read an unreadable count as "this agent is idle"
            // and started another turn anyway, which is the one interpretation that cannot be
            // recovered from -- the budget exists precisely to stop unbounded concurrent turns.
            // Skipping the agent for this pass costs one poll interval and fixes itself; the
            // deliveries stay pending either way.
            let active = match self.store.active_turns_for_agent(agent.id).await {
                Ok(n) => n,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        agent = %agent.id,
                        "Cannot read this agent's active turn count; skipping it this pass rather \
                         than risking a turn over budget"
                    );
                    return;
                }
            };
            if active >= self.budgets.max_active_turns_per_agent {
                return;
            }
            self.drain_one(agent, delivery, summary).await;
        }
    }

    async fn drain_one(
        &self,
        agent: &AgentProfile,
        delivery: AgentDelivery,
        summary: &mut DrainSummary,
    ) {
        let key = delivery.key;
        // A concurrent poll (or a second `hive bots work` process for the same agent, which
        // shouldn't normally run but isn't prevented here) may have already claimed this --
        // that's not an error, just nothing left for this pass to do.
        let claimed = match self.store.delivery_claim(key).await {
            Ok(c) => c,
            Err(error) => {
                // Usually benign and frequent (another poll got there first), so `debug` rather
                // than `warn` -- but it was previously indistinguishable from a hub refusing
                // every claim forever, which looks exactly like an idle worker.
                tracing::debug!(%error, %key.message_id, agent = %agent.id, "Delivery not claimed");
                return;
            }
        };
        let lease = claimed.lease_generation;
        // Use the freshly claimed row's causation, not the pre-claim copy.
        match self.attempt_reply(agent, &claimed).await {
            AttemptOutcome::Delivered => {
                if self.store.delivery_complete(key, lease).await.is_ok() {
                    summary.delivered += 1;
                } else {
                    // Reply already landed (see this module's doc: a known, narrow gap) --
                    // mark the delivery failed rather than leave it stuck `running` with a
                    // lease nothing will ever match again.
                    let _ = self.store.delivery_fail(key, lease, None).await;
                    summary.failed += 1;
                }
            }
            AttemptOutcome::NoCapacity => {
                let retry_at =
                    chrono::Utc::now() + chrono::Duration::seconds(NO_CAPACITY_RETRY_SECONDS);
                let _ = self.store.delivery_fail(key, lease, Some(retry_at)).await;
                summary.requeued += 1;
            }
            AttemptOutcome::Failed => {
                let _ = self.store.delivery_fail(key, lease, None).await;
                summary.failed += 1;
            }
            AttemptOutcome::NoRunner => {
                // Hand it straight back as pending, with no retry delay: another host (or this
                // one, once a cloud runner is configured) may be able to run it immediately.
                let _ = self.store.delivery_fail(key, lease, None).await;
                summary.requeued += 1;
            }
        }
    }

    /// Resolve context, run the turn, and send the reply message. Does not claim the delivery
    /// (`drain_one` already did that before calling this) or mark it complete/failed on the
    /// way out -- `drain_one` does all delivery-status transitions itself, from the returned
    /// outcome, using the `lease_generation` its own `bots_delivery_claim` call returned, so
    /// `bots_delivery_complete`/`bots_delivery_fail`'s generation check always matches.
    async fn attempt_reply(
        &self,
        agent: &AgentProfile,
        delivery: &AgentDelivery,
    ) -> AttemptOutcome {
        let key = delivery.key;
        let incoming: Message = match self.store.message_get(key.message_id).await {
            Ok(m) => m,
            Err(error) => {
                tracing::warn!(%error, %key.message_id, "Bots turn aborted: cannot read the incoming message");
                return AttemptOutcome::Failed;
            }
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
            .unwrap_or_else(|error| {
                // The turn still runs, because a reply with no context beats no reply at all --
                // but it will read as though the agent forgot the conversation, and that is worth
                // being able to explain afterwards.
                tracing::warn!(%error, agent = %agent.id, "Replying without conversation history");
                Vec::new()
            });

        let policy_revision = match self
            .conversation_policy_revision(agent.id, incoming.conversation_id)
            .await
        {
            Some(r) => r,
            None => {
                tracing::warn!(agent = %agent.id, conversation = %incoming.conversation_id, "Bots turn aborted: no policy revision -- is this agent a member of the conversation?");
                return AttemptOutcome::Failed;
            }
        };

        // Read the roster before the turn, not just for mention resolution afterwards: the model
        // needs names to follow a multi-party transcript at all.
        let roster = self
            .store
            .room_agents(Principal::Agent(agent.id), incoming.conversation_id)
            .await
            .unwrap_or_else(|error| {
                // Without the roster the transcript has no speaker names and no mention can
                // resolve, so the agent answers into a room it cannot address.
                tracing::warn!(%error, agent = %agent.id, "Replying without the room roster");
                Vec::new()
            });
        let mut speakers: Vec<(Principal, String)> = roster
            .iter()
            .map(|a| (Principal::Agent(a.id), a.name.clone()))
            .collect();
        // A single token, deliberately. A live three-agent run against llama3.1 had an agent
        // reply "@the person ..." when this label was "the person" -- and a mention name stops at
        // the first space, so that resolved to "@the" and was reported as an unrecognized name.
        // One word keeps a mention of the human well-formed, and `HUMAN_LABEL` is passed to the
        // resolver as a known participant so it wakes nobody instead of reading as a typo.
        //
        // One human per room today. Rooms with several people need their real display names.
        speakers.push((Principal::User(self.owner), HUMAN_LABEL.to_string()));

        // Who is addressable, and by what name, is a context decision -- so the sentence is built
        // here and the runner only renders it.
        let others: Vec<&str> = roster
            .iter()
            .filter(|a| a.id != agent.id)
            .map(|a| a.name.as_str())
            .collect();
        let participants_note = if others.is_empty() {
            format!(" {HUMAN_LABEL} is the person you are helping.")
        } else {
            format!(
                " Also in this conversation: {}. Address one of them by writing @ before their \
                 name, and only when you actually want them to reply. {HUMAN_LABEL} is the person \
                 you are helping.",
                others.join(", ")
            )
        };

        let profile = self.store.user_profile(self.owner).await.unwrap_or_default();
        let participants_note = participants_note + &profile.prompt_context();
        let request = LocalTurnRequest {
            conversation_id: incoming.conversation_id,
            history,
            incoming: incoming.clone(),
            speakers: speakers.clone(),
            participants_note,
        };
        let Some(runner) = self.runner_for(agent) else {
            // No runner for this runtime on this host. Leave the delivery alone so
            // `bots_report_unroutable` can explain it and a later host can still answer.
            return AttemptOutcome::NoRunner;
        };
        let outcome = match runner.run_turn(agent, request).await {
            Ok(o) => o,
            Err(LocalTurnError::NoCapacity) => return AttemptOutcome::NoCapacity,
            Err(error) => {
                tracing::warn!(%error, agent = %agent.id, "Bots turn aborted: the runner failed");
                return AttemptOutcome::Failed;
            }
        };

        // --- Track A: who, if anyone, does this reply wake? --------------------------------
        //
        // Before this, the reply was always sent with an empty recipient list, so an agent's
        // words reached no other agent, ever. Everything below is what makes a non-empty list
        // safe: the chain root and depth carried by the delivery being drained, and the budgets
        // checked against them.
        let root = delivery.root_message_id.unwrap_or(key.message_id);
        let new_depth = delivery.turn_depth.saturating_add(1);
        let cause = DeliveryCause {
            cause_message_id: key.message_id,
            root_message_id: root,
            depth: new_depth,
        };

        let mut notices: Vec<String> = Vec::new();
        let mut recipients: Vec<AgentId> = Vec::new();
        let mut hold = false;

        if new_depth > self.budgets.max_depth {
            // The chain terminator. The reply is still said -- it just stops waking people.
            //
            // `max_depth == 0` is the special case: agent-to-agent is switched *off* by
            // configuration, which is the mode the first release ships in. That is not a budget
            // being hit, so it gets no notice -- announcing a limit under every single reply
            // would be pure noise in the one configuration where it is expected.
            if self.budgets.max_depth > 0 {
                notices.push(format!(
                    "Depth limit reached ({} hops); this reply notified no one.",
                    self.budgets.max_depth
                ));
            }
        } else {
            let mentions = crate::bots::resolve_mentions_with_participants(
                &outcome.reply_body,
                &roster,
                Principal::Agent(agent.id),
                &[HUMAN_LABEL.to_string()],
            );
            recipients = mentions.recipients;

            // Fan-out width. A human may address a whole room; an agent may not. Asymmetric on
            // purpose -- one human sentence costing six turns is a choice, one agent's reply
            // costing six is a multiplier.
            let width = self.budgets.max_active_specialist_handoffs_per_run as usize;
            if recipients.len() > width {
                notices.push(format!(
                    "An agent reply may address at most {width} teammates; {} of {} were notified.",
                    width,
                    recipients.len()
                ));
                recipients.truncate(width);
            }

            if !mentions.unresolved.is_empty() {
                notices.push(format!(
                    "Unrecognized name(s) in a reply, nobody notified for them: {}.",
                    mentions.unresolved.join(", ")
                ));
            }

            // The gate. Reaching it pauses the chain for a person instead of killing it, which
            // is the whole reason the number can be as high as it is.
            if !recipients.is_empty() {
                // Fails closed, and this is the one that matters most. `.unwrap_or(0)` read an
                // unreadable count as "nothing spent on this thread yet", which disables the
                // affordability gate exactly when the database is unhappy -- and this gate is
                // what stops one sentence in a six-agent room from turning into unbounded
                // fan-out. Holding pauses the chain for a person, which is visible and
                // releasable; guessing zero is neither.
                //
                // The notice says which of the two happened, because "limit reached" would be a
                // lie about a number nobody managed to read.
                let spent = self.store.turns_for_root(root).await;
                if let Err(error) = &spent {
                    tracing::warn!(
                        %error,
                        agent = %agent.id,
                        %root,
                        "Cannot read this thread's turn count; holding rather than replying past \
                         a budget that cannot be checked"
                    );
                }
                if let BudgetDecision::Hold(notice) = turn_budget_decision(
                    spent.ok(),
                    recipients.len(),
                    self.budgets.max_turns_per_root,
                ) {
                    hold = true;
                    notices.push(notice);
                }
            }
        }

        let sent = self.store.message_send_with_cause(
            Principal::Agent(agent.id),
            incoming.conversation_id,
            // Deterministic per (message, recipient): a retried drain pass over the same
            // still-pending delivery can never double-post a reply, same idempotency
            // mechanism `message_send` already gives every other caller.
            format!("delivery:{}:{}", key.message_id, key.recipient),
            policy_revision,
            recipients,
            NewMessage {
                thread_root: incoming.thread_root.or(Some(incoming.id)),
                kind: MessageKind::Text,
                body: Some(outcome.reply_body),
                attachment_refs: Vec::new(),
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
            Some(cause),
            hold,
        );
        let sent = match sent.await {
            Ok(m) => m,
            Err(error) => {
                tracing::warn!(%error, agent = %agent.id, "Bots turn aborted: the reply was refused");
                return AttemptOutcome::Failed;
            }
        };

        // Every budget event is visible. A room where agents quietly stop answering each other
        // is far harder to debug than one that says why -- and a held chain that nobody can see
        // is indistinguishable from a broken one. System messages create no deliveries, so these
        // are free.
        for (index, notice) in notices.iter().enumerate() {
            let _ = self
                .store
                .message_send_with_cause(
                    Principal::Agent(agent.id),
                    incoming.conversation_id,
                    format!("notice:{}:{}:{index}", key.message_id, key.recipient),
                    policy_revision,
                    Vec::new(),
                    NewMessage {
                        thread_root: sent.thread_root.or(Some(sent.id)),
                        kind: MessageKind::System,
                        body: Some(notice.clone()),
                        attachment_refs: Vec::new(),
                        task_ref: None,
                        turn_ref: None,
                        source_event_ref: None,
                    },
                    None,
                    false,
                )
                .await;
        }
        AttemptOutcome::Delivered
    }

    /// Release a chain a person has decided to let continue. Returns how many held deliveries
    /// went back to `pending`. Kept on the executor so a UI/CLI has one obvious entry point.
    pub async fn release_root(&self, root_message_id: MessageId) -> u32 {
        // 0 meant both "nothing was held" and "the release failed", and a person who just
        // pressed Release cannot tell those apart from the outside.
        match self.store.deliveries_release_root(root_message_id).await {
            Ok(n) => n,
            Err(error) => {
                tracing::warn!(%error, %root_message_id, "Cannot release this held chain");
                0
            }
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

#[cfg(test)]
mod budget_tests {
    use super::{turn_budget_decision, BudgetDecision};

    fn held(d: BudgetDecision) -> Option<String> {
        match d {
            BudgetDecision::Hold(n) => Some(n),
            BudgetDecision::Proceed => None,
        }
    }

    #[test]
    fn a_thread_under_budget_proceeds() {
        assert!(held(turn_budget_decision(Some(2), 1, 30)).is_none());
        // Exactly at the limit is still affordable; the gate is on exceeding it.
        assert!(held(turn_budget_decision(Some(29), 1, 30)).is_none());
    }

    #[test]
    fn a_thread_that_would_exceed_its_budget_is_held_and_told_why() {
        let notice = held(turn_budget_decision(Some(30), 1, 30)).expect("must hold");
        assert!(notice.contains("30-turn limit reached"), "{notice}");
        assert!(notice.contains("1 reply is held"), "{notice}");
        let many = held(turn_budget_decision(Some(28), 5, 30)).expect("must hold");
        assert!(many.contains("5 replies are held"), "{many}");
    }

    /// The one that matters. An unreadable count used to read as zero, which disabled the gate
    /// precisely when the database was unhappy -- so a six-agent room could fan out without
    /// limit at the worst possible moment. Holding is visible and releasable; a wrong zero is
    /// neither.
    #[test]
    fn an_unreadable_count_holds_rather_than_assuming_nothing_was_spent() {
        let notice = held(turn_budget_decision(None, 2, 30)).expect("must hold when unreadable");
        assert!(notice.contains("Couldn't check"), "{notice}");
        assert!(notice.contains("2 replies are held"), "{notice}");
        // And it must not claim a limit was reached -- nobody read the number.
        assert!(!notice.contains("limit reached"), "{notice}");
        // Even a generous budget cannot rescue an unreadable count.
        assert!(held(turn_budget_decision(None, 1, 100_000)).is_some());
    }
}
