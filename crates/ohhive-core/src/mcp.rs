//! Minimal stdio MCP (Model Context Protocol) client (#177, ADR-023).
//!
//! Spawns a member-configured MCP server (`hive.member_mcp_servers`, migration
//! `20260913090000_member_mcp_servers.sql`) as a child process and speaks newline-delimited
//! JSON-RPC 2.0 over its stdin/stdout — the actual MCP stdio wire format (one JSON object per
//! line, no embedded newlines; not LSP-style `Content-Length` framing). Hand-rolled rather than
//! pulling in a third-party MCP client crate: `Cargo.lock` has no existing MCP dependency, and the
//! protocol surface actually needed here (one request/response pair per call, no batching, no
//! streaming, no resources/prompts) is small enough that hand-rolling it is more auditable than
//! taking on an early-stage, security-adjacent external dependency — see ADR-023 decision 3 for
//! the full reasoning.
//!
//! **This module does not sandbox anything.** Unlike [`crate::sandbox`], which runs a WASI
//! component under wasmtime with no filesystem/network escape by construction, an MCP server
//! spawned here is a real, unconstrained OS subprocess with whatever access the member who
//! configured it chose to grant it on their own machine. Hive's job stops at deciding *whether*
//! to spawn it at all — see `hive.node_claim_card`'s `mcp_server_id` branch and
//! `hive_member_mcp_server_get_node` (both in the migration above) for the ownership/enabled/
//! `tools_level` gate — never at constraining what the process does once it's running. Read
//! ADR-023 in full before changing anything in this file.
//!
//! Every call here is one-shot: spawn, `initialize`, `tools/list` (to fail cleanly on an unknown
//! tool name rather than surface whatever error shape the server itself returns), `tools/call`,
//! then kill the child. There is no persistent MCP session across steps or across cards in v1 —
//! same "fresh state every time" model [`crate::sandbox::Sandbox::run`] already uses for
//! `exec_wasm`. A card cannot yet hold a multi-turn conversation with an MCP server; see
//! `crate::tools`'s module doc and ADR-023's "deferred" list for why.

use serde::Deserialize;
use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

/// The `initialize`/`tools/list` handshake should be fast (a well-behaved server responds in
/// milliseconds); a generous fixed timeout catches a hung or misconfigured process without
/// needing a per-server tuning knob in v1.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// A real tool call (a filesystem walk, a dev-tool invocation) may legitimately take much longer
/// than the handshake — deliberately generous. There is no per-server override in v1 (ADR-023
/// "deferred"); a server that needs longer than this for one call isn't a good fit for the
/// pre-`Draft`, one-call-per-step model this is wired into anyway.
const TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// What a card actually needs to reach a member-configured server — the fields
/// `hive_member_mcp_server_get_node` returns, mirrored here (rather than imported from
/// [`crate::hub`]) so this module has no dependency on the `hub` feature: `sandbox` (which this
/// module lives under, see `lib.rs`) is a valid build without `hub` (e.g. a hypothetical
/// sandbox-only test binary), and every other type this low-level client needs is either `std` or
/// already a plain dependency of `sandbox`. `crate::tools::run_mcp_tool_call` (which *is*
/// `hub`-gated, since it needs [`crate::hub::HubClient`] to fetch the config in the first place)
/// converts [`crate::hub::McpServerConfig`] into this shape.
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

