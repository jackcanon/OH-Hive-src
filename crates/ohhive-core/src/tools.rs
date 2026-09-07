//! Agent tool surface (ADR-006 D45; the "Open questions" v1 WASI tool list).
//!
//! Four tools, all real now: `exec_wasm` (run a WASI Preview 2 component
//! through [`crate::sandbox::Sandbox`]), `artifact_get`/`artifact_put`
//! (fetch/store bytes through a regional server, via [`crate::hub::HubClient`]),
//! and `spawn_child_card` (create a sibling card, ADR-006 D44). A card opts
//! into each independently via `required_capabilities`:
//!
//! - `exec_wasm: true` — run `component_path_for(data_dir, card.id)`.
//! - `artifact_get_hash: "<sha256>"` — fetch that artifact and stage it at
//!   `/in/<hash>` (read-only) for the `exec_wasm` call that follows.
//! - `artifact_put: true` — after `exec_wasm` runs, if it wrote a file at
//!   `./out` in its scratch dir, upload that file as a new artifact.
//! - `spawn_child` — `{"key", "title", "modality", "inputs"}` — create one
//!   child card in the same project, once, after the above.
//!
//! Every value here is host-trusted card data set when the card was created,
//! never something a running model can invent or redirect at runtime — the
//! same "nothing the model says can widen policy" property the sandbox
//! itself relies on (ADR-006's mitigation for prompt injection via tool
//! calls). `worker.rs`'s `maybe_run_tool` runs these in the fixed order
//! above, once, before `Draft` — not the full multi-turn ReAct loop ADR-006's
//! step machine describes (a card can't yet ask for a *second* `exec_wasm`
//! call mid-run); see `worker.rs`'s own module doc for what that would take.
//!
//! `spawn_child_card` creates a card only — nothing here or in `worker.rs`
//! yet pauses the parent card until the child finishes (D44's sub-delegation
//! is card-creation only for now; full pause/resume is filed as its own
//! roadmap item, not built in this pass).

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
    hub: &crate::hub::HubClient,
    data_dir: &Path,
    card_id: Uuid,
    hash: &str,
) -> Result<ToolOutcome, ToolError> {
    let (bytes, _mime) = hub.artifact_fetch(hash).await?;
    let dir = inputs_dir_for(data_dir, &card_id.to_string());
    std::fs::create_dir_all(&dir)?;
    let len = bytes.len();
    std::fs::write(dir.join(hash), bytes)?;
    Ok(ToolOutcome {
        ok: true,
        summary: format!("artifact_get: fetched {len} bytes for {hash}, staged at /in/{hash}"),
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

/// After `exec_wasm` ran, upload whatever it wrote to `./out` in its scratch
/// dir as a new artifact. A card opts in via `required_capabilities.artifact_put`.
/// If `exec_wasm` didn't write `./out` (didn't run at all, or ran but produced
/// nothing there), this is a normal non-error outcome — not every tool run
/// produces an artifact.
#[cfg(feature = "hub")]
pub async fn run_artifact_put(
    hub: &crate::hub::HubClient,
    data_dir: &Path,
    card_id: Uuid,
    project_id: Option<Uuid>,
) -> Result<ToolOutcome, ToolError> {
    let out = scratch_dir_for(data_dir, &card_id.to_string()).join("out");
    if !out.exists() {
        return Ok(ToolOutcome {
            ok: false,
            summary: "artifact_put: exec_wasm did not write ./out — nothing to upload".to_string(),
        });
    }
    let bytes = std::fs::read(&out)?;
    let reply = hub
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
}

/// Create one child card under `parent_card_id` (ADR-006 D44). Card-creation
/// only — see the module doc for what's not yet built on top of this.
#[cfg(feature = "hub")]
pub async fn run_spawn_child_card(
    hub: &crate::hub::HubClient,
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
    })
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
