//! Agent tool surface (ADR-006 D45; the "Open questions" v1 WASI tool list).
//!
//! Five tools, all real now: `exec_wasm` (run a WASI Preview 2 component
//! through [`crate::sandbox::Sandbox`]), `artifact_get`/`artifact_put`
//! (fetch/store bytes through a regional server, via [`crate::hub::HubClient`]),
//! `spawn_child_card` (create a sibling card, ADR-006 D44), and `mcp_tool_call`
//! (call a tool on a member-configured MCP server, #177/ADR-023). A card opts
//! into each independently via `required_capabilities`:
//!
//! Plus one whole-card tool, [`run_code_session`], for `modality = 'code'` cards (ADR-024,
//! #185): unlike the five above, which each run once in the fixed pre-`Draft` Act→Observe pass
//! `worker.rs`'s `maybe_run_tool` describes, a coding session's multi-turn tool-calling loop *is*
//! the entire card — see [`run_code_session`]'s own doc and [`crate::coder`]'s module doc for the
//! full picture.
//!
//! - `exec_wasm: true` — run `component_path_for(data_dir, card.id)`.
//! - `artifact_get_hash: "<sha256>"` — fetch that artifact and stage it at
//!   `/in/<hash>` (read-only) for the `exec_wasm` call that follows.
//! - `artifact_put: true` — after `exec_wasm` runs, if it wrote a file at
//!   `./out` in its scratch dir, upload that file as a new artifact.
//! - `spawn_child` — `{"key", "title", "modality", "inputs"}` — create one
//!   child card in the same project, once, after the above.
//! - `mcp_server_id` (uuid) + `mcp_tool_name` (string) + optional
//!   `mcp_tool_args` (object) — call one tool on one of the *requesting
//!   member's own* configured MCP servers (`hive.member_mcp_servers`). Unlike
//!   every other tool here, this is not sandboxed by Hive at all — it is the
//!   member's own subprocess, on their own hardware, running as their own OS
//!   user (see [`crate::mcp`]'s module doc and ADR-023 for the full trust
//!   model). `hive.node_claim_card` only ever leases a card naming this to a
//!   node the requesting member owns, with `tools_level = 'sandboxed_tools'`,
//!   for a server that member explicitly enabled — [`run_mcp_tool_call`]
//!   re-checks ownership/enabled a second time at run time via
//!   `hive_member_mcp_server_get_node`.
//!
//! Every value here is host-trusted card data set when the card was created,
//! never something a running model can invent or redirect at runtime — the
//! same "nothing the model says can widen policy" property the sandbox
//! itself relies on (ADR-006's mitigation for prompt injection via tool
//! calls). `worker.rs`'s `maybe_run_tool` runs these in the fixed order
//! above, once, before `Draft` — not the full multi-turn ReAct loop ADR-006's
//! step machine describes (a card can't yet ask for a *second* `exec_wasm`
//! call, or a second MCP tool call, mid-run); see `worker.rs`'s own module
//! doc for what that would take.
//!
//! `spawn_child` may set `"wait": true` (ADR-006 D44 sub-delegation) to pause the parent
//! on the child it just created: `worker.rs` releases the lease and marks the card
//! `waiting_on_child` instead of running `Draft` immediately, and a DB trigger resumes it
//! once the child finishes (or cascades a failure up if the child fails) — see `worker.rs`'s
//! module doc for the resume side. Omitting `wait` (or setting it `false`) keeps the old
//! fire-and-forget behavior: the child is created and the parent carries straight on.

