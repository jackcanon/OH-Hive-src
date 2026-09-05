//! llama.cpp adapter (ADR-003 D16/D61) — talks to a `llama-server` process
//! over its OpenAI-compatible HTTP API and meters from the server's own
//! `usage` object so token counts match llama.cpp's.
//!
//! Status: SKELETON. Process management (spawn `llama-server` with a GGUF,
//! health-wait, kill on drop) and the SSE streaming client are the work item
//! "Define the Backend trait … ship the llama.cpp adapter" in Cmd Work.
//! Do not enable the `llama-cpp` feature until it compiles against reqwest.

use super::{Backend, BackendError, ChunkStream};
use crate::capability::Capabilities;
use crate::job::Job;
use async_trait::async_trait;

pub struct LlamaCppBackend {
    /// e.g. http://127.0.0.1:8080
    pub base_url: String,
    /// Catalog ids of GGUFs this server was started with.
    pub model_ids: Vec<String>,
}

#[async_trait]
impl Backend for LlamaCppBackend {
    fn name(&self) -> &'static str {
        "llama_cpp"
    }

    async fn capabilities(&self) -> Result<Capabilities, BackendError> {
        Err(BackendError::Unavailable(
            "llama.cpp adapter not implemented yet".into(),
        ))
    }

    async fn run<'a>(&'a self, _job: &'a Job) -> Result<ChunkStream<'a>, BackendError> {
        Err(BackendError::Unavailable(
            "llama.cpp adapter not implemented yet".into(),
        ))
    }
}
