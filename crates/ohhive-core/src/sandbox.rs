//! WASI-component tool sandbox (ADR-006 decisions 3-5: D45, D46, D47, D48).
//!
//! Every agent tool is compiled to a WASI Preview 2 component and run under
//! **wasmtime**, embedded in the node core, on every OS. A card gets an
//! isolated scratch directory preopened as the only writable path, a CPU-fuel
//! budget, and a memory cap. No host process spawning happens from inside a
//! sandboxed run — the guest cannot `exec`, `fork`, or reach anything on the
//! host filesystem outside its own scratch directory.
//!
//! Network access is deny-by-default and gated by [`NetPolicy`]: a no-op that
//! only permits outbound connections when the node's `allow_internet` *and*
//! the running card's `requires_internet` both hold (D46/D47). The node flag
//! alone never opens the network. Every address checked — allowed or denied —
//! is logged for audit (`tracing`, target `hive_core::sandbox::net`).
//!
//! `tools_level` is enforced here as the *second* of its two checks (D48):
//! the coordinator already refuses to schedule a tools-requiring card onto an
//! `inference_only` node (ADR-005); [`Sandbox::run`] refuses a second time,
//! at the point tools would actually execute, so a mis-scheduled card fails
//! closed instead of running tools.
//!
//! This module only builds behind the `sandbox` feature and is only linked
//! into crates that actually execute cards (`hive`, `ohhive-desktop`) —
//! `hive-server` never runs a card and stays free of this dependency, per the
//! Pi-class footprint rule (ADR-003 D12/D62).

use crate::capability::ToolsLevel;
use std::path::{Path, PathBuf};
use thiserror::Error;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::p2::bindings::Command;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

