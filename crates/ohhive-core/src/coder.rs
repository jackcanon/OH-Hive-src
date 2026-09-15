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
//! # The vault tools (optional, ADR-028)
//!
//! When a card's `required_capabilities` names a `vault_name` ([`CodeSessionSpec::vault_name`]),
//! two more tools are wired in: `vault_search`/`vault_read`, reading the same "this machine only"
//! vault store the desktop app already reads (`ohhive-ffi/src/local_hub.rs`'s
//! `vault-host.sqlite3`, opened and self-enrolled the same way that file's `vault_open` does).
//! `vault_name` is host-trusted card data, same as `workspace_path`/`task` — the running brain
//! never chooses *which* vault it can see, only that it may search/read whichever vault the card
//! was created against. `tool_specs()` always advertises both tools (so a brain can discover them
//! without a round-trip); calling either one without a configured `vault_name`, or in a build
//! compiled without the `local-hub` feature, is a normal tool error, not a panic — see
//! `execute_tool`'s `"vault_search"`/`"vault_read"` arms. Cross-machine vault reading and folder
//! ingestion are not wired here (`ohhive-ffi/src/local_hub.rs`'s header doc covers why); this only
//! ever reaches a vault that already lives on the same machine `run_session` is running on.
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

/// Bounded, not paginated (Sif's efficiency audit, finding 3, 2026-09-15): `list_dir` used to
/// collect/inspect/sort every entry in a directory regardless of size. A real cursor-based
/// continuation would need a new tool-call argument for the brain to pass back -- out of scope
/// for this pass, same "narrower than the ideal fix, flagged honestly" scoping as finding 4's
/// mid-batch lease recheck. This just stops the unbounded work and says so via `truncated`.
pub const LIST_DIR_MAX_ENTRIES: usize = 2000;

/// After a killed/exited child's pipes close, its two stdout/stderr reader tasks (see
/// `run_command_tool`) should finish almost immediately -- this just bounds that "almost"
/// rather than trusting it unconditionally.
pub const READER_DRAIN_GRACE: Duration = Duration::from_secs(5);

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
    /// Name of a vault (ADR-028) this session may search/read via the `vault_search`/`vault_read`
    /// tools, matched against `LocalHub::vault_list()`'s `VaultInfo.name` on this machine. `None`
    /// (the default -- every card written before this field existed omits it) still advertises
    /// both tools in `tool_specs()`, but every call to either one fails cleanly with "no vault is
    /// configured for this session". Never set by the running brain -- see this module's "vault
    /// tools" doc for the full host-trusted-data framing.
    #[serde(default)]
    pub vault_name: Option<String>,
    /// ADR-032: this session may spawn child cards (`spawn_card`) and pause itself to wait on
    /// one (`wait_for_child`) -- both tools are only advertised in `tool_specs()` when this is
    /// true. `false` (the default -- every card written before this field existed omits it)
    /// leaves an ordinary coding session exactly as it was before ADR-032: no coordinator
    /// tools, no ability to spawn anything under it. Never set by the running brain itself --
    /// same host-trusted-data framing as `vault_name` above.
    #[serde(default)]
    pub coordinator: bool,
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

// ── The CodeBrain seam ────────────────────────────────────────────────────────────────────────────────
//
// Moved to `crate::brain` (2026-09-15) per ADR-029 phase 1's "shared multimodal messages and
// executor interface" -- `desktop::ContentBlock`'s own doc already anticipated this. Re-exported
// here unchanged so nothing outside this module (worker.rs included, confirmed by grep before
// this move: it only ever names `coder::CodeBrain`/`CodeSessionSpec`/`LocalBrain`/`CloudBrain`,
// never the seam types directly) has to change how it spells these types.
pub use crate::brain::{BrainRole, BrainToolCall, BrainMessage, ToolSpec, BrainTurn, CodeBrainError, CodeBrain};


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
        content: m.text(),
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
        content: m.text(),
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
    let mut child = cmd.spawn().map_err(CoderError::GitSpawn)?;
    let stdout = child.stdout.take().expect("stdout requested at spawn");
    let stderr = child.stderr.take().expect("stderr requested at spawn");

    // Bounded drain, same helper, same cap and reasoning as `run_command_tool`'s stdout/stderr
    // handling (Sif's efficiency audit, finding 3, 2026-09-15): `cmd.output()` used to buffer
    // both pipes to completion, unbounded, before this function ever looked at the exit status.
    // stdout is still drained (never surfaced -- matches this function's pre-existing behavior of
    // only ever reporting stderr) purely so a noisy child can't deadlock on a full pipe while
    // this function is waiting on it.
    let stdout_task = tokio::spawn(read_capped(stdout, READ_FILE_MAX_BYTES));
    let stderr_task = tokio::spawn(read_capped(stderr, READ_FILE_MAX_BYTES));

    let status = match tokio::time::timeout(GIT_TIMEOUT, child.wait()).await {
        Ok(res) => res.map_err(CoderError::GitSpawn)?,
        Err(_) => {
            let _ = child.start_kill();
            return Err(CoderError::GitTimeout(owned_args));
        }
    };

    if status.success() {
        return Ok(());
    }
    let (stderr_bytes, _) = match tokio::time::timeout(READER_DRAIN_GRACE, stderr_task).await {
        Ok(Ok(v)) => v,
        _ => (Vec::new(), true),
    };
    // stdout's result is never surfaced -- just make sure that task has actually finished rather
    // than left permanently detached, same grace window as stderr's above.
    let _ = tokio::time::timeout(READER_DRAIN_GRACE, stdout_task).await;
    Err(CoderError::GitFailed {
        args: owned_args,
        code: status.code(),
        stderr: String::from_utf8_lossy(&stderr_bytes).to_string(),
    })
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
    /// Covers "no vault configured for this session", "no such vault", a bad document id, and any
    /// underlying `local_hub` store/session error — folded into one string variant rather than
    /// mirroring `hive_core::local_hub`'s own error enum here, since every caller of this type
    /// (`execute_tool`, by way of `json_error`) only ever displays it, never matches on it.
    #[error("vault error: {0}")]
    Vault(String),
    /// Covers every `crate::skills::SkillError` this module's skill-aware `write_file`/mark-used
    /// paths can hit (bad id, malformed frontmatter, already exists, size/count limit, store
    /// busy/changed) -- folded into one string variant for the same reason `Vault` is: every
    /// caller only ever displays it, never matches on it.
    #[error("skill error: {0}")]
    Skill(String),
    /// ADR-032: `spawn_card`/`wait_for_child` failures -- covers every `HubError` either tool
    /// can hit (RPC rejected, e.g. this card isn't holding the parent lease or the named child
    /// isn't actually this card's child; transport failure; bad key). Same one-string-variant
    /// convention as `Vault`/`Skill` above.
    #[error("hub error: {0}")]
    Hub(String),
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

