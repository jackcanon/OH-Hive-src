//! Worker loop (ADR-005 pull dispatch) running the v1 agent loop (ADR-006 D41/D42).
//!
//! Per card, a bounded state machine, one inference per step:
//!   Draft → Critique → (Revise → Critique)* → Done      max 2 revisions
//! After every step the node checkpoints `LoopState` to the hub and the lease is
//! extended. On claim, if the hub hands back a checkpoint from a dead holder, the
//! loop resumes at that step instead of starting over.
//!
//! The wasmtime/WASI sandbox mechanism lives in [`crate::sandbox`] (D45-D48: fuel/memory-limited
//! WASI components, scratch-dir-only filesystem, network shim); [`crate::tools`] is the one v1
//! tool built on top of it (`exec_wasm`). A card that sets `required_capabilities.exec_wasm` gets
//! it run once, before `Draft`, and the result folded into every prompt as tool output — a single
//! Act→Observe pass, not the full multi-turn ReAct loop the ADR's step machine describes; a card
//! can't yet ask for a *second* tool call mid-loop. Not yet: `artifact_get/put` and
//! `spawn_child_card` (both need hub RPCs that don't exist), sub-delegation (D44), or non-text
//! modalities.
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
    /// Where per-card scratch dirs and staged tool components live (see [`crate::tools`]).
    #[cfg(feature = "sandbox")]
    pub data_dir: std::path::PathBuf,
    /// The wasmtime engine for `exec_wasm` calls. `None` is a legitimate configuration —
    /// [`Worker::maybe_run_tool`] checks `caps.tools_level` first and only reaches for this
    /// when a card actually asks for a tool, so an inference-only contributor never needs one.
    #[cfg(feature = "sandbox")]
    pub sandbox: Option<&'a crate::sandbox::Sandbox>,
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
///
/// `version` bumped 1 → 2 for `tool_output`: an old checkpoint just isn't resumed (see
/// `tick()`'s version filter) rather than risk misreading a shape it wasn't written in —
/// the same versioning the ADR's checkpoint-incompatibility mitigation calls for.
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
    /// Result of the one pre-Draft `exec_wasm` call, if the card asked for one (see
    /// [`Worker::maybe_run_tool`]). `None` for cards that don't use tools at all.
    #[serde(default)]
    tool_output: Option<String>,
}

const LOOP_STATE_VERSION: u32 = 2;

