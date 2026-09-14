//! A real coding agent, scoped to a member's own Private Fleet (ADR-024, #185).
//!
//! Wires the `hive.modality = 'code'` value (present in the schema for a while, never used
//! before this) to an actual agentic loop: read/write files, run real shell commands, iterate
//! across many turns until the task is done — driven by either a local model already running on
//! the member's own hardware (this pass) or a cloud BYOK model (#186, not built here). The whole
//! session runs inside one claimed card's lease, the same way `exec_wasm` already does (ADR-024
//! decision 5) — `crate::worker`'s `run_code_card` calls [`run_session`] once and reports
//! whatever it returns via `node_complete_card`; there is no per-turn checkpointing and no
//! resume support in this pass (a re-claimed card starts its session over from turn 1).
//!
//! # `required_capabilities` contract (the shape #186's web UI must produce)
//!
//! A `code`-modality card opts in with a `required_capabilities` object shaped like
//! [`CodeSessionSpec`]:
//!
//! ```json
//! {
//!   "tools_level": "sandboxed_tools",
//!   "task": "human-readable description of what to do",
//!   "workspace_path": "/absolute/path/already/on/this/node",
//!   "repo_url": "https://github.com/... (mutually exclusive with workspace_path)",
//!   "repo_ref": "optional branch/tag/sha, only meaningful with repo_url",
//!   "brain": "local",
//!   "model_id": "optional -- which local model to use if brain=local",
//!   "max_turns": 40
//! }
//! ```
//!
//! `tools_level` is read by `hive.node_claim_card` and `crate::worker`, not by this module.
//! Exactly one of `workspace_path`/`repo_url` must be set ([`CodeSessionSpec::from_required_capabilities`]
//! rejects both-absent; if both are present, `workspace_path` wins and `repo_url`/`repo_ref` are
//! ignored — see that function's doc). `brain` defaults to `"local"`; `max_turns` defaults to 40.
//! Only `"brain": "local"` runs end to end in this pass — [`LocalBrain`] is the only
//! [`CodeBrain`] implementation that exists yet; any other value fails the card cleanly (see
//! `crate::worker::Worker::run_code_card`) rather than silently falling back.
//!
//! # The `CodeBrain` seam (the contract #186's cloud brain implements)
//!
//! [`CodeBrain::next_turn`] is the *only* place "what should I do next" gets decided, and it
//! knows nothing about how that decision was produced — a local HTTP call to Ollama
//! ([`LocalBrain`]) today, a future Edge Function round-trip to Anthropic/OpenAI/Nous for #186.
//! Implement that trait against [`BrainMessage`]/[`ToolSpec`]/[`BrainTurn`] and nothing else in
//! this module needs to change to support a new brain — see [`crate::worker::Worker`]'s
//! `run_code_card` for the one place a brain gets picked, which is the only code that would grow
//! a new match arm.
//!
//! # The four tools
//!
//! `read_file`, `write_file`, `list_dir`, `run_command` — real, unsandboxed access to one
//! workspace directory (an existing `workspace_path`, or a fresh `git clone` of `repo_url` into
//! this node's scratch space). **This module does not sandbox anything** (same framing as
//! [`crate::mcp`]'s module doc for the same reason): `run_command` spawns a real child process
//! as this OS user, with whatever access that user has. Every path a tool call names is resolved
//! against the workspace root and rejected if it would escape that one directory (via `..` or an
//! absolute path elsewhere) — **this check exists only to keep the model from wandering outside
//! the one directory it was told to work in by mistake (a hallucinated `../../etc/passwd`, a
//! typo'd absolute path); it is not a security boundary against the member's own machine.** A
//! member who runs `run_command` gets exactly what ADR-024 decision 2 describes: their own
//! subprocess, on their own hardware, as their own OS user, with no sandboxing at all — a
//! trusted command that does `rm -rf .` still empties the actual workspace it's pointed at, and
//! nothing here stops it. See [`resolve_in_workspace`].
//!
//! `run_command` never goes through a shell (`Command::new(command).args(args)`, exactly
//! `crate::mcp::McpSession::start`'s style) — nothing in the model's own `command`/`args` output
//! can be reinterpreted as shell syntax. Same for the `git clone`/`git checkout` calls this
//! module makes itself during workspace prep.
//!
//! # Timeouts
//!
//! - [`RUN_COMMAND_TIMEOUT`] (10 minutes): a build or test run can legitimately take minutes;
//!   this is deliberately generous and, like `crate::mcp`'s timeouts, a fixed constant rather
//!   than a per-card tuning knob in v1.
//! - [`GIT_TIMEOUT`] (5 minutes): cloning a large repo over a slow connection can be slow, but a
//!   git operation that hasn't finished in 5 minutes on workspace prep is more likely hung or
//!   pointed at a bad URL than genuinely still working.
//! - [`READ_FILE_MAX_BYTES`] (200 KB): mirrors `crate::tools::truncate_for_summary`'s reasoning
//!   for keeping one tool result from blowing up the running conversation's context.
//!
//! # What isn't tested yet
//!
//! This has not run against a real Ollama/Hermes instance as of this writing — see
//! `crate::backend::llama_cpp`'s "Tool-calling completions" section doc for the specific
//! response-shape risk areas that matter most for the first real run (missing `tool_calls[].id`,
//! a model narrating text alongside a tool call). Everything else here (workspace prep, the four
//! tools, the loop's turn-taking and progress-posting) only depends on `tokio::process`/
//! `std::fs`/`crate::hub`, all already exercised elsewhere in this codebase.

use crate::hub::Hub;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

/// A build/test/lint run can legitimately take minutes; not configurable per-card in v1 (matches
/// `crate::mcp`'s "documented, generous, non-configurable" constant style).
pub const RUN_COMMAND_TIMEOUT: Duration = Duration::from_secs(600);

