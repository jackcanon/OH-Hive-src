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

pub mod backend;
pub mod capability;
pub mod job;
pub mod ledger;
pub mod node;

#[cfg(feature = "hub")]
pub mod hub;
/// Stdout + rolling-file `tracing` setup shared by every long-running binary (ADR-none, ops fix).
#[cfg(feature = "hub")]
pub mod logging;
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
/// The agent tool surface built on top of [`sandbox`] — `exec_wasm`, `artifact_get`/`artifact_put`,
/// and `spawn_child_card` (ADR-006's v1 tool list).
#[cfg(feature = "sandbox")]
pub mod tools;
/// Cloudflare Tunnel automation (ADR-013 D74), moved here per ADR-018 decision 2/amendment
/// 2026-09-09 -- shared by the Tauri shell and the native Swift shell's regional-server role.
#[cfg(feature = "tunnel")]
pub mod tunnel;
/// The pull-dispatch worker loop (ADR-005/006) — shared by `hive work` and the desktop app.
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