impl LoopState {
    fn new(model: Option<String>) -> Self {
        LoopState {
            version: LOOP_STATE_VERSION,
            phase: Phase::Draft,
            step: 0,
            revisions: 0,
            draft: None,
            critique: None,
            usage: Usage::default(),
            model,
            tool_output: None,
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
    tool_output: Option<&str>,
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
    if let Some(t) = tool_output {
        p.push_str(&format!(
            "Tool output (exec_wasm ran before drafting):\n{t}\n\n"
        ));
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

    /// Run whichever tools this card asked for via `required_capabilities`, in a fixed
    /// order — `artifact_get` (stage input), `exec_wasm` (run), `artifact_put` (store
    /// output), `spawn_child_card` (create a sibling) — once, before `Draft`. Returns the
    /// combined text to fold into the card's context, or `None` if the card asked for
    /// nothing. See `crate::tools`'s module doc for exactly which fields each tool reads.
    ///
    /// `exec_wasm` fails closed like the sandbox itself does: no engine configured, or
    /// this node's operator restricted it to inference-only, both produce a message
    /// explaining why — not a silently-skipped tool call the model isn't told about.
    /// `artifact_get`/`artifact_put`/`spawn_child_card` aren't gated by `tools_level` at
    /// all — they don't execute untrusted code, only move bytes through the hub/regional
    /// servers or create a row, so an inference-only node can still use them.
    #[cfg(feature = "sandbox")]
    async fn maybe_run_tool(&self, card: &ClaimedCard) -> Option<String> {
        let wants_exec_wasm = card
            .required_capabilities
            .get("exec_wasm")
            .and_then(|v| v.as_bool())
            == Some(true);
        let get_hash = card
            .required_capabilities
            .get("artifact_get_hash")
            .and_then(|v| v.as_str());
        let wants_put = card
            .required_capabilities
            .get("artifact_put")
            .and_then(|v| v.as_bool())
            == Some(true);
        let spawn_spec = card
            .required_capabilities
            .get("spawn_child")
            .and_then(|v| serde_json::from_value::<crate::tools::SpawnChildSpec>(v.clone()).ok());

        if !wants_exec_wasm && get_hash.is_none() && !wants_put && spawn_spec.is_none() {
            return None;
        }

        let mut parts = Vec::new();
        let mut inputs_dir = None;

        if let Some(hash) = get_hash {
            match crate::tools::run_artifact_get(self.hub, &self.data_dir, card.id, hash).await {
                Ok(outcome) => {
                    tracing::info!(card = %card.key, hash, ok = outcome.ok, "artifact_get ran");
                    inputs_dir = Some(crate::sandbox::inputs_dir_for(
                        &self.data_dir,
                        &card.id.to_string(),
                    ));
                    parts.push(outcome.summary);
                }
                Err(e) => {
                    tracing::warn!(card = %card.key, "artifact_get failed: {e}");
                    parts.push(format!("[tool error] artifact_get: {e}"));
                }
            }
        }

        if wants_exec_wasm {
            parts.push(self.run_exec_wasm_tool(card, inputs_dir.as_deref()).await);
        }

        if wants_put {
            match crate::tools::run_artifact_put(self.hub, &self.data_dir, card.id, None).await {
                Ok(outcome) => {
                    tracing::info!(card = %card.key, ok = outcome.ok, "artifact_put ran");
                    parts.push(outcome.summary);
                }
                Err(e) => {
                    tracing::warn!(card = %card.key, "artifact_put failed: {e}");
                    parts.push(format!("[tool error] artifact_put: {e}"));
                }
            }
        }

        if let Some(spec) = spawn_spec {
            match crate::tools::run_spawn_child_card(self.hub, card.id, &spec).await {
                Ok(outcome) => {
                    tracing::info!(card = %card.key, child = %spec.key, "spawn_child_card ran");
                    parts.push(outcome.summary);
                }
                Err(e) => {
                    tracing::warn!(card = %card.key, "spawn_child_card failed: {e}");
                    parts.push(format!("[tool error] spawn_child_card: {e}"));
                }
            }
        }

        Some(parts.join("\n"))
    }

    /// The `exec_wasm` step of [`Worker::maybe_run_tool`], split out because it's the one
    /// step with its own fail-closed gate (the sandbox engine / `tools_level`).
    #[cfg(feature = "sandbox")]
    async fn run_exec_wasm_tool(
        &self,
        card: &ClaimedCard,
        inputs_dir: Option<&std::path::Path>,
    ) -> String {
        let Some(sandbox) = self.sandbox else {
            tracing::warn!(card = %card.key, "card requests exec_wasm but this node has no sandbox engine configured");
            return "[tool error] this node cannot run sandboxed tools (no engine configured)"
                .into();
        };
        if self.caps.tools_level != crate::capability::ToolsLevel::SandboxedTools {
            tracing::warn!(card = %card.key, "card requests exec_wasm but this node is tools_level=inference_only");
            return "[tool skipped] this node's operator has disabled sandboxed tools".into();
        }
        let net = crate::sandbox::NetPolicy::new(self.caps.allow_internet, card.requires_internet);
        match crate::tools::run_exec_wasm(
            sandbox,
            &self.data_dir,
            card.id,
            inputs_dir,
            self.caps.tools_level,
            net,
        )
        .await
        {
            Ok(outcome) => {
                tracing::info!(card = %card.key, ok = outcome.ok, "exec_wasm tool ran");
                outcome.summary
            }
            Err(e) => {
                tracing::warn!(card = %card.key, "exec_wasm tool call failed: {e}");
                format!("[tool error] {e}")
            }
        }
    }

    #[cfg(not(feature = "sandbox"))]
    async fn maybe_run_tool(&self, _card: &ClaimedCard) -> Option<String> {
        None
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
        let single = single_step(&card);
        let mut st = match resume {
            Some(mut s) => {
                tracing::info!(card = %card.key, step = s.step, phase = ?s.phase, "resuming from checkpoint");
                // The checkpoint's model is a record of what ran, not a requirement: this node's choice wins
                // (a different node may not have it; this node may have a better default now).
                s.model = model.clone();
                s
            }
            None => {
                let mut s = LoopState::new(model.clone());
                // One Act→Observe pass before Draft (see the module doc). Only on a fresh
                // start — a resumed card already ran this, and re-running would re-execute
                // a tool a checkpoint may have already charged/logged.
                s.tool_output = self.maybe_run_tool(&card).await;
                s
            }
        };
        let ctx = context(&card, &project, &deps, st.tool_output.as_deref());

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
                    .filter(|s| s.version == LOOP_STATE_VERSION);
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
