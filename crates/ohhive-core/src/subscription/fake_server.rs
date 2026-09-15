//! An in-process stand-in for a real `codex app-server` child, driven directly by tests over an
//! in-memory `tokio::io::duplex` pair (see `subscription/tests.rs`) -- the Stage-1 deliverable
//! section 10 calls "a fake app-server test double". Deliberately dumb: it has no script/state
//! machine of its own, just thin async methods a test calls in whatever order it needs, so a test
//! can freely interleave client (`Supervisor`) and server (`FakeServer`) actions within one
//! `#[tokio::test]` body with no spawned task and no ordering races.

use serde::Serialize;

use super::protocol::{self, Notification, RawResponse, RequestId, RpcError, ServerRequest};
use super::transport::{self, FrameReader, FrameWriter, TransportError};

pub struct FakeServer<R, W> {
    reader: FrameReader<R>,
    writer: FrameWriter<W>,
}

impl<R, W> FakeServer<R, W>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    pub fn new(reader: R, writer: W, max_frame_bytes: usize) -> Self {
        Self {
            reader: FrameReader::new(reader, max_frame_bytes),
            writer: FrameWriter::new(writer),
        }
    }

    /// Reads the next request the client sent and returns its id, so the test can decide how (or
    /// whether) to answer it. `None` means the client's write half closed cleanly.
    pub async fn next_request_id(&mut self) -> Result<Option<RequestId>, TransportError> {
        match self.reader.read_frame().await? {
            Some(frame) => transport::extract_request_id(&frame).map(Some),
            None => Ok(None),
        }
    }

    pub async fn reply_ok(&mut self, id: RequestId, result: serde_json::Value) -> Result<(), TransportError> {
        self.write(&RawResponse {
            id,
            result: Some(result),
            error: None,
        })
        .await
    }

    pub async fn reply_err(&mut self, id: RequestId, error: RpcError) -> Result<(), TransportError> {
        self.write(&RawResponse {
            id,
            result: None,
            error: Some(error),
        })
        .await
    }

    pub async fn push_notification(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), TransportError> {
        self.write(&Notification {
            method: method.to_string(),
            params,
        })
        .await
    }

    pub async fn push_server_request(
        &mut self,
        id: RequestId,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), TransportError> {
        self.write(&ServerRequest {
            id,
            method: method.to_string(),
            params,
        })
        .await
    }

    /// Writes a frame's raw bytes verbatim (no shape validation) -- for malformed-frame test
    /// coverage. Callers own including (or deliberately omitting) the trailing delimiter.
    pub async fn push_raw(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.writer.write_frame(bytes).await
    }

    /// Reads back whatever the client writes next, raw -- used to observe an auto-declined
    /// server request (`Supervisor::pump_once`) without needing to guess its exact JSON shape.
    pub async fn read_raw_frame(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        self.reader.read_frame().await
    }

    async fn write<T: Serialize>(&mut self, value: &T) -> Result<(), TransportError> {
        let bytes =
            protocol::encode_frame(value).map_err(|e| TransportError::Malformed(e.to_string()))?;
        self.writer.write_frame(&bytes).await
    }
}
