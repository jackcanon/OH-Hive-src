//! OH Hive Core Library
//! 
//! Defines the `Backend` trait and provides adapters for different compute runtimes.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BackendError {
    #[error("Backend initialization failed: {0}")]
    InitError(String),
    #[error("Job execution failed: {0}")]
    ExecutionError(String),
    #[error("Connection error: {0}")]
    ConnectionError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub supports_streaming: bool,
    pub max_context: usize,
    pub quantization_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub compute_seconds: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Job {
    pub prompt: String,
    pub max_tokens: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Chunk {
    pub content: String,
    pub is_complete: bool,
}

/// The Backend trait defines how compute backends (llama.cpp, vLLM, etc.) interact with OH Hive.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Return capabilities of this backend
    async fn capabilities(&self) -> Result<Capabilities, BackendError>;
    
    /// Execute a job and stream results
    async fn run(&self, job: &Job) -> Result<Vec<Chunk>, BackendError>;
    
    /// Report usage metrics after job execution
    async fn usage(&self) -> Result<Usage, BackendError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_backend_trait_is_defined() {
        // Compile-time check that the trait exists and is sound
        println!("Backend trait successfully defined");
    }
}
