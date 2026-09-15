//! Newline-delimited JSON-RPC framing over any `AsyncRead`/`AsyncWrite` pair (a real Codex
//! `app-server` child's stdio in later stages, or an in-memory `tokio::io::duplex` half under
//! test today -- see `fake_server.rs`). Section 5's transport requirements: "a single stdout
//! reader that demultiplexes responses, notifications and server-initiated requests; a bounded
//! serialized writer... Reject malformed/oversized frames, report a protocol error and
//! reconnect/reconcile." The 8 MiB/frame and 128-queued-control-message budgets it names live
//! here and in `supervisor.rs` respectively.

use serde::Serialize;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::protocol::{self, Notification, RawResponse, RequestId, ServerRequest};

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("frame too large: {actual} bytes exceeds the {limit}-byte limit")]
    FrameTooLarge { limit: usize, actual: usize },
    #[error("malformed frame: {0}")]
    Malformed(String),
    #[error("too many outstanding requests (queue full)")]
    QueueFull,
}

/// Section 5's budgets. `handshake_timeout` isn't enforced by this Stage-1 scaffold yet (no real
/// process is spawned here) -- it's carried so Stage 2's real supervisor has a place to read it
/// from rather than inventing a second config type.
#[derive(Debug, Clone, Copy)]
pub struct FrameLimits {
    pub max_frame_bytes: usize,
    pub max_queued_control: usize,
    pub handshake_timeout: std::time::Duration,
}

impl Default for FrameLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: 8 * 1024 * 1024,
            max_queued_control: 128,
            handshake_timeout: std::time::Duration::from_secs(15),
        }
    }
}

/// One demultiplexed incoming frame -- exactly the three shapes section 5 names.
#[derive(Debug, Clone)]
pub enum Incoming {
    Response(RawResponse),
    Notification(Notification),
    ServerRequest(ServerRequest),
}

/// Classifies one already-read JSON frame by the presence of `id`/`method`, matching how Codex's
/// JSON-RPC-ish wire format (no `jsonrpc` field, per section 5) distinguishes the three shapes:
/// a response has `id` and no `method`; a notification has `method` and no `id`; a server request
/// has both.
pub fn classify_frame(bytes: &[u8]) -> Result<Incoming, TransportError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| TransportError::Malformed(format!("invalid JSON: {e}")))?;
    let has_method = value.get("method").is_some();
    let has_id = value.get("id").is_some();
    if has_method && has_id {
        let sr: ServerRequest = serde_json::from_value(value)
            .map_err(|e| TransportError::Malformed(format!("bad server request: {e}")))?;
        Ok(Incoming::ServerRequest(sr))
    } else if has_method {
        let n: Notification = serde_json::from_value(value)
            .map_err(|e| TransportError::Malformed(format!("bad notification: {e}")))?;
        Ok(Incoming::Notification(n))
    } else if has_id {
        let r: RawResponse = serde_json::from_value(value)
            .map_err(|e| TransportError::Malformed(format!("bad response: {e}")))?;
        Ok(Incoming::Response(r))
    } else {
        Err(TransportError::Malformed(
            "frame has neither \"id\" nor \"method\"".to_string(),
        ))
    }
}

/// Reads newline-delimited frames from `inner`, retaining any bytes read past a frame boundary
/// (a single `read()` can return more than one frame's worth) for the next call rather than
/// dropping them.
pub struct FrameReader<R> {
    inner: R,
    limit: usize,
    pending: Vec<u8>,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub fn new(inner: R, limit: usize) -> Self {
        Self {
            inner,
            limit,
            pending: Vec::new(),
        }
    }

    /// Returns the next frame's bytes (without the trailing `\n`), or `None` on a clean EOF
    /// between frames -- the caller (`Supervisor::pump_once`) treats that as "the connection
    /// closed", not an error, and decides whether to reconnect.
    pub async fn read_frame(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        loop {
            if let Some(pos) = self.pending.iter().position(|&b| b == b'\n') {
                let mut frame = self.pending.split_off(0);
                self.pending = frame.split_off(pos + 1);
                frame.truncate(pos);
                return Ok(Some(frame));
            }
            if self.pending.len() > self.limit {
                return Err(TransportError::FrameTooLarge {
                    limit: self.limit,
                    actual: self.pending.len(),
                });
            }
            let mut chunk = [0u8; 4096];
            let n = self.inner.read(&mut chunk).await?;
            if n == 0 {
                if self.pending.is_empty() {
                    return Ok(None);
                }
                return Err(TransportError::Malformed(
                    "stream closed mid-frame".to_string(),
                ));
            }
            self.pending.extend_from_slice(&chunk[..n]);
        }
    }
}

/// Writes whole frames to `inner`, flushing each one -- "a bounded serialized writer" (section 5)
/// in the sense that callers (`Supervisor::call`) are responsible for bounding how many are
/// outstanding; this type just guarantees one frame's bytes are written and flushed atomically
/// with respect to any other caller of the same `FrameWriter` (there is only ever one, owned by
/// `Supervisor`, so no internal locking is needed).
pub struct FrameWriter<W> {
    inner: W,
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    pub async fn write_frame(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.inner.write_all(bytes).await?;
        self.inner.flush().await?;
        Ok(())
    }

    pub async fn write_value<T: Serialize>(&mut self, value: &T) -> Result<(), TransportError> {
        let bytes =
            protocol::encode_frame(value).map_err(|e| TransportError::Malformed(e.to_string()))?;
        self.write_frame(&bytes).await
    }
}

pub(crate) fn extract_request_id(frame: &[u8]) -> Result<RequestId, TransportError> {
    let value: serde_json::Value = serde_json::from_slice(frame)
        .map_err(|e| TransportError::Malformed(format!("invalid JSON: {e}")))?;
    value
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| TransportError::Malformed("request missing a numeric \"id\"".to_string()))
}
