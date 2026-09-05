use ohhive_core::{Backend, Capabilities, Job, BackendError};
use async_trait::async_trait;

// Mock backend for testing
struct MockBackend;

#[async_trait]
impl Backend for MockBackend {
    async fn capabilities(&self) -> Result<Capabilities, BackendError> {
        Ok(Capabilities {
            supports_streaming: true,
            max_context: 262144,
            quantization_format: "Q4_K_M".to_string(),
        })
    }

    async fn run(&self, job: &Job) -> Result<Vec<ohhive_core::Chunk>, BackendError> {
        Ok(vec![ohhive_core::Chunk {
            content: "Mock response".to_string(),
            is_complete: true,
        }])
    }

    async fn usage(&self) -> Result<ohhive_core::Usage, BackendError> {
        Ok(ohhive_core::Usage {
            tokens_in: 10,
            tokens_out: 20,
            compute_seconds: 0.5,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("OH Hive Backend Trait - Rust Crate");
    
    let backend = MockBackend;
    let caps = backend.capabilities().await?;
    println!("Backend capabilities: {:?}", caps);
    
    let job = Job {
        prompt: "test".to_string(),
        max_tokens: 100,
    };
    
    let result = backend.run(&job).await?;
    println!("Job result: {:?}", result);
    
    let usage = backend.usage().await?;
    println!("Usage: {:?}", usage);
    
    Ok(())
}
