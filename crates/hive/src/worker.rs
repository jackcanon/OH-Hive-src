//! Worker loop (ADR-005 pull dispatch) running the v1 agent loop (ADR-006 D41/D42).
//!
//! Per card, a bounded state machine, one inference per step:
//!   Draft → Critique → (Revise → Critique)* → Done      max 2 revisions
//! After every step the node checkpoints `LoopState` to the hub and the lease is
//! extended. On claim, if the hub hands back a checkpoint from a dead holder, the
//! loop resumes at that step instead of starting over.
//!
//! Not yet: tools/sandbox (D45), sub-delegation (D44), non-text modalities.

use anyhow::Result;
use ohhive_core::backend::Backend;
use ohhive_core::capability::{Capabilities, Requirements};
use ohhive_core::hub::{Claim, ClaimedCard, ClaimedProject, HubClient};
use ohhive_core::job::{Job, JobKind};
use ohhive_core::ledger::Usage;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const MAX_REVISIONS: u32 = 2;

pub struct Worker<'a> {
    pub hub: &'a HubClient,
    pub backend: &'a dyn Backend,
    pub caps: &'a Capabilities,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Draft,
    Critique,
    Revise,
    Done,
}

/// Everything needed to resume. This is what gets checkpointed.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoopState {
    version: u32,
    phase: Phase,
    step: u32,
    revisions: u32,
    draft: Option<String>,
    critique: Option<String>,
    usage: Usage,
    model: Option<String>,
}

impl LoopState {
    fn new(model: Option<String>) -> Self {
        LoopState { version: 1, phase: Phase::Draft, step: 0, revisions: 0, draft: None, critique: None, usage: Usage::default(), model }
    }
}

/// `required_capabilities.loop == "single"`: one Draft step, no critique/revise. Used for
/// conversational cards (interview turns), where the card's `inputs` *is* the full prompt.
fn single_step(card: &ClaimedCard) -> bool {
    card.required_capabilities.get("loop").and_then(|v| v.as_str()) == Some("single")
}

fn max_tokens_for(card: &ClaimedCard, phase: &Phase) -> u64 {
    if let Some(n) = card.required_capabilities.get("max_tokens").and_then(|v| v.as_u64()) {
        return n;
    }
    if *phase == Phase::Critique { 300 } else { 1024 }
}

fn context(card: &ClaimedCard, project: &ClaimedProject, deps: &serde_json::Map<String, serde_json::Value>) -> String {
    if single_step(card) {
        // The prompt was rendered by the hub; don't wrap it in the project/card framing.
        return card.inputs.clone();
    }
    let mut p = String::new();
    p.push_str("You are a worker node in OH Hive, a community compute network.\n");
    p.push_str(&format!("Project: {}\nProject goal: {}\n\n", project.title, project.goal));
    if !deps.is_empty() {
        p.push_str("Outputs from cards this one depends on:\n");
        for (k, v) in deps {
            p.push_str(&format!("--- {k} ---\n{}\n", v.as_str().unwrap_or("")));
        }
        p.push('\n');
    }
    p.push_str(&format!("Card: {}\nTask:\n{}\n\nAcceptance criteria: {}\n", card.title, card.inputs, card.acceptance));
    p
}

fn prompt_for(phase: &Phase, ctx: &str, st: &LoopState, single: bool) -> String {
    match phase {
        Phase::Draft if single => ctx.to_string(),
        Phase::Draft => format!("{ctx}\nRespond with the deliverable only — no preamble, no commentary."),
        Phase::Critique => format!(
            "{ctx}\nHere is a draft deliverable:\n<<<\n{}\n>>>\n\nCheck the draft strictly against the task and the acceptance criteria. \
             If it fully satisfies them, reply with exactly: PASS\nOtherwise reply with a short numbered list of concrete problems to fix. Nothing else.",
            st.draft.as_deref().unwrap_or("")
        ),
        Phase::Revise => format!(
            "{ctx}\nPrevious draft:\n<<<\n{}\n>>>\n\nReviewer found these problems:\n{}\n\nProduce a corrected deliverable that fixes every problem. Respond with the deliverable only.",
            st.draft.as_deref().unwrap_or(""),
            st.critique.as_deref().unwrap_or("")
        ),
        Phase::Done => String::new(),
    }
}

impl<'a> Worker<'a> {
    async fn infer(&self, project: &ClaimedProject, card: &ClaimedCard, model: Option<String>, prompt: String, max_tokens: u64) -> Result<(String, Usage)> {
        let job = Job {
            id: uuid::Uuid::new_v4(),
            kind: JobKind::AgentCard,
            project_id: project.id,
            card_id: Some(card.id),
            parent: None,
            requirements: Requirements { model_id: model, ..Default::default() },
            // Only ever *disable* thinking (single-step cards); forcing it on would error on models without it.
            input: if single_step(card) {
                serde_json::json!({ "prompt": prompt, "max_tokens": max_tokens, "think": false })
            } else {
                serde_json::json!({ "prompt": prompt, "max_tokens": max_tokens })
            },
            resume_from: None,
            created_at: chrono::Utc::now(),
        };
        let stream = self.backend.run(&job).await?;
        let (text, usage) = ohhive_core::backend::collect(stream).await?;
        Ok((text.trim().to_string(), usage))
    }

