//! The `Backend` trait (ADR-003 D61) and its adapters.
//!
//! Every inference runtime — llama.cpp, MLX, ComfyUI, whisper.cpp, TTS —
//! implements this one trait. Adapters are feature-gated so `hive-server`
//! (regional, Pi-class) compiles with none of them.
//!
//! Shipped so far (M8): `llama_cpp` (text/code), `whisper` (speech-to-text).
//! `comfyui` (image; video reuses it later per D61) exists as a first working
//! slice — text-to-image only, one checkpoint per process, no live model
//! discovery yet. `mlx` and `tts` are still just feature-flag placeholders
//! with no adapter behind them.
//!
//! Contract:
//! - `capabilities()` is cheap and may be called on every heartbeat.
//! - `run()` returns a stream of [`Chunk`]s; the final chunk carries the
//!   authoritative [`Usage`] for the call. The coordinator meters from what it
//!   receives (ADR-002 decision 12), so a backend must not inflate counts.
//! - `run()` must be cancel-safe: dropping the stream aborts the job.

use crate::capability::Capabilities;
use crate::job::Job;
use crate::ledger::Usage;
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod mock;

#[cfg(feature = "llama-cpp")]
pub mod llama_cpp;

#[cfg(feature = "whisper")]
pub mod whisper;

#[cfg(feature = "comfyui")]
pub mod comfyui;

#[derive(Error, Debug)]
pub enum BackendError {
    #[error("backend not available: {0}")]
    Unavailable(String),
    #[error("model not loaded: {0}")]
    ModelNotLoaded(String),
    #[error("job rejected: {0}")]
    Rejected(String),
    #[error("execution failed: {0}")]
    Execution(String),
    #[error("cancelled")]
    Cancelled,
}

/// One unit of streamed output. `usage` is `Some` only on the final chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    /// Text delta for token backends; empty for binary modalities.
    pub text: String,
    /// Content hash of a produced artifact (image/video/audio), when any.
    pub artifact_hash: Option<String>,
    pub done: bool,
    pub usage: Option<Usage>,
}

impl Chunk {
    pub fn text(s: impl Into<String>) -> Self {
        Chunk {
            text: s.into(),
            artifact_hash: None,
            done: false,
            usage: None,
        }
    }
    pub fn done(usage: Usage) -> Self {
        Chunk {
            text: String::new(),
            artifact_hash: None,
            done: true,
            usage: Some(usage),
        }
    }
}

pub type ChunkStream<'a> = BoxStream<'a, Result<Chunk, BackendError>>;

#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// Stable identifier used in `ModelRef::backend`, e.g. "llama_cpp".
    fn name(&self) -> &'static str;

    /// Advertised capabilities of this backend on this machine.
    async fn capabilities(&self) -> Result<Capabilities, BackendError>;

    /// Execute a job, streaming output. Final chunk has `done == true` and
    /// carries `Usage`.
    async fn run<'a>(&'a self, job: &'a Job) -> Result<ChunkStream<'a>, BackendError>;

    /// Health probe. Default: succeed if `capabilities()` succeeds.
    async fn healthy(&self) -> bool {
        self.capabilities().await.is_ok()
    }

    /// Type-erased `self`, so a caller holding only `&dyn Backend` can downcast back to a
    /// concrete adapter when it needs backend-specific surface this trait doesn't expose.
    /// Added for the coding-agent path (`crate::coder`, ADR-024): `Worker` only ever stores a
    /// node's model backend as `&dyn Backend` (it's shared with the plain-text Draft/Critique
    /// loop), but a `code` card's local brain needs `LlamaCppBackend::chat_with_tools` — new,
    /// additive surface deliberately *not* added to this trait (tool-calling completions are a
    /// fundamentally different call shape than `run`, and not every backend needs them; see
    /// `chat_with_tools`'s own doc). `downcast_ref::<LlamaCppBackend>()` on the result is how
    /// `crate::worker` recovers the concrete type when it's actually there.
    ///
    /// No default body: a default `fn as_any(&self) -> &dyn Any { self }` here doesn't compile,
    /// because the default method's `self` is typed against this trait's own (potentially
    /// unsized, `dyn Backend`-erased) `Self`, not the concrete implementor's `Self` — the cast to
    /// `&dyn Any` needs `Self: Sized`, and a `where Self: Sized` bound on the method would make it
    /// unreachable through `&dyn Backend`, which is the only way `crate::worker` ever calls it.
    /// Every implementor (`mock`/`whisper`/`comfyui`/`llama_cpp`) instead has the one-line, always
    /// identical `fn as_any(&self) -> &dyn std::any::Any { self }` — trivial, but a required method
    /// rather than a default so it's actually usable from a trait object.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Drain a stream and return concatenated text plus final usage. Used by tests
/// and by the coordinator's spot-check replay (ADR-005).
pub async fn collect(mut stream: ChunkStream<'_>) -> Result<(String, Usage), BackendError> {
    use futures::StreamExt;
    let mut text = String::new();
    let mut usage = Usage::default();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        text.push_str(&chunk.text);
        if let Some(u) = chunk.usage {
            usage = u;
        }
        if chunk.done {
            break;
        }
    }
    Ok((text, usage))
}