/// `git clone`/`git checkout` during workspace prep. Shorter than `RUN_COMMAND_TIMEOUT` on the
/// theory that a git operation is either fast or stuck/misconfigured (bad URL, auth prompt with
/// nothing to answer it — ADR-024 decision 4 explicitly has no credential handling in this pass,
/// so an auth-requiring private repo will hang here until this timeout fires it closed).
pub const GIT_TIMEOUT: Duration = Duration::from_secs(300);

/// Same cap and reasoning as `crate::tools::truncate_for_summary`: keep one tool result from
/// blowing up the running conversation's context. Applies to `read_file` and to each of
/// `run_command`'s stdout/stderr streams independently.
pub const READ_FILE_MAX_BYTES: usize = 200 * 1024;

fn default_brain() -> String {
    "local".to_string()
}
fn default_max_turns() -> u32 {
    40
}

/// A `code`-modality card's `required_capabilities`, deserialized. See this module's doc for the
/// full field-by-field contract; every field here is host-trusted card data set when the card
/// was created, same as every other tool in `crate::tools` — nothing a running brain says can
/// change a session's workspace or task mid-run.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CodeSessionSpec {
    pub task: String,
    #[serde(default)]
    pub workspace_path: Option<String>,
    #[serde(default)]
    pub repo_url: Option<String>,
    #[serde(default)]
    pub repo_ref: Option<String>,
    #[serde(default = "default_brain")]
    pub brain: String,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
}

impl CodeSessionSpec {
    /// Parse and validate. `workspace_path` wins if both it and `repo_url` are set (see this
    /// module's doc) — that's a caller mistake, not an error worth failing the whole card over,
    /// since "an existing checkout" and "clone this" aren't in tension when both are named; the
    /// existing path is simply the more specific instruction.
    pub fn from_required_capabilities(v: &serde_json::Value) -> Result<Self, CoderError> {
        let spec: CodeSessionSpec = serde_json::from_value(v.clone())
            .map_err(|e| CoderError::InvalidSpec(e.to_string()))?;
        if spec.task.trim().is_empty() {
            return Err(CoderError::InvalidSpec("task must not be empty".into()));
        }
        if spec.workspace_path.is_none() && spec.repo_url.is_none() {
            return Err(CoderError::InvalidSpec(
                "must set workspace_path or repo_url".into(),
            ));
        }
        if spec.max_turns == 0 {
            return Err(CoderError::InvalidSpec(
                "max_turns must be at least 1".into(),
            ));
        }
        Ok(spec)
    }
}

