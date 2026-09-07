//! Worker loop (ADR-005 pull dispatch) running the v1 agent loop (ADR-006 D41/D42).
//!
//! Per card, a bounded state machine, one inference per step:
//!   Draft → Critique → (Revise → Critique)* → Done      max 2 revisions
//! After every step the node checkpoints `LoopState` to the hub and the lease is
//! extended. On claim, if the hub hands back a checkpoint from a dead holder, the
//! loop resumes at that step instead of starting over.
//!
//! The wasmtime/WASI sandbox mechanism itself lives in [`crate::sandbox`] (D45-D48:
//! fuel/memory-limited WASI components, scratch-dir-only filesystem, network shim) and is
//! wired in as the runtime enforcement point for `tools_level`. Not yet: this loop doesn't
//! call it for any real tool yet — there is no agent tool surface (artifact_get/put,
//! exec_wasm, spawn_child_card) defined here, sub-delegation (D44), or non-text modalities.
//!
//! Shared by the CLI and the desktop app (ADR-003 D27): stopping is a `watch` flag the shell owns
//! (Ctrl-C/SIGTERM in the CLI, a menu item in the app), progress is an optional broadcast of
//! [`WorkerEvent`]s the shell can render.

use crate::backend::Backend;
use crate::capability::{Capabilities, Requirements};
use crate::hub::{Claim, ClaimedCard, ClaimedProject, HubClient};
use crate::job::{Job, JobKind};
use crate::ledger::Usage;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::{broadcast, watch};

const MAX_REVISIONS: u32 = 2;

/// What the worker is doing, for a UI. Cheap to clone; sent on a broadcast channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerEvent {
    Leased {
        card: String,
        project: String,
        resume: bool,
    },
    Step {
        card: String,
        step: u32,
        next: String,
        tokens_out: u64,
    },
    Completed {
        card: String,
        project: String,
        earned_honey: f64,
        tokens_out: u64,
        wallet_balance: f64,
        fund_balance: f64,
    },
    Failed {
        card: String,
        error: String,
    },
    Released {
        card: String,
    },
    Idle,
}

pub struct Worker<'a> {
    pub hub: &'a HubClient,
    pub backend: &'a dyn Backend,
    pub caps: &'a Capabilities,
    pub default_model: Option<String>,
    /// Flip to `true` to stop: mid-card the lease is released (checkpoints stay), then check-out.
    pub stop: watch::Receiver<bool>,
    /// Optional progress feed for a UI.
    pub events: Option<broadcast::Sender<WorkerEvent>>,
}

/// Resolves when the stop flag becomes `true` (or the sender is dropped).
async fn stopped(mut rx: watch::Receiver<bool>) {
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
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
        LoopState {
            version: 1,
            phase: Phase::Draft,
            step: 0,
            revisions: 0,
            draft: None,
            critique: None,
            usage: Usage::default(),
            model,
        }
    }
}

/// `required_capabilities.loop == "single"`: one Draft step, no critique/revise. Used for
/// conversational cards (interview turns), where the card's `inputs` *is* the full prompt.
fn single_step(card: &ClaimedCard) -> bool {
    card.required_capabilities
        .get("loop")
        .and_then(|v| v.as_str())
        == Some("single")
}

fn max_tokens_for(card: &ClaimedCard, phase: &Phase) -> u64 {
    if let Some(n) = card
        .required_capabilities
        .get("max_tokens")
        .and_then(|v| v.as_u64())
    {
        return n;
    }
    if *phase == Phase::Critique {
        300
    } else {
        1024
    }
}

