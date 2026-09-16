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
//! tool set on top of it (`exec_wasm`, `artifact_get/put`, `spawn_child_card`, and — #177/ADR-023 —
//! `mcp_tool_call`, which spawns a member-configured MCP server via [`crate::mcp`] instead of
//! running inside the wasmtime sandbox at all; see that module's doc for the different trust
//! model). `maybe_run_tool` runs whichever of these a card asks for, in that fixed order, once,
//! before `Draft` — a single Act→Observe pass, not the full multi-turn ReAct loop the ADR's step
//! machine describes; a card can't yet ask for a *second* tool call mid-loop (that's
//! `multi-tool-loop-design` on the roadmap, not built in this pass — it needs the `Backend` trait
//! to expose structured function-calling, which the llama.cpp adapter doesn't yet).
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
//!
//! **`modality = 'code'` is the one exception to all of the above** (ADR-024, #185): `run_card`
//! dispatches a `'code'` card straight to `run_code_card` before any Draft-phase state is even
//! built, running [`crate::coder`]'s own multi-turn tool-calling loop instead of the bounded
//! Draft/Critique/Revise machine — the "needs the `Backend` trait to expose structured
//! function-calling" gap the paragraph above describes for the general case is exactly what
//! `crate::backend::llama_cpp`'s new `chat_with_tools` surface (plus [`Backend::as_any`] for
//! recovering the concrete backend from this struct's `&dyn Backend` field) closes, but only for
//! this one modality's own separate loop — every other modality still gets one tool call before
//! `Draft`, not a real ReAct loop.

use crate::backend::Backend;
use crate::capability::{Capabilities, Requirements};
use crate::hub::{Claim, ClaimedCard, ClaimedProject, Hub};
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
    #[cfg(test)]
    pub capacity_path: std::path::PathBuf,
    pub hub: &'a dyn Hub,
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

/// Per-card output cap. A card can name its own `required_capabilities.max_tokens`; these are the
/// defaults for one that doesn't.
///
/// The Draft/Revise default is deliberately a whole-artifact budget, not a paragraph one: those
/// phases produce the thing the card is *for*, and at the old 1024 a code card that wrote a real
/// source file got cut off mid-file. It is not larger than this because `max_tokens` also has to
/// fit inside the model's context window, and a local node may be running something with a small
/// one — a card that genuinely needs more says so explicitly. What makes that safe now is that
/// hitting the cap is *loud*: [`crate::backend::Completion::truncated`] carries
/// `finish_reason == "length"` back up, and [`Worker::run_card`] fails a truncated Draft/Revise
/// rather than shipping half an artifact (ADR-036's first line of defence, at the producing node).
const MAX_TOKENS_ARTIFACT: u64 = 4096;
/// Critique writes prose about the draft, not the draft itself, so it needs far less — but 300 was
/// too tight to review anything real, and a critique cut mid-sentence feeds a misleading Revise.
const MAX_TOKENS_CRITIQUE: u64 = 1024;