/// Returns both the JSON handed back to the brain and the exact raw bytes read, so a caller that
/// needs to know precisely what content the brain was shown (`execute_tool`'s skill-usage hook)
/// doesn't have to re-read the file itself -- see `maybe_mark_skill_used`'s doc for why a second,
/// independent read was a real bug, not just a style nit.
async fn read_file_tool(
    workspace_root: &Path,
    path: &str,
) -> Result<(serde_json::Value, Vec<u8>), ToolExecError> {
    let resolved = resolve_in_workspace(workspace_root, path)?;
    let mut file = tokio::fs::File::open(&resolved)
        .await
        .map_err(|e| ToolExecError::Io(resolved.display().to_string(), e))?;
    let meta = file
        .metadata()
        .await
        .map_err(|e| ToolExecError::Io(resolved.display().to_string(), e))?;
    // Reject directories/FIFOs/devices before ever attempting a read -- a FIFO with no writer in
    // particular would just hang here rather than error (Sif's efficiency audit, finding 3,
    // 2026-09-15: "regular-file validation").
    if !meta.is_file() {
        return Err(ToolExecError::Io(
            resolved.display().to_string(),
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a regular file"),
        ));
    }
    // Bounded read of at most `READ_FILE_MAX_BYTES + 1` bytes -- the "+1" is only so `truncated`
    // can be detected without reading anything past the cap. A multi-gigabyte file used to be
    // read/allocated here in full before this function truncated its *response*; `meta.len()`
    // (a cheap stat already in hand from the open file, no extra syscall) reports the true total
    // instead of needing the whole file read to know it.
    let mut buf = Vec::with_capacity((READ_FILE_MAX_BYTES + 1).min(meta.len() as usize + 1));
    (&mut file)
        .take((READ_FILE_MAX_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .await
        .map_err(|e| ToolExecError::Io(resolved.display().to_string(), e))?;
    let truncated = buf.len() > READ_FILE_MAX_BYTES;
    buf.truncate(READ_FILE_MAX_BYTES.min(buf.len()));
    // Lossy decode, not a char-boundary walk-back: this also has to handle a genuinely binary
    // file gracefully (never panics either way), unlike `crate::tools::truncate_for_summary`
    // which only ever truncates a value that started life as valid UTF-8.
    let mut content = String::from_utf8_lossy(&buf).to_string();
    if truncated {
        content.push_str(&format!(
            "\n... [truncated, {} of {} bytes shown]",
            buf.len(),
            meta.len()
        ));
    }
    let v = serde_json::json!({
        "path": path,
        "content": content,
        "truncated": truncated,
        "total_bytes": meta.len(),
    });
    Ok((v, buf))
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
    let mut truncated = false;
    while let Some(entry) = rd
        .next_entry()
        .await
        .map_err(|e| ToolExecError::Io(target.display().to_string(), e))?
    {
        if entries.len() >= LIST_DIR_MAX_ENTRIES {
            // See LIST_DIR_MAX_ENTRIES's doc: bounded, not paginated. Stops here rather than
            // reading every remaining entry (each costing its own `file_type()` syscall) just to
            // discard most of them.
            truncated = true;
            break;
        }
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
    Ok(serde_json::json!({
        "path": path,
        "entries": entries,
        "truncated": truncated,
    }))
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

    // Read stdout/stderr on their own tasks, independent of the timed wait below, so a timeout
    // can still recover whatever they'd captured so far. Previously the whole
    // `tokio::join!(stdout, stderr, wait)` was wrapped in one `timeout`, so timing out dropped
    // that entire future -- reader tasks included -- discarding any output already read (Sif's
    // efficiency audit, finding 4, 2026-09-15). These tasks finish on their own once the child's
    // pipes close, whether that's a normal exit or the `start_kill()` below.
    let stdout_task = tokio::spawn(read_capped(stdout, READ_FILE_MAX_BYTES));
    let stderr_task = tokio::spawn(read_capped(stderr, READ_FILE_MAX_BYTES));

    let wait_outcome = tokio::time::timeout(RUN_COMMAND_TIMEOUT, child.wait()).await;
    let timed_out = wait_outcome.is_err();
    let status = match wait_outcome {
        Ok(status) => {
            status.map_err(|e| ToolExecError::Io(resolved_cwd.display().to_string(), e))?
        }
        Err(_) => {
            let _ = child.start_kill();
            child
                .wait()
                .await
                .map_err(|e| ToolExecError::Io(resolved_cwd.display().to_string(), e))?
        }
    };

    // The pipes are closed by now (the child exited, or `start_kill()` just closed them), so
    // these normally resolve immediately -- `READER_DRAIN_GRACE` bounds the wait rather than
    // trusting that unconditionally. `kill_on_drop` above still owns the direct child only, not
    // any grandchild it may have spawned -- real process-tree ownership is a larger, platform-
    // specific change (finding 4) not attempted tonight; flagging it as a known gap rather than
    // silently claiming this is sandboxed cleanup.
    let (out, out_trunc) = match tokio::time::timeout(READER_DRAIN_GRACE, stdout_task).await {
        Ok(Ok(v)) => v,
        _ => (Vec::new(), true),
    };
    let (err, err_trunc) = match tokio::time::timeout(READER_DRAIN_GRACE, stderr_task).await {
        Ok(Ok(v)) => v,
        _ => (Vec::new(), true),
    };

    Ok(serde_json::json!({
        "command": command,
        "args": args,
        "exit_code": status.code(),
        "stdout": String::from_utf8_lossy(&out).to_string(),
        "stdout_truncated": out_trunc,
        "stderr": String::from_utf8_lossy(&err).to_string(),
        "stderr_truncated": err_trunc,
        "timed_out": timed_out,
        "timeout_seconds": if timed_out {
            serde_json::json!(RUN_COMMAND_TIMEOUT.as_secs())
        } else {
            serde_json::Value::Null
        },
    }))
}

// ── The skills prompt (optional; ADR-027) ─────────────────────────────────────────────────────────────
//
// No new tool (ADR-027 decision 5): a skill is just a file at `.hive/skills/<id>/SKILL.md`
// inside the workspace, so the brain reads one with the `read_file` tool it already has and
// writes a new one with `write_file` -- both already permitted anywhere under the workspace root
// by `resolve_in_workspace`. This section's job is narrower: list what's already there in the
// system prompt (so the brain knows a skill exists before it goes looking), and mark a skill
// "used" the moment the brain actually reads its procedure (decision 4's "creation is
// fully automatic, no approval" only covers the write side -- usage tracking still has to be
// driven from here, since `crate::skills::SkillStore` has no way to know *why* a file was read).
// Malformed or oversized skill files never fail a session: `SkillStore::list` already reports
// them as `issues` rather than errors, and this module's own two helpers below are best-effort on
// top of that -- a broken `.hive/skills/` entry costs that one skill's visibility, never the run.

#[cfg(feature = "skills")]
fn skills_prompt_block(workspace_root: &Path) -> String {
    let inventory = match crate::skills::SkillStore::open(workspace_root).and_then(|s| s.list()) {
        Ok(inv) => inv,
        // No `.hive/skills/` yet, or this workspace isn't writable that way -- silently no block,
        // same as having zero skills. Never a reason to fail the session over.
        Err(_) => return String::new(),
    };
    // Creation guidance always applies, even (especially) with zero skills saved yet -- an empty
    // catalog is exactly when a brain most needs to know saving one is possible (fixes a gap Sif
    // caught in review: the old early-return on `inventory.skills.is_empty()` meant a first-ever
    // skill in a workspace could never get offered).
    let mut block = String::new();
    if inventory.skills.is_empty() {
        block.push_str("\n\nNo skills saved in this workspace yet (`.hive/skills/`, ADR-027).");
    } else {
        block.push_str(
            "\n\nSkills available in this workspace (`.hive/skills/`, ADR-027) -- reusable \
             procedures from earlier sessions here. Read one in full with read_file at the path \
             shown before relying on it; a one-line description is not the procedure:\n",
        );
        for s in &inventory.skills {
            block.push_str(&format!(
                "- {}: {} -- .hive/skills/{}/SKILL.md\n",
                s.name, s.description, s.id
            ));
        }
    }
    block.push_str(
        "\nIf you work out a reusable procedure this session that isn't already covered above \
         (a reliable build/test recipe, a project-specific gotcha, a tool invocation pattern) \
         and it would genuinely help a future session here, you may save it: write_file to \
         `.hive/skills/<id>/SKILL.md` where <id> is lowercase letters/digits/hyphens only. Start \
         the file with YAML frontmatter giving `name` and `description`, then the procedure in \
         Markdown below the closing `---`. This is entirely your judgment call, not required --  \
         skip it for routine or one-off tasks. Each skill file is create-only (write_file to that \
         path routes through the skill store, not a raw file write -- see below): pick a new <id> \
         rather than overwriting one that already exists.",
    );
    block
}

#[cfg(not(feature = "skills"))]
fn skills_prompt_block(_workspace_root: &Path) -> String {
    String::new()
}

/// `Some(id)` iff `path` is exactly `.hive/skills/<id>/SKILL.md` -- the one shape both the
/// usage-marking hook (below) and `write_file`'s skill-store routing (see `execute_tool`) key
/// off of. Kept as pure, allocation-cheap path parsing (no I/O, no lock) so both call sites can
/// afford to run it on every read_file/write_file without a store round-trip just to find out
/// whether one is warranted.
#[cfg(feature = "skills")]
fn skill_id_for_path(path: &str) -> Option<String> {
    let comps: Vec<_> = Path::new(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let [dot_hive, skills, id, file] = comps.as_slice() else {
        return None;
    };
    if dot_hive != ".hive" || skills != "skills" || file != "SKILL.md" {
        return None;
    }
    Some(id.clone())
}

#[cfg(not(feature = "skills"))]
fn skill_id_for_path(_path: &str) -> Option<String> {
    None
}

/// Best-effort: called after a successful `read_file` whose path is exactly
/// `.hive/skills/<id>/SKILL.md`, so a deliberate "read this skill's procedure" counts as usage --
/// `SkillStore::mark_used`'s own doc is explicit that inventory/preview reads (the prompt listing
/// above) must not count. `bytes` must be the exact bytes `read_file` just handed the brain --
/// the revision marked used is hashed from those bytes directly (the same algorithm
/// `SkillStore` hashes with internally), not from a second, independent read. That second read
/// is what the original version of this function did, and Sif's review caught the bug in it: a
/// fresh `store.read()` here always reports *whatever revision is on disk right now*, which is
/// not necessarily what the brain was actually shown -- a write racing this call would have
/// gotten silently (and wrongly) marked as "used" under the old code. Binding to the delivered
/// bytes instead means `mark_used`'s own `Changed` check (comparing our hash against a fresh
/// load) now does exactly the right thing if the file moved in between: refuse, rather than
/// mis-attribute usage to content the brain never saw. Never surfaced to the brain and never
/// affects the read it's piggybacking on: every failure path here (feature off, no such skill,
/// store busy, changed) is silently swallowed.
#[cfg(feature = "skills")]
async fn maybe_mark_skill_used(workspace_root: &Path, path: &str, bytes: &[u8]) {
    let Some(id) = skill_id_for_path(path) else {
        return;
    };
    let revision = crate::skills::hash(bytes);
    let workspace_root = workspace_root.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        crate::skills::SkillStore::open(&workspace_root)?.mark_used(&id, &revision)
    })
    .await;
}

#[cfg(not(feature = "skills"))]
async fn maybe_mark_skill_used(_workspace_root: &Path, _path: &str, _bytes: &[u8]) {}

/// Skill-store-backed `write_file`: what `execute_tool` calls instead of the generic
/// `write_file_tool` when the path is exactly `.hive/skills/<id>/SKILL.md` (`skill_id_for_path`
/// matched). Fixes the other half of Sif's review finding: routing a skill write through the
/// plain filesystem `write_file_tool` bypassed every guarantee `SkillStore::write_new` exists to
/// provide -- create-only (silently clobbering an existing skill), frontmatter validation (a
/// malformed file would sit there passing as a listed skill until something tried to parse it),
/// and the store's own count/size bounds. Routing through `write_new` here means the brain gets
/// a clear, structured error (already-exists, bad id, invalid frontmatter, limit) instead of a
/// silent raw write that only breaks later.
#[cfg(feature = "skills")]
async fn write_new_skill_tool(
    workspace_root: &Path,
    id: &str,
    content: &str,
) -> Result<serde_json::Value, ToolExecError> {
    let workspace_root = workspace_root.to_path_buf();
    let id_owned = id.to_string();
    let content_owned = content.to_string();
    tokio::task::spawn_blocking(move || -> Result<serde_json::Value, ToolExecError> {
        let store = crate::skills::SkillStore::open(&workspace_root)
            .map_err(|e| ToolExecError::Skill(e.to_string()))?;
        let summary = store
            .write_new(&id_owned, &content_owned)
            .map_err(|e| ToolExecError::Skill(e.to_string()))?;
        Ok(serde_json::json!({
            "path": format!(".hive/skills/{}/SKILL.md", summary.id),
            "bytes_written": content_owned.len(),
            "skill_id": summary.id,
            "skill_name": summary.name,
        }))
    })
    .await
    .map_err(|e| ToolExecError::Skill(format!("skill write task panicked: {e}")))?
}

#[cfg(not(feature = "skills"))]
async fn write_new_skill_tool(
    _workspace_root: &Path,
    _id: &str,
    _content: &str,
) -> Result<serde_json::Value, ToolExecError> {
    // `skill_id_for_path` (the only caller of this, via `execute_tool`) always returns `None`
    // without the "skills" feature, so this stub is never actually invoked -- it exists only so
    // the crate compiles either way, matching this module's established feature-gating pattern.
    Err(ToolExecError::Skill(
        "skill support isn't compiled into this build (missing the 'skills' feature)".into(),
    ))
}


// ── The vault tools (optional; see this module's doc) ──────────────────────────────────────

#[cfg(feature = "local-hub")]
fn vault_store_path() -> PathBuf {
    crate::nodeconfig::path().with_file_name("vault-host.sqlite3")
}

/// Pure name lookup, factored out so it's unit-testable without touching disk — everything else
/// vault resolution needs is a real SQLite store. Exact match only, same as the desktop app's own
/// `vault_create` — there is no fuzzy matching and no dedup-on-create anywhere in this feature, so
/// two vaults can share a name; this returns whichever one `vault_list()` happened to return
/// first, the same "good enough for v1" scope as everything else ADR-028 has shipped so far.
#[cfg(feature = "local-hub")]
fn find_vault_by_name<'a>(
    vaults: &'a [crate::local_hub::vault::VaultInfo],
    name: &str,
) -> Option<&'a crate::local_hub::vault::VaultInfo> {
    vaults.iter().find(|v| v.name == name)
}