fn context(
    card: &ClaimedCard,
    project: &ClaimedProject,
    deps: &serde_json::Map<String, serde_json::Value>,
) -> String {
    if single_step(card) {
        // The prompt was rendered by the hub; don't wrap it in the project/card framing.
        return card.inputs.clone();
    }
    let mut p = String::new();
    p.push_str("You are a worker node in OH Hive, a community compute network.\n");
    p.push_str(&format!(
        "Project: {}\nProject goal: {}\n\n",
        project.title, project.goal
    ));
    if !deps.is_empty() {
        p.push_str("Outputs from cards this one depends on:\n");
        for (k, v) in deps {
            p.push_str(&format!("--- {k} ---\n{}\n", v.as_str().unwrap_or("")));
        }
        p.push('\n');
    }
    p.push_str(&format!(
        "Card: {}\nTask:\n{}\n\nAcceptance criteria: {}\n",
        card.title, card.inputs, card.acceptance
    ));
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
    fn emit(&self, e: WorkerEvent) {
        if let Some(tx) = &self.events {
            let _ = tx.send(e);
        }
    }

    async fn infer(
        &self,
        project: &ClaimedProject,
        card: &ClaimedCard,
        model: Option<String>,
        prompt: String,
        max_tokens: u64,
    ) -> Result<(String, Usage)> {
        let job = Job {
            id: uuid::Uuid::new_v4(),
            kind: JobKind::AgentCard,
            project_id: project.id,
            card_id: Some(card.id),
            parent: None,
            requirements: Requirements {
                model_id: model,
                ..Default::default()
            },
            // Hidden reasoning is opt-in per card (`required_capabilities.think: true`): members pay for
            // every token and never see reasoning tokens. We only ever *disable* it — forcing it on
            // errors on models without it.
            input: if card
                .required_capabilities
                .get("think")
                .and_then(|v| v.as_bool())
                == Some(true)
            {
                serde_json::json!({ "prompt": prompt, "max_tokens": max_tokens })
            } else {
                serde_json::json!({ "prompt": prompt, "max_tokens": max_tokens, "think": false })
            },
            resume_from: None,
            created_at: chrono::Utc::now(),
        };
        let stream = self.backend.run(&job).await?;
        let (text, usage) = crate::backend::collect(stream).await?;
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
            Some(mut s) => {
                tracing::info!(card = %card.key, step = s.step, phase = ?s.phase, "resuming from checkpoint");
                // The checkpoint's model is a record of what ran, not a requirement: this node's choice wins
                // (a different node may not have it; this node may have a better default now).
                s.model = model.clone();
                s
            }
            None => LoopState::new(model.clone()),
        };

        while st.phase != Phase::Done {
            let phase = st.phase.clone();
            let max_tokens = max_tokens_for(&card, &phase);
            let (text, usage) = match self
                .infer(
                    &project,
                    &card,
                    st.model.clone(),
                    prompt_for(&phase, &ctx, &st, single),
                    max_tokens,
                )
                .await
            {
                Ok(x) => x,
                Err(e) => {
                    tracing::error!(card = %card.key, phase = ?phase, "backend failed: {e}");
                    self.hub
                        .fail_card(card.id, &format!("{phase:?}: {e}"))
                        .await?;
                    self.emit(WorkerEvent::Failed {
                        card: card.title.clone(),
                        error: e.to_string(),
                    });
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
                    let pass = text.trim().eq_ignore_ascii_case("pass")
                        || text.trim().to_uppercase().starts_with("PASS");
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
            self.emit(WorkerEvent::Step {
                card: card.title.clone(),
                step: st.step,
                next: format!("{:?}", st.phase).to_lowercase(),
                tokens_out: st.usage.tokens_out,
            });
            if st.phase != Phase::Done {
                if let Err(e) = self
                    .hub
                    .checkpoint(card.id, st.step, &serde_json::to_value(&st)?, st.usage)
                    .await
                {
                    tracing::warn!(card = %card.key, "checkpoint failed (continuing): {e}");
                }
            }
        }

        let content = st.draft.clone().unwrap_or_default();
        let done = self
            .hub
            .complete_card(card.id, &content, st.model.as_deref(), st.usage)
            .await?;
        tracing::info!(card = %card.key, steps = st.step, revisions = st.revisions, tokens_out = st.usage.tokens_out,
            earned = done.earned_honey, wallet = done.wallet_balance, fund = done.fund_balance, "card complete → review");
        self.emit(WorkerEvent::Completed {
            card: card.title.clone(),
            project: project.title.clone(),
            earned_honey: done.earned_honey,
            tokens_out: st.usage.tokens_out,
            wallet_balance: done.wallet_balance,
            fund_balance: done.fund_balance,
        });
        if self.events.is_none() {
            println!(
                "\n[{}] {}  ({} steps, {} revision{})\n{}\n  → earned {:.4} $honey ({} tokens); wallet {:.2}, project fund {:.2}",
                project.title, card.title, st.step, st.revisions, if st.revisions == 1 { "" } else { "s" },
                content, done.earned_honey, st.usage.tokens_out, done.wallet_balance, done.fund_balance
            );
        }
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
            Claim::Leased {
                card,
                project,
                dep_outputs,
                checkpoint,
                lease_expires_at,
            } => {
                tracing::info!(card = %card.key, project = %project.title, expires = %lease_expires_at, resume = checkpoint.is_some(), "leased card");
                let resume = checkpoint
                    .and_then(|c| serde_json::from_value::<LoopState>(c.state).ok())
                    .filter(|s| s.version == 1);
                let card_id = card.id;
                let key = card.key.clone();
                let title = card.title.clone();
                self.emit(WorkerEvent::Leased {
                    card: title.clone(),
                    project: project.title.clone(),
                    resume: resume.is_some(),
                });
                // Graceful shutdown mid-card: hand the card back (checkpoints stay, next claimant resumes).
                tokio::select! {
                    r = self.run_card(card, project, dep_outputs, resume) => { r?; Ok(true) }
                    _ = stopped(self.stop.clone()) => {
                        tracing::warn!(card = %key, "shutdown requested mid-card; releasing lease");
                        if let Err(e) = self.hub.release_card(card_id, "node shutting down").await {
                            tracing::warn!("release failed (housekeeping will reap the lease): {e}");
                        }
                        self.emit(WorkerEvent::Released { card: title });
                        Err(anyhow::anyhow!("shutdown"))
                    }
                }
            }
        }
    }

    /// Poll for cards until the stop flag flips. Checks out of the hub on the way out.
    pub async fn run_forever(&self, poll: Duration, heartbeat_every: u32) -> Result<()> {
        let mut n: u32 = 0;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(poll) => {}
                _ = stopped(self.stop.clone()) => {
                    let p = self.hub.check_out().await?;
                    tracing::info!("checked out ({p})");
                    return Ok(());
                }
            }
            n = n.wrapping_add(1);
            if n.is_multiple_of(heartbeat_every) {
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
                        tracing::info!("checked out ({p})");
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!("tick failed: {e}");
                        break;
                    }
                }
            }
            self.emit(WorkerEvent::Idle);
        }
    }
}