use crate::capability::ToolsLevel;
use crate::sandbox::{
    inputs_dir_for, scratch_dir_for, NetPolicy, Sandbox, SandboxError, SandboxLimits,
};
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub ok: bool,
    /// Short human-readable summary, meant to be folded into the agent loop's
    /// context for the next inference step.
    pub summary: String,
    /// Structured data a caller needs beyond the summary text. `run_spawn_child_card` returns the
    /// new card's id and key here so `worker.rs` can act on them (block on `id`, look up output
    /// by `key`) without parsing `summary`; `run_mcp_tool_call` returns the MCP tool's raw
    /// (untruncated) JSON result here, since `summary` only carries a truncated preview of it.
    /// `None` for every other tool, and for a failed call of either of the two above.
    pub data: Option<serde_json::Value>,
}

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("card {0} requested exec_wasm but no component is staged at {1}")]
    NotStaged(Uuid, PathBuf),
    #[error("local filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[cfg(feature = "hub")]
    #[error(transparent)]
    Hub(#[from] crate::hub::HubError),
    #[error(transparent)]
    Sandbox(#[from] SandboxError),
}

/// Where a staged `exec_wasm` component lives for a given card, under the
/// node's data directory. Deterministic and card-scoped — see the module
/// doc for why that matters.
pub fn component_path_for(data_dir: &Path, card_id: &Uuid) -> PathBuf {
    data_dir
        .join("tool-components")
        .join(format!("{card_id}.wasm"))
}

/// Fetch an artifact by hash and stage it for the `exec_wasm` call that
/// follows, at `/in/<hash>` (read-only — see [`Sandbox::run`]'s `inputs_dir`).
/// A card opts in via `required_capabilities.artifact_get_hash`.
#[cfg(feature = "hub")]
pub async fn run_artifact_get(
    hub: &dyn crate::hub::Hub,
    data_dir: &Path,
    card_id: Uuid,
    hash: &str,
) -> Result<ToolOutcome, ToolError> {
    let community = hub.community_client().ok_or_else(|| {
        crate::hub::HubError::Rejected(
            "community artifact access is unavailable on a fully local hub".into(),
        )
    })?;
    let (bytes, _mime) = community.artifact_fetch(hash).await?;
    let dir = inputs_dir_for(data_dir, &card_id.to_string());
    std::fs::create_dir_all(&dir)?;
    let len = bytes.len();
    std::fs::write(dir.join(hash), bytes)?;
    Ok(ToolOutcome {
        ok: true,
        summary: format!("artifact_get: fetched {len} bytes for {hash}, staged at /in/{hash}"),
        data: None,
    })
}

/// Run `exec_wasm` for one card. `inputs_dir` should be `Some` exactly when
/// [`run_artifact_get`] staged something this call needs to see at `/in`.
/// Callers should only invoke this for cards that actually set
/// `required_capabilities.exec_wasm = true`; it does not check that flag
/// itself (the caller already knows why it's calling this).
pub async fn run_exec_wasm(
    sandbox: &Sandbox,
    data_dir: &Path,
    card_id: Uuid,
    inputs_dir: Option<&Path>,
    tools_level: ToolsLevel,
    net: NetPolicy,
) -> Result<ToolOutcome, ToolError> {
    let component = component_path_for(data_dir, &card_id);
    if !component.exists() {
        return Err(ToolError::NotStaged(card_id, component));
    }
    let scratch = scratch_dir_for(data_dir, &card_id.to_string());
    let lease_id = card_id.to_string();
    match sandbox
        .run(
            &component,
            &scratch,
            inputs_dir,
            tools_level,
            net,
            SandboxLimits::default(),
            &lease_id,
        )
        .await
    {
        Ok(()) => Ok(ToolOutcome {
            ok: true,
            summary: "exec_wasm completed successfully".to_string(),
            data: None,
        }),
        // A trapped/failed/refused tool is a normal *outcome* to feed back to the
        // model (it can say so in the draft, or the reviewer can catch it) — not
        // a reason to fail the whole card. Only a bug in calling the sandbox
        // itself (there isn't one, once `component.exists()` holds) would.
        Err(e) => Ok(ToolOutcome {
            ok: false,
            summary: format!("exec_wasm failed: {e}"),
            data: None,
        }),
    }
}

/// After `exec_wasm` ran, upload whatever it wrote to `./out` in its scratch
/// dir as a new artifact. A card opts in via `required_capabilities.artifact_put`.
/// If `exec_wasm` didn't write `./out` (didn't run at all, or ran but produced
/// nothing there), this is a normal non-error outcome — not every tool run
/// produces an artifact.
#[cfg(feature = "hub")]
pub async fn run_artifact_put(
    hub: &dyn crate::hub::Hub,
    data_dir: &Path,
    card_id: Uuid,
    project_id: Option<Uuid>,
) -> Result<ToolOutcome, ToolError> {
    let out = scratch_dir_for(data_dir, &card_id.to_string()).join("out");
    if !out.exists() {
        return Ok(ToolOutcome {
            ok: false,
            summary: "artifact_put: exec_wasm did not write ./out — nothing to upload".to_string(),
            data: None,
        });
    }
    let bytes = std::fs::read(&out)?;
    let community = hub.community_client().ok_or_else(|| {
        crate::hub::HubError::Rejected(
            "community artifact upload is unavailable on a fully local hub".into(),
        )
    })?;
    let reply = community
        .artifact_upload(
            bytes,
            "application/octet-stream",
            "output",
            project_id,
            Some(card_id),
        )
        .await?;
    let hash = reply.get("hash").and_then(|v| v.as_str()).unwrap_or("?");
    Ok(ToolOutcome {
        ok: true,
        summary: format!("artifact_put: uploaded ./out as artifact {hash}"),
        data: None,
    })
}

/// One child card to create, as declared in a parent card's
/// `required_capabilities.spawn_child`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SpawnChildSpec {
    pub key: String,
    pub title: String,
    pub modality: String,
    pub inputs: String,
    #[serde(default)]
    pub acceptance: String,
    #[serde(default)]
    pub required_capabilities: serde_json::Value,
    /// ADR-006 D44: pause the parent on this child instead of firing-and-forgetting it.
    /// See the module doc and `worker.rs`'s module doc for the block/resume mechanics.
    #[serde(default)]
    pub wait: bool,
}