/// Opens this machine's vault store and returns an authenticated reader session — the same two
/// steps `ohhive-ffi/src/local_hub.rs`'s `vault_open` does for the desktop app (open-or-create the
/// store, mint-or-reuse `HIVE_VAULT_SELF_KEY`), duplicated here rather than shared because a
/// worker process and the desktop app are different processes with no handle to share across.
///
/// `LocalHubStore::open`/`from_connection` marks *every vault on this machine* unavailable
/// pending revalidation on every open (meant for a folder-backed vault's watcher to undo) — a
/// hand-curated vault has no watcher, so `vault_reopen_manual` immediately republishes those, but
/// a folder-backed vault stays unavailable until its own watcher notices and reconciles. This
/// write is global and immediately visible to every other reader of the same SQLite store,
/// including the real, already-running vault host -- calling this on every single tool call (as
/// this module used to) meant a burst of `vault_search`/`vault_read` calls within one coding
/// session could repeatedly bounce a perfectly healthy folder vault's availability for other
/// readers, not just cost a few milliseconds of its own SQLite writes (Sif's efficiency audit,
/// finding 2, 2026-09-15). [`vault_reader`] is the fix in scope for this pass: reuse one opened
/// session per coding session (`ohhive-ffi/src/local_hub.rs`'s `VaultState` already does the
/// analogous thing for the desktop app's reader). That narrows the blast radius to "once per
/// session" rather than "once per tool call," but doesn't eliminate it across *concurrent*
/// sessions each opening their own first reader around the same time -- fully separating host
/// startup/recovery from opening a reader (this doc's other recommended option) would need this
/// module to talk to the already-running host service instead of opening a competing connection
/// at all, a larger change not attempted here.
#[cfg(feature = "local-hub")]
fn open_vault_reader() -> Result<crate::local_hub::LocalHub, ToolExecError> {
    let store = crate::local_hub::LocalHubStore::open(vault_store_path())
        .map_err(|e| ToolExecError::Vault(format!("couldn't open the vault store: {e}")))?;
    store
        .vault_reopen_manual()
        .map_err(|e| ToolExecError::Vault(format!("couldn't republish this machine's vaults: {e}")))?;
    let raw_key = match crate::nodeconfig::get_extra("HIVE_VAULT_SELF_KEY") {
        Some(k) => k,
        None => {
            let creds = store.enroll_owner("this machine").map_err(|e| {
                ToolExecError::Vault(format!("couldn't enroll this machine's vault reader: {e}"))
            })?;
            crate::nodeconfig::set("HIVE_VAULT_SELF_KEY", &creds.raw_key).map_err(|e| {
                ToolExecError::Vault(format!("couldn't save this machine's vault reader key: {e}"))
            })?;
            creds.raw_key
        }
    };
    store
        .connect(&raw_key)
        .map_err(|e| ToolExecError::Vault(format!("couldn't open a vault reader session: {e}")))
}