fn max_tokens_for(card: &ClaimedCard, phase: &Phase) -> u64 {
    if let Some(n) = card
        .required_capabilities
        .get("max_tokens")
        .and_then(|v| v.as_u64())
    {
        return n;
    }
    if *phase == Phase::Critique {
        MAX_TOKENS_CRITIQUE
    } else {
        MAX_TOKENS_ARTIFACT
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
    ) -> Result<crate::backend::Completion> {
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
        let completion = crate::backend::collect(stream).await?;
        Ok(crate::backend::Completion {
            text: completion.text.trim().to_string(),
            ..completion
        })
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
        // #177/ADR-023: mcp_server_id + mcp_tool_name are the two host-trusted, card-creation-time
        // fields a card sets to opt into calling one tool on one of the member's own configured
        // MCP servers (crate::tools's module doc). Both a valid server id *and* a tool name are
        // required to actually run anything -- see below.
        let mcp_server_id = card
            .required_capabilities
            .get("mcp_server_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());
        let mcp_tool_name = card
            .required_capabilities
            .get("mcp_tool_name")
            .and_then(|v| v.as_str());

        if !wants_exec_wasm
            && get_hash.is_none()
            && !wants_put
            && spawn_spec.is_none()
            && mcp_server_id.is_none()
        {
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

        if let Some(server_id) = mcp_server_id {
            match mcp_tool_name {
                Some(tool_name) => {
                    parts.push(self.run_mcp_tool(card, server_id, tool_name).await);
                }
                None => {
                    tracing::warn!(card = %card.key, "card set mcp_server_id without mcp_tool_name; nothing to call");
                    parts.push(
                        "[tool error] mcp_server_id was set without mcp_tool_name — nothing to call"
                            .to_string(),
                    );
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

    /// The `mcp_tool_call` step of [`Worker::maybe_run_tool`] (#177, ADR-023) — split out for the
    /// same reason `run_exec_wasm_tool` is: it's the one step with its own fail-closed
    /// `tools_level` gate. Unlike `exec_wasm`, there is no separate "engine configured?" check
    /// here — there's no engine to configure; the gate is entirely `tools_level` plus whatever
    /// `hub.mcp_server_config` itself enforces (ownership + `enabled`, re-checked server-side
    /// independently of what `hive.node_claim_card` already checked at claim time).
    #[cfg(feature = "sandbox")]
    async fn run_mcp_tool(&self, card: &ClaimedCard, server_id: Uuid, tool_name: &str) -> String {
        if self.caps.tools_level != crate::capability::ToolsLevel::SandboxedTools {
            tracing::warn!(card = %card.key, "card requests mcp_server_id but this node is tools_level=inference_only");
            return "[tool skipped] this node's operator has disabled sandboxed tools".into();
        }
        let arguments = card
            .required_capabilities
            .get("mcp_tool_args")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        match crate::tools::run_mcp_tool_call(self.hub, server_id, tool_name, arguments).await {
            Ok(outcome) => {
                tracing::info!(card = %card.key, server = %server_id, tool = tool_name, ok = outcome.ok, "mcp_tool_call ran");
                outcome.summary
            }
            Err(e) => {
                tracing::warn!(card = %card.key, server = %server_id, tool = tool_name, "mcp_tool_call failed: {e}");
                format!("[tool error] {e}")
            }
        }
    }

    #[cfg(not(feature = "sandbox"))]
    async fn maybe_run_tool(&self, _card: &ClaimedCard) -> ToolPhaseResult {
        ToolPhaseResult::default()
    }

    /// Pick this session's brain when `required_capabilities.brain == "local"` (ADR-024 decision
    /// 3): downcast the node's own `&dyn Backend` back to a concrete `LlamaCppBackend` (see
    /// `Backend::as_any`'s doc for why that downcast is needed at all) and resolve which model to
    /// use, in the same precedence `run_card`'s own `model` variable already uses (card-declared
    /// `model_id`, else this node's configured default, else its first advertised model).
    /// `Err` is a plain human-readable message, not a full error type, since its only consumer
    /// (`run_code_card`) just needs something to `fail_card` with.
    #[cfg(all(feature = "sandbox", feature = "llama-cpp"))]
    fn local_brain<'b>(
        &'b self,
        card: &ClaimedCard,
        spec: &crate::coder::CodeSessionSpec,
    ) -> Result<Box<dyn crate::coder::CodeBrain + 'b>, String> {
        let llama = self
            .backend
            .as_any()
            .downcast_ref::<crate::backend::llama_cpp::LlamaCppBackend>()
            .ok_or_else(|| {
                "brain: local requested but this node's configured backend is not llama.cpp/Ollama"
                    .to_string()
            })?;
        let model = spec
            .model_id
            .clone()
            .or_else(|| self.default_model.clone())
            .or_else(|| self.caps.models.first().map(|m| m.id.clone()))
            .ok_or_else(|| {
                "no model available for a local coding brain (no model_id, no default, no advertised models)"
                    .to_string()
            })?;
        let max_tokens = max_tokens_for(card, &Phase::Draft);
        Ok(Box::new(crate::coder::LocalBrain::new(
            llama, model, max_tokens,
        )))
    }

    /// This node was built without the `llama-cpp` feature at all -- `"brain": "local"` fails
    /// cleanly instead of `crate::coder`'s `LocalBrain` (which needs that feature) failing to
    /// compile in the first place.
    #[cfg(all(feature = "sandbox", not(feature = "llama-cpp")))]
    fn local_brain<'b>(
        &'b self,
        _card: &ClaimedCard,
        _spec: &crate::coder::CodeSessionSpec,
    ) -> Result<Box<dyn crate::coder::CodeBrain + 'b>, String> {
        Err("brain: local requested but this node was built without llama-cpp support".to_string())
    }

    /// Run a `'code'`-modality card (ADR-024, #185) — dispatched from `run_card`'s own top, see
    /// that function's doc comment for why this is a separate control flow rather than another
    /// `Phase`. `crate::tools::run_code_session` does the actual work (workspace prep +
    /// `crate::coder`'s multi-turn tool-calling loop); this method's job is picking a brain,
    /// handling the small number of ways a session can fail to even start, and reporting the
    /// result via `node_complete_card` — the same RPC `run_card`'s own Draft/Critique loop
    /// reports through at the end, just called directly here instead of after a `while` loop.
    ///
    /// Every failure path here uses `fail_card`, not a degraded `complete_card`: each one
    /// (wrong `tools_level`, a bad spec, an unavailable brain) means the session never actually
    /// started doing any work, the same class of failure `run_card`'s own `self.infer()` error
    /// path already reports via `fail_card` rather than shipping a low-quality draft. Once
    /// `crate::tools::run_code_session` returns an actual [`crate::tools::ToolOutcome`] (meaning
    /// the session ran — however many turns, however it went), this calls `complete_card` with its
    /// summary, even when `outcome.ok` is `false` (a session that hit `max_turns` or otherwise
    /// didn't cleanly finish still produced real work the member should be able to review — see
    /// `crate::coder`'s module doc: hitting the turn limit is reported honestly, not hidden as a
    /// failure) — **except** when the session stopped because `lease_expires_at` passed, in which
    /// case this releases the card instead (2026-09-14, ADR-029 review finding: completing here
    /// would race a lease the hub may have already reassigned to another node).
    ///
    /// No usage/honey metering in this pass: local coding-agent compute isn't billed (ADR-024's
    /// gate requires `execution_mode = 'local'` for every `'code'` card, and honey only applies
    /// to `'hive'`-mode funded projects), so `complete_card` is called with `Usage::default()`.
    #[cfg(feature = "sandbox")]
    async fn run_code_card(
        &self,
        card: ClaimedCard,
        project: ClaimedProject,
        lease_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        if self.caps.tools_level != crate::capability::ToolsLevel::SandboxedTools {
            // Should be unreachable: `hive.node_claim_card`'s ADR-024 gate already requires
            // tools_level = sandboxed_tools for any 'code' card. Checked again here, fail-closed,
            // for the same defense-in-depth reason `run_exec_wasm_tool`/`run_mcp_tool` do.
            let msg =
                "this node is tools_level=inference_only; cannot run a coding session".to_string();
            tracing::error!(card = %card.key, "claimed a 'code' card but {msg} (should be unreachable)");
            self.hub.fail_card(card.id, &msg).await?;
            self.emit(WorkerEvent::Failed {
                card: card.title.clone(),
                error: msg,
            });
            return Ok(());
        }

        let spec = match crate::coder::CodeSessionSpec::from_required_capabilities(
            &card.required_capabilities,
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(card = %card.key, "bad code-session spec: {e}");
                self.hub
                    .fail_card(
                        card.id,
                        &format!("invalid required_capabilities for a code card: {e}"),
                    )
                    .await?;
                self.emit(WorkerEvent::Failed {
                    card: card.title.clone(),
                    error: e.to_string(),
                });
                return Ok(());
            }
        };

        let brain = match spec.brain.as_str() {
            "local" => match self.local_brain(&card, &spec) {
                Ok(b) => b,
                Err(msg) => {
                    self.hub.fail_card(card.id, &msg).await?;
                    self.emit(WorkerEvent::Failed {
                        card: card.title.clone(),
                        error: msg,
                    });
                    return Ok(());
                }
            },
            // #186: every actual model call for these providers happens server-side, in the
            // code-brain-turn Edge Function, on the card owner's own BYOK key -- this node never
            // sees the key. Tool execution (read_file/write_file/list_dir/run_command) still
            // only ever happens here, same as "local" -- see `crate::coder::CloudBrain`'s doc.
            // No availability check needed here the way `local_brain` checks for a downcastable
            // backend: if the member has no key for this provider, `code_brain_turn`'s first call
            // fails and `run_code_session` reports it as a normal `code_session_error`/fail_card,
            // not a brain-selection error.
            provider @ ("anthropic" | "openai" | "nous") => Box::new(
                crate::coder::CloudBrain::new(self.hub, provider, spec.model_id.clone()),
            ),
            other => {
                let msg = format!(
                    "brain '{other}' is not implemented on this node yet (\"local\", \"anthropic\", \"openai\", or \"nous\" run today)"
                );
                self.hub.fail_card(card.id, &msg).await?;
                self.emit(WorkerEvent::Failed {
                    card: card.title.clone(),
                    error: msg,
                });
                return Ok(());
            }
        };

        let outcome = match crate::tools::run_code_session(
            self.hub,
            &self.data_dir,
            &card,
            brain.as_ref(),
            lease_expires_at,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                // Should not normally happen -- see `run_code_session`'s own doc for why this is
                // essentially dead code in practice (it only wraps genuine setup/session failures
                // as a normal `ok: false` outcome, not an `Err`).
                tracing::error!(card = %card.key, "code session tool wrapper returned a hard error: {e}");
                self.hub
                    .fail_card(card.id, &format!("coding session failed: {e}"))
                    .await?;
                self.emit(WorkerEvent::Failed {
                    card: card.title.clone(),
                    error: e.to_string(),
                });
                return Ok(());
            }
        };

        // ADR-032: `wait_for_child` already released this lease and set the card's status to
        // `waiting_on_child` inside `coder::run_session` itself -- there is nothing left here to
        // complete, release, or fail. Checked before `lease_expired` below since the two are
        // mutually exclusive in practice (a successful `wait_on_child` call returns from
        // `run_session` immediately, before any further lease-expiry check could run).
        let waiting_on_child = outcome
            .data
            .as_ref()
            .and_then(|d| d.get("waiting_on_child"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(child_id) = waiting_on_child {
            tracing::info!(card = %card.key, waiting_on = %child_id,
                "code session paused to wait on a spawned child");
            self.emit(WorkerEvent::Blocked {
                card: card.title.clone(),
                waiting_on: child_id,
            });
            return Ok(());
        }

        // Same defense-in-depth as `run_card`'s own mid-loop check: `run_code_session` (via
        // `coder::run_session`) already stopped itself rather than starting another turn past
        // this lease, so release the card instead of completing it here -- completing would race
        // a lease the hub may have already handed to someone else.
        let lease_expired = outcome
            .data
            .as_ref()
            .and_then(|d| d.get("lease_expired"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if lease_expired {
            tracing::warn!(card = %card.key,
                "code session's lease expired mid-session; releasing rather than completing");
            if let Err(e) = self
                .hub
                .release_card(card.id, "lease expired mid-session")
                .await
            {
                tracing::warn!(card = %card.key,
                    "release after lease expiry failed (housekeeping will reap it): {e}");
            }
            self.emit(WorkerEvent::Released {
                card: card.title.clone(),
            });
            return Ok(());
        }

        let done = self
            .hub
            .complete_card(
                card.id,
                &outcome.summary,
                spec.model_id.as_deref(),
                Usage::default(),
            )
            .await?;
        tracing::info!(card = %card.key, ok = outcome.ok, "code session complete -> review");
        self.emit(WorkerEvent::Completed {
            card: card.title.clone(),
            project: project.title.clone(),
            earned_honey: done.earned_honey,
            tokens_out: 0,
            wallet_balance: done.wallet_balance,
            fund_balance: done.fund_balance,
        });
        Ok(())
    }

    /// This node was built without the `sandbox` feature at all -- `crate::coder`/
    /// `crate::tools::run_code_session` don't exist in that build, so a `'code'` card fails
    /// cleanly here instead of `run_card`'s dispatch failing to compile.
    #[cfg(not(feature = "sandbox"))]
    async fn run_code_card(
        &self,
        card: ClaimedCard,
        _project: ClaimedProject,
        _lease_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let msg =
            "this node was built without sandbox support; cannot run a coding session".to_string();
        self.hub.fail_card(card.id, &msg).await?;
        self.emit(WorkerEvent::Failed {
            card: card.title.clone(),
            error: msg,
        });
        Ok(())
    }

    #[cfg(feature = "whisper")]
    async fn run_speech_card(
        &self,
        card: ClaimedCard,
        project: ClaimedProject,
        lease_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let remaining = (lease_expires_at - chrono::Utc::now())
            .to_std()
            .unwrap_or_default();
        let operation = async {
            let input: crate::speech::SpeechInput = serde_json::from_str(&card.inputs)?;
            input.validate()?;
            let hub = self
                .hub
                .community_client()
                .ok_or_else(|| anyhow::anyhow!("Community transcription requires a Hive job"))?;
            let cfg = crate::nodeconfig::load()?;
            let endpoint = cfg
                .whisper_url
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("No whisper.cpp server configured"))?;
            let model = cfg.whisper_model.unwrap_or_else(|| "whisper".into());
            if card
                .required_capabilities
                .get("model_id")
                .and_then(|v| v.as_str())
                .is_some_and(|m| m != model)
            {
                anyhow::bail!("Requested speech model is not configured");
            }
            let (audio, _) = hub
                .artifact_fetch_bounded(
                    &input.artifact_hash,
                    crate::speech::MAX_AUDIO_BYTES as usize,
                )
                .await?;
            let backend = crate::backend::whisper::WhisperCppBackend::new(endpoint, &model);
            let output = crate::speech::transcribe(
                &backend,
                &input,
                &audio,
                project.id,
                card.id,
                &std::env::temp_dir(),
                remaining.min(Duration::from_secs(600)),
                self.stop.clone(),
            )
            .await?;
            Ok::<_, anyhow::Error>((output, model))
        };
        let outcome = tokio::select! {
            biased;
            _ = stopped(self.stop.clone()) => None,
            _ = tokio::time::sleep(remaining.min(Duration::from_secs(600))) => None,
            result = operation => Some(result),
        };
        let Some(result) = outcome else {
            self.hub
                .release_card(
                    card.id,
                    "transcription interrupted or lease deadline reached",
                )
                .await?;
            self.emit(WorkerEvent::Released { card: card.title });
            return Ok(());
        };
        match result {
            Ok((output, model)) => {
                if chrono::Utc::now() >= lease_expires_at || *self.stop.borrow() {
                    self.hub
                        .release_card(card.id, "transcription lease expired or cancelled")
                        .await?;
                    self.emit(WorkerEvent::Released { card: card.title });
                    return Ok(());
                }
                // Existing completion records the authenticated node and usage on card_outputs.
                // No token fabrication or invented speech tariff: the hub owns all settlement.
                let done = self
                    .hub
                    .complete_card(card.id, &output.text, Some(&model), output.usage)
                    .await?;
                self.emit(WorkerEvent::Completed {
                    card: card.title,
                    project: project.title,
                    earned_honey: done.earned_honey,
                    tokens_out: 0,
                    wallet_balance: done.wallet_balance,
                    fund_balance: done.fund_balance,
                });
            }
            Err(error) => {
                tracing::warn!(%error, card = %card.id, "speech execution failed");
                let reason =
                    "Transcription failed; check the audio manifest and configured speech service";
                self.hub.fail_card(card.id, reason).await?;
                self.emit(WorkerEvent::Failed {
                    card: card.title,
                    error: reason.into(),
                });
            }
        }
        Ok(())
    }

    /// Run the agent loop for one leased card to completion (or failure).
    async fn run_card(
        &self,
        card: ClaimedCard,
        project: ClaimedProject,
        deps: serde_json::Map<String, serde_json::Value>,
        resume: Option<LoopState>,
        lease_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        // ADR-024 (#185): a 'code' card is one continuous local coding-agent session, not the
        // Draft/Critique/Revise state machine below -- a fundamentally different control flow,
        // not just a different prompt. Dispatched here, before any Draft-phase state is even
        // constructed, the same way `WaitingOnChild` is resolved to `Draft` before that loop
        // starts rather than threading a special case through every phase of it. No
        // checkpoint/resume support in this pass (ADR-024 decision 5: one continuous session
        // inside one lease, no new claim/lease machinery) -- `deps` and `resume` are simply
        // unused on this path; if this card is somehow re-claimed after a dead holder, its
        // session starts over from turn 1, same "fresh state every time" model `exec_wasm`
        // already uses within one call, extended here to the scope of a whole session.
        match card.modality.as_str() {
            "text" => {}
            "code" => return self.run_code_card(card, project, lease_expires_at).await,
            "speech" => {
                #[cfg(feature = "whisper")]
                return self.run_speech_card(card, project, lease_expires_at).await;
                #[cfg(not(feature = "whisper"))]
                {
                    let reason = "This worker was built without speech support";
                    self.hub.fail_card(card.id, reason).await?;
                    self.emit(WorkerEvent::Failed {
                        card: card.title,
                        error: reason.into(),
                    });
                    return Ok(());
                }
            }
            _ => {
                // Unsupported work must never fall through to prose or return to a hot retry loop.
                let reason = format!(
                    "unsupported_modality: this worker cannot execute {} jobs",
                    card.modality
                );
                self.hub.fail_card(card.id, &reason).await?;
                self.emit(WorkerEvent::Failed {
                    card: card.title,
                    error: reason,
                });
                return Ok(());
            }
        }
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
            // Defense-in-depth (ADR-029 review finding, 2026-09-14): the hub's own lease
            // housekeeping is the real enforcement -- it reaps a dead/expired lease and hands
            // the card to someone else regardless of what this node does. This is a *local*
            // check so this node stops volunteering more inference on a card it may no longer
            // hold, rather than finding out only when `hub.checkpoint`/`complete_card`
            // eventually fails against a lease the hub already reassigned. Checked once per
            // step, not continuously -- a step already in flight (one `self.infer` call) is
            // never interrupted mid-call.
            if chrono::Utc::now() >= lease_expires_at {
                tracing::warn!(card = %card.key, phase = ?st.phase,
                    "lease expired mid-card; releasing rather than continuing past it");
                if let Err(e) = self
                    .hub
                    .release_card(card.id, "lease expired mid-card")
                    .await
                {
                    tracing::warn!(card = %card.key,
                        "release after lease expiry failed (housekeeping will reap it): {e}");
                }
                self.emit(WorkerEvent::Released {
                    card: card.title.clone(),
                });
                return Ok(());
            }
            let phase = st.phase.clone();
            let max_tokens = max_tokens_for(&card, &phase);
            let crate::backend::Completion {
                text,
                usage,
                truncated,
            } = match self
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
            // A Draft or Revise cut off at the output cap is half an artifact. Shipping it is the
            // worst of the available outcomes: the card reports done, the member is billed, and
            // the truncation only surfaces when somebody reads the file. Fail the card instead,
            // naming both the cap and the knob that raises it. Critique is different — it is prose
            // *about* the draft, and a short review costs nothing but a warning.
            if truncated {
                if phase == Phase::Critique {
                    tracing::warn!(card = %card.key,
                        "critique hit the {max_tokens}-token cap and stops short; \
                         judging the draft on the part that arrived");
                } else {
                    let reason = format!(
                        "{phase:?}: output cut off at the {max_tokens}-token cap \
                         (finish_reason=length) — this card needs a bigger \
                         required_capabilities.max_tokens; refusing to report a partial artifact \
                         as finished"
                    );
                    tracing::error!(card = %card.key, phase = ?phase, "{reason}");
                    self.hub.fail_card(card.id, &reason).await?;
                    self.emit(WorkerEvent::Failed {
                        card: card.title.clone(),
                        error: reason,
                    });
                    return Ok(());
                }
            }
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
        // Hold the same OS-user slot as Bots from before claim through every terminal path.
        // Never claim an unrelated card merely to reserve capacity for a conversation.
        #[cfg(test)]
        let permit = crate::execution_capacity::try_acquire_at(&self.capacity_path)?;
        #[cfg(not(test))]
        let permit = crate::execution_capacity::try_acquire()?;
        let Some(_capacity) = permit else {
            return Ok(false);
        };
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
                // Parsed once here rather than inside `run_card` so a malformed string can't
                // panic deep in the step loop; a parse failure just skips the extra local safety
                // net for this one card (the hub's own housekeeping still enforces the real
                // deadline regardless) -- strictly no worse than before this check existed.
                let lease_deadline = chrono::DateTime::parse_from_rfc3339(&lease_expires_at)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|e| {
                        tracing::warn!(card = %card.key, lease_expires_at = %lease_expires_at,
                            "couldn't parse lease_expires_at ({e}); mid-card expiry check won't catch this lease");
                        chrono::Utc::now() + chrono::Duration::days(365)
                    });
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
                    r = self.run_card(card, project, dep_outputs, resume, lease_deadline) => { r?; Ok(true) }
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

    /// Sends one heartbeat, updating `last_rtt_ms` from the result for next time. Split out so
    /// `heartbeat_loop`'s ticker `select!` arm (see that fn) stays one line.
    async fn send_heartbeat(&self, last_rtt_ms: &mut Option<u64>) {
        match self.hub.heartbeat(*last_rtt_ms).await {
            Ok((_, rtt)) => *last_rtt_ms = Some(rtt),
            Err(e) => tracing::warn!("heartbeat failed: {e}"),
        }
    }

    /// The poll/claim/dispatch half of `run_forever`, split into its own fn so it can be polled
    /// concurrently with an independent heartbeat ticker (see `run_forever`) instead of
    /// sequentially. Previously, a single very-long-running card's own in-flight step delayed
    /// the heartbeat until that card's `tick()` call returned, however long that took (ADR-029
    /// review finding, 2026-09-14) -- the fix isn't inside this fn at all, it's that
    /// `run_forever` no longer sends a heartbeat from anywhere in this loop's own control flow,
    /// so nothing this loop does can block it. See `run_card`'s own lease-expiry check for the
    /// complementary per-card-authority half of that same finding.
    async fn dispatch_loop(&self, poll: Duration) -> Result<()> {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(poll) => {}
                _ = stopped(self.stop.clone()) => {
                    let p = self.hub.check_out().await?;
                    tracing::info!("checked out ({p})");
                    return Ok(());
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

    /// Self-contained heartbeat ticker -- runs on its own timer until `self.stop` flips. Split
    /// out of `run_forever` (rather than an inline `select!` arm that awaits `send_heartbeat`
    /// sequentially) so it can be combined with `dispatch_loop` via `tokio::join!` instead of
    /// `select!`: `join!` polls every not-yet-finished future on every wake for as long as any of
    /// them is pending, whereas the old `select!`-with-inline-`.await` pattern stopped polling
    /// `dispatch_loop` entirely for the duration of each heartbeat call -- a real gap even after
    /// heartbeat and dispatch were split into separate futures, because the code inside the
    /// resolved `select!` branch still ran sequentially before the loop went back to polling
    /// either future again (Sif's efficiency audit, finding 1, 2026-09-15; `Hub::heartbeat`'s own
    /// bounded timeout, finding 6, keeps a slow heartbeat from ever becoming an unbounded stall).
    async fn heartbeat_loop(&self, heartbeat_interval: Duration) {
        let mut last_rtt_ms: Option<u64> = None;
        let mut heartbeat_tick = tokio::time::interval(heartbeat_interval);
        heartbeat_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = heartbeat_tick.tick() => {
                    self.send_heartbeat(&mut last_rtt_ms).await;
                }
                _ = stopped(self.stop.clone()) => return,
            }
        }
    }

    /// Poll for cards until the stop flag flips. Checks out of the hub on the way out.
    ///
    /// Runs [`Self::dispatch_loop`] (claim/run/checkpoint cards) and [`Self::heartbeat_loop`]
    /// concurrently via `tokio::join!` for as long as this fn is alive -- see `heartbeat_loop`'s
    /// own doc for why `join!`, not `select!`, is what actually delivers "neither can delay the
    /// other": a card whose `tick()` takes minutes no longer holds up the heartbeat, and a slow
    /// heartbeat request no longer holds up dispatch either (ADR-029 review finding, 2026-09-14,
    /// tightened by Sif's efficiency audit finding 1, 2026-09-15). Both loops watch `self.stop`
    /// themselves and return once it flips, so this still completes promptly on shutdown.
    pub async fn run_forever(&self, poll: Duration, heartbeat_every: u32) -> Result<()> {
        // Same target cadence as before (every `heartbeat_every` polls, roughly).
        let heartbeat_interval = poll.saturating_mul(heartbeat_every.max(1));
        let (dispatch_result, ()) = tokio::join!(
            self.dispatch_loop(poll),
            self.heartbeat_loop(heartbeat_interval)
        );
        dispatch_result
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

#[cfg(test)]
#[path = "worker_modality_tests.rs"]
mod modality_tests;