    /// Run the agent loop for one leased card to completion (or failure).
    async fn run_card(
        &self,
        card: ClaimedCard,
        project: ClaimedProject,
        deps: serde_json::Map<String, serde_json::Value>,
        resume: Option<LoopState>,
    ) -> Result<()> {
        let model = card
            .required_capabilities
            .get("model_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| self.default_model.clone())
            .or_else(|| self.caps.models.first().map(|m| m.id.clone()));
        let ctx = context(&card, &project, &deps);
        let single = single_step(&card);
        let mut st = match resume {
            Some(s) => {
                tracing::info!(card = %card.key, step = s.step, phase = ?s.phase, "resuming from checkpoint");
                s
            }
            None => LoopState::new(model.clone()),
        };

        while st.phase != Phase::Done {
            let phase = st.phase.clone();
            let max_tokens = max_tokens_for(&card, &phase);
            let (text, usage) = match self.infer(&project, &card, st.model.clone(), prompt_for(&phase, &ctx, &st, single), max_tokens).await {
                Ok(x) => x,
                Err(e) => {
                    tracing::error!(card = %card.key, phase = ?phase, "backend failed: {e}");
                    self.hub.fail_card(card.id, &format!("{phase:?}: {e}")).await?;
                    return Ok(());
                }
            };
            st.usage.add(usage);
            st.step += 1;
            match phase {
                Phase::Draft => {
                    st.draft = Some(text);
                    st.phase = if single { Phase::Done } else { Phase::Critique };
                }
                Phase::Critique => {
                    let pass = text.trim().eq_ignore_ascii_case("pass") || text.trim().to_uppercase().starts_with("PASS");
                    if pass || st.revisions >= MAX_REVISIONS {
                        if !pass {
                            tracing::warn!(card = %card.key, "critique still failing after {} revisions; shipping best draft", st.revisions);
                        }
                        st.critique = Some(text);
                        st.phase = Phase::Done;
                    } else {
                        st.critique = Some(text);
                        st.phase = Phase::Revise;
                    }
                }
                Phase::Revise => {
                    st.revisions += 1;
                    st.draft = Some(text);
                    st.phase = Phase::Critique;
                }
                Phase::Done => unreachable!(),
            }
            tracing::info!(card = %card.key, step = st.step, next = ?st.phase, tokens_out = st.usage.tokens_out, "step complete");
            if st.phase != Phase::Done {
                if let Err(e) = self.hub.checkpoint(card.id, st.step, &serde_json::to_value(&st)?, st.usage).await {
                    tracing::warn!(card = %card.key, "checkpoint failed (continuing): {e}");
                }
            }
        }

        let content = st.draft.clone().unwrap_or_default();
        let done = self.hub.complete_card(card.id, &content, st.model.as_deref(), st.usage).await?;
        tracing::info!(card = %card.key, steps = st.step, revisions = st.revisions, tokens_out = st.usage.tokens_out,
            earned = done.earned_honey, wallet = done.wallet_balance, fund = done.fund_balance, "card complete → review");
        println!(
            "\n[{}] {}  ({} steps, {} revision{})\n{}\n  → earned {:.4} $honey ({} tokens); wallet {:.2}, project fund {:.2}",
            project.title, card.title, st.step, st.revisions, if st.revisions == 1 { "" } else { "s" },
            content, done.earned_honey, st.usage.tokens_out, done.wallet_balance, done.fund_balance
        );
        Ok(())
    }

    /// One dispatch cycle. Returns true if a card was worked.
    pub async fn tick(&self) -> Result<bool> {
        match self.hub.claim_card().await? {
            Claim::NothingToDo => Ok(false),
            Claim::NotCheckedIn => {
                tracing::warn!("hub says we are not checked in; re-checking in");
                self.hub.check_in(self.caps, None).await?;
                Ok(false)
            }
            Claim::AlreadyLeased => {
                tracing::warn!("hub says we hold a lease already (previous run died?) — housekeeping will reap it");
                Ok(false)
            }
            Claim::Leased { card, project, dep_outputs, checkpoint, lease_expires_at } => {
                tracing::info!(card = %card.key, project = %project.title, expires = %lease_expires_at, resume = checkpoint.is_some(), "leased card");
                let resume = checkpoint.and_then(|c| serde_json::from_value::<LoopState>(c.state).ok()).filter(|s| s.version == 1);
                let card_id = card.id;
                let key = card.key.clone();
                // Graceful shutdown mid-card: hand the card back (checkpoints stay, next claimant resumes).
                tokio::select! {
                    r = self.run_card(card, project, dep_outputs, resume) => { r?; Ok(true) }
                    _ = shutdown_signal() => {
                        tracing::warn!(card = %key, "shutdown requested mid-card; releasing lease");
                        if let Err(e) = self.hub.release_card(card_id, "node shutting down").await {
                            tracing::warn!("release failed (housekeeping will reap the lease): {e}");
                        }
                        Err(anyhow::anyhow!("shutdown"))
                    }
                }
            }
        }
    }

    pub async fn run_forever(&self, poll: Duration, heartbeat_every: u32) -> Result<()> {
        let mut n: u32 = 0;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(poll) => {}
                _ = shutdown_signal() => {
                    let p = self.hub.check_out().await?;
                    println!("\nchecked out ({p})");
                    return Ok(());
                }
            }
            n = n.wrapping_add(1);
            if n % heartbeat_every == 0 {
                if let Err(e) = self.hub.heartbeat().await {
                    tracing::warn!("heartbeat failed: {e}");
                }
            }
            loop {
                match self.tick().await {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(e) if e.to_string() == "shutdown" => {
                        let p = self.hub.check_out().await?;
                        println!("\nchecked out ({p})");
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!("tick failed: {e}");
                        break;
                    }
                }
            }
        }
    }
}

/// Ctrl-C or SIGTERM (systemd stop).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("sigterm handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
