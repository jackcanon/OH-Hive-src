//! OH Hive core library.
//!
//! One core, two shells (ADR-003 D27): this crate is linked into the headless
//! regional server (`hive-server`), the coordinator (`hive-coordinator`), the
//! CLI (`hive`), and the Tauri desktop app. It owns the shared types and the
//! `Backend` trait; it does **not** own UI, persistence, or any Python.
//!
//! Module map:
//! - [`capability`] — what a node can do; the scheduler's matching input (ADR-005).
//! - [`job`] — cards, jobs, leases, checkpoints (ADR-005/006).
//! - [`backend`] — the `Backend` trait and adapters behind feature flags (ADR-003 D61).
//! - [`node`] — node identity, registration record, trust flags (ADR-006 D46/D48).
//! - [`ledger`] — usage metering types the coordinator turns into $honey (ADR-002).

pub mod acceptance;
pub mod backend;
/// Shared agent-loop vocabulary between the coding agent (`coder`, ADR-024) and the desktop
/// session (`desktop`, ADR-029): the `CodeBrain` seam and the multimodal `ContentBlock` both
/// modules build turns from. Ungated (matches `desktop` being ungated) since it's pure types --
/// no sandbox/hub dependency.
pub mod brain;
pub mod capability;
/// ADR-029 contracts and simulated policy checks; no native desktop control.
pub mod desktop;
pub mod job;
pub mod ledger;
pub mod node;

/// A real coding agent, scoped to a member's own Private Fleet (ADR-024, #185): workspace prep
/// (an existing checkout, or a fresh `git clone`), the `CodeBrain` seam (local today via
/// `backend::llama_cpp`'s tool-calling completions; a cloud/BYOK brain is #186's follow-on), and
/// the four unsandboxed tools (`read_file`/`write_file`/`list_dir`/`run_command`). Gated on both
/// `sandbox` (needs `sandbox::default_data_dir` for clone scratch space, and is wired into
/// `tools`/`worker` the same way `mcp` is) and `hub` (posts progress to the Private Fleet
/// channel and is driven entirely by `HubClient` data) — see `mcp`'s own doc for the identical
/// reasoning behind picking these two gates, applied here for the same reasons.
#[cfg(all(feature = "sandbox", feature = "hub"))]
pub mod coder;
#[cfg(feature = "hub")]
pub mod hub;
/// Stdout + rolling-file `tracing` setup shared by every long-running binary (ADR-none, ops fix).
#[cfg(feature = "hub")]
pub mod logging;
/// Minimal stdio MCP (Model Context Protocol) client (#177, ADR-023) — spawns a
/// member-configured MCP server and speaks its JSON-RPC-over-stdio handshake. Lives under the
/// same feature gate as [`tools`] (the module that actually wires it into a card's tool step),
/// even though this module itself has no wasmtime dependency — see its module doc.
#[cfg(feature = "sandbox")]
pub mod mcp;
/// `~/.config/ohhive/node.env` — one identity file shared by `hive` and `hive-server`.
#[cfg(feature = "hub")]
pub mod nodeconfig;
#[cfg(feature = "probe")]
pub mod probe;
/// WASI-component tool sandbox (ADR-006 D45-D48) — only where cards execute.
#[cfg(feature = "sandbox")]
pub mod sandbox;
/// First-run hardware assessment + Ollama install/pull (ADR-010, moved here per ADR-018
/// decision 2 so the Tauri shell and the native Swift shell share one implementation).
#[cfg(feature = "setup")]
pub mod setup;
/// The pull-dispatch worker loop (ADR-005/006) — shared by `hive work` and the desktop app.
#[cfg(feature = "hub")]
pub mod supervisor;
/// The agent tool surface built on top of [`sandbox`] — `exec_wasm`, `artifact_get`/`artifact_put`,
/// `spawn_child_card` (ADR-006's v1 tool list), and `mcp_tool_call` (#177/ADR-023).
#[cfg(feature = "sandbox")]
pub mod tools;
/// Cloudflare Tunnel automation (ADR-013 D74), moved here per ADR-018 decision 2/amendment
/// 2026-09-09 -- shared by the Tauri shell and the native Swift shell's regional-server role.
#[cfg(feature = "tunnel")]
pub mod tunnel;
/// Gated on `hub` because that is what it is made of: `crate::hub`, `execution_capacity` and
/// `anyhow` are all behind that feature and every one of them is used unconditionally here.
/// Ungated, `cargo check -p hive-core` did not compile at all -- workspace feature unification
/// turned `hub` on for everyone else, so no gate ever saw it. Nothing can have depended on the
/// module without the feature, because without the feature it never built.
#[cfg(feature = "hub")]
pub mod worker;

pub use backend::{Backend, BackendError, Chunk};
pub use capability::{Capabilities, Modality, ToolsLevel};
pub use job::{Checkpoint, Job, JobId, Lease};
pub use ledger::Usage;
pub use node::{NodeId, NodeRecord, Region};
#[cfg(feature = "sandbox")]
pub use sandbox::{NetPolicy, Sandbox, SandboxError, SandboxLimits};

/// Crate version, surfaced in the desktop About pane and `hive --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Fully local single-owner project storage and node pairing (ADR-025).
#[cfg(feature = "local-hub")]
pub mod local_hub;

#[cfg(feature = "hub")]
pub mod coordinator_hub;

/// Workspace-local SKILL.md storage; does not execute skills or grant tools.
#[cfg(feature = "skills")]
pub mod skills;

/// ADR-033 Stage 1: local Codex app-server JSON-RPC runtime contract (supervisor, protocol
/// schema types, fake server, event reducer). No cloud calls, no process spawning, no
/// credentials -- see the module doc comment for exactly what is and isn't here yet.
#[cfg(feature = "subscription-coordinator")]
pub mod subscription;

/// ADR-035 C0: Bots chat and agent collaboration -- domain types and a shared `BotsService`
/// trait only. No storage implementation, no FFI, no UI -- see the module doc comment for
/// exactly what is and isn't here yet.
#[cfg(feature = "bots")]
pub mod bots;

#[cfg(any(feature = "hub", feature = "bots"))]
pub mod execution_capacity;

/// Bounded, one-pass transcription with staged artifact inputs.
#[cfg(feature = "whisper")]
pub mod speech;

/// Shared local-model advertisement memory gate.
pub mod model_fit;
