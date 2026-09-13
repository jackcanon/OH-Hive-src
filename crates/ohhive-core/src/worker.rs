//! Worker loop (ADR-005 pull dispatch) running the v1 agent loop (ADR-006 D41/D42).
//!
//! Per card, a bounded state machine, one inference per step:
//!   Draft → Critique → (Revise → Critique)* → Done      max 2 revisions
//! After every step the node checkpoints `LoopState` to the hub and the lease is
//! extended. On claim, if the hub hands back a checkpoint from a dead holder, the
//! loop resumes at that step instead of starting over.
//!
//! The wasmtime/WASI sandbox mechanism lives in [`crate::sandbox`] (D45-D48: fuel/memory-limited
//! WASI components, scratch-dir-only filesystem, network shim); [`crate::tools`] builds the v1
//! tool set on top of it (`exec_wasm`, `artifact_get/put`, `spawn_child_card`). `maybe_run_tool`
//! runs whichever of these a card asks for, in that fixed order, once, before `Draft` — a single
//! Act→Observe pass, not the full multi-turn ReAct loop the ADR's step machine describes; a card
//! can't yet ask for a *second* tool call mid-loop (that's `multi-tool-loop-design` on the
//! roadmap, not built in this pass — it needs the `Backend` trait to expose structured
//! function-calling, which the llama.cpp adapter doesn't yet).
//!
//! `spawn_child` *can* pause the loop mid-run, though: setting `required_capabilities.spawn_child`'s
//! `wait: true` (ADR-006 D44) adds a phase, `WaitingOnChild`, between the pre-Draft tool step and
//! `Draft` itself. On a fresh start, once the child is created, the card checkpoints, releases
//! its lease via `HubClient::wait_on_child` (marking it `waiting_on_child` in `hive.cards` instead
//! of `ready`), and `run_card` returns — same shape as the graceful-shutdown release path, just
//! without a `WorkerEvent::Released`. A DB trigger (`hive.cascade_child_status`) flips the card
//! back to `ready` once every child it spawned reaches `review`/`done`, or cascades a `blocked`
//! up immediately if a child fails, so a parent can never wait forever on a child that won't
//! finish. Whichever node next claims it — not necessarily this one, per D42 — gets the child's
//! output already sitting in `dep_outputs` (the hub folds spawned-children output into the same
//! payload declared `deps` use), folds it into `tool_output` the same way a tool's summary would
//! be, and resumes straight into `Draft`.
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
use uuid::Uuid;

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
    /// ADR-006 D44: this card spawned a child with `wait: true` and has released its lease
    /// to wait for it — see the module doc.
    Blocked {
        card: String,
        waiting_on: String,
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
    /// Between the pre-Draft tool step and `Draft`, only reachable via a `spawn_child` with
    /// `wait: true` (ADR-006 D44). The card holds no lease while in this phase in the DB
    /// (`hive.cards.status = 'waiting_on_child'`) — see the module doc.
    WaitingOnChild,
    Draft,
    Critique,
    Revise,
    Done,
}

/// Everything needed to resume. This is what gets checkpointed.
///
/// `version` bumped 1 → 2 for `tool_output`, 2 → 3 for `pending_child_key`: an old checkpoint
/// just isn't resumed (see `tick()`'s version filter) rather than risk misreading a shape it
/// wasn't written in — the same versioning the ADR's checkpoint-incompatibility mitigation
/// calls for.
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
    /// Set exactly while `phase == WaitingOnChild`: the `key` of the child card this one is
    /// blocked on (ADR-006 D44). Looked up in `dep_outputs` on resume — see the module doc.
    #[serde(default)]
    pending_child_key: Option<String>,
}

const LOOP_STATE_VERSION: u32 = 3;

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
            pending_child_key: None,
        }
    }
}

/// What [`Worker::maybe_run_tool`] found. Two independent things can come out of the one
/// pre-Draft tool step: text to fold into context (as before), and — new for ADR-006 D44 —
/// a child card to pause on before `Draft` runs at all.
#[derive(Default)]
struct ToolPhaseResult {
    /// Joined summary of every tool that ran, or `None` if the card asked for no tools.
    summary: Option<String>,
    /// Set when a `spawn_child` with `wait: true` created its child successfully: that
    /// child's `(id, key)`. `run_card` blocks on this instead of proceeding to `Draft`.
    wait_on_child: Option<(Uuid, String)>,
}

