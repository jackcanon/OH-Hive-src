//! Hand-authored JSON-RPC types for the Codex app-server protocol (ADR-033 Stage 1: "generated
//! Codex app-server protocol schema types"). These are **not yet generated** from a pinned
//! runtime -- Sif's handoff
//! (`docs/SIF-CHATGPT-SUBSCRIPTION-INTEGRATION-HANDOFF-2026-09-15.md`, section 5) specifies
//! running `codex app-server generate-json-schema --out <fixture-dir>` against a pinned version
//! (her evidence: codex-cli 0.149.0) and committing the generated fixture hashes alongside
//! integration tests. This device has no Codex binary and no way to run that command, so this
//! file hand-authors the shapes section 5 documents verbatim (the `initialize`/`account/read`/
//! `account/login/start` minimal-flow example) and leaves everything else as an opaque
//! `serde_json::Value` payload behind typed envelopes. Swapping in real generated types for the
//! untyped parts is the next increment, not a gap nobody noticed.
//!
//! Transport shape: newline-delimited JSON, JSON-RPC ids on requests/responses, with the
//! `jsonrpc` field *omitted* (Codex's own spec, per section 5) -- `Request`/`RawResponse` below
//! reflect that rather than the standard JSON-RPC 2.0 envelope.

use serde::{Deserialize, Serialize};

/// A request id. Codex's documented example flow uses small increasing integers (`"id":1`,
/// `"id":2`, ...); `Supervisor` (`supervisor.rs`) owns allocating these.
pub type RequestId = u64;

/// One outgoing call. `method` is one of the documented wire methods (section 5's minimal flow:
/// `initialize`, `account/read`, `account/login/start`, plus `model/list`,
/// `account/rateLimits/read`, `thread/start`, `thread/resume`, `turn/start`, `thread/read`,
/// `turn/interrupt`, `account/login/cancel`, `account/logout` named elsewhere in the handoff) --
/// kept as a plain `String` rather than an enum since the full method list isn't pinned down
/// without the real generated schema, and a typo'd method name should fail at the fake-server/
/// real-server boundary (a clear protocol error), not at compile time.
#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub id: RequestId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A reply to one `Request`, correlated by `id`. `into_result` turns the wire's result-or-error
/// shape into an actual `Result`, so callers never have to remember to check `error` first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawResponse {
    pub id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RawResponse {
    pub fn into_result(self) -> Result<Option<serde_json::Value>, RpcError> {
        match self.error {
            Some(e) => Err(e),
            None => Ok(self.result),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// A server-initiated notification (no `id`) -- `initialized`, `account/login/completed`,
/// `account/updated`, `turn/started`, text deltas, tool/item events, `turn/completed`, rate-limit
/// updates, etc. (handoff sections 5/8). Kept as method + opaque payload for the same reason
/// `Request`'s method is a `String`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A server-initiated *request* (has an id, but the server sent it, not us) -- e.g. an approval
/// prompt. Section 5: "unknown server requests must receive a supported protocol error/decline
/// rather than being silently accepted or leaving the turn hanging" -- Stage 1's fake server and
/// `Supervisor` both round-trip this shape even though nothing real sends one yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerRequest {
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

// ---- Typed params for the methods section 5's minimal flow spells out verbatim ----

#[derive(Debug, Clone, Serialize)]
pub struct ClientInfo {
    pub name: String,
    pub title: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InitializeParams {
    #[serde(rename = "clientInfo")]
    pub client_info: ClientInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountReadParams {
    #[serde(rename = "refreshToken")]
    pub refresh_token: bool,
}

/// `"chatgpt"` (managed browser login) or `"chatgptDeviceCode"` (documented fallback, section 5).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LoginType {
    Chatgpt,
    ChatgptDeviceCode,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountLoginStartParams {
    #[serde(rename = "type")]
    pub kind: LoginType,
}

// ---- Method-name constants -- confirmed ones cite the handoff section that names them; the
// rest are this module's own best guess pending real schema generation, called out inline. ----

/// Section 5 minimal flow.
pub const METHOD_INITIALIZE: &str = "initialize";
/// Section 5 minimal flow (notification, no id).
pub const METHOD_INITIALIZED: &str = "initialized";
/// Section 5 minimal flow.
pub const METHOD_ACCOUNT_READ: &str = "account/read";
/// Section 5 minimal flow.
pub const METHOD_ACCOUNT_LOGIN_START: &str = "account/login/start";
/// Section 5: "handle `account/login/completed` and `account/updated`".
pub const METHOD_ACCOUNT_LOGIN_COMPLETED: &str = "account/login/completed";
/// Section 5: "handle `account/login/completed` and `account/updated`".
pub const METHOD_ACCOUNT_UPDATED: &str = "account/updated";
/// Section 5: "Handle `turn/interrupt`, `account/login/cancel`, `account/logout`, rate-limit
/// updates and account changes."
pub const METHOD_ACCOUNT_LOGIN_CANCEL: &str = "account/login/cancel";
pub const METHOD_ACCOUNT_LOGOUT: &str = "account/logout";
/// Section 5: "query `model/list` with pagination".
pub const METHOD_MODEL_LIST: &str = "model/list";
/// Section 5: "`account/rateLimits/read`".
pub const METHOD_ACCOUNT_RATE_LIMITS_READ: &str = "account/rateLimits/read";
/// Section 5: "start/resume via `thread/start` / `thread/resume`".
pub const METHOD_THREAD_START: &str = "thread/start";
pub const METHOD_THREAD_RESUME: &str = "thread/resume";
/// Section 5: "recover final items using `thread/read`".
pub const METHOD_THREAD_READ: &str = "thread/read";
/// Section 5: "start work with `turn/start`".
pub const METHOD_TURN_START: &str = "turn/start";
/// Section 5: "reduce `turn/started`, text deltas, tool/item events and `turn/completed`".
pub const METHOD_TURN_STARTED: &str = "turn/started";
pub const METHOD_TURN_COMPLETED: &str = "turn/completed";
pub const METHOD_TURN_INTERRUPT: &str = "turn/interrupt";
/// Not named verbatim by the handoff ("rate-limit updates" is described, not spelled out as a
/// method name) -- this module's best guess, mirroring `account/rateLimits/read`'s shape, pending
/// real schema generation.
pub const METHOD_ACCOUNT_RATE_LIMITS_UPDATED: &str = "account/rateLimits/updated";

/// Serializes a `Request`/`Notification`/`RawResponse`/`ServerRequest`-shaped value to one
/// newline-delimited JSON frame (a single trailing `\n`, matching `transport::FrameReader`).
pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}