#[derive(Error, Debug)]
pub enum CoderError {
    #[error("invalid code-session required_capabilities: {0}")]
    InvalidSpec(String),
    /// First field is already a display-formatted path (`PathBuf` has no `Display` impl, only
    /// `Debug` — stringifying at the construction site keeps every format string below plain
    /// `{0}`/`{1}` references instead of mixing in `.display()` calls).
    #[error("workspace_path '{0}' does not exist or isn't readable: {1}")]
    WorkspacePath(String, #[source] std::io::Error),
    #[error("workspace_path '{0}' exists but is not a directory")]
    WorkspaceNotADirectory(String),
    #[error("local filesystem error at {0}: {1}")]
    Io(String, #[source] std::io::Error),
    #[error("path {0:?} can't be represented as UTF-8 for a subprocess argument")]
    NonUtf8Path(PathBuf),
    #[error("git {0:?} timed out after {timeout}s", timeout = GIT_TIMEOUT.as_secs())]
    GitTimeout(Vec<String>),
    #[error("failed to spawn git: {0}")]
    GitSpawn(#[source] std::io::Error),
    #[error("git {args:?} failed (exit {code:?}): {stderr}")]
    GitFailed {
        args: Vec<String>,
        code: Option<i32>,
        stderr: String,
    },
    #[error(transparent)]
    Brain(#[from] CodeBrainError),
}

// ── The CodeBrain seam ──────────────────────────────────────────────────────────────────────

/// One role in a coding-agent conversation — the same four roles every tool-calling chat API
/// (OpenAI, Anthropic, Ollama) uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainRole {
    System,
    User,
    Assistant,
    Tool,
}

/// One tool call a brain asked for. `arguments` is a parsed JSON object (not the wire-format
/// JSON-encoded string some APIs use — each [`CodeBrain`] implementation is responsible for its
/// own wire format's encoding/decoding; this is the one shape every implementation converts
/// to/from).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainToolCall {
    /// Stable id correlating this call to the [`BrainMessage::tool_result`] that answers it.
    /// Synthesized by the brain implementation if its own wire format doesn't provide one.
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// One message in the running coding-agent conversation [`CodeBrain::next_turn`] is asked to
/// continue. This is the *only* vocabulary a second `CodeBrain` implementation (#186's cloud
/// brain) needs to match — nothing about this shape assumes anything about how a turn is
/// produced (a local HTTP call to Ollama, vs. a future Edge Function round-trip that serializes
/// this same conversation as JSON and sends it to Anthropic/OpenAI/Nous).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainMessage {
    pub role: BrainRole,
    /// Absent on a pure tool-call assistant turn (mirrors the wire shape most tool-trained
    /// models use for a tool-calls-only reply: no prose, just calls).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Set only on an `Assistant` message that made tool calls; echoed back on every later turn
    /// (both cloud and local tool-calling protocols require the exact prior assistant
    /// `tool_calls` to still be present in the next request).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<BrainToolCall>,
    /// Set only on a `Tool` message: which call (by [`BrainToolCall::id`]) this is the result of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl BrainMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: BrainRole::System,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: BrainRole::User,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn assistant_tool_calls(tool_calls: Vec<BrainToolCall>) -> Self {
        Self {
            role: BrainRole::Assistant,
            content: None,
            tool_calls,
            tool_call_id: None,
        }
    }
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: BrainRole::Tool,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// One tool's JSON-schema description, in the (now near-universal) OpenAI function-calling
/// shape. Every [`CodeBrain`] implementation is expected to translate this into its own wire
/// format (verbatim for `LocalBrain`/Ollama; #186's cloud brain will do the same for whichever
/// provider it targets, since Anthropic/OpenAI both accept this shape with only minor
/// reshaping).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// What a brain decided to do with the conversation it was handed.
#[derive(Debug, Clone)]
pub enum BrainTurn {
    /// The brain is done calling tools and this is its final (or intermediate-but-textual)
    /// reply. [`run_session`] treats any `Text` turn as the session's answer and stops looping —
    /// a brain that wants to keep working must call a tool, not narrate in prose.
    Text(String),
    /// The brain wants one or more tools run before it says anything else. Never empty — an
    /// implementation that gets an empty `tool_calls` array from its own API should treat that
    /// as [`BrainTurn::Text`] with whatever text (possibly empty) came with it instead.
    ToolCalls(Vec<BrainToolCall>),
}

#[derive(Error, Debug)]
pub enum CodeBrainError {
    #[error("brain backend error: {0}")]
    Backend(String),
}

/// The one seam between the agentic loop ([`run_session`]) and however a "what should I do
/// next" turn actually gets answered (ADR-024 decision 3). Implement this and nothing else in
/// this module needs to change: [`run_session`] only ever calls [`CodeBrain::next_turn`] with
/// the conversation so far and the fixed tool schema, and only ever inspects the
/// [`BrainTurn`] it gets back — it has no idea whether that turn came from a local HTTP call to
/// Ollama or a round-trip to a cloud provider, and it must never be given a reason to care.
///
/// A second implementation (#186, a cloud/BYOK brain) plugs in here with no other context from
/// this module needed beyond this trait, [`BrainMessage`], [`BrainToolCall`], [`ToolSpec`], and
/// [`BrainTurn`] — see this module's doc for the full picture of what a `code` card's
/// `required_capabilities` looks like and where a brain gets selected
/// (`crate::worker::Worker::run_code_card`).
#[async_trait::async_trait]
pub trait CodeBrain: Send + Sync {
    /// Given the conversation so far (system prompt, the task, every prior assistant/tool
    /// turn) and the tool schema available this session, decide what happens next. Called once
    /// per loop turn; implementations should treat each call as stateless (all state the brain
    /// needs is in `messages`) since [`run_session`] doesn't guarantee the same `CodeBrain`
    /// instance is reused across turns any more than it's required to be.
    async fn next_turn(
        &self,
        messages: &[BrainMessage],
        tools: &[ToolSpec],
    ) -> Result<BrainTurn, CodeBrainError>;
}

/// The v1 (and, in this pass, only) [`CodeBrain`]: the node's own `LlamaCppBackend` (Ollama),
/// via the new `chat_with_tools` surface (`crate::backend::llama_cpp`'s "Tool-calling
/// completions" section). No network call leaves the node; no BYOK key involved.
///
/// Gated on the `llama-cpp` feature (independent of the `sandbox`+`hub` gate this whole module
/// lives under, see the module doc / `lib.rs`): a `code` card naming `"brain": "local"` on a
/// build without this feature fails cleanly at brain-selection time
/// (`crate::worker::Worker::run_code_card`'s `local_brain` fallback) rather than this module
/// failing to compile.
#[cfg(feature = "llama-cpp")]
pub struct LocalBrain<'a> {
    backend: &'a crate::backend::llama_cpp::LlamaCppBackend,
    model: String,
    max_tokens: u64,
}

#[cfg(feature = "llama-cpp")]
impl<'a> LocalBrain<'a> {
    pub fn new(
        backend: &'a crate::backend::llama_cpp::LlamaCppBackend,
        model: impl Into<String>,
        max_tokens: u64,
    ) -> Self {
        Self {
            backend,
            model: model.into(),
            max_tokens,
        }
    }
}

#[cfg(feature = "llama-cpp")]
fn to_wire_message(m: &BrainMessage) -> crate::backend::llama_cpp::ToolChatMessage {
    use crate::backend::llama_cpp::{ToolCallFunction, ToolCallOut, ToolChatMessage};
    let role = match m.role {
        BrainRole::System => "system",
        BrainRole::User => "user",
        BrainRole::Assistant => "assistant",
        BrainRole::Tool => "tool",
    };
    ToolChatMessage {
        role: role.to_string(),
        content: m.content.clone(),
        tool_calls: (!m.tool_calls.is_empty()).then(|| {
            m.tool_calls
                .iter()
                .map(|c| ToolCallOut {
                    id: c.id.clone(),
                    kind: "function".to_string(),
                    function: ToolCallFunction {
                        name: c.name.clone(),
                        arguments: c.arguments.to_string(),
                    },
                })
                .collect()
        }),
        tool_call_id: m.tool_call_id.clone(),
    }
}

#[cfg(feature = "llama-cpp")]
fn to_wire_tool(t: &ToolSpec) -> crate::backend::llama_cpp::ToolSchema {
    crate::backend::llama_cpp::ToolSchema {
        kind: "function".to_string(),
        function: crate::backend::llama_cpp::ToolFunctionSchema {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
        },
    }
}

#[cfg(feature = "llama-cpp")]
#[async_trait::async_trait]
impl<'a> CodeBrain for LocalBrain<'a> {
    async fn next_turn(
        &self,
        messages: &[BrainMessage],
        tools: &[ToolSpec],
    ) -> Result<BrainTurn, CodeBrainError> {
        use crate::backend::llama_cpp::ToolChatResult;
        let wire_messages: Vec<_> = messages.iter().map(to_wire_message).collect();
        let wire_tools: Vec<_> = tools.iter().map(to_wire_tool).collect();
        let (result, _usage) = self
            .backend
            .chat_with_tools(&self.model, &wire_messages, &wire_tools, self.max_tokens)
            .await
            .map_err(|e| CodeBrainError::Backend(e.to_string()))?;
        match result {
            ToolChatResult::Text(text) => Ok(BrainTurn::Text(text)),
            ToolChatResult::ToolCalls(calls) => {
                let calls = calls
                    .into_iter()
                    .enumerate()
                    .map(|(i, c)| {
                        // Some Ollama/llama.cpp versions omit `id` on a tool_calls entry (see
                        // `chat_with_tools`'s doc, risk 1) -- synthesize a stable, turn-unique one
                        // so the tool-result message that follows always has something to
                        // correlate against.
                        let id = if c.id.trim().is_empty() {
                            format!("call_{i}")
                        } else {
                            c.id
                        };
                        let arguments = serde_json::from_str(&c.function.arguments)
                            .unwrap_or(serde_json::Value::Null);
                        BrainToolCall {
                            id,
                            name: c.function.name,
                            arguments,
                        }
                    })
                    .collect();
                Ok(BrainTurn::ToolCalls(calls))
            }
        }
    }
}

/// #186's cloud/BYOK brain (ADR-024 decision 3): every turn round-trips through the
/// `code-brain-turn` Edge Function (`crate::hub::HubClient::code_brain_turn`), which calls
/// `provider`/`model` on the member's own key and reports back what to do next. The member's raw
/// key never reaches this node -- it stays in Supabase Vault, same as every other BYOK path
/// (chat, interview). Unlike `LocalBrain`, this needs no feature gate: it only depends on
/// `crate::hub`, which this whole module already requires.
///
/// Tool execution never happens in the Edge Function or here -- only in `run_session`'s loop,
/// exactly like `LocalBrain`. That's the entire point of ADR-024: the coding agent's filesystem
/// and process access stays on the member's own machine regardless of which brain is choosing
/// what to do.
pub struct CloudBrain<'a> {
    hub: &'a dyn Hub,
    provider: String,
    model: Option<String>,
}