/// One coding session's cached vault reader (see [`open_vault_reader`]'s doc for why this
/// matters). `Arc` so a clone can move into `vault_search_tool`/`vault_read_tool`'s
/// `spawn_blocking` closures, which need `'static` captures -- a plain borrowed reference to a
/// `run_session`-local `Mutex` doesn't satisfy that. `()` without the "local-hub" feature so
/// `execute_tool`'s signature (used regardless of that feature) doesn't have to change shape
/// per-feature; every real access to the inner value only compiles under "local-hub" anyway.
#[cfg(feature = "local-hub")]
pub(crate) type VaultReaderCache = std::sync::Arc<std::sync::Mutex<Option<crate::local_hub::LocalHub>>>;
#[cfg(not(feature = "local-hub"))]
pub(crate) type VaultReaderCache = ();

/// Returns this session's cached reader if one's already open, otherwise opens one (the one real
/// `LocalHubStore::open` this session should ever make -- see `open_vault_reader`'s doc) and
/// caches it for every subsequent vault tool call in the same session. Holding the lock across
/// `open_vault_reader()` itself is deliberate, not an oversight: it means two vault tool calls
/// racing to be first within one session serialize onto a single open rather than each starting
/// their own.
#[cfg(feature = "local-hub")]
fn vault_reader(cache: &VaultReaderCache) -> Result<crate::local_hub::LocalHub, ToolExecError> {
    let mut guard = cache
        .lock()
        .map_err(|_| ToolExecError::Vault("vault reader cache lock poisoned".into()))?;
    if let Some(reader) = guard.as_ref() {
        return Ok(reader.clone());
    }
    let reader = open_vault_reader()?;
    *guard = Some(reader.clone());
    Ok(reader)
}

#[cfg(feature = "local-hub")]
fn resolve_vault(reader: &crate::local_hub::LocalHub, name: &str) -> Result<Uuid, ToolExecError> {
    let vaults = reader
        .vault_list()
        .map_err(|e| ToolExecError::Vault(format!("couldn't list this machine's vaults: {e}")))?;
    // A configured `vault_name` (`CodeSessionSpec::vault_name`, host-trusted card data) may
    // itself be a vault's id rather than its display name -- resolving by id first is
    // unambiguous by construction and sidesteps name collisions entirely, so try it before any
    // name lookup (Sif's efficiency audit, finding 2, 2026-09-15).
    if let Ok(id) = Uuid::parse_str(name) {
        if let Some(v) = vaults.iter().find(|v| v.id == id) {
            return Ok(v.id);
        }
    }
    // Two (or more) vaults sharing a name used to resolve to "whichever `vault_list()` happened
    // to return first" (see `find_vault_by_name`'s own doc) -- silently reading the wrong vault
    // rather than refusing. Reject the ambiguity instead; the fix is configuring this card's
    // vault by id.
    let match_count = vaults.iter().filter(|v| v.name == name).count();
    if match_count > 1 {
        return Err(ToolExecError::Vault(format!(
            "'{name}' matches {match_count} vaults on this machine -- configure this card's vault by id instead of name to disambiguate"
        )));
    }
    find_vault_by_name(&vaults, name).map(|v| v.id).ok_or_else(|| {
        let available: Vec<&str> = vaults.iter().map(|v| v.name.as_str()).collect();
        ToolExecError::Vault(format!(
            "no vault named '{name}' on this machine (available: {})",
            if available.is_empty() { "none".to_string() } else { available.join(", ") }
        ))
    })
}

#[cfg(feature = "local-hub")]
async fn vault_search_tool(
    vault_name: &str,
    query: &str,
    limit: u32,
    cache: &VaultReaderCache,
) -> Result<serde_json::Value, ToolExecError> {
    let vault_name = vault_name.to_string();
    let query = query.to_string();
    let cache = cache.clone();
    // `local_hub`'s vault methods are plain synchronous SQLite calls (see that module's doc) —
    // run them on a blocking thread so a slow or lock-contended store can't stall this session's
    // whole tokio runtime the way calling them directly here would.
    tokio::task::spawn_blocking(move || -> Result<serde_json::Value, ToolExecError> {
        let reader = vault_reader(&cache)?;
        let vault = resolve_vault(&reader, &vault_name)?;
        let hits = reader
            .vault_search(vault, &query, limit)
            .map_err(|e| ToolExecError::Vault(format!("vault search failed: {e}")))?;
        Ok(serde_json::json!({
            "vault": vault_name,
            "query": query,
            "hits": hits.into_iter().map(|h| serde_json::json!({
                "id": h.id.to_string(),
                "path": h.path,
                "revision": h.revision,
                "title": h.title,
                "snippet": h.snippet,
                "score": h.score,
            })).collect::<Vec<_>>(),
        }))
    })
    .await
    .map_err(|e| ToolExecError::Vault(format!("vault search task panicked: {e}")))?
}

#[cfg(not(feature = "local-hub"))]
async fn vault_search_tool(
    _vault_name: &str,
    _query: &str,
    _limit: u32,
    _cache: &VaultReaderCache,
) -> Result<serde_json::Value, ToolExecError> {
    Err(ToolExecError::Vault(
        "vault support isn't compiled into this build (missing the 'local-hub' feature)".into(),
    ))
}

#[cfg(feature = "local-hub")]
async fn vault_read_tool(
    vault_name: &str,
    document_id: &str,
    revision: &str,
    cache: &VaultReaderCache,
) -> Result<serde_json::Value, ToolExecError> {
    let vault_name = vault_name.to_string();
    let document_id_owned = document_id.to_string();
    let revision = revision.to_string();
    let cache = cache.clone();
    tokio::task::spawn_blocking(move || -> Result<serde_json::Value, ToolExecError> {
        let id = Uuid::parse_str(&document_id_owned).map_err(|_| {
            ToolExecError::Vault(format!("'{document_id_owned}' is not a valid document id"))
        })?;
        let reader = vault_reader(&cache)?;
        let vault = resolve_vault(&reader, &vault_name)?;
        let doc = reader
            .vault_read(vault, id, &revision)
            .map_err(|e| ToolExecError::Vault(format!("vault read failed: {e}")))?;
        Ok(serde_json::json!({
            "vault": vault_name,
            "id": doc.id.to_string(),
            "path": doc.path,
            "revision": doc.revision,
            "title": doc.title,
            "content": doc.content,
        }))
    })
    .await
    .map_err(|e| ToolExecError::Vault(format!("vault read task panicked: {e}")))?
}

#[cfg(not(feature = "local-hub"))]
async fn vault_read_tool(
    _vault_name: &str,
    _document_id: &str,
    _revision: &str,
    _cache: &VaultReaderCache,
) -> Result<serde_json::Value, ToolExecError> {
    Err(ToolExecError::Vault(
        "vault support isn't compiled into this build (missing the 'local-hub' feature)".into(),
    ))
}

/// The fixed tool schema every session advertises, in OpenAI function-calling shape.
pub fn tool_specs(coordinator: bool) -> Vec<ToolSpec> {
    let mut specs = vec![
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
        ToolSpec {
            name: "vault_search".into(),
            description:
                "Full-text search this session's configured vault (a personal/team knowledge library, ADR-028) and return matching notes with short snippets. Returns a clean error if no vault is configured for this session."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms." },
                    "limit": { "type": "integer", "description": "Maximum results to return (defaults to 10)." }
                },
                "required": ["query"]
            }),
        },
        ToolSpec {
            name: "vault_read".into(),
            description:
                "Read one vault note in full by its id and revision (both returned by vault_search). Returns a clean error if the note has changed since that search -- search again to get its current revision."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_id": { "type": "string", "description": "The note's id, as returned by vault_search." },
                    "revision": { "type": "string", "description": "The note's revision, as returned by vault_search." }
                },
                "required": ["document_id", "revision"]
            }),
        },
    ];
    // ADR-032: only advertised when this session was explicitly submitted as a coordinator
    // (`CodeSessionSpec::coordinator`) -- Jack's "shaped set of skills/tools per job" call: an
    // ordinary coding card never even sees these two exist, rather than seeing them and being
    // told (or trusted) not to use them.
    if coordinator {
        specs.push(ToolSpec {
            name: "spawn_card".into(),
            description:
                "Create one child card in this same project (ADR-006 D44/ADR-032). The child is scheduled independently -- any of your own nodes may claim it. Use wait_for_child afterward if this session should pause until it finishes."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Unique key for the new card within this project." },
                    "title": { "type": "string", "description": "Short human-readable title." },
                    "modality": { "type": "string", "description": "e.g. \"code\" for another coding session." },
                    "inputs": { "type": "string", "description": "The task/instructions for the child." },
                    "acceptance": { "type": "string", "description": "Optional acceptance criteria text." },
                    "required_capabilities": { "type": "object", "description": "Optional; passed through as this card's required_capabilities (e.g. a nested code-session spec if modality is \"code\")." }
                },
                "required": ["key", "title", "modality", "inputs"]
            }),
        });
        specs.push(ToolSpec {
            name: "wait_for_child".into(),
            description:
                "Pause this session to wait on a card previously created with spawn_card (ADR-006 D44/ADR-032). This node releases its lease on the current card immediately -- the session ends right here, and a later node pick-up resumes the parent automatically once every card it spawned reaches review/done (or fails immediately if one is blocked). Calling this always ends the session: nothing after it in the same turn runs."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "child_card_id": { "type": "string", "description": "The child's card_id, as returned by spawn_card." }
                },
                "required": ["child_card_id"]
            }),
        });
    }
    specs
}