#[derive(Error, Debug)]
pub enum SandboxError {
    #[error("tools_level is inference_only on this node — refusing to run a sandboxed tool")]
    ToolsDisabled,
    #[error("scratch directory setup failed: {0}")]
    Scratch(#[source] std::io::Error),
    #[error("wasmtime engine/config error: {0}")]
    Engine(#[source] wasmtime::Error),
    #[error("failed to load tool component: {0}")]
    Load(#[source] wasmtime::Error),
    #[error("tool instantiation failed: {0}")]
    Instantiate(#[source] wasmtime::Error),
    #[error("tool trapped or exceeded its fuel/memory budget: {0}")]
    Trapped(#[source] wasmtime::Error),
    #[error("tool exited with a failure result")]
    ToolFailed,
}

/// Whether this run may open outbound network connections (D46/D47). Computed
/// *once* per lease from `node.allow_internet && card.requires_internet` —
/// never from anything the running tool or model says, so nothing a card
/// does at runtime can widen its own network policy.
#[derive(Debug, Clone, Copy)]
pub struct NetPolicy {
    allow: bool,
}

impl NetPolicy {
    /// Both flags must hold. Neither alone opens the network (D46).
    pub fn new(node_allow_internet: bool, card_requires_internet: bool) -> Self {
        Self {
            allow: node_allow_internet && card_requires_internet,
        }
    }

    pub fn closed() -> Self {
        Self { allow: false }
    }
}

/// Resource limits for one sandboxed run. Conservative defaults suitable for
/// a volunteer's machine running someone else's task.
#[derive(Debug, Clone, Copy)]
pub struct SandboxLimits {
    /// wasmtime fuel budget. Fuel is consumed roughly per Wasm instruction;
    /// this is a coarse CPU-time proxy, not a wall-clock timeout.
    pub fuel: u64,
    /// Max linear memory (bytes) any one instance may grow to.
    pub max_memory_bytes: usize,
    /// Max table elements (funcrefs etc.) any one instance may grow to.
    pub max_table_elements: usize,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            fuel: 5_000_000_000,
            max_memory_bytes: 256 * 1024 * 1024,
            max_table_elements: 10_000,
        }
    }
}

struct HostState {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// One sandboxed execution engine. Cheap to keep around (an [`Engine`]
/// compiles/caches Cranelift machine code); create one per node process and
/// reuse it across cards.
pub struct Sandbox {
    engine: Engine,
}

impl Sandbox {
    pub fn new() -> Result<Self, SandboxError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.wasm_component_model(true);
        let engine = Engine::new(&config).map_err(SandboxError::Engine)?;
        Ok(Self { engine })
    }

    /// Run one WASI-component tool against a card's scratch directory.
    ///
    /// `scratch_root` is wiped and recreated for this run, then preopened as
    /// the guest's `.` — the *only* writable path the tool can reach. No
    /// other host directory is preopened by default, so a tool cannot write
    /// anything else on the node, and cannot spawn a host process at all
    /// (WASI components have no `exec`/`fork` — there is nothing to gate
    /// there).
    ///
    /// `inputs_dir`, if given, is preopened **read-only** at `/in` — for
    /// tools whose inputs were staged by an earlier tool call
    /// ([`crate::tools::run_artifact_get`]) rather than embedded in the
    /// component itself. It is never wiped (that isn't scratch), so its
    /// contents are exactly whatever the caller staged before this call.
    #[allow(clippy::too_many_arguments)] // each is a distinct, independently-meaningful policy/limit input; bundling them would just rename this list
    pub async fn run(
        &self,
        component_path: &Path,
        scratch_root: &Path,
        inputs_dir: Option<&Path>,
        tools_level: ToolsLevel,
        net: NetPolicy,
        limits: SandboxLimits,
        lease_id: &str,
    ) -> Result<(), SandboxError> {
        // D48, second enforcement: fail closed even if the scheduler placed
        // this card here by mistake. Nothing downstream ever runs a tool
        // component when the node itself is inference_only.
        if tools_level != ToolsLevel::SandboxedTools {
            tracing::warn!(
                lease_id,
                "refusing tool run: node is tools_level=inference_only"
            );
            return Err(SandboxError::ToolsDisabled);
        }

        reset_scratch_dir(scratch_root).map_err(SandboxError::Scratch)?;

        let lease = lease_id.to_string();
        let net_for_check = net;
        let mut wasi_builder = WasiCtxBuilder::new();
        wasi_builder
            .preopened_dir(scratch_root, ".", wasmtime_wasi::FsPerms::ReadWrite)
            .map_err(SandboxError::Engine)?;

        if let Some(inputs) = inputs_dir {
            wasi_builder
                .preopened_dir(inputs, "/in", wasmtime_wasi::FsPerms::ReadOnly)
                .map_err(SandboxError::Engine)?;
        }

        if net.allow {
            wasi_builder
                .allow_tcp(true)
                .allow_udp(true)
                .allow_ip_name_lookup(true);
        }
        // Installed unconditionally: this is the actual audited choke point
        // (D46's "single audited choke point"), independent of the allow_tcp
        // toggle above, and it is what actually decides every connection.
        wasi_builder.socket_addr_check(move |addr, use_| {
            let allowed = net_for_check.allow;
            let lease = lease.clone();
            Box::pin(async move {
                tracing::info!(
                    target: "hive_core::sandbox::net",
                    lease_id = %lease,
                    addr = %addr,
                    use_ = ?use_,
                    allowed,
                    "sandboxed tool network check"
                );
                allowed
            })
        });
        let wasi = wasi_builder.build();

        let store_limits = StoreLimitsBuilder::new()
            .memory_size(limits.max_memory_bytes)
            .table_elements(limits.max_table_elements)
            .build();

        let mut store = Store::new(
            &self.engine,
            HostState {
                wasi,
                table: ResourceTable::new(),
                limits: store_limits,
            },
        );
        store.limiter(|state| &mut state.limits);
        store.set_fuel(limits.fuel).map_err(SandboxError::Engine)?;

        let mut linker: Linker<HostState> = Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(SandboxError::Engine)?;

        let component =
            Component::from_file(&self.engine, component_path).map_err(SandboxError::Load)?;

        let command = Command::instantiate_async(&mut store, &component, &linker)
            .await
            .map_err(SandboxError::Instantiate)?;

        let result = command
            .wasi_cli_run()
            .call_run(&mut store)
            .await
            .map_err(SandboxError::Trapped)?;

        result.map_err(|()| SandboxError::ToolFailed)
    }
}

/// Wipe and recreate the per-card scratch directory. Called at the start of
/// every run so one card's leftovers are never visible to the next — a fresh
/// scratch per attempt, not a shared workspace across cards or members.
fn reset_scratch_dir(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    std::fs::create_dir_all(path)
}

/// Where a node keeps per-card scratch directories, rooted under the node's
/// data directory. Kept as a helper here so callers (the worker step loop)
/// don't each reinvent the naming scheme.
pub fn scratch_dir_for(data_dir: &Path, card_id: &str) -> PathBuf {
    data_dir.join("sandbox-scratch").join(card_id)
}

/// Where `artifact_get` stages a fetched artifact for a card, before `exec_wasm` mounts it
/// read-only at `/in` (see [`Sandbox::run`]). Separate from `scratch_dir_for` on purpose: scratch
/// is wiped at the start of every `Sandbox::run`, and a staged input must survive that wipe.
pub fn inputs_dir_for(data_dir: &Path, card_id: &str) -> PathBuf {
    data_dir.join("tool-inputs").join(card_id)
}

/// Fallback data directory for nodes that have no more specific one configured
/// (the headless `hive work` CLI has no `HIVE_DATA_DIR` of its own today — only
/// the desktop app's regional-server role and `hive-server` do). Sibling to
/// `node.env` rather than a new config key, so a plain `hive work` node gets
/// somewhere to stage tool components and scratch dirs with zero setup.
/// A node that already has a real data directory (the desktop app) should
/// pass that in instead of calling this.
pub fn default_data_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ohhive")
        .join("sandbox")
}

#[cfg(test)]
mod tests {
    use super::*;

    // D48's *second* enforcement: even before wasmtime is touched, an
    // inference_only node must refuse. This is the fail-closed check that
    // matters if a card is ever mis-scheduled onto a node that shouldn't run
    // tools at all.
    #[tokio::test]
    async fn refuses_tools_on_inference_only_node_without_touching_wasmtime() {
        let sandbox = Sandbox::new().expect("engine construction never touches policy");
        let err = sandbox
            .run(
                Path::new("/nonexistent/component.wasm"),
                Path::new("/tmp/ohhive-sandbox-test-scratch-should-not-be-created"),
                None,
                ToolsLevel::InferenceOnly,
                NetPolicy::new(true, true), // even wide-open net policy must not matter
                SandboxLimits::default(),
                "test-lease",
            )
            .await;
        assert!(matches!(err, Err(SandboxError::ToolsDisabled)));
        // Refusing before touching the filesystem means the scratch dir this
        // run would have used was never created.
        assert!(!Path::new("/tmp/ohhive-sandbox-test-scratch-should-not-be-created").exists());
    }

    // Neither flag alone opens the network (D46) — both must hold.
    #[test]
    fn net_policy_requires_both_flags() {
        assert!(!NetPolicy::new(false, false).allow);
        assert!(
            !NetPolicy::new(true, false).allow,
            "node allow_internet alone must not open the network"
        );
        assert!(
            !NetPolicy::new(false, true).allow,
            "card requires_internet alone must not open the network"
        );
        assert!(NetPolicy::new(true, true).allow);
        assert!(!NetPolicy::closed().allow);
    }

    #[test]
    fn scratch_dir_is_rooted_under_a_dedicated_subdir() {
        let p = scratch_dir_for(Path::new("/data/node-a"), "card-123");
        assert_eq!(p, Path::new("/data/node-a/sandbox-scratch/card-123"));
    }
}