impl<'a> CloudBrain<'a> {
    pub fn new(hub: &'a dyn Hub, provider: impl Into<String>, model: Option<String>) -> Self {
        Self {
            hub,
            provider: provider.into(),
            model,
        }
    }
}

fn to_hub_message(m: &BrainMessage) -> crate::hub::CodeBrainMessage {
    let role = match m.role {
        BrainRole::System => "system",
        BrainRole::User => "user",
        BrainRole::Assistant => "assistant",
        BrainRole::Tool => "tool",
    };
    crate::hub::CodeBrainMessage {
        role: role.to_string(),
        content: m.content.clone(),
        tool_calls: m
            .tool_calls
            .iter()
            .map(|c| crate::hub::CodeBrainToolCall {
                id: c.id.clone(),
                name: c.name.clone(),
                arguments: c.arguments.clone(),
            })
            .collect(),
        tool_call_id: m.tool_call_id.clone(),
    }
}

fn to_hub_tool(t: &ToolSpec) -> crate::hub::CodeBrainTool {
    crate::hub::CodeBrainTool {
        name: t.name.clone(),
        description: t.description.clone(),
        parameters: t.parameters.clone(),
    }
}

#[async_trait::async_trait]
impl<'a> CodeBrain for CloudBrain<'a> {
    async fn next_turn(
        &self,
        messages: &[BrainMessage],
        tools: &[ToolSpec],
    ) -> Result<BrainTurn, CodeBrainError> {
        let wire_messages: Vec<_> = messages.iter().map(to_hub_message).collect();
        let wire_tools: Vec<_> = tools.iter().map(to_hub_tool).collect();
        let community = self.hub.community_client().ok_or_else(|| CodeBrainError::Backend(
            "fully local hub requires a local model; direct-to-provider cloud coding is not implemented".into()
        ))?;
        let result = community
            .code_brain_turn(
                &self.provider,
                self.model.as_deref(),
                &wire_messages,
                &wire_tools,
            )
            .await
            .map_err(|e| CodeBrainError::Backend(e.to_string()))?;
        Ok(match result {
            crate::hub::CodeBrainTurnResult::Text { text, .. } => BrainTurn::Text(text),
            crate::hub::CodeBrainTurnResult::ToolCalls { calls, .. } => {
                if calls.is_empty() {
                    // Same defensive fallback LocalBrain's doc on BrainTurn::ToolCalls calls
                    // for: an empty tool-calls array from the provider is treated as a (possibly
                    // empty) text turn rather than a call `run_session` would loop forever on.
                    BrainTurn::Text(String::new())
                } else {
                    BrainTurn::ToolCalls(
                        calls
                            .into_iter()
                            .map(|c| BrainToolCall {
                                id: c.id,
                                name: c.name,
                                arguments: c.arguments,
                            })
                            .collect(),
                    )
                }
            }
        })
    }
}

// ── Workspace prep ──────────────────────────────────────────────────────────────────────────

/// Where a `repo_url` session clones into, under this node's data directory — sibling to
/// `crate::tools::component_path_for`'s and `crate::sandbox::scratch_dir_for`'s naming scheme,
/// card-scoped so a hypothetical future resume/retry lands in the same place rather than
/// accumulating scratch clones per attempt (no resume support exists yet — see this module's
/// doc — but nothing about this path depends on that changing).
fn clone_dir_for(data_dir: &Path, card_id: &Uuid) -> PathBuf {
    data_dir.join("code-workspaces").join(card_id.to_string())
}