fn json_error(e: &ToolExecError) -> serde_json::Value {
    serde_json::json!({ "error": e.to_string() })
}

/// Run one tool call and return `(result_for_the_brain, human_summary_for_the_channel)`.
/// `vault_name` is `spec.vault_name` threaded straight through from `run_session` — host-trusted
/// card data, never something a tool call argument can override. `vault_cache` is this coding
/// session's own [`VaultReaderCache`], owned by `run_session` and passed through unopened on
/// every call so `vault_search`/`vault_read` reuse one vault reader for the whole session (Sif's
/// efficiency audit, finding 2, 2026-09-15) rather than each opening their own.
async fn execute_tool(
    workspace_root: &Path,
    call: &BrainToolCall,
    vault_name: Option<&str>,
    hub: &dyn Hub,
    card_id: Uuid,
    vault_cache: &VaultReaderCache,
) -> (serde_json::Value, String) {
    match call.name.as_str() {
        "read_file" => {
            let path = call
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match read_file_tool(workspace_root, path).await {
                Ok((v, bytes)) => {
                    let truncated = v
                        .get("truncated")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    let summary = format!(
                        "read `{path}`{}",
                        if truncated { " (truncated)" } else { "" }
                    );
                    // Never for a truncated read (see this fn's own doc on the SKILL.md hook) --
                    // `bytes` is only the exact content actually shown when `!truncated`.
                    if !truncated {
                        maybe_mark_skill_used(workspace_root, path, &bytes).await;
                    }
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
            // A path landing exactly on `.hive/skills/<id>/SKILL.md` goes through the skill
            // store instead of a raw filesystem write, so ADR-027's create-only/validation/count
            // guarantees actually apply to it (see `write_new_skill_tool`'s doc) rather than
            // being silently bypassable through the same generic tool that writes everything
            // else in the workspace.
            if let Some(id) = skill_id_for_path(path) {
                match write_new_skill_tool(workspace_root, &id, content).await {
                    Ok(v) => (v, format!("created skill `{id}` ({} bytes)", content.len())),
                    Err(e) => (json_error(&e), format!("write_file `{path}` failed: {e}")),
                }
            } else {
                match write_file_tool(workspace_root, path, content).await {
                    Ok(v) => (v, format!("wrote `{path}` ({} bytes)", content.len())),
                    Err(e) => (json_error(&e), format!("write_file `{path}` failed: {e}")),
                }
            }
        }
        "spawn_card" => {
            let key = call.arguments.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let title = call.arguments.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let modality = call.arguments.get("modality").and_then(|v| v.as_str()).unwrap_or("");
            let inputs = call.arguments.get("inputs").and_then(|v| v.as_str()).unwrap_or("");
            let acceptance = call.arguments.get("acceptance").and_then(|v| v.as_str()).unwrap_or("");
            let required_capabilities = call
                .arguments
                .get("required_capabilities")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            match hub
                .spawn_child_card(card_id, key, title, modality, inputs, acceptance, required_capabilities)
                .await
            {
                Ok(spawned) => {
                    let summary = format!("spawn_card: created '{}' ({})", spawned.key, spawned.card_id);
                    (
                        serde_json::json!({
                            "card_id": spawned.card_id,
                            "key": spawned.key,
                            "project_id": spawned.project_id,
                        }),
                        summary,
                    )
                }
                Err(e) => {
                    let err = ToolExecError::Hub(e.to_string());
                    (json_error(&err), format!("spawn_card `{key}` failed: {err}"))
                }
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
                    let truncated = v
                        .get("truncated")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    let shown = if path.trim().is_empty() { "." } else { path };
                    (
                        v.clone(),
                        format!(
                            "listed `{shown}` ({n} entries{})",
                            if truncated { ", truncated" } else { "" }
                        ),
                    )
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
        "vault_search" => {
            let query = call
                .arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = call
                .arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32)
                .unwrap_or(10);
            match vault_name {
                None => {
                    let msg =
                        "vault_search failed: no vault is configured for this session".to_string();
                    (serde_json::json!({ "error": msg }), msg)
                }
                Some(vault) => match vault_search_tool(vault, query, limit, vault_cache).await {
                    Ok(v) => {
                        let n = v
                            .get("hits")
                            .and_then(|h| h.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);
                        (
                            v,
                            format!("searched vault `{vault}` for \"{query}\" ({n} hits)"),
                        )
                    }
                    Err(e) => (json_error(&e), format!("vault_search `{query}` failed: {e}")),
                },
            }
        }
        "vault_read" => {
            let document_id = call
                .arguments
                .get("document_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let revision = call
                .arguments
                .get("revision")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match vault_name {
                None => {
                    let msg =
                        "vault_read failed: no vault is configured for this session".to_string();
                    (serde_json::json!({ "error": msg }), msg)
                }
                Some(vault) => match vault_read_tool(vault, document_id, revision, vault_cache).await {
                    Ok(v) => (v, format!("read vault note `{document_id}`")),
                    Err(e) => (json_error(&e), format!("vault_read `{document_id}` failed: {e}")),
                },
            }
        }
        other => {
            let msg = format!(
                "unknown tool '{other}' (not one of read_file/write_file/list_dir/run_command/vault_search/vault_read/spawn_card/wait_for_child)"
            );
            (serde_json::json!({ "error": msg }), msg)
        }
    }
}

// ── The agentic loop ────────────────────────────────────────────────────────────────────────

fn system_prompt(
    task: &str,
    workspace_root: &Path,
    vault_name: Option<&str>,
    skills_block: &str,
) -> String {
    let vault_line = match vault_name {
        Some(name) => format!(
            " You also have two read-only vault tools, vault_search and vault_read, scoped to \
             one configured vault named \"{name}\" (a personal/team knowledge library) -- search \
             it, then read a specific note by the id/revision the search returned.",
        ),
        None => String::new(),
    };
    format!(
        "You are an autonomous coding agent running directly on a member's own machine (Hive, \
         ADR-024). You have real, unsandboxed access to exactly one working directory:\n\n  {}\n\n\
         Tools available: read_file, write_file, list_dir, run_command. Every path you pass to \
         them is resolved relative to that directory -- an absolute path, or one that tries to \
         escape it with '..', is rejected. run_command runs a real subprocess directly (no \
         shell): pass the program and its arguments separately, never as one shell-syntax \
         string.{vault_line}{skills_block}\n\nTask:\n{}\n\n\
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

/// What one coding session produced. Always constructed for a clean finish, a turn-limit stop, or
/// a lease-expiry stop — see this module's doc: hitting `max_turns` is reported honestly, not
/// hidden as a silent success, and the same now goes for running past the lease.
#[derive(Debug, Clone)]
pub struct CodeSessionOutcome {
    /// The brain's final text reply, or a synthesized message if `max_turns` or the lease deadline
    /// was hit first.
    pub final_text: String,
    pub turns: u32,
    pub hit_turn_limit: bool,
    /// This node's lease on the card expired before the session finished (checked once per turn
    /// boundary, same defense-in-depth reasoning as `worker.rs`'s `run_card`: the hub's own lease
    /// housekeeping is the real enforcement, this just stops the session volunteering more turns
    /// past a lease it may no longer hold). `crate::worker`'s `run_code_card` releases the card
    /// instead of completing it when this is true.
    pub lease_expired: bool,
    /// ADR-032: `wait_for_child` succeeded mid-session -- this node released its lease on this
    /// card (via `Hub::wait_on_child`) to wait on the named child, and the loop below stopped
    /// itself immediately rather than starting another turn against a lease it no longer holds.
    /// `crate::worker`'s `run_code_card` must not complete/release/fail the card when this is
    /// `Some` -- the DB-side state transition (lease released, status set to
    /// `waiting_on_child`) already happened inside this function, not in the caller.
    pub waiting_on_child: Option<Uuid>,
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
/// with `hit_turn_limit: true`. Running past `lease_expires_at` is the same: a normal, honestly
/// reported outcome (`lease_expired: true`), not an `Err` (2026-09-14, ADR-029 review finding —
/// this session previously had no lease awareness at all; see `CodeSessionOutcome::lease_expired`).
pub async fn run_session(
    hub: &dyn Hub,
    data_dir: &Path,
    card_id: Uuid,
    spec: &CodeSessionSpec,
    brain: &dyn CodeBrain,
    lease_expires_at: chrono::DateTime<chrono::Utc>,
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

    let tools = tool_specs(spec.coordinator);
    let skills_block = skills_prompt_block(&workspace_root);
    let mut messages = vec![
        BrainMessage::system(system_prompt(
            &spec.task,
            &workspace_root,
            spec.vault_name.as_deref(),
            &skills_block,
        )),
        BrainMessage::user(spec.task.clone()),
    ];

    // Owned by this session, reused across every vault_search/vault_read this session makes --
    // see `execute_tool`'s doc and `open_vault_reader`'s doc (Sif's efficiency audit, finding 2,
    // 2026-09-15).
    let vault_cache: VaultReaderCache = Default::default();

    let mut turns = 0u32;
    let outcome = 'turns: loop {
        if turns >= spec.max_turns {
            break CodeSessionOutcome {
                final_text: format!(
                    "session stopped after {} turns without the brain declaring the task done",
                    spec.max_turns
                ),
                turns,
                hit_turn_limit: true,
                lease_expired: false,
                waiting_on_child: None,
            };
        }
        // Defense-in-depth (2026-09-14, same finding as `worker.rs`'s `run_card`): a coding
        // session has no checkpoint/resume machinery of its own (ADR-024 decision 5 -- one
        // continuous session inside one lease), so without this check it would happily keep
        // calling the brain and running tools past a lease the hub may have already reassigned.
        // Checked at the top of every turn, and again before each individual tool call within
        // a turn's batch (see the `'turns` loop below) -- but a turn already in flight (an
        // in-progress `brain.next_turn` call, or a tool call already started) is never
        // interrupted mid-call; that would need a real cancellation token, not attempted here.
        if chrono::Utc::now() >= lease_expires_at {
            tracing::warn!(card = %card_id, turns,
                "lease expired mid-session; stopping rather than starting another turn");
            break CodeSessionOutcome {
                final_text: format!(
                    "session stopped after {turns} turns: this node's lease on the card expired"
                ),
                turns,
                hit_turn_limit: false,
                lease_expired: true,
                waiting_on_child: None,
            };
        }
        turns += 1;
        match brain.next_turn(&messages, &tools).await {
            Ok(BrainTurn::Text(text)) => {
                break CodeSessionOutcome {
                    final_text: text,
                    turns,
                    hit_turn_limit: false,
                    lease_expired: false,
                    waiting_on_child: None,
                };
            }
            Ok(BrainTurn::ToolCalls(calls)) => {
                messages.push(BrainMessage::assistant_tool_calls(calls.clone()));
                for call in &calls {
                    // Re-check lease authority before *each* tool call in this batch, not only
                    // once per turn boundary above -- a turn's tool-call batch can contain
                    // several calls, and the once-per-turn check only caught expiry between
                    // turns, so a response that arrived (or a batch that ran long) after expiry
                    // could still execute every remaining side-effecting call in it (Sif's
                    // efficiency audit, finding 4, 2026-09-15). This still can't interrupt a call
                    // already in flight -- only a real cancellation token threaded into
                    // `execute_tool`/`brain.next_turn` itself could do that, a larger change not
                    // attempted tonight -- but it does stop a many-call batch from running past
                    // the deadline once it's noticed.
                    if chrono::Utc::now() >= lease_expires_at {
                        tracing::warn!(card = %card_id, turns,
                            "lease expired mid-batch; stopping before this tool call rather than running it");
                        break 'turns CodeSessionOutcome {
                            final_text: format!(
                                "session stopped after {turns} turns: this node's lease on the card expired mid-batch"
                            ),
                            turns,
                            hit_turn_limit: false,
                            lease_expired: true,
                            waiting_on_child: None,
                        };
                    }
                    // ADR-032: `wait_for_child` is not an ordinary tool -- a successful call
                    // releases this node's lease on `card_id` right now (`Hub::wait_on_child`),
                    // so nothing after it (further calls in this same batch, another brain turn)
                    // may run. Handled here, before the generic `execute_tool` dispatch below,
                    // rather than as one more arm inside it, because it's the one tool whose
                    // effect is "stop the session," not "produce a result and keep going."
                    if call.name == "wait_for_child" {
                        let child_str = call
                            .arguments
                            .get("child_card_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let parsed = Uuid::parse_str(child_str);
                        match parsed {
                            Ok(child_id) => match hub.wait_on_child(card_id, child_id).await {
                                Ok(_) => {
                                    let final_text = format!(
                                        "paused after {turns} turns to wait on child card {child_id}"
                                    );
                                    post_event(
                                        hub,
                                        "code_session_waiting_on_child",
                                        &final_text,
                                        serde_json::json!({ "card_id": card_id, "child_card_id": child_id, "turns": turns }),
                                    )
                                    .await;
                                    return Ok(CodeSessionOutcome {
                                        final_text,
                                        turns,
                                        hit_turn_limit: false,
                                        lease_expired: false,
                                        waiting_on_child: Some(child_id),
                                    });
                                }
                                Err(e) => {
                                    // Couldn't actually pause (bad/unrelated child id, lease
                                    // already gone, etc.) -- report it as a normal failed tool
                                    // call and let the brain keep going, exactly like any other
                                    // tool error. The lease is still held; nothing to unwind.
                                    let err = ToolExecError::Hub(e.to_string());
                                    let summary = format!("wait_for_child failed: {err}");
                                    tracing::info!(card = %card_id, tool = %call.name, "code session tool ran: {summary}");
                                    messages.push(BrainMessage::tool_result(
                                        call.id.clone(),
                                        json_error(&err).to_string(),
                                    ));
                                    post_event(
                                        hub,
                                        "code_session_tool",
                                        &summary,
                                        serde_json::json!({ "card_id": card_id, "tool": call.name }),
                                    )
                                    .await;
                                }
                            },
                            Err(_) => {
                                let msg = format!(
                                    "wait_for_child failed: '{child_str}' is not a valid card id"
                                );
                                tracing::info!(card = %card_id, tool = %call.name, "code session tool ran: {msg}");
                                messages.push(BrainMessage::tool_result(
                                    call.id.clone(),
                                    serde_json::json!({ "error": msg }).to_string(),
                                ));
                                post_event(
                                    hub,
                                    "code_session_tool",
                                    &msg,
                                    serde_json::json!({ "card_id": card_id, "tool": call.name }),
                                )
                                .await;
                            }
                        }
                        continue;
                    }
                    let (result_value, summary) = execute_tool(
                        &workspace_root,
                        call,
                        spec.vault_name.as_deref(),
                        hub,
                        card_id,
                        &vault_cache,
                    )
                    .await;
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
    use crate::hub::{Claim, Completion, HubError, McpServerConfig, SpawnedCard};
    use crate::Capabilities;

    fn root() -> PathBuf {
        PathBuf::from("/data/workspace")
    }

    /// Every method `unimplemented!()` except `post_activity` (which `post_event` -- called
    /// unconditionally throughout `run_session` -- must not panic on). Good enough for any test
    /// that needs *some* `&dyn Hub` to satisfy `execute_tool`'s/`run_session`'s signature but
    /// never actually exercises a hub call (e.g. the vault-tool tests below, which fail before
    /// touching the hub at all).
    struct NoopHub;
    #[async_trait::async_trait]
    impl Hub for NoopHub {
        async fn claim_card(&self) -> Result<Claim, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn complete_card(
            &self,
            _card_id: Uuid,
            _content: &str,
            _model_id: Option<&str>,
            _usage: crate::ledger::Usage,
        ) -> Result<Completion, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn checkpoint(
            &self,
            _card_id: Uuid,
            _step: u32,
            _state: &serde_json::Value,
            _usage: crate::ledger::Usage,
        ) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn fail_card(&self, _card_id: Uuid, _reason: &str) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn release_card(&self, _card_id: Uuid, _reason: &str) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn spawn_child_card(
            &self,
            _parent_card_id: Uuid,
            _key: &str,
            _title: &str,
            _modality: &str,
            _inputs: &str,
            _acceptance: &str,
            _required_capabilities: serde_json::Value,
        ) -> Result<SpawnedCard, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn wait_on_child(&self, _card_id: Uuid, _child_card_id: Uuid) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn mcp_server_config(&self, _server_id: Uuid) -> Result<McpServerConfig, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn check_in(&self, _caps: &Capabilities, _region: Option<&str>) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn heartbeat(&self, _prev_rtt_ms: Option<u64>) -> Result<(String, u64), HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn check_out(&self) -> Result<String, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn get_schedule(&self) -> Result<Option<serde_json::Value>, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn post_activity(
            &self,
            _event_type: &str,
            _body: &str,
            _payload: serde_json::Value,
        ) -> Result<(), HubError> {
            Ok(())
        }
    }

    /// `NoopHub` plus a working `spawn_child_card`/`wait_on_child` pair -- for ADR-032's
    /// coordinator tools. `spawn_child_card` always returns a card under the fixed
    /// `CHILD_CARD_ID`/`CHILD_PROJECT_ID` constants below (no real project/lease bookkeeping --
    /// this is testing coder.rs's tool wiring, not `hive.spawn_child_card`'s own RPC logic,
    /// which already has its own SQL-level coverage). `wait_on_child` succeeds only for that same
    /// child id, mirroring the real RPC's `not_a_child_of_this_card` check closely enough for
    /// this module's own tests.
    struct SpawningHub;
    const CHILD_CARD_ID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_00000000c41d);
    const CHILD_PROJECT_ID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_0000000091ec);
    #[async_trait::async_trait]
    impl Hub for SpawningHub {
        async fn claim_card(&self) -> Result<Claim, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn complete_card(
            &self,
            _card_id: Uuid,
            _content: &str,
            _model_id: Option<&str>,
            _usage: crate::ledger::Usage,
        ) -> Result<Completion, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn checkpoint(
            &self,
            _card_id: Uuid,
            _step: u32,
            _state: &serde_json::Value,
            _usage: crate::ledger::Usage,
        ) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn fail_card(&self, _card_id: Uuid, _reason: &str) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn release_card(&self, _card_id: Uuid, _reason: &str) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn spawn_child_card(
            &self,
            _parent_card_id: Uuid,
            key: &str,
            _title: &str,
            _modality: &str,
            _inputs: &str,
            _acceptance: &str,
            _required_capabilities: serde_json::Value,
        ) -> Result<SpawnedCard, HubError> {
            Ok(SpawnedCard {
                card_id: CHILD_CARD_ID,
                key: key.to_string(),
                project_id: CHILD_PROJECT_ID,
                requires_internet: false,
            })
        }
        async fn wait_on_child(&self, _card_id: Uuid, child_card_id: Uuid) -> Result<serde_json::Value, HubError> {
            if child_card_id == CHILD_CARD_ID {
                Ok(serde_json::json!({ "status": "waiting_on_child" }))
            } else {
                Err(HubError::Rejected("not_a_child_of_this_card".into()))
            }
        }
        async fn mcp_server_config(&self, _server_id: Uuid) -> Result<McpServerConfig, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn check_in(&self, _caps: &Capabilities, _region: Option<&str>) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn heartbeat(&self, _prev_rtt_ms: Option<u64>) -> Result<(String, u64), HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn check_out(&self) -> Result<String, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn get_schedule(&self) -> Result<Option<serde_json::Value>, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn post_activity(
            &self,
            _event_type: &str,
            _body: &str,
            _payload: serde_json::Value,
        ) -> Result<(), HubError> {
            Ok(())
        }
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
        let (read, bytes) = read_file_tool(&dir, "sub/hello.txt").await.unwrap();
        assert_eq!(
            read.get("content").and_then(|v| v.as_str()),
            Some("hi there")
        );
        assert_eq!(bytes, b"hi there");
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[test]
    fn tool_specs_always_advertises_vault_tools() {
        // Advertised unconditionally (see this module's doc): a brain should be able to discover
        // vault_search/vault_read without a round-trip, even before this session's own
        // `vault_name` (or the `local-hub` feature) is known to be configured.
        let names: Vec<String> = tool_specs(false).into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"vault_search".to_string()));
        assert!(names.contains(&"vault_read".to_string()));
    }

    #[tokio::test]
    async fn execute_tool_vault_search_without_a_configured_vault_is_a_clean_error() {
        let dir = std::env::temp_dir().join(format!("ohhive-coder-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let call = BrainToolCall {
            id: "call-1".into(),
            name: "vault_search".into(),
            arguments: serde_json::json!({ "query": "anything" }),
        };
        let vault_cache = VaultReaderCache::default();
        let (value, summary) = execute_tool(&dir, &call, None, &NoopHub, Uuid::nil(), &vault_cache).await;
        assert!(value.get("error").is_some());
        assert!(summary.contains("no vault is configured"));
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn execute_tool_vault_read_without_a_configured_vault_is_a_clean_error() {
        let dir = std::env::temp_dir().join(format!("ohhive-coder-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let call = BrainToolCall {
            id: "call-1".into(),
            name: "vault_read".into(),
            arguments: serde_json::json!({ "document_id": "x", "revision": "y" }),
        };
        let vault_cache = VaultReaderCache::default();
        let (value, summary) = execute_tool(&dir, &call, None, &NoopHub, Uuid::nil(), &vault_cache).await;
        assert!(value.get("error").is_some());
        assert!(summary.contains("no vault is configured"));
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[test]
    fn tool_specs_advertises_coordinator_tools_only_when_flagged() {
        let plain: Vec<String> = tool_specs(false).into_iter().map(|t| t.name).collect();
        assert!(!plain.contains(&"spawn_card".to_string()));
        assert!(!plain.contains(&"wait_for_child".to_string()));

        let coord: Vec<String> = tool_specs(true).into_iter().map(|t| t.name).collect();
        assert!(coord.contains(&"spawn_card".to_string()));
        assert!(coord.contains(&"wait_for_child".to_string()));
        // Coordinator sessions still get the ordinary four plus vault -- ADR-032 only adds, it
        // doesn't take anything away.
        assert!(coord.contains(&"read_file".to_string()));
        assert!(coord.contains(&"vault_search".to_string()));
    }

    #[tokio::test]
    async fn execute_tool_spawn_card_reaches_the_hub() {
        let dir = std::env::temp_dir().join(format!("ohhive-coder-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let call = BrainToolCall {
            id: "call-1".into(),
            name: "spawn_card".into(),
            arguments: serde_json::json!({
                "key": "child1",
                "title": "do the sub-task",
                "modality": "code",
                "inputs": "fix the thing",
            }),
        };
        let vault_cache = VaultReaderCache::default();
        let (value, summary) =
            execute_tool(&dir, &call, None, &SpawningHub, Uuid::nil(), &vault_cache).await;
        assert_eq!(
            value.get("card_id").and_then(|v| v.as_str()),
            Some(CHILD_CARD_ID.to_string().as_str())
        );
        assert_eq!(value.get("key").and_then(|v| v.as_str()), Some("child1"));
        assert!(summary.contains("child1"));
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn run_session_wait_for_child_pauses_without_another_turn() {
        // Turn 1: the brain spawns a child. Turn 2: it asks to wait on the exact id
        // `SpawningHub::spawn_child_card` just returned. The session must stop right there --
        // regression coverage for the ADR-032 control-flow addition (early `return Ok(..)` from
        // inside `run_session`'s loop, not just another `execute_tool` arm).
        struct SpawnThenWait(std::sync::atomic::AtomicU32);
        #[async_trait::async_trait]
        impl CodeBrain for SpawnThenWait {
            async fn next_turn(
                &self,
                _messages: &[BrainMessage],
                _tools: &[ToolSpec],
            ) -> std::result::Result<BrainTurn, CodeBrainError> {
                let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n == 0 {
                    Ok(BrainTurn::ToolCalls(vec![BrainToolCall {
                        id: "1".into(),
                        name: "spawn_card".into(),
                        arguments: serde_json::json!({
                            "key": "child1", "title": "t", "modality": "code", "inputs": "do x"
                        }),
                    }]))
                } else {
                    Ok(BrainTurn::ToolCalls(vec![BrainToolCall {
                        id: "2".into(),
                        name: "wait_for_child".into(),
                        arguments: serde_json::json!({ "child_card_id": CHILD_CARD_ID.to_string() }),
                    }]))
                }
            }
        }

        let dir = std::env::temp_dir().join(format!("ohhive-coder-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let spec = CodeSessionSpec {
            task: "coordinate".into(),
            workspace_path: Some(dir.to_string_lossy().into()),
            repo_url: None,
            repo_ref: None,
            brain: "local".into(),
            model_id: None,
            max_turns: 5,
            vault_name: None,
            coordinator: true,
        };
        let brain = SpawnThenWait(std::sync::atomic::AtomicU32::new(0));
        let result = run_session(
            &SpawningHub,
            &dir,
            Uuid::nil(),
            &spec,
            &brain,
            chrono::Utc::now() + chrono::Duration::hours(1),
        )
        .await
        .unwrap();
        assert_eq!(result.waiting_on_child, Some(CHILD_CARD_ID));
        assert_eq!(result.turns, 2);
        assert!(!result.hit_turn_limit);
        assert!(!result.lease_expired);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Same 13-method shape as `SpawningHub` above, except `spawn_child_card` sleeps 50ms before
    /// returning -- gives `run_session_mid_batch_lease_expiry_stops_before_the_next_tool_call`
    /// below a real await point to put the lease deadline in the middle of, without racing the
    /// system clock.
    struct SlowThenNoopHub;
    #[async_trait::async_trait]
    impl Hub for SlowThenNoopHub {
        async fn claim_card(&self) -> Result<Claim, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn complete_card(
            &self,
            _card_id: Uuid,
            _content: &str,
            _model_id: Option<&str>,
            _usage: crate::ledger::Usage,
        ) -> Result<Completion, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn checkpoint(
            &self,
            _card_id: Uuid,
            _step: u32,
            _state: &serde_json::Value,
            _usage: crate::ledger::Usage,
        ) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn fail_card(&self, _card_id: Uuid, _reason: &str) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn release_card(&self, _card_id: Uuid, _reason: &str) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn spawn_child_card(
            &self,
            _parent_card_id: Uuid,
            key: &str,
            _title: &str,
            _modality: &str,
            _inputs: &str,
            _acceptance: &str,
            _required_capabilities: serde_json::Value,
        ) -> Result<SpawnedCard, HubError> {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(SpawnedCard {
                card_id: CHILD_CARD_ID,
                key: key.to_string(),
                project_id: CHILD_PROJECT_ID,
                requires_internet: false,
            })
        }
        async fn wait_on_child(&self, _card_id: Uuid, _child_card_id: Uuid) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn mcp_server_config(&self, _server_id: Uuid) -> Result<McpServerConfig, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn check_in(&self, _caps: &Capabilities, _region: Option<&str>) -> Result<serde_json::Value, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn heartbeat(&self, _prev_rtt_ms: Option<u64>) -> Result<(String, u64), HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn check_out(&self) -> Result<String, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn get_schedule(&self) -> Result<Option<serde_json::Value>, HubError> {
            unimplemented!("not exercised by this test")
        }
        async fn post_activity(
            &self,
            _event_type: &str,
            _body: &str,
            _payload: serde_json::Value,
        ) -> Result<(), HubError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn run_session_mid_batch_lease_expiry_stops_before_the_next_tool_call() {
        // One turn, one batch, two `spawn_card` calls. The lease is still valid when the turn
        // starts (so the once-per-turn check above doesn't catch it) but expires while the
        // batch's first call is in flight (`SlowThenNoopHub::spawn_child_card` sleeps 50ms) --
        // the second call must never run. Regression coverage for the mid-batch recheck (Sif's
        // efficiency audit, finding 4, 2026-09-15).
        struct TwoSpawnsOneTurn;
        #[async_trait::async_trait]
        impl CodeBrain for TwoSpawnsOneTurn {
            async fn next_turn(
                &self,
                _messages: &[BrainMessage],
                _tools: &[ToolSpec],
            ) -> std::result::Result<BrainTurn, CodeBrainError> {
                Ok(BrainTurn::ToolCalls(vec![
                    BrainToolCall {
                        id: "1".into(),
                        name: "spawn_card".into(),
                        arguments: serde_json::json!({
                            "key": "child1", "title": "t", "modality": "code", "inputs": "do x"
                        }),
                    },
                    BrainToolCall {
                        id: "2".into(),
                        name: "spawn_card".into(),
                        arguments: serde_json::json!({
                            "key": "child2", "title": "t", "modality": "code", "inputs": "do y"
                        }),
                    },
                ]))
            }
        }

        let dir = std::env::temp_dir().join(format!("ohhive-coder-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let spec = CodeSessionSpec {
            task: "coordinate".into(),
            workspace_path: Some(dir.to_string_lossy().into()),
            repo_url: None,
            repo_ref: None,
            brain: "local".into(),
            model_id: None,
            max_turns: 5,
            vault_name: None,
            coordinator: true,
        };
        let result = run_session(
            &SlowThenNoopHub,
            &dir,
            Uuid::nil(),
            &spec,
            &TwoSpawnsOneTurn,
            chrono::Utc::now() + chrono::Duration::milliseconds(20),
        )
        .await
        .unwrap();
        assert!(result.lease_expired);
        assert_eq!(result.turns, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "local-hub")]
    #[test]
    fn find_vault_by_name_matches_exact_name_only() {
        let vaults = vec![
            crate::local_hub::vault::VaultInfo {
                id: Uuid::new_v4(),
                name: "Recipes".into(),
                state: "ready".into(),
            },
            crate::local_hub::vault::VaultInfo {
                id: Uuid::new_v4(),
                name: "recipes".into(),
                state: "ready".into(),
            },
        ];
        let hit = find_vault_by_name(&vaults, "Recipes").expect("exact match");
        assert_eq!(hit.name, "Recipes");
        assert!(find_vault_by_name(&vaults, "Recipe").is_none());
        assert!(find_vault_by_name(&vaults, "nope").is_none());
    }

    #[cfg(feature = "local-hub")]
    #[test]
    fn resolve_vault_prefers_id_and_rejects_ambiguous_names() {
        let store = crate::local_hub::LocalHubStore::in_memory().unwrap();
        let creds = store.enroll_owner("reader").unwrap();
        let reader = store.connect(&creds.raw_key).unwrap();
        let first = store.vault_create("Notes").unwrap();
        let second = store.vault_create("Notes").unwrap();
        let third = store.vault_create("Recipes").unwrap();
        for v in [first, second, third] {
            store.vault_grant(v, creds.node_id, true).unwrap();
        }
        // Unique name still resolves the ordinary way.
        assert_eq!(resolve_vault(&reader, "Recipes").unwrap(), third);
        // A configured id resolves directly, even though its name collides with another vault.
        assert_eq!(resolve_vault(&reader, &first.to_string()).unwrap(), first);
        assert_eq!(resolve_vault(&reader, &second.to_string()).unwrap(), second);
        // The ambiguous name itself is rejected, not silently resolved to "whichever's first".
        let err = resolve_vault(&reader, "Notes").unwrap_err();
        assert!(matches!(err, ToolExecError::Vault(msg) if msg.contains("matches 2 vaults")));
        // An id that doesn't match any vault falls through to the ordinary "no vault named"
        // error rather than a confusing id-shaped failure.
        assert!(resolve_vault(&reader, &Uuid::new_v4().to_string()).is_err());
    }
}
