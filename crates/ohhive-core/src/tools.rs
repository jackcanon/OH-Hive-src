//! Agent tool surface (ADR-006 D45; the "Open questions" v1 WASI tool list).
//!
//! One tool ships for real here: `exec_wasm` — run a WASI Preview 2 component
//! through [`crate::sandbox::Sandbox`]. A card opts in by setting
//! `required_capabilities.exec_wasm = true`; nothing in the card's own fields
//! ever names a host path. The component this runs is always
//! `component_path_for(data_dir, card.id)` — a location derived purely from
//! the node's own data directory and the card's id, never from plan/model
//! output. That closes off the obvious prompt-injection shape (ADR-006's own
//! "nothing the model says can widen policy" mitigation): there is no field
//! here for an injected instruction to redirect.
//!
//! `artifact_get`/`artifact_put` (fetching that component from hub storage
//! instead of assuming it is already on disk) and `spawn_child_card` are the
//! other two tools in ADR-006's default v1 list. Both need hub RPCs that don't
//! exist yet — `HubClient` has no artifact-fetch-by-hash or child-lease-create
//! call — so they are deliberately not built here. Until `artifact_get` lands,
//! staging the component file at the path below is a manual/out-of-band step
//! (a local test, or an operator copying a file in); see `worker.rs`'s module
//! doc for the standing list of what's left.

use crate::capability::ToolsLevel;
use crate::sandbox::{scratch_dir_for, NetPolicy, Sandbox, SandboxError, SandboxLimits};
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

/// A request to run the one v1 tool for one card.
#[derive(Debug, Clone, Copy)]
pub struct ToolCall {
    pub card_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub ok: bool,
    /// Short human-readable summary, meant to be folded into the agent loop's
    /// context for the next inference step (not the tool's raw stdout — the
    /// sandbox doesn't currently capture stdout separately from the guest's
    /// own scratch-dir writes).
    pub summary: String,
}

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("card {0} requested exec_wasm but no component is staged at {1}")]
    NotStaged(Uuid, PathBuf),
    #[error(transparent)]
    Sandbox(#[from] SandboxError),
}

/// Where a staged tool component lives for a given card, under the node's data
/// directory. Deterministic and card-scoped — see the module doc for why that
/// matters.
pub fn component_path_for(data_dir: &Path, card_id: &Uuid) -> PathBuf {
    data_dir
        .join("tool-components")
        .join(format!("{card_id}.wasm"))
}

/// Run `exec_wasm` for one card. Callers should only invoke this for cards
/// that actually set `required_capabilities.exec_wasm = true`; it does not
/// check that flag itself (the caller already knows why it's calling this).
pub async fn run_exec_wasm(
    sandbox: &Sandbox,
    data_dir: &Path,
    call: ToolCall,
    tools_level: ToolsLevel,
    net: NetPolicy,
) -> Result<ToolOutcome, ToolError> {
    let component = component_path_for(data_dir, &call.card_id);
    if !component.exists() {
        return Err(ToolError::NotStaged(call.card_id, component));
    }
    let scratch = scratch_dir_for(data_dir, &call.card_id.to_string());
    let lease_id = call.card_id.to_string();
    match sandbox
        .run(
            &component,
            &scratch,
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
        }),
        // A trapped/failed/refused tool is a normal *outcome* to feed back to the
        // model (it can say so in the draft, or the reviewer can catch it) — not
        // a reason to fail the whole card. Only a bug in calling the sandbox
        // itself (there isn't one, once `component.exists()` holds) would.
        Err(e) => Ok(ToolOutcome {
            ok: false,
            summary: format!("exec_wasm failed: {e}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_path_is_scoped_to_data_dir_and_card_id() {
        let id = Uuid::nil();
        let p = component_path_for(Path::new("/data/node-a"), &id);
        assert_eq!(
            p,
            Path::new("/data/node-a/tool-components/00000000-0000-0000-0000-000000000000.wasm")
        );
    }

    #[tokio::test]
    async fn missing_component_is_a_clean_error_not_a_panic() {
        let sandbox = Sandbox::new().expect("engine construction never touches the filesystem");
        let call = ToolCall {
            card_id: Uuid::new_v4(),
        };
        let err = run_exec_wasm(
            &sandbox,
            Path::new("/tmp/ohhive-tools-test-nonexistent-data-dir"),
            call,
            ToolsLevel::SandboxedTools,
            NetPolicy::closed(),
        )
        .await;
        assert!(matches!(err, Err(ToolError::NotStaged(_, _))));
    }
}