/// Resolve a session's workspace root: an existing `workspace_path` (verified to exist and be a
/// directory), or a fresh `git clone` of `repo_url` (+ `git checkout repo_ref` if given) into
/// this node's scratch space. The returned path is canonicalized once, up front — every tool
/// call's containment check ([`resolve_in_workspace`]) is relative to this fixed, symlink-resolved
/// root for the rest of the session, rather than re-resolving (and potentially re-following a
/// symlink that changed) on every call.
async fn prepare_workspace(
    data_dir: &Path,
    card_id: Uuid,
    spec: &CodeSessionSpec,
) -> Result<PathBuf, CoderError> {
    if let Some(path) = &spec.workspace_path {
        let root = PathBuf::from(path);
        let meta = tokio::fs::metadata(&root)
            .await
            .map_err(|e| CoderError::WorkspacePath(root.display().to_string(), e))?;
        if !meta.is_dir() {
            return Err(CoderError::WorkspaceNotADirectory(
                root.display().to_string(),
            ));
        }
        return tokio::fs::canonicalize(&root)
            .await
            .map_err(|e| CoderError::WorkspacePath(root.display().to_string(), e));
    }

    let url = spec.repo_url.as_ref().expect(
        "CodeSessionSpec::from_required_capabilities guarantees workspace_path or repo_url",
    );
    let dest = clone_dir_for(data_dir, &card_id);
    if dest.exists() {
        // No resume support (this module's doc): a re-claimed session's leftover clone from a
        // previous attempt is stale and could be a half-finished clone from a killed node -- wipe
        // it rather than risk `git clone` refusing to clone into a non-empty directory.
        tokio::fs::remove_dir_all(&dest)
            .await
            .map_err(|e| CoderError::Io(dest.display().to_string(), e))?;
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| CoderError::Io(parent.display().to_string(), e))?;
    }
    let dest_str = dest
        .to_str()
        .ok_or_else(|| CoderError::NonUtf8Path(dest.clone()))?;
    run_git(&["clone", url, dest_str], None).await?;
    if let Some(reference) = &spec.repo_ref {
        run_git(&["checkout", reference], Some(&dest)).await?;
    }
    tokio::fs::canonicalize(&dest)
        .await
        .map_err(|e| CoderError::WorkspacePath(dest.display().to_string(), e))
}

