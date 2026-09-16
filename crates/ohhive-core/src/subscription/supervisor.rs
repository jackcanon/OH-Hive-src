//! Ties `transport.rs`'s framing to `reducer.rs`'s event mapping into the "supervisor scaffold"
//! Stage 1 asks for. Deliberately **not** an actor/background-task design (no `tokio::spawn`, no
//! channels): callers drive it with a synchronous-looking `call()`/`pump_once()` pump loop, which
//! is far easier to hand-verify without a compiler (this device has none -- see the commit
//! message) and is sufficient to exercise every Stage-1 gate scenario section 10 names
//! (interleaved requests/events, bounded buffers, crash/restart, unknown messages; see
//! `subscription/tests.rs`). A background-task version that fans events out to a UI-facing
//! channel is Stage 2+ work, once there's a real process and real FFI to drive it from.
//!
//! **No cloud calls, no process spawning, no credentials here** -- `Supervisor::new` takes an
//! already-connected reader/writer pair (a real child's stdio in a later stage, or a fake
//! server's duplex half under test today).

use std::collections::{HashMap, HashSet};

use tokio::io::{AsyncRead, AsyncWrite};

use super::protocol::{self, ClientInfo, InitializeParams, RequestId, RpcError};
use super::reducer::{CoordinatorEvent, Reducer};
use super::transport::{FrameLimits, FrameReader, FrameWriter, Incoming, TransportError};

pub struct Supervisor<R, W> {
    reader: FrameReader<R>,
    writer: FrameWriter<W>,
    reducer: Reducer,
    generation: u64,
    limits: FrameLimits,
    next_id: RequestId,
    in_flight: HashSet<RequestId>,
    results: HashMap<RequestId, Result<Option<serde_json::Value>, RpcError>>,
}

impl<R, W> Supervisor<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(reader: R, writer: W, limits: FrameLimits, generation: u64) -> Self {
        Self {
            reader: FrameReader::new(reader, limits.max_frame_bytes),
            writer: FrameWriter::new(writer),
            reducer: Reducer::new(generation),
            generation,
            limits,
            next_id: 1,
            in_flight: HashSet::new(),
            results: HashMap::new(),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn auth_state(&self) -> super::reducer::AuthState {
        self.reducer.auth_state()
    }

    pub fn coordinator_state(&self) -> super::reducer::CoordinatorState {
        self.reducer.coordinator_state()
    }

    /// Sends one request. Returns `TransportError::QueueFull` (no I/O performed) once
    /// `limits.max_queued_control` calls are outstanding -- section 5's "128 queued control
    /// messages" budget; callers must `pump_once` responses to drain it, matching a real Codex
    /// app-server connection where an unbounded backlog of un-acknowledged calls is exactly the
    /// failure mode this budget exists to prevent.
    pub async fn call(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<RequestId, TransportError> {
        if self.in_flight.len() + self.results.len() >= self.limits.max_queued_control {
            return Err(TransportError::QueueFull);
        }
        let id = self.next_id;
        self.next_id += 1;
        let request = protocol::Request {
            id,
            method: method.to_string(),
            params,
        };
        self.writer.write_value(&request).await?;
        self.in_flight.insert(id);
        Ok(id)
    }

    /// Convenience wrapper for the section 5 minimal-flow handshake call.
    pub async fn initialize(
        &mut self,
        client_info: ClientInfo,
    ) -> Result<RequestId, TransportError> {
        let params = InitializeParams { client_info };
        let value =
            serde_json::to_value(&params).map_err(|e| TransportError::Malformed(e.to_string()))?;
        self.call(protocol::METHOD_INITIALIZE, Some(value)).await
    }

    /// Reads and processes exactly one incoming frame. A `Response` resolves a pending `call`
    /// (fetch it with `take_result`); a `Notification`/`ServerRequest` is routed through the
    /// `Reducer` and its events returned. An unrecognized server request is declined
    /// automatically (section 5) before this returns. `Ok(None)` is a clean EOF -- the caller
    /// decides whether that's a graceful shutdown or a crash needing `reconnect`.
    pub async fn pump_once(&mut self) -> Result<Option<Vec<CoordinatorEvent>>, TransportError> {
        let frame = match self.reader.read_frame().await {
            Ok(Some(f)) => f,
            Ok(None) => return Ok(None),
            // A read-side failure (I/O error, oversized frame, stream closed mid-frame) means
            // the connection itself is unusable -- section 5: "report a protocol error and
            // reconnect/reconcile". `classify_frame`'s errors below are different: the stream is
            // still readable, only that one frame's content was bad.
            Err(e) => {
                let event = self.reducer.reduce_transport_error(e.to_string(), true);
                return Ok(Some(vec![event]));
            }
        };
        let incoming = match super::transport::classify_frame(&frame) {
            Ok(i) => i,
            Err(e) => {
                let event = self.reducer.reduce_transport_error(e.to_string(), false);
                return Ok(Some(vec![event]));
            }
        };
        match incoming {
            Incoming::Response(r) => {
                // Unknown/duplicate responses must not grow the retained-result map.
                if self.in_flight.remove(&r.id) {
                    self.results.insert(r.id, r.into_result());
                }
                Ok(Some(Vec::new()))
            }
            Incoming::Notification(n) => Ok(Some(self.reducer.reduce_notification(n))),
            Incoming::ServerRequest(sr) => {
                let outcome = self.reducer.reduce_server_request(&sr);
                if outcome.decline {
                    let response = serde_json::json!({
                        "id": sr.id,
                        "error": {"code": -32601,
                                  "message": format!("unsupported server request: {}", sr.method)}
                    });
                    self.writer.write_value(&response).await?;
                }
                Ok(Some(outcome.events))
            }
        }
    }

    /// Takes and removes a previously completed call's result, if `pump_once` has already seen
    /// its `Response`. `None` means "not answered yet" -- callers interleave `call`/`pump_once`/
    /// `take_result` however they need to.
    pub fn take_result(
        &mut self,
        id: RequestId,
    ) -> Option<Result<Option<serde_json::Value>, RpcError>> {
        self.results.remove(&id)
    }

    pub fn is_in_flight(&self, id: RequestId) -> bool {
        self.in_flight.contains(&id)
    }

    /// Replaces the connection after a crash/restart: any calls still outstanding on the old
    /// connection are abandoned (never resolved -- this *is* "reject late responses from a
    /// previous generation", since there is no longer any way for the old connection's frames to
    /// reach this `Supervisor` at all), and the shared `Reducer`'s generation is bumped so
    /// anything that later inspects it can tell a reconnect happened. Auth and coordinator state are reset until the new connection is verified.
    pub fn reconnect(&mut self, reader: R, writer: W) {
        self.reader = FrameReader::new(reader, self.limits.max_frame_bytes);
        self.writer = FrameWriter::new(writer);
        self.in_flight.clear();
        self.results.clear();
        self.next_id = 1;
        self.generation = self.reducer.bump_generation();
    }
}