/// Create one child card under `parent_card_id` (ADR-006 D44). Card-creation only — whether
/// the parent then pauses for it is `worker.rs`'s call, driven by `spec.wait`; this function
/// always just creates the card and returns, putting the new card's id/key in
/// [`ToolOutcome::data`] as `{"card_id", "key"}` so the caller can act on `spec.wait` without
/// re-parsing `summary`.
#[cfg(feature = "hub")]
pub async fn run_spawn_child_card(
    hub: &dyn crate::hub::Hub,
    parent_card_id: Uuid,
    spec: &SpawnChildSpec,
) -> Result<ToolOutcome, ToolError> {
    let spawned = hub
        .spawn_child_card(
            parent_card_id,
            &spec.key,
            &spec.title,
            &spec.modality,
            &spec.inputs,
            &spec.acceptance,
            spec.required_capabilities.clone(),
        )
        .await?;
    Ok(ToolOutcome {
        ok: true,
        summary: format!(
            "spawn_child_card: created '{}' ({}) in project {}",
            spawned.key, spawned.card_id, spawned.project_id
        ),
        data: Some(serde_json::json!({ "card_id": spawned.card_id, "key": spawned.key })),
    })
}

/// Call one tool on a member-configured MCP server (#177, ADR-023). A card opts in via
/// `required_capabilities.mcp_server_id` (uuid) + `mcp_tool_name` (string) + optional
/// `mcp_tool_args` (object, defaults to `{}`) — see this module's doc for the full contract.
///
/// This is **not sandboxed** the way `run_exec_wasm` is (see [`crate::mcp`]'s module doc) — the
/// spawned process has whatever access the member who configured `server_id` gave it on their own
/// machine. Callers should only invoke this for cards that actually set
/// `required_capabilities.mcp_server_id` on a node with `tools_level = SandboxedTools`, same
/// convention as `run_exec_wasm`; this function's own defense-in-depth is the `hub` round-trip
/// below, which re-resolves and re-checks ownership + `enabled` server-side regardless of what the
/// caller already believes about the card.
///
/// A failed/misconfigured/timed-out MCP call is a normal *outcome* to feed back to the model
/// (`ok: false`, summarized) — not a reason to fail the whole card, same reasoning `run_exec_wasm`
/// documents for a trapped tool. Only a failure to even *fetch* the server's config (bad node key,
/// hub unreachable, server not found/owned/enabled) propagates as a real [`ToolError`], via the
/// existing `#[from] HubError` conversion.
#[cfg(feature = "hub")]
pub async fn run_mcp_tool_call(
    hub: &dyn crate::hub::Hub,
    server_id: Uuid,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<ToolOutcome, ToolError> {
    let hub_config = hub.mcp_server_config(server_id).await?;
    let config = crate::mcp::McpServerConfig {
        name: hub_config.name,
        transport: hub_config.transport,
        command: hub_config.command,
        args: hub_config.args,
        env: hub_config.env,
    };
    let server_name = config.name.clone();
    match crate::mcp::McpSession::call_one_shot(&config, tool_name, arguments).await {
        Ok(result) => Ok(ToolOutcome {
            ok: true,
            summary: format!(
                "mcp_tool_call: '{tool_name}' on server '{server_name}' returned: {}",
                truncate_for_summary(&result.to_string())
            ),
            data: Some(result),
        }),
        Err(e) => Ok(ToolOutcome {
            ok: false,
            summary: format!("mcp_tool_call: '{tool_name}' on server '{server_name}' failed: {e}"),
            data: None,
        }),
    }
}

/// Run one `code`-modality coding-agent session to completion (ADR-024, #185) — a fundamentally
/// different control flow from every other tool here: not one Act→Observe pass before `Draft`,
/// but the *entire* card, a multi-turn tool-calling loop that runs until the brain declares
/// itself done or `required_capabilities.max_turns` is hit (`crate::coder::run_session`).
/// `worker.rs`'s `run_code_card` calls this once, in place of the Draft/Critique/Revise state
/// machine every other modality runs — see that function's own doc for why.
///
/// Same "a failed run is a normal outcome, not a propagated panic" convention every other
/// function in this module documents: `crate::coder::CodeSessionSpec` parse failures, workspace-prep
/// failures, and a brain erroring mid-session all come back as `Ok(ToolOutcome { ok: false, .. })`
/// with a clear summary, never as a propagated panic. This function should essentially never
/// return `Err` in practice — it exists so this function's signature matches every sibling tool
/// function's shape, not because a real failure mode is expected to surface through it.
/// `ToolOutcome::data` carries `{"turns", "hit_turn_limit", "lease_expired", "waiting_on_child"}`
/// (the last, ADR-032, a card id or null) so `worker.rs` can
/// log/report those without re-parsing `summary`.
#[cfg(feature = "hub")]
pub async fn run_code_session(
    hub: &dyn crate::hub::Hub,
    data_dir: &Path,
    card: &crate::hub::ClaimedCard,
    brain: &dyn crate::coder::CodeBrain,
    lease_expires_at: chrono::DateTime<chrono::Utc>,
) -> Result<ToolOutcome, ToolError> {
    run_code_session_with_deps(
        hub,
        data_dir,
        card,
        brain,
        lease_expires_at,
        &Default::default(),
    )
    .await
}

#[cfg(feature = "hub")]
pub async fn run_code_session_with_deps(
    hub: &dyn crate::hub::Hub,
    data_dir: &Path,
    card: &crate::hub::ClaimedCard,
    brain: &dyn crate::coder::CodeBrain,
    lease_expires_at: chrono::DateTime<chrono::Utc>,
    deps: &serde_json::Map<String, serde_json::Value>,
) -> Result<ToolOutcome, ToolError> {
    let spec = match crate::coder::CodeSessionSpec::from_required_capabilities(
        &card.required_capabilities,
    ) {
        Ok(s) => s,
        Err(e) => {
            return Ok(ToolOutcome {
                ok: false,
                summary: format!("code session: invalid required_capabilities: {e}"),
                data: None,
            })
        }
    };
    let context = if spec.coordinator {
        match crate::coder::coordinator::Context::from_deps(card.id, deps) {
            Ok(c) => Some(c),
            Err(e) => {
                return Ok(ToolOutcome {
                    ok: false,
                    summary: e.to_string(),
                    data: None,
                })
            }
        }
    } else {
        None
    };
    match crate::coder::run_session_with_context(
        hub,
        data_dir,
        card.id,
        &spec,
        brain,
        lease_expires_at,
        context.as_ref(),
    )
    .await
    {
        Ok(outcome) => Ok(ToolOutcome {
            // A session that ran past its lease is no more "ok" than one that hit the turn
            // limit -- neither finished with the brain declaring itself done.
            ok: !outcome.hit_turn_limit
                && !outcome.lease_expired
                && !outcome.acceptance.blocks_completion(),
            summary: format!("{}\n\n{}", outcome.final_text, outcome.acceptance.receipt()),
            data: Some(serde_json::json!({
                "usage": outcome.usage,
                "model_id": outcome.model_id,
                "acceptance": outcome.acceptance,
                "acceptance_failed": outcome.acceptance.blocks_completion(),
                "turns": outcome.turns,
                "hit_turn_limit": outcome.hit_turn_limit,
                "lease_expired": outcome.lease_expired,
                // ADR-032: present (a card id) only when `wait_for_child` paused the session --
                // `worker.rs`'s `run_code_card` must not complete/release/fail the card when
                // this is set (see `CodeSessionOutcome::waiting_on_child`'s own doc).
                "waiting_on_child": outcome.waiting_on_child,
            })),
        }),
        Err(e) => Ok(ToolOutcome {
            ok: false,
            summary: format!("code session failed: {e}"),
            data: None,
        }),
    }
}

/// Keep a tool result out of the model's context from blowing up the next prompt if an MCP tool
/// returns something huge (a big file read, a long directory listing). Summary-only truncation —
/// the untruncated value is still available in `ToolOutcome::data` for a caller that wants it.
#[cfg(feature = "hub")]
fn truncate_for_summary(s: &str) -> String {
    const MAX: usize = 2000;
    if s.len() <= MAX {
        return s.to_string();
    }
    // Walk back to the nearest UTF-8 char boundary at or before MAX so a slice on multi-byte
    // content (an MCP result containing non-ASCII text) never panics.
    let mut end = MAX;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... [truncated, {} bytes total]", &s[..end], s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "hub")]
    #[test]
    fn truncate_for_summary_never_panics_on_a_multibyte_boundary() {
        // Each '€' is 3 UTF-8 bytes, so byte offset MAX=2000 (not a multiple of 3) lands
        // mid-character -- a naive `&s[..2000]` would panic. Regression guard for the
        // char-boundary walk-back.
        let s: String = "€".repeat(700); // 2100 bytes total
        let out = truncate_for_summary(&s); // must not panic
        assert!(out.starts_with('€'));
        assert!(out.contains("truncated"));
    }

    #[test]
    fn component_path_is_scoped_to_data_dir_and_card_id() {
        let id = Uuid::nil();
        let p = component_path_for(Path::new("/data/node-a"), &id);
        assert_eq!(
            p,
            Path::new("/data/node-a/tool-components/00000000-0000-0000-0000-000000000000.wasm")
        );
    }

    #[test]
    fn spawn_child_spec_wait_defaults_to_false() {
        // Fire-and-forget must stay the default (ADR-006 D44 is opt-in): a plan written
        // before `wait` existed, or one that never mentions it, must not suddenly start
        // pausing cards.
        let spec: SpawnChildSpec = serde_json::from_value(serde_json::json!({
            "key": "child-a", "title": "t", "modality": "text", "inputs": "do the thing",
        }))
        .expect("minimal spec without `wait` should still parse");
        assert!(!spec.wait);
    }

    #[test]
    fn spawn_child_spec_wait_true_parses() {
        let spec: SpawnChildSpec = serde_json::from_value(serde_json::json!({
            "key": "child-a", "title": "t", "modality": "text", "inputs": "do the thing", "wait": true,
        }))
        .expect("spec with wait: true should parse");
        assert!(spec.wait);
    }

    #[tokio::test]
    async fn missing_component_is_a_clean_error_not_a_panic() {
        let sandbox = Sandbox::new().expect("engine construction never touches the filesystem");
        let err = run_exec_wasm(
            &sandbox,
            Path::new("/tmp/ohhive-tools-test-nonexistent-data-dir"),
            Uuid::new_v4(),
            None,
            ToolsLevel::SandboxedTools,
            NetPolicy::closed(),
        )
        .await;
        assert!(matches!(err, Err(ToolError::NotStaged(_, _))));
    }
}
