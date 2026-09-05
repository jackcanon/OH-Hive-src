//! Deterministic mock backend for tests and for the desktop app's
//! "dry run" mode. Echoes the prompt as whitespace-split tokens.

use super::{Backend, BackendError, Chunk, ChunkStream};
use crate::capability::{Capabilities, GpuVendor, Hardware, Modality, ModelRef, ToolsLevel};
use crate::job::Job;
use crate::ledger::Usage;
use async_trait::async_trait;
use futures::stream;

pub struct MockBackend;

#[async_trait]
impl Backend for MockBackend {
    fn name(&self) -> &'static str {
        "mock"
    }

    async fn capabilities(&self) -> Result<Capabilities, BackendError> {
        Ok(Capabilities {
            hardware: Hardware {
                cpu_model: "mock".into(),
                cpu_cores: 1,
                ram_bytes: 1 << 30,
                gpu_vendor: GpuVendor::None,
                gpu_model: None,
                vram_bytes: None,
                disk_free_bytes: 1 << 30,
                upload_mbps: None,
                download_mbps: None,
            },
            modalities: vec![Modality::Text],
            models: vec![ModelRef { id: "mock-echo".into(), modality: Modality::Text, backend: "mock".into() }],
            allow_internet: false,
            tools_level: ToolsLevel::InferenceOnly,
            storage_gb_offered: None,
            shard_capable: None,
        })
    }

    async fn run<'a>(&'a self, job: &'a Job) -> Result<ChunkStream<'a>, BackendError> {
        let prompt = job.input.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let words: Vec<String> = prompt.split_whitespace().map(|w| format!("{w} ")).collect();
        let n_in = words.len() as u64;
        let mut chunks: Vec<Result<Chunk, BackendError>> = words.into_iter().map(|w| Ok(Chunk::text(w))).collect();
        chunks.push(Ok(Chunk::done(Usage { tokens_in: n_in, tokens_out: n_in, compute_seconds: 0.0 })));
        Ok(Box::pin(stream::iter(chunks)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::collect;
    use crate::capability::Requirements;
    use crate::job::JobKind;
    use chrono::Utc;
    use uuid::Uuid;

    #[tokio::test]
    async fn mock_echoes_and_meters() {
        let job = Job {
            id: Uuid::new_v4(),
            kind: JobKind::Inference,
            project_id: Uuid::new_v4(),
            card_id: None,
            parent: None,
            requirements: Requirements::default(),
            input: serde_json::json!({ "prompt": "hello from the hive" }),
            resume_from: None,
            created_at: Utc::now(),
        };
        let stream = MockBackend.run(&job).await.unwrap();
        let (text, usage) = collect(stream).await.unwrap();
        assert_eq!(text.trim(), "hello from the hive");
        assert_eq!(usage.tokens_out, 4);
    }
}