/// Spawn `git` directly (never through a shell — see this module's doc) with a hard timeout.
/// No credential handling (ADR-024 decision 4): a private repo needing auth will simply hang
/// until `GIT_TIMEOUT` fires this closed, surfaced as a normal [`CoderError::GitTimeout`].
async fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<(), CoderError> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let owned_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let output = match tokio::time::timeout(GIT_TIMEOUT, cmd.output()).await {
        Ok(res) => res.map_err(CoderError::GitSpawn)?,
        Err(_) => return Err(CoderError::GitTimeout(owned_args)),
    };
    if !output.status.success() {
        return Err(CoderError::GitFailed {
            args: owned_args,
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    Ok(())
}

// ── Path containment (correctness guard, not a security sandbox — see module doc) ──────────

#[derive(Error, Debug)]
pub enum ToolExecError {
    #[error("path '{0}' would escape the workspace root")]
    PathEscapesWorkspace(String),
    /// First field is already a display-formatted path — see [`CoderError::Io`]'s doc for why.
    #[error("i/o error at {0}: {1}")]
    Io(String, #[source] std::io::Error),
    #[error("failed to spawn '{0}': {1}")]
    Spawn(String, #[source] std::io::Error),
}

/// Resolve a path the model gave us against `workspace_root`, rejecting anything that would
/// escape it (an absolute path, or enough `..` segments to walk past the root). Lexical only —
/// it does not require the path to exist (needed for `write_file`, which creates new paths) and
/// does not follow symlinks *inside* the workspace, only the one canonicalization
/// [`prepare_workspace`] already did on the root itself.
///
/// **This is not a security sandbox.** It exists purely so a wandering or hallucinating model
/// can't read or clobber a file outside the one directory it was told to work in — see this
/// module's doc for the full framing. It does nothing to stop a command the model runs *inside*
/// the workspace from doing whatever it wants to that workspace (or, via `run_command`, to
/// anything else this OS user can reach) — that is the accepted, explicit trust boundary ADR-024
/// decision 2 draws, not a gap in this check.
fn resolve_in_workspace(workspace_root: &Path, candidate: &str) -> Result<PathBuf, ToolExecError> {
    let candidate_path = Path::new(candidate);
    if candidate_path.is_absolute() {
        return Err(ToolExecError::PathEscapesWorkspace(candidate.to_string()));
    }
    let joined = workspace_root.join(candidate_path);
    let normalized = normalize_lexically(&joined);
    if !normalized.starts_with(workspace_root) {
        return Err(ToolExecError::PathEscapesWorkspace(candidate.to_string()));
    }
    Ok(normalized)
}

/// Collapse `.`/`..` components without touching the filesystem (so this works for a path that
/// doesn't exist yet). A `..` that would pop past an empty prefix is simply dropped rather than
/// erroring here — [`resolve_in_workspace`]'s `starts_with` check afterward is what actually
/// rejects an escape; this function's only job is lexical normalization.
fn normalize_lexically(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ── The four tools ──────────────────────────────────────────────────────────────────────────

async fn read_file_tool(
    workspace_root: &Path,
    path: &str,
) -> Result<serde_json::Value, ToolExecError> {
    let resolved = resolve_in_workspace(workspace_root, path)?;
    let bytes = tokio::fs::read(&resolved)
        .await
        .map_err(|e| ToolExecError::Io(resolved.display().to_string(), e))?;
    let truncated = bytes.len() > READ_FILE_MAX_BYTES;
    let cutoff = bytes.len().min(READ_FILE_MAX_BYTES);
    // Lossy decode, not a char-boundary walk-back: this also has to handle a genuinely binary
    // file gracefully (never panics either way), unlike `crate::tools::truncate_for_summary`
    // which only ever truncates a value that started life as valid UTF-8.
    let mut content = String::from_utf8_lossy(&bytes[..cutoff]).to_string();
    if truncated {
        content.push_str(&format!(
            "\n... [truncated, {} of {} bytes shown]",
            cutoff,
            bytes.len()
        ));
    }
    Ok(serde_json::json!({
        "path": path,
        "content": content,
        "truncated": truncated,
        "total_bytes": bytes.len(),
    }))
}

async fn write_file_tool(
    workspace_root: &Path,
    path: &str,
    content: &str,
) -> Result<serde_json::Value, ToolExecError> {
    let resolved = resolve_in_workspace(workspace_root, path)?;
    if let Some(parent) = resolved.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ToolExecError::Io(parent.display().to_string(), e))?;
    }
    tokio::fs::write(&resolved, content)
        .await
        .map_err(|e| ToolExecError::Io(resolved.display().to_string(), e))?;
    Ok(serde_json::json!({
        "path": path,
        "bytes_written": content.len(),
    }))
}

async fn list_dir_tool(
    workspace_root: &Path,
    path: &str,
) -> Result<serde_json::Value, ToolExecError> {
    let target = if path.trim().is_empty() {
        workspace_root.to_path_buf()
    } else {
        resolve_in_workspace(workspace_root, path)?
    };
    let mut rd = tokio::fs::read_dir(&target)
        .await
        .map_err(|e| ToolExecError::Io(target.display().to_string(), e))?;
    let mut entries = Vec::new();
    while let Some(entry) = rd
        .next_entry()
        .await
        .map_err(|e| ToolExecError::Io(target.display().to_string(), e))?
    {
        let file_type = entry
            .file_type()
            .await
            .map_err(|e| ToolExecError::Io(entry.path().display().to_string(), e))?;
        entries.push((
            entry.file_name().to_string_lossy().to_string(),
            file_type.is_dir(),
        ));
    }
    entries.sort();
    let entries: Vec<_> = entries
        .into_iter()
        .map(|(name, is_dir)| serde_json::json!({ "name": name, "is_dir": is_dir }))
        .collect();
    Ok(serde_json::json!({ "path": path, "entries": entries }))
}

/// Drain a child pipe up to `cap` bytes, discarding (but still reading, to avoid the child
/// deadlocking on a full OS pipe buffer once its output exceeds the cap) whatever comes after.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    cap: usize,
) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < cap {
                    let take = (cap - buf.len()).min(n);
                    buf.extend_from_slice(&chunk[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    (buf, truncated)
}

/// `command`/`args` spawned directly, never through a shell (see this module's doc). stdout and
/// stderr are captured **separately, not interleaved** — simpler and race-free to implement than
/// merging two pipes in real time, at the cost of losing the original interleaving order; a
/// model reading both back can still see everything either stream produced.
async fn run_command_tool(
    workspace_root: &Path,
    command: &str,
    args: &[String],
    cwd: Option<&str>,
) -> Result<serde_json::Value, ToolExecError> {
    let resolved_cwd = match cwd {
        Some(c) if !c.trim().is_empty() => resolve_in_workspace(workspace_root, c)?,
        _ => workspace_root.to_path_buf(),
    };
    let mut cmd = tokio::process::Command::new(command);
    cmd.args(args)
        .current_dir(&resolved_cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|e| ToolExecError::Spawn(command.to_string(), e))?;
    let stdout = child.stdout.take().expect("stdout requested at spawn");
    let stderr = child.stderr.take().expect("stderr requested at spawn");

    match tokio::time::timeout(RUN_COMMAND_TIMEOUT, async {
        tokio::join!(
            read_capped(stdout, READ_FILE_MAX_BYTES),
            read_capped(stderr, READ_FILE_MAX_BYTES),
            child.wait()
        )
    })
    .await
    {
        Ok(((out, out_trunc), (err, err_trunc), status)) => {
            let status =
                status.map_err(|e| ToolExecError::Io(resolved_cwd.display().to_string(), e))?;
            Ok(serde_json::json!({
                "command": command,
                "args": args,
                "exit_code": status.code(),
                "stdout": String::from_utf8_lossy(&out).to_string(),
                "stdout_truncated": out_trunc,
                "stderr": String::from_utf8_lossy(&err).to_string(),
                "stderr_truncated": err_trunc,
                "timed_out": false,
            }))
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            Ok(serde_json::json!({
                "command": command,
                "args": args,
                "exit_code": serde_json::Value::Null,
                "stdout": serde_json::Value::Null,
                "stderr": serde_json::Value::Null,
                "timed_out": true,
                "timeout_seconds": RUN_COMMAND_TIMEOUT.as_secs(),
            }))
        }
    }
}

/// The fixed tool schema every session advertises, in OpenAI function-calling shape.
pub fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "read_file".into(),
            description:
                "Read a file's contents (best-effort UTF-8 decoded), relative to the workspace root."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relative to the workspace root." }
                },
                "required": ["path"]
            }),
        },
        ToolSpec {
            name: "write_file".into(),
            description:
                "Create or overwrite a file (creating parent directories as needed), relative to the workspace root."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relative to the workspace root." },
                    "content": { "type": "string", "description": "Full file contents to write." }
                },
                "required": ["path", "content"]
            }),
        },
        ToolSpec {
            name: "list_dir".into(),
            description:
                "List the immediate contents (name + whether it's a directory) of a directory, relative to the workspace root. Omit path (or pass an empty string) for the workspace root itself."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relative to the workspace root; omit or empty for the workspace root." }
                }
            }),
        },
        ToolSpec {
            name: "run_command".into(),
            description: format!(
                "Run a real command (spawned directly, never through a shell) and capture stdout/stderr/exit code. Times out after {} seconds.",
                RUN_COMMAND_TIMEOUT.as_secs()
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The program to run, e.g. \"cargo\" or \"npm\" -- not a shell command line." },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "Arguments, passed directly to the program -- never interpreted as shell syntax." },
                    "cwd": { "type": "string", "description": "Working directory relative to the workspace root; defaults to the workspace root." }
                },
                "required": ["command"]
            }),
        },
    ]
}

