//! ADR-033 Stage 1 (2026-09-15, Jack: "Scaffold Stage 1 only... explicitly no real login or
//! cloud calls yet"): the Rust-side runtime contract for talking to a Codex app-server over
//! stdio JSON-RPC (`docs/SIF-CHATGPT-SUBSCRIPTION-INTEGRATION-HANDOFF-2026-09-15.md`, section 10
//! delivery stage 1: "supervisor, generated schemas, fake server and event reducer; no cloud
//! calls. Gate: interleaved requests/events, bounded buffers, crash/restart and unknown-message
//! tests pass").
//!
//! **What's real here:** the transport framing (`transport.rs`: newline-delimited JSON, bounded
//! frame size, demultiplexing responses/notifications/server-requests), request/response
//! correlation and an in-flight budget plus generation-scoped reconnect (`supervisor.rs`, so a
//! crashed-and-restarted connection can never resolve a call that belongs to the connection
//! before it), and an event reducer (`reducer.rs`) that turns protocol notifications into the
//! typed events section 8 specifies. An in-process fake server (`fake_server.rs`) drives all of
//! that under test without a real `codex` binary -- see `tests.rs` for the Stage-1 gate itself.
//!
//! **What's deliberately NOT here yet (Stage 2+):** actually spawning `codex app-server`, any
//! managed login/OAuth, and `auth.rs`/`sessions.rs`/`journal.rs`/`broker.rs`/`policy.rs` from the
//! handoff's seam table (section 3) -- none of those exist yet, on purpose. Also not here: any
//! FFI/UI wiring (`crates/ohhive-ffi`), which stays untouched today. **This module makes zero
//! network calls, spawns no process, and holds no credentials.**
//!
//! **Also see `Halo-src/docs/CONTINUITY.md`, 2026-09-15 "later":** Anthropic's Jan/Feb 2026 ToS
//! ban on third-party use of Claude subscription OAuth tokens means there is no Claude-side
//! equivalent of this module to build later -- this scaffold is ChatGPT/Codex-specific (ADR-033),
//! not a template for a symmetric `claude`-flavored sibling.

pub mod fake_server;
pub mod generated;
pub mod protocol;
pub mod reducer;
pub mod supervisor;
pub mod transport;

#[cfg(test)]
mod tests;

pub use fake_server::FakeServer;
pub use protocol::{
    AccountLoginStartParams, AccountReadParams, ClientInfo, InitializeParams, LoginType,
    Notification, RawResponse, Request, RequestId, RpcError, ServerRequest,
};
pub use reducer::{AuthState, CoordinatorEvent, CoordinatorState, Reducer, ServerRequestOutcome};
pub use supervisor::Supervisor;
pub use transport::{FrameLimits, FrameReader, FrameWriter, Incoming, TransportError};

pub mod account;

/// Host-local durable subscription delivery guard; no provider calls or credentials.
pub mod journal;
