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
#[cfg(feature = "probe")]
pub mod probe;

pub use backend::{Backend, BackendError, Chunk};
pub use capability::{Capabilities, Modality, ToolsLevel};
pub use job::{Checkpoint, Job, JobId, Lease};
pub use ledger::Usage;
pub use node::{NodeId, NodeRecord, Region};

/// Crate version, surfaced in the desktop About pane and `hive --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