fn json_error(e: &ToolExecError) -> serde_json::Value {
    serde_json::json!({ "error": e.to_string() })
}

/// Run one tool call and return `(result_for_the_brain, human_summary_for_the_channel)`.
async fn execute_tool(workspace_root: &Path, call: &BrainToolCall) -> (serde_json::Value, String) {
    match call.name.as_str() {
        "read_file" => {
            let path = call
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match read_file_tool(workspace_root, path).await {
                Ok(v) => {
                    let truncated = v
                        .get("truncated")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    let summary = format!(
                        "read `{path}`{}",
                        if truncated { " (truncated)" } else { "" }
                    );
                    (v, summary)
                }
                Err(e) => (json_error(&e), format!("read_file `{path}` failed: {e}")),
            }
        }
        "write_file" => {
            let path = call
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = call
                .arguments
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match write_file_tool(workspace_root, path, content).await {
                Ok(v) => (v, format!("wrote `{path}` ({} bytes)", content.len())),
                Err(e) => (json_error(&e), format!("write_file `{path}` failed: {e}")),
            }
        }
        "list_dir" => {
            let path = call
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match list_dir_tool(workspace_root, path).await {
                Ok(v) => {
                    let n = v
                        .get("entries")
                        .and_then(|e| e.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let shown = if path.trim().is_empty() { "." } else { path };
                    (v.clone(), format!("listed `{shown}` ({n} entries)"))
                }
                Err(e) => (json_error(&e), format!("list_dir `{path}` failed: {e}")),
            }
        }
        "run_command" => {
            let command = call
                .arguments
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args: Vec<String> = call
                .arguments
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let cwd = call.arguments.get("cwd").and_then(|v| v.as_str());
            match run_command_tool(workspace_root, command, &args, cwd).await {
                Ok(v) => {
                    let timed_out = v
                        .get("timed_out")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    let joined_args = args.join(" ");
                    let summary = if timed_out {
                        format!(
                            "ran `{command} {joined_args}` -- timed out after {}s",
                            RUN_COMMAND_TIMEOUT.as_secs()
                        )
                    } else {
                        let exit = v
                            .get("exit_code")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        format!("ran `{command} {joined_args}` (exit {exit})")
                    };
                    (v, summary)
                }
                Err(e) => (
                    json_error(&e),
                    format!("run_command `{command}` failed: {e}"),
                ),
            }
        }
        other => {
            let msg = format!(
                "unknown tool '{other}' (not one of read_file/write_file/list_dir/run_command)"
            );
            (serde_json::json!({ "error": msg }), msg)
        }
    }
}

// ── The agentic loop ────────────────────────────────────────────────────────────────────────

fn system_prompt(task: &str, workspace_root: &Path) -> String {
    format!(
        "You are an autonomous coding agent running directly on a member's own machine (Hive, \
         ADR-024). You have real, unsandboxed access to exactly one working directory:\n\n  {}\n\n\
         Tools available: read_file, write_file, list_dir, run_command. Every path you pass to \
         them is resolved relative to that directory -- an absolute path, or one that tries to \
         escape it with '..', is rejected. run_command runs a real subprocess directly (no \
         shell): pass the program and its arguments separately, never as one shell-syntax \
         string.\n\nTask:\n{}\n\n\
         Work by calling tools until the task is complete. When you are done, reply with plain \
         text (no further tool calls) summarizing what you did -- that reply becomes this \
         session's final report. If the task can't be completed, say so plainly in that final \
         text reply instead of calling more tools.",
        workspace_root.display(),
        task
    )
}

/// Post one node-authored progress event to the member's Private Fleet channel (ADR-024 decision
/// 6). Best-effort: a failed post is logged and otherwise ignored, same reasoning
/// `crate::worker`'s own checkpoint-failure handling already uses ("continuing" rather than
/// treating a channel hiccup as a reason to abort a multi-hour session).
async fn post_event(hub: &dyn Hub, event_type: &str, body: &str, payload: serde_json::Value) {
    if let Err(e) = hub.post_activity(event_type, body, payload).await {
        tracing::warn!("code session: failed to post progress event '{event_type}': {e}");
    }
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let short: String = s.chars().take(max).collect();
    format!("{short}...")
}

/// What one coding session produced. Always constructed for both a clean finish and a
/// turn-limit stop — see this module's doc: hitting `max_turns` is reported honestly, not hidden
/// as a silent success.
#[derive(Debug, Clone)]
pub struct CodeSessionOutcome {
    /// The brain's final text reply, or a synthesized message if `max_turns` was hit first.
    pub final_text: String,
    pub turns: u32,
    pub hit_turn_limit: bool,
}

/// Run one coding-agent session to completion: prepare the workspace, then loop turn-by-turn
/// against `brain` until it replies with plain text or `spec.max_turns` is exhausted. Posts a
/// progress event after every tool call and once at the end (ADR-024 decision 6).
///
/// Returns `Err` only for a genuine setup/operational failure (bad workspace, git failure, or
/// the brain itself erroring — e.g. the local Ollama server unreachable) — `crate::tools::run_code_session`
/// (the `ToolOutcome`-shaped wrapper `crate::worker` actually calls) turns any `Err` here into a
/// normal `ok: false` outcome rather than a propagated panic, per this codebase's existing
/// tool-call convention (see that function's doc). Hitting `max_turns` without the brain
/// declaring itself done is *not* an `Err` — it's a normal, honestly-reported [`CodeSessionOutcome`]
/// with `hit_turn_limit: true`.
pub async fn run_session(
    hub: &dyn Hub,
    data_dir: &Path,
    card_id: Uuid,
    spec: &CodeSessionSpec,
    brain: &dyn CodeBrain,
) -> Result<CodeSessionOutcome, CoderError> {
    let workspace_root = prepare_workspace(data_dir, card_id, spec).await?;
    post_event(
        hub,
        "code_session_started",
        &format!(
            "started a coding session in {} ({})",
            workspace_root.display(),
            truncate_preview(&spec.task, 200)
        ),
        serde_json::json!({ "card_id": card_id, "workspace_root": workspace_root.to_string_lossy() }),
    )
    .await;

    let tools = tool_specs();
    let mut messages = vec![
        BrainMessage::system(system_prompt(&spec.task, &workspace_root)),
        BrainMessage::user(spec.task.clone()),
    ];

    let mut turns = 0u32;
    let outcome = loop {
        if turns >= spec.max_turns {
            break CodeSessionOutcome {
                final_text: format!(
                    "session stopped after {} turns without the brain declaring the task done",
                    spec.max_turns
                ),
                turns,
                hit_turn_limit: true,
            };
        }
        turns += 1;
        match brain.next_turn(&messages, &tools).await {
            Ok(BrainTurn::Text(text)) => {
                break CodeSessionOutcome {
                    final_text: text,
                    turns,
                    hit_turn_limit: false,
                };
            }
            Ok(BrainTurn::ToolCalls(calls)) => {
                messages.push(BrainMessage::assistant_tool_calls(calls.clone()));
                for call in &calls {
                    let (result_value, summary) = execute_tool(&workspace_root, call).await;
                    tracing::info!(card = %card_id, tool = %call.name, "code session tool ran: {summary}");
                    messages.push(BrainMessage::tool_result(
                        call.id.clone(),
                        result_value.to_string(),
                    ));
                    post_event(
                        hub,
                        "code_session_tool",
                        &summary,
                        serde_json::json!({ "card_id": card_id, "tool": call.name }),
                    )
                    .await;
                }
            }
            Err(e) => {
                let msg = format!("coding session failed: {e}");
                post_event(
                    hub,
                    "code_session_error",
                    &msg,
                    serde_json::json!({ "card_id": card_id }),
                )
                .await;
                return Err(CoderError::Brain(e));
            }
        }
    };

    post_event(
        hub,
        "code_session_finished",
        &format!(
            "session finished after {} turns: {}",
            outcome.turns,
            truncate_preview(&outcome.final_text, 200)
        ),
        serde_json::json!({ "card_id": card_id, "turns": outcome.turns, "hit_turn_limit": outcome.hit_turn_limit }),
    )
    .await;

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/data/workspace")
    }

    #[test]
    fn resolve_in_workspace_accepts_a_plain_relative_path() {
        let p = resolve_in_workspace(&root(), "src/main.rs").unwrap();
        assert_eq!(p, PathBuf::from("/data/workspace/src/main.rs"));
    }

    #[test]
    fn resolve_in_workspace_rejects_an_absolute_path() {
        assert!(matches!(
            resolve_in_workspace(&root(), "/etc/passwd"),
            Err(ToolExecError::PathEscapesWorkspace(_))
        ));
    }

    #[test]
    fn resolve_in_workspace_rejects_a_dot_dot_escape() {
        assert!(matches!(
            resolve_in_workspace(&root(), "../../etc/passwd"),
            Err(ToolExecError::PathEscapesWorkspace(_))
        ));
    }

    #[test]
    fn resolve_in_workspace_allows_a_dot_dot_that_stays_inside() {
        // "a/../b" normalizes to "b", still inside the workspace root.
        let p = resolve_in_workspace(&root(), "a/../b").unwrap();
        assert_eq!(p, PathBuf::from("/data/workspace/b"));
    }

    #[test]
    fn code_session_spec_requires_a_workspace_or_repo() {
        let v = serde_json::json!({ "task": "do the thing" });
        let err = CodeSessionSpec::from_required_capabilities(&v);
        assert!(matches!(err, Err(CoderError::InvalidSpec(_))));
    }

    #[test]
    fn code_session_spec_defaults_brain_and_max_turns() {
        let v = serde_json::json!({ "task": "do the thing", "workspace_path": "/tmp/x" });
        let spec = CodeSessionSpec::from_required_capabilities(&v).unwrap();
        assert_eq!(spec.brain, "local");
        assert_eq!(spec.max_turns, 40);
    }

    #[test]
    fn code_session_spec_rejects_empty_task() {
        let v = serde_json::json!({ "task": "   ", "workspace_path": "/tmp/x" });
        assert!(matches!(
            CodeSessionSpec::from_required_capabilities(&v),
            Err(CoderError::InvalidSpec(_))
        ));
    }

    #[tokio::test]
    async fn run_command_tool_runs_a_real_process_and_captures_output() {
        let dir = std::env::temp_dir().join(format!("ohhive-coder-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let out = run_command_tool(&dir, "echo", &["hello".to_string()], None)
            .await
            .unwrap();
        assert_eq!(out.get("exit_code").and_then(|v| v.as_i64()), Some(0));
        assert!(out
            .get("stdout")
            .and_then(|v| v.as_str())
            .unwrap()
            .contains("hello"));
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn write_then_read_file_round_trips() {
        let dir = std::env::temp_dir().join(format!("ohhive-coder-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        write_file_tool(&dir, "sub/hello.txt", "hi there")
            .await
            .unwrap();
        let read = read_file_tool(&dir, "sub/hello.txt").await.unwrap();
        assert_eq!(
            read.get("content").and_then(|v| v.as_str()),
            Some("hi there")
        );
        tokio::fs::remove_dir_all(&dir).await.ok();
    }
}