#[derive(Error, Debug)]
pub enum McpError {
    #[error("unsupported MCP transport '{0}' (v1 only supports stdio)")]
    UnsupportedTransport(String),
    #[error("failed to spawn MCP server process '{0}': {1}")]
    Spawn(String, #[source] std::io::Error),
    #[error("MCP server process has no stdin/stdout pipe (should be unreachable — both are requested at spawn)")]
    NoPipe,
    #[error("writing to MCP server stdin failed: {0}")]
    Write(#[source] std::io::Error),
    #[error("reading from MCP server stdout failed: {0}")]
    Read(#[source] std::io::Error),
    #[error("MCP server closed stdout before responding")]
    Eof,
    #[error("timed out waiting for MCP server after {0:?}")]
    Timeout(Duration),
    #[error("MCP server sent malformed JSON-RPC: {0}")]
    BadJson(#[source] serde_json::Error),
    #[error("MCP server returned a JSON-RPC error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("MCP server '{server}' has no tool named '{tool}' (advertises: {available})")]
    NoSuchTool {
        server: String,
        tool: String,
        available: String,
    },
}

/// One tool as advertised by a server's `tools/list` response.
#[derive(Debug, Clone, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "inputSchema", default)]
    pub input_schema: Option<serde_json::Value>,
}

/// One live stdio MCP session: a spawned child process plus JSON-RPC framing on top of its
/// stdin/stdout. Kills the child on drop (`kill_on_drop` at spawn time, plus an explicit
/// [`Drop`] impl as a belt-and-suspenders second path) so a card's MCP subprocess never outlives
/// the single tool call that started it.
pub struct McpSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpSession {
    /// Spawn `config.command` with `config.args`/`config.env` — as a direct `exec`, never through
    /// a shell, so nothing in a member's own `args`/`env` values can be reinterpreted as shell
    /// syntax — and run the MCP `initialize` handshake. `config.transport` must be `"stdio"`; the
    /// DB check constraint already enforces this, this is defense in depth.
    pub async fn start(config: &McpServerConfig) -> Result<Self, McpError> {
        if config.transport != "stdio" {
            return Err(McpError::UnsupportedTransport(config.transport.clone()));
        }
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited, not piped-and-ignored: an unread stderr pipe risks deadlocking the child
            // once its OS pipe buffer fills, and this protocol never reads from stderr anyway.
            // Inheriting sends a misbehaving server's diagnostics to this node's own log/terminal,
            // which is exactly where a member debugging their own server would look.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|e| McpError::Spawn(config.command.clone(), e))?;
        let stdin = child.stdin.take().ok_or(McpError::NoPipe)?;
        let stdout = child.stdout.take().ok_or(McpError::NoPipe)?;
        let mut session = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        };
        session.initialize().await?;
        Ok(session)
    }

    async fn initialize(&mut self) -> Result<(), McpError> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "hive-node", "version": env!("CARGO_PKG_VERSION") }
        });
        self.request("initialize", params, HANDSHAKE_TIMEOUT)
            .await?;
        // One-way notification (no "id", no response expected) that completes the MCP handshake.
        self.notify("notifications/initialized", serde_json::json!({}))
            .await
    }

    /// `tools/list` — called by [`Self::call_one_shot`] before every `tools/call` so an unknown
    /// tool name fails with a clear [`McpError::NoSuchTool`] naming what the server *does*
    /// advertise, rather than whatever error shape the server returns for an unrecognized name.
    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let resp = self
            .request("tools/list", serde_json::json!({}), HANDSHAKE_TIMEOUT)
            .await?;
        let tools = resp
            .get("tools")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));
        serde_json::from_value(tools).map_err(McpError::BadJson)
    }

    /// `tools/call`. `arguments` is passed through verbatim — whatever
    /// `required_capabilities.mcp_tool_args` the card declared, the same "card-declared,
    /// host-trusted, nothing a running inference step can redirect" data every other tool call in
    /// this codebase uses (see `crate::tools`'s module doc).
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let params = serde_json::json!({ "name": name, "arguments": arguments });
        self.request("tools/call", params, TOOL_CALL_TIMEOUT).await
    }

    /// Spawn, handshake, verify `tool_name` is actually advertised, call it once, and let the
    /// session (and therefore the child process) drop at the end of this call. This is the
    /// convenience entry point `crate::tools::run_mcp_tool_call` uses — see this module's doc for
    /// why v1 has no longer-lived session across multiple calls.
    pub async fn call_one_shot(
        config: &McpServerConfig,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let mut session = Self::start(config).await?;
        let tools = session.list_tools().await?;
        if !tools.iter().any(|t| t.name == tool_name) {
            let available = if tools.is_empty() {
                "(none)".to_string()
            } else {
                tools
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            return Err(McpError::NoSuchTool {
                server: config.name.clone(),
                tool: tool_name.to_string(),
                available,
            });
        }
        session.call_tool(tool_name, arguments).await
        // `session` drops here — `Drop` kills the child (see below).
    }

    async fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
        call_timeout: Duration,
    ) -> Result<serde_json::Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;
        let msg =
            serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write_line(&msg, call_timeout).await?;
        // One timeout for the *whole* read loop below, not one per line read: a chatty server
        // sending notifications ahead of the real response (the spec allows this, even though none
        // of the servers this targets in v1 are expected to) must not be able to reset the clock on
        // every line and effectively defeat `call_timeout`.
        let read_loop = async {
            loop {
                let line = self.read_line_untimed().await?;
                let value: serde_json::Value =
                    serde_json::from_str(&line).map_err(McpError::BadJson)?;
                // Skip any message that isn't the response to *this* request (id mismatch, or no
                // id at all — a notification) and keep waiting within the same overall deadline.
                if value.get("id").and_then(|v| v.as_i64()) != Some(id) {
                    continue;
                }
                if let Some(err) = value.get("error") {
                    let code = err.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
                    let message = err
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown MCP error")
                        .to_string();
                    return Err(McpError::Rpc { code, message });
                }
                return Ok(value
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null));
            }
        };
        match timeout(call_timeout, read_loop).await {
            Ok(result) => result,
            Err(_) => Err(McpError::Timeout(call_timeout)),
        }
    }

    async fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), McpError> {
        let msg = serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.write_line(&msg, HANDSHAKE_TIMEOUT).await
    }

    async fn write_line(
        &mut self,
        msg: &serde_json::Value,
        call_timeout: Duration,
    ) -> Result<(), McpError> {
        // Compact (no pretty-printing) serialization never contains an embedded newline, which
        // the MCP stdio framing requires (one JSON object per line).
        let mut line = serde_json::to_string(msg).map_err(McpError::BadJson)?;
        line.push('\n');
        timeout(call_timeout, async {
            self.stdin.write_all(line.as_bytes()).await?;
            self.stdin.flush().await
        })
        .await
        .map_err(|_| McpError::Timeout(call_timeout))?
        .map_err(McpError::Write)
    }

    /// Read one line with no timeout of its own — callers wrap the whole loop they run this in
    /// under one [`timeout`] (see [`Self::request`]) rather than resetting a deadline on every
    /// individual line.
    async fn read_line_untimed(&mut self) -> Result<String, McpError> {
        let mut line = String::new();
        let n = self
            .stdout
            .read_line(&mut line)
            .await
            .map_err(McpError::Read)?;
        if n == 0 {
            return Err(McpError::Eof);
        }
        Ok(line)
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        // `kill_on_drop(true)` at spawn time already asks tokio to reap the child when this
        // session is dropped; `start_kill` here is belt-and-suspenders for the (unexpected) case
        // this is dropped outside a context where that hook fires.
        let _ = self.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(command: &str) -> McpServerConfig {
        McpServerConfig {
            name: "test-server".into(),
            transport: "stdio".into(),
            command: command.into(),
            args: vec![],
            env: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn refuses_a_non_stdio_transport_without_spawning_anything() {
        let mut cfg = config("/bin/does-not-matter");
        cfg.transport = "http".into();
        let err = McpSession::start(&cfg).await;
        assert!(matches!(err, Err(McpError::UnsupportedTransport(t)) if t == "http"));
    }

    #[tokio::test]
    async fn a_command_that_does_not_exist_is_a_clean_spawn_error() {
        let cfg = config("/definitely/not/a/real/binary/ohhive-mcp-test");
        let err = McpSession::start(&cfg).await;
        assert!(matches!(err, Err(McpError::Spawn(_, _))));
    }
}