/// Pulls the child card's id back out of `run_spawn_child_card`'s [`ToolOutcome::data`]
/// (`{"card_id": "<uuid>", "key": "..."}`, ADR-006 D44). A free function, not inlined into
/// [`Worker::maybe_run_tool`], specifically so this JSON-shape assumption is unit-testable
/// without a live `HubClient`.
fn spawned_card_id(data: &Option<serde_json::Value>) -> Option<Uuid> {
    data.as_ref()
        .and_then(|d| d.get("card_id"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
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
    p.push_str("You are a worker node in Hive, a community compute network.\n");
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
        Phase::WaitingOnChild => {
            unreachable!("run_card resolves WaitingOnChild to Draft before ever building a prompt")
        }
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
    /// output), `spawn_child_card` (create a sibling) — once, before `Draft`. See
    /// `crate::tools`'s module doc for exactly which fields each tool reads.
    ///
    /// `exec_wasm` fails closed like the sandbox itself does: no engine configured, or
    /// this node's operator restricted it to inference-only, both produce a message
    /// explaining why — not a silently-skipped tool call the model isn't told about.
    /// `artifact_get`/`artifact_put`/`spawn_child_card` aren't gated by `tools_level` at
    /// all — they don't execute untrusted code, only move bytes through the hub/regional
    /// servers or create a row, so an inference-only node can still use them.
    #[cfg(feature = "sandbox")]
    async fn maybe_run_tool(&self, card: &ClaimedCard) -> ToolPhaseResult {
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
            return ToolPhaseResult::default();
        }

        let mut parts = Vec::new();
        let mut inputs_dir = None;
        let mut wait_on_child = None;

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
            let wait = spec.wait;
            match crate::tools::run_spawn_child_card(self.hub, card.id, &spec).await {
                Ok(outcome) => {
                    tracing::info!(card = %card.key, child = %spec.key, wait, "spawn_child_card ran");
                    if wait {
                        match spawned_card_id(&outcome.data) {
                            Some(id) => wait_on_child = Some((id, spec.key.clone())),
                            None => {
                                tracing::warn!(card = %card.key, "spawn_child_card said wait=true but returned no usable card_id; proceeding without blocking")
                            }
                        }
                    }
                    parts.push(outcome.summary);
                }
                Err(e) => {
                    tracing::warn!(card = %card.key, "spawn_child_card failed: {e}");
                    parts.push(format!("[tool error] spawn_child_card: {e}"));
                }
            }
        }

        ToolPhaseResult {
            summary: (!parts.is_empty()).then(|| parts.join("\n")),
            wait_on_child,
        }
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
    async fn maybe_run_tool(&self, _card: &ClaimedCard) -> ToolPhaseResult {
        ToolPhaseResult::default()
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
                // ADR-006 D44: resuming out of WaitingOnChild. If this card is here at all,
                // hive.node_claim_card only ever claims 'ready' cards and the DB trigger only
                // flips this one back to 'ready' once every child it spawned reached
                // review/done — so the child's output should already be sitting in
                // `dep_outputs`, exactly where a declared dep's output would be (see
                // hive.card_dep_outputs). Fold it into tool_output the same way a tool
                // summary would be, then fall through to Draft like any fresh start.
                if s.phase == Phase::WaitingOnChild {
                    if let Some(child_key) = s.pending_child_key.take() {
                        match deps.get(&child_key).and_then(|v| v.as_str()) {
                            Some(output) => {
                                let section = format!("Child card '{child_key}' output:\n{output}");
                                s.tool_output = Some(match s.tool_output.take() {
                                    Some(prev) => format!("{prev}\n\n{section}"),
                                    None => section,
                                });
                            }
                            None => tracing::warn!(card = %card.key, child = %child_key,
                                "resumed out of waiting_on_child but no output for it yet; drafting without it"),
                        }
                    }
                    s.phase = Phase::Draft;
                }
                s
            }
            None => {
                let mut s = LoopState::new(model.clone());
                // One Act→Observe pass before Draft (see the module doc). Only on a fresh
                // start — a resumed card already ran this, and re-running would re-execute
                // a tool a checkpoint may have already charged/logged.
                let tool = self.maybe_run_tool(&card).await;
                s.tool_output = tool.summary;
                if let Some((child_id, child_key)) = tool.wait_on_child {
                    s.phase = Phase::WaitingOnChild;
                    s.pending_child_key = Some(child_key.clone());
                    if let Err(e) = self
                        .hub
                        .checkpoint(card.id, 0, &serde_json::to_value(&s)?, s.usage)
                        .await
                    {
                        tracing::warn!(card = %card.key, "checkpoint before blocking on child failed (continuing): {e}");
                    }
                    match self.hub.wait_on_child(card.id, child_id).await {
                        Ok(_) => {
                            tracing::info!(card = %card.key, child = %child_key, "blocked on spawned child, lease released");
                            self.emit(WorkerEvent::Blocked {
                                card: card.title.clone(),
                                waiting_on: child_key,
                            });
                            return Ok(());
                        }
                        Err(e) => {
                            // Couldn't release/mark the card blocked (hub down, lease already
                            // gone, whatever). Don't strand it in a phase it can never leave
                            // under its own steam if the DB never got the memo — fall through
                            // and draft anyway rather than get stuck.
                            tracing::warn!(card = %card.key, "wait_on_child failed, drafting without waiting: {e}");
                            s.phase = Phase::Draft;
                        }
                    }
                }
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
                Phase::Done => unreachable!("the while loop's own condition excludes Done"),
                Phase::WaitingOnChild => {
                    unreachable!(
                        "run_card resolves WaitingOnChild to Draft before this loop starts"
                    )
                }
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
        // This node's hub round-trip time, as measured on the previous heartbeat -- fed back
        // into the next call so the hub always has a (one-interval-stale) number.
        let mut last_rtt_ms: Option<u64> = None;
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
                match self.hub.heartbeat(last_rtt_ms).await {
                    Ok((_, rtt)) => last_rtt_ms = Some(rtt),
                    Err(e) => tracing::warn!("heartbeat failed: {e}"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawned_card_id_reads_a_well_formed_data_payload() {
        let id = Uuid::new_v4();
        let data = Some(serde_json::json!({ "card_id": id.to_string(), "key": "child-a" }));
        assert_eq!(spawned_card_id(&data), Some(id));
    }

    #[test]
    fn spawned_card_id_is_none_without_data() {
        assert_eq!(spawned_card_id(&None), None);
    }

    #[test]
    fn spawned_card_id_is_none_when_card_id_is_missing_or_malformed() {
        assert_eq!(
            spawned_card_id(&Some(serde_json::json!({ "key": "child-a" }))),
            None
        );
        assert_eq!(
            spawned_card_id(&Some(serde_json::json!({ "card_id": "not-a-uuid" }))),
            None
        );
        assert_eq!(
            spawned_card_id(&Some(serde_json::json!({ "card_id": 12345 }))),
            None
        );
    }

    #[test]
    fn loop_state_version_bump_means_an_old_waiting_on_child_free_checkpoint_is_not_misread() {
        // Regression guard for the version bump itself (2 -> 3): a checkpoint written before
        // `pending_child_key` existed still deserializes (serde default), but `tick()` only
        // resumes checkpoints whose `version` matches LOOP_STATE_VERSION, so an old one is
        // never handed to `run_card` at all -- it starts the card fresh instead, exactly the
        // ADR-006 checkpoint-incompatibility mitigation this module's doc comment describes.
        let old_shape = serde_json::json!({
            "version": 2,
            "phase": "draft",
            "step": 1,
            "revisions": 0,
            "draft": null,
            "critique": null,
            "usage": { "tokens_in": 0, "tokens_out": 0, "compute_seconds": 0.0 },
            "model": null,
            "tool_output": null,
        });
        let parsed: LoopState = serde_json::from_value(old_shape)
            .expect("old shape still parses (pending_child_key defaults)");
        assert_eq!(parsed.pending_child_key, None);
        assert_ne!(
            parsed.version, LOOP_STATE_VERSION,
            "this test's fixture must predate the bump it's guarding"
        );
    }
}
